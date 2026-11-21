use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{self, BufRead, BufReader, Read, Write},
    mem::MaybeUninit,
    net::Shutdown,
    os::{
        fd::{AsRawFd, RawFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    process::{Child, Stdio},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{Value as Json, json};

use super::{
    super::{
        calls::RunId,
        runtime::{Command, Level},
    },
    Event,
};

const PROTOCOL: u32 = 1;
const MAX_FRAME: usize = 16 << 20;
const BUDGET: u64 = 512 << 20;
const HANDSHAKE: Duration = Duration::from_secs(30);
const EXIT_WAIT: Duration = Duration::from_secs(10);
const WRITE_WAIT: Duration = Duration::from_secs(10);
const TAIL: usize = 40;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum Inbound {
    Ready {
        protocol: u32,
    },
    Call {
        run: u64,
        id: u64,
        request: Json,
    },
    Ask {
        run: u64,
        id: u64,
        request: Json,
    },
    Cancel {
        run: u64,
        id: u64,
    },
    Log {
        run: u64,
        level: LogLevel,
        text: String,
    },
    Ended {
        run: u64,
        outcome: Ended,
    },
}

impl Inbound {
    fn run(&self) -> Option<RunId> {
        match self {
            Self::Ready { .. } => None,
            Self::Call { run, .. }
            | Self::Ask { run, .. }
            | Self::Cancel { run, .. }
            | Self::Log { run, .. }
            | Self::Ended { run, .. } => Some(RunId(*run)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Debug => Self::Debug,
            LogLevel::Info => Self::Info,
            LogLevel::Warn => Self::Warn,
            LogLevel::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum Ended {
    Completed,
    Stopped,
    Failed {
        message: String,
        stack: Option<String>,
    },
}

pub(super) struct Worker {
    child: Mutex<Option<Child>>,
    writer: Mutex<UnixStream>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    routes: HashMap<RunId, mpsc::Sender<Command>>,
    ended: HashSet<RunId>,
    stopping: HashMap<RunId, Instant>,
    retired: bool,
    lost: bool,
}

impl State {
    fn overdue(&self) -> bool {
        let now = Instant::now();
        self.stopping.values().any(|deadline| now >= *deadline)
    }

    fn retire(&mut self) {
        if !self.retired && !self.lost {
            self.retired = true;
            DRAINING.fetch_add(1, Ordering::SeqCst);
        }
    }
}

static CURRENT: Mutex<Option<Arc<Worker>>> = Mutex::new(None);
static DRAINING: AtomicUsize = AtomicUsize::new(0);

pub(super) fn attach(run: RunId, inbox: &mpsc::Sender<Command>) -> Result<Arc<Worker>, String> {
    let mut current = lock(&CURRENT);
    if let Some(worker) = current.as_ref()
        && worker.admit(run, inbox)
    {
        return Ok(worker.clone());
    }
    let worker = spawn()?;
    worker.admit(run, inbox);
    *current = Some(worker.clone());
    Ok(worker)
}

fn spawn() -> Result<Arc<Worker>, String> {
    let setup = super::setup()?;
    let (ours, theirs) = UnixStream::pair()
        .map_err(|error| format!("Couldn't open a channel to the Skua runner: {error}."))?;
    let mut process = super::runner(setup)?;
    process
        .arg("worker")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let fd = theirs.as_raw_fd();
    // SAFETY: the hook runs in the forked child before exec and only calls dup2/fcntl,
    // which are async-signal-safe.
    unsafe {
        process.pre_exec(move || inherit(fd));
    }
    let mut child = process
        .spawn()
        .map_err(|error| super::not_started(&error))?;
    drop(theirs);
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    let echoes = [
        echo(child.stdout.take(), tail.clone()),
        echo(child.stderr.take(), tail.clone()),
    ];
    ours.set_write_timeout(Some(WRITE_WAIT))
        .map_err(|error| format!("Couldn't open a channel to the Skua runner: {error}."))?;
    let mut reader = ours
        .try_clone()
        .map_err(|error| format!("Couldn't read from the Skua runner: {error}."))?;
    let greeted = greet(&mut reader);
    if let Err(problem) = greeted {
        let _ = child.kill();
        let status = child.wait();
        for echo in echoes {
            let _ = echo.join();
        }
        let stderr = text(&tail);
        return Err(match status {
            Ok(status) if !status.success() && problem.is_none() => super::quit(status, &stderr),
            _ => problem.unwrap_or_else(|| "The Skua runner quit while starting.".into()),
        });
    }
    let worker = Arc::new(Worker {
        child: Mutex::new(Some(child)),
        writer: Mutex::new(ours),
        state: Mutex::default(),
    });
    let listening = worker.clone();
    thread::Builder::new()
        .name("mira-skua-worker".into())
        .spawn(move || listening.listen(reader, &tail))
        .map_err(|error| format!("Couldn't watch the Skua runner: {error}."))?;
    Ok(worker)
}

fn inherit(fd: RawFd) -> io::Result<()> {
    // SAFETY: plain descriptor calls on a descriptor this process owns.
    let result = unsafe {
        if fd == 3 {
            libc::fcntl(3, libc::F_SETFD, 0)
        } else {
            libc::dup2(fd, 3)
        }
    };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn greet(reader: &mut UnixStream) -> Result<(), Option<String>> {
    reader.set_read_timeout(Some(HANDSHAKE)).map_err(|_| None)?;
    let greeting = read(reader);
    reader.set_read_timeout(None).map_err(|_| None)?;
    match greeting {
        Ok(Some(frame)) => match serde_json::from_value(frame) {
            Ok(Inbound::Ready { protocol }) if protocol == PROTOCOL => Ok(()),
            _ => Err(Some(
                "This Skua runner doesn't match this copy of Mira. Reinstall Mira.".into(),
            )),
        },
        Ok(None) => Err(None),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            Err(Some("The Skua runner didn't start in time.".into()))
        }
        Err(_) => Err(None),
    }
}

fn echo(
    pipe: Option<impl Read + Send + 'static>,
    tail: Arc<Mutex<VecDeque<String>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Some(pipe) = pipe else {
            return;
        };
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            tracing::info!(target: "mira::skua", "{line}");
            let mut tail = lock(&tail);
            if tail.len() == TAIL {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    })
}

fn text(tail: &Mutex<VecDeque<String>>) -> String {
    lock(tail)
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

impl Worker {
    #[cfg(test)]
    pub(super) fn connected(stream: UnixStream) -> Arc<Self> {
        let worker = Arc::new(Self {
            child: Mutex::new(None),
            writer: Mutex::new(stream.try_clone().unwrap()),
            state: Mutex::default(),
        });
        let listening = worker.clone();
        thread::spawn(move || {
            let lost = listening.forward(stream);
            listening.lose(&lost);
        });
        worker
    }

    pub(super) fn admit(&self, run: RunId, inbox: &mpsc::Sender<Command>) -> bool {
        let mut state = lock(&self.state);
        if state.overdue() {
            state.retire();
        }
        if state.retired || state.lost {
            return false;
        }
        state.routes.insert(run, inbox.clone());
        true
    }

    pub(super) fn send(&self, frame: &Json) -> io::Result<()> {
        let mut writer = lock(&self.writer);
        let written = write(&mut *writer, frame);
        if let Err(error) = &written
            && error.kind() != io::ErrorKind::InvalidInput
        {
            tracing::warn!(%error, "lost the channel to the Skua runner");
            let _ = writer.shutdown(Shutdown::Both);
        }
        written
    }

    pub(super) fn leave(&self, run: RunId) {
        let mut state = lock(&self.state);
        state.routes.remove(&run);
        state.ended.remove(&run);
        if !state.retired
            && DRAINING.load(Ordering::SeqCst) == 0
            && lock(&self.child)
                .as_ref()
                .and_then(|child| libc::pid_t::try_from(child.id()).ok())
                .and_then(footprint)
                .is_some_and(|bytes| bytes > BUDGET)
        {
            state.retire();
        }
        self.settle(&mut state);
    }

    pub(super) fn abandon(self: &Arc<Self>, run: RunId, deadline: Instant) {
        let ended = {
            let mut state = lock(&self.state);
            if state.routes.remove(&run).is_none() {
                return;
            }
            let ended = state.ended.remove(&run);
            if !ended {
                state.stopping.insert(run, deadline);
            }
            ended
        };
        if !ended {
            let _ = self.send(&json!({ "type": "stop", "run": run.0 }));
            let worker = Arc::clone(self);
            thread::spawn(move || {
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
                worker.settle(&mut lock(&worker.state));
            });
        }
        self.settle(&mut lock(&self.state));
    }

    pub(super) fn finished(&self, run: RunId) {
        let mut state = lock(&self.state);
        if state.stopping.remove(&run).is_some() {
            self.settle(&mut state);
        }
    }

    fn settle(&self, state: &mut State) {
        if state.overdue() {
            state.retire();
        }
        if !state.retired || !state.routes.is_empty() {
            return;
        }
        if state.stopping.is_empty() {
            let _ = lock(&self.writer).shutdown(Shutdown::Both);
        } else if let Some(child) = lock(&self.child).as_mut() {
            let _ = child.kill();
        }
    }

    fn listen(&self, reader: UnixStream, tail: &Mutex<VecDeque<String>>) {
        let lost = self.forward(reader);
        let deadline = Instant::now() + EXIT_WAIT;
        let status = loop {
            let mut slot = lock(&self.child);
            let Some(child) = slot.as_mut() else {
                break None;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    slot.take();
                    break Some(status);
                }
                Ok(None) if Instant::now() < deadline => {
                    drop(slot);
                    thread::sleep(Duration::from_millis(50));
                }
                _ => {
                    let _ = child.kill();
                    let status = child.wait().ok();
                    slot.take();
                    break status;
                }
            }
        };
        let message = match status {
            Some(status) if !status.success() => super::quit(status, &text(tail)),
            _ => lost,
        };
        self.lose(&message);
    }

    fn forward(&self, mut reader: UnixStream) -> String {
        loop {
            match read(&mut reader) {
                Ok(Some(frame)) => match serde_json::from_value::<Inbound>(frame) {
                    Ok(frame) => self.route(frame),
                    Err(error) => {
                        tracing::warn!(%error, "the Skua runner sent a frame Mira can't read")
                    }
                },
                Ok(None) => break "The Skua runner quit.".to_owned(),
                Err(error) => break format!("Lost the Skua runner: {error}."),
            }
        }
    }

    fn route(&self, frame: Inbound) {
        let Some(run) = frame.run() else {
            return;
        };
        let mut state = lock(&self.state);
        if let Some(inbox) = state.routes.get(&run).cloned() {
            if matches!(frame, Inbound::Ended { .. }) {
                state.ended.insert(run);
            }
            let _ = inbox.send(Command::Skua(Event::Frame(frame)));
            return;
        }
        if !state.stopping.contains_key(&run) {
            return;
        }
        match frame {
            Inbound::Ended { .. } => {
                drop(state);
                self.finished(run);
            }
            Inbound::Call { id, .. } | Inbound::Ask { id, .. } => {
                drop(state);
                let _ = self.send(&json!({
                    "type": "reply", "run": run.0, "id": id, "error": "The script was stopped.",
                }));
            }
            _ => {}
        }
    }

    fn lose(&self, message: &str) {
        let routes = {
            let mut state = lock(&self.state);
            if state.retired && !state.lost {
                DRAINING.fetch_sub(1, Ordering::SeqCst);
            }
            state.lost = true;
            state.stopping.clear();
            state.ended.clear();
            std::mem::take(&mut state.routes)
        };
        for inbox in routes.into_values() {
            let _ = inbox.send(Command::Skua(Event::Lost(message.to_owned())));
        }
    }
}

fn footprint(pid: libc::pid_t) -> Option<u64> {
    let mut info = MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: the buffer is a zeroed rusage_info_v2, the layout RUSAGE_INFO_V2 writes.
    let read =
        unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, info.as_mut_ptr().cast()) };
    // SAFETY: proc_pid_rusage filled the buffer when it returned 0.
    (read == 0).then(|| unsafe { info.assume_init() }.ri_phys_footprint)
}

pub(super) fn read(stream: &mut impl Read) -> io::Result<Option<Json>> {
    let mut length = [0; 4];
    match stream.read_exact(&mut length) {
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        result => result?,
    }
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a frame was larger than 16 MiB",
        ));
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(io::Error::other)
}

pub(super) fn write(stream: &mut impl Write, frame: &Json) -> io::Result<()> {
    let body = serde_json::to_vec(frame)?;
    let length = u32::try_from(body.len())
        .ok()
        .filter(|length| *length as usize <= MAX_FRAME)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a frame over 16 MiB"))?;
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(&body)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
