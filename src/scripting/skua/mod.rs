mod compile;
mod worker;

use std::{
    io,
    path::{Path, PathBuf},
    process::{Command as Process, ExitStatus, Stdio},
    sync::{Arc, Mutex, OnceLock, PoisonError, mpsc},
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{Value as Json, json};

pub(crate) use compile::{Compiled, Halt, compile, describe};
pub(crate) use worker::Inbound;
use worker::{Ended, Worker};

use super::{
    calls::{self, Call, CallId, Replier, Reply, ToPlayer},
    dialog::{self, Dialog, ToDialogs},
    feeds::{Feed, FeedId, Feeds},
    inputs::{self, Values},
    limits::Clock,
    runtime::{Command, Launch, Link, Log, Outcome},
};
use crate::bridge::{self, CallError, EXPORTS, Event as LoaderEvent, Loader, Subscription};

const STOP_GRACE: Duration = Duration::from_secs(10);
const NO_DOTNET: &str =
    "Skua scripts need .NET 10. Install it from dot.net, or set where dotnet is in Settings.";
const NO_RUNTIME: &str = "Skua scripts need the .NET 10 runtime. Install it from dot.net.";
const NO_RUNNER: &str =
    "This copy of Mira can't run Skua scripts because its Skua runner is missing.";
const CALLABLE: [&str; 2] = ["skua.", "flash."];

pub(crate) struct Setup {
    pub(crate) runner: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) cache: PathBuf,
}

static SETUP: OnceLock<Setup> = OnceLock::new();
static DOTNET: Mutex<Option<PathBuf>> = Mutex::new(None);

pub(crate) fn init(setup: Setup) {
    let _ = SETUP.set(setup);
}

pub(crate) fn set_dotnet(path: Option<PathBuf>) {
    *DOTNET.lock().unwrap_or_else(PoisonError::into_inner) = path;
}

pub(crate) fn is_entry(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "cs")
}

fn setup() -> Result<&'static Setup, String> {
    SETUP.get().ok_or_else(|| NO_RUNNER.to_owned())
}

fn runner(setup: &Setup) -> Result<Process, String> {
    if !setup.runner.is_file() {
        return Err(NO_RUNNER.into());
    }
    let dotnet = DOTNET
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| PathBuf::from("dotnet"));
    let mut process = Process::new(dotnet);
    process
        .arg(&setup.runner)
        .env("MIRA_SKUA_DIR", &setup.home)
        .stdin(Stdio::null());
    Ok(process)
}

fn not_started(error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::NotFound {
        NO_DOTNET.into()
    } else {
        format!("Couldn't start dotnet: {error}.")
    }
}

fn quit(status: ExitStatus, stderr: &str) -> String {
    if status.code() == Some(150) || stderr.contains("You must install or update .NET") {
        return NO_RUNTIME.into();
    }
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let tail = lines[lines.len().saturating_sub(6)..].join("\n");
    if tail.is_empty() {
        format!("The Skua runner quit ({status}).")
    } else {
        format!("The Skua runner quit ({status}):\n{tail}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Event {
    Frame(Inbound),
    Lost(String),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Call {
        function: String,
        #[serde(default)]
        args: Vec<Json>,
    },
    Events {
        id: FeedId,
    },
}

fn asked(mut request: Json) -> Result<calls::Request, serde_json::Error> {
    if request.get("domain").and_then(Json::as_str) != Some("skua") {
        return serde_json::from_value(request);
    }
    if let Some(fields) = request.as_object_mut() {
        fields.remove("domain");
    }
    serde_json::from_value(request).map(calls::Request::Skua)
}

pub(super) fn serve(
    loader: &impl Loader,
    loaded: bool,
    subscribe: impl FnOnce(Box<dyn Fn(&LoaderEvent) -> bool>) -> Subscription,
    feeds: &Feeds,
    request: Request,
) -> Reply {
    match request {
        Request::Call { function, args } => {
            let callback = EXPORTS
                .iter()
                .copied()
                .find(|name| *name == function)
                .filter(|name| CALLABLE.iter().any(|prefix| name.starts_with(prefix)))
                .ok_or(CallError::NotExposed(function))?;
            let value = loader.call(callback, args.into_iter().map(bridge::from_json).collect())?;
            Ok(bridge::to_json(value))
        }
        Request::Events { id } => {
            let events = subscribe(Box::new(|event| item(event).is_some()));
            let primed = loaded
                .then(|| item(&LoaderEvent::Loaded))
                .into_iter()
                .flatten()
                .collect();
            feeds.open(
                id,
                Feed::new(events, |event, items| items.extend(item(&event))).primed(primed),
            );
            Ok(Json::Null)
        }
    }
}

fn item(event: &LoaderEvent) -> Option<Json> {
    Some(match event {
        LoaderEvent::ExtensionResponse { packet } => {
            json!({ "event": "extensionResponse", "packet": packet })
        }
        LoaderEvent::SmartFoxDebug { line } => json!({ "event": "smartFoxDebug", "line": line }),
        LoaderEvent::Loaded => json!({ "event": "loaded" }),
        LoaderEvent::Connection { status } => json!({ "event": "connection", "status": status }),
        LoaderEvent::Debug { .. } | LoaderEvent::Progress { .. } => return None,
    })
}

pub(super) fn execute(
    launch: &Launch,
    clock: &Arc<Clock>,
    commands: &mpsc::Receiver<Command>,
    log: Log,
    link: Link,
) -> Outcome {
    let display = launch.entry.file_name().map_or_else(
        || launch.entry.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let compiled = match compile(&launch.entry, &|| clock.stop_requested()) {
        Ok(compiled) => compiled,
        Err(Halt::Stopped) => return Outcome::Stopped,
        Err(Halt::Failed(message)) => return failed(message),
    };
    let options = match inputs::resolve(&display, &compiled.definition, &launch.inputs) {
        Ok(options) => options,
        Err(message) => return failed(message),
    };
    if clock.stop_requested() {
        return Outcome::Stopped;
    }
    let worker = match worker::attach(link.run, &link.commands) {
        Ok(worker) => worker,
        Err(message) => return failed(message),
    };
    Session {
        worker,
        link,
        log,
        grace: STOP_GRACE,
    }
    .run(&compiled, &launch.entry, options, clock, commands)
}

struct Session {
    worker: Arc<Worker>,
    link: Link,
    log: Log,
    grace: Duration,
}

impl Session {
    fn run(
        self,
        compiled: &Compiled,
        entry: &Path,
        options: Values,
        clock: &Clock,
        commands: &mpsc::Receiver<Command>,
    ) -> Outcome {
        let run = self.link.run;
        let sent = self.worker.send(&json!({
            "type": "start",
            "run": run.0,
            "assembly": compiled.assembly,
            "core": compiled.core,
            "class": compiled.class,
            "script": entry,
            "options": options,
        }));
        if let Err(error) = sent {
            self.worker.leave(run);
            return failed(format!(
                "Couldn't start the script in the Skua runner: {error}."
            ));
        }
        let mut stopping: Option<Instant> = None;
        let stop = |stopping: &mut Option<Instant>| {
            if stopping.is_none() {
                *stopping = Some(Instant::now());
                let _ = self.worker.send(&json!({ "type": "stop", "run": run.0 }));
            }
        };
        loop {
            if clock.stopped_now() {
                break;
            }
            if clock.stop_requested() {
                stop(&mut stopping);
            }
            let received = match stopping {
                Some(since) => match self.grace.checked_sub(since.elapsed()) {
                    Some(left) if !left.is_zero() => commands.recv_timeout(left),
                    _ => break,
                },
                None => commands
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(Command::Stop) if clock.stopped_now() => break,
                Ok(Command::Stop) => stop(&mut stopping),
                Ok(Command::Reply { id, reply }) => self.reply(id, reply),
                Ok(Command::Skua(Event::Frame(frame))) => {
                    if let Some(outcome) = self.handle(frame) {
                        self.worker.leave(run);
                        return outcome;
                    }
                }
                Ok(Command::Skua(Event::Lost(message))) => return failed(message),
                Err(_) => break,
            }
        }
        let since = stopping.unwrap_or_else(Instant::now);
        self.worker.abandon(run, since + self.grace);
        Outcome::Stopped
    }

    fn handle(&self, frame: Inbound) -> Option<Outcome> {
        let run = self.link.run;
        match frame {
            Inbound::Call { id, request, .. } => match asked(request) {
                Ok(request) => (self.link.player)(ToPlayer::Call(Call {
                    run,
                    id: CallId(id),
                    request,
                    reply: Replier::new(CallId(id), self.link.commands.clone()),
                })),
                Err(error) => self.refuse(id, &format!("Mira can't serve that request: {error}")),
            },
            Inbound::Ask { id, request, .. } => {
                match serde_json::from_value::<dialog::Request>(request) {
                    Ok(request) => (self.link.dialogs)(ToDialogs::Open(Dialog::new(
                        CallId(id),
                        request,
                        Replier::new(CallId(id), self.link.commands.clone()),
                    ))),
                    Err(error) => self.refuse(id, &format!("Mira can't show that dialog: {error}")),
                }
            }
            Inbound::Cancel { id, .. } => {
                (self.link.player)(ToPlayer::Cancel {
                    run,
                    id: CallId(id),
                });
                (self.link.dialogs)(ToDialogs::Close(CallId(id)));
            }
            Inbound::Log { level, text, .. } => (self.log)(level.into(), &text),
            Inbound::Ended { outcome, .. } => {
                return Some(match outcome {
                    Ended::Completed => Outcome::Completed,
                    Ended::Stopped => Outcome::Stopped,
                    Ended::Failed { message, stack } => Outcome::Failed { message, stack },
                });
            }
            Inbound::Ready { .. } => {}
        }
        None
    }

    fn reply(&self, id: CallId, reply: Reply) {
        let run = self.link.run.0;
        let sent = self.worker.send(&match reply {
            Ok(value) => json!({ "type": "reply", "run": run, "id": id.0, "ok": value }),
            Err(error) => {
                json!({ "type": "reply", "run": run, "id": id.0, "error": error.to_string() })
            }
        });
        if sent.is_err_and(|error| error.kind() == io::ErrorKind::InvalidInput) {
            self.refuse(id.0, "Mira's answer was larger than 16 MiB.");
        }
    }

    fn refuse(&self, id: u64, message: &str) {
        let _ = self.worker.send(&json!({
            "type": "reply",
            "run": self.link.run.0,
            "id": id,
            "error": message,
        }));
    }
}

fn failed(message: String) -> Outcome {
    Outcome::Failed {
        message,
        stack: None,
    }
}

#[cfg(test)]
mod tests;
