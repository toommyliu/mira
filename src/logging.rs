use std::{
    fs::{self, File},
    io::{self, Write},
    panic::{self, PanicHookInfo},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{Local, SecondsFormat};
use tracing::Subscriber;
use tracing_subscriber::{
    EnvFilter, Layer as _,
    field::RecordFields,
    fmt::{
        self, FormatFields,
        format::{DefaultFields, Writer},
    },
    layer::SubscriberExt as _,
    util::SubscriberInitExt as _,
};

const FILE_FILTER: &str = if cfg!(debug_assertions) {
    "warn,mira=debug,avm_trace=info"
} else {
    "warn,mira=debug"
};
const CONSOLE_FILTER: &str = if cfg!(debug_assertions) {
    "info"
} else {
    "info,avm_trace=off"
};
const LOG_FILE: &str = "~/Library/Logs/Mira/mira.log";
const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

pub(crate) struct Logging(Vec<Arc<Mutex<Output>>>);

pub(crate) fn init() -> Logging {
    let console = output(Box::new(io::stdout()));
    let console_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(CONSOLE_FILTER));
    let (opened, file) = match log_file() {
        Ok(file) => (Ok(()), Some(output(Box::new(file)))),
        Err(error) => (Err(error), None),
    };
    let outputs = std::iter::once(console.clone())
        .chain(file.clone())
        .collect();
    subscriber(console, console_filter, file).init();
    panic::set_hook(Box::new(log_panic));
    match opened {
        Ok(()) => tracing::info!("writing the log file to {LOG_FILE}"),
        Err(error) => tracing::warn!(%error, "couldn't open the log file"),
    }
    Logging(outputs)
}

fn subscriber(
    console: Arc<Mutex<Output>>,
    console_filter: EnvFilter,
    file: Option<Arc<Mutex<Output>>>,
) -> impl Subscriber + Send + Sync {
    let console = fmt::layer()
        .without_time()
        .with_writer(move || CoalescingWriter(console.clone()))
        .with_filter(console_filter);
    let file = file.map(|sink| {
        fmt::layer()
            .fmt_fields(FileFields::default())
            .without_time()
            .with_ansi(false)
            .with_writer(move || CoalescingWriter(sink.clone()))
            .with_filter(EnvFilter::new(FILE_FILTER))
    });
    tracing_subscriber::registry().with(console).with(file)
}

// Each fmt layer caches a span's formatted fields under its field formatter's type. Sharing
// DefaultFields with the console would put the console's color codes into the file.
#[derive(Default)]
struct FileFields(DefaultFields);

impl<'writer> FormatFields<'writer> for FileFields {
    fn format_fields<R: RecordFields>(
        &self,
        writer: Writer<'writer>,
        fields: R,
    ) -> std::fmt::Result {
        self.0.format_fields(writer, fields)
    }
}

fn log_panic(info: &PanicHookInfo) {
    let thread = std::thread::current();
    let backtrace = std::backtrace::Backtrace::force_capture();
    tracing::error!(
        target: "mira::panic",
        thread = thread.name().unwrap_or("unnamed"),
        location = info.location().map(tracing::field::display),
        panic = info.payload_as_str().map(tracing::field::debug),
        %backtrace,
        "panicked"
    );
}

fn log_file() -> io::Result<LogFile> {
    let directory = dirs::home_dir()
        .ok_or_else(|| io::Error::other("no home directory"))?
        .join("Library/Logs/Mira");
    LogFile::open(directory, MAX_LOG_BYTES)
}

struct LogFile {
    directory: PathBuf,
    file: File,
    len: u64,
    max_len: u64,
}

impl LogFile {
    fn open(directory: PathBuf, max_len: u64) -> io::Result<Self> {
        fs::create_dir_all(&directory)?;
        let file = Self::start(&directory)?;
        Ok(Self {
            directory,
            file,
            len: 0,
            max_len,
        })
    }

    fn start(directory: &Path) -> io::Result<File> {
        let path = directory.join("mira.log");
        match fs::rename(&path, directory.join("mira.previous.log")) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        File::create(path)
    }
}

impl Write for LogFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.len > 0 && self.len + bytes.len() as u64 > self.max_len {
            self.file = Self::start(&self.directory)?;
            self.len = 0;
        }
        let written = self.file.write(bytes)?;
        self.len += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn output(sink: Box<dyn Write + Send>) -> Arc<Mutex<Output>> {
    Arc::new(Mutex::new(Output {
        sink,
        line: Vec::new(),
        count: 0,
        reported: 0,
        last_report: Instant::now(),
    }))
}

impl Drop for Logging {
    fn drop(&mut self) {
        for output in &self.0 {
            if let Ok(mut output) = output.lock() {
                let _ = output.flush_repeats();
            }
        }
    }
}

struct CoalescingWriter(Arc<Mutex<Output>>);

impl Write for CoalescingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // tracing-subscriber writes each fully formatted event in one write_all call.
        let mut output = self.0.lock().unwrap();
        if bytes == output.line {
            output.count = output.count.saturating_add(1);
            if output.last_report.elapsed() >= Duration::from_secs(1) {
                output.flush_repeats()?;
            }
        } else {
            output.flush_repeats()?;
            output.write_line(bytes, 1)?;
            output.line.clear();
            output.line.extend_from_slice(bytes);
            output.count = 1;
            output.reported = 1;
            output.last_report = Instant::now();
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut output = self.0.lock().unwrap();
        output.flush_repeats()?;
        output.sink.flush()
    }
}

struct Output {
    sink: Box<dyn Write + Send>,
    line: Vec<u8>,
    count: u64,
    reported: u64,
    last_report: Instant,
}

impl Output {
    fn flush_repeats(&mut self) -> io::Result<()> {
        if self.count > self.reported {
            let line = std::mem::take(&mut self.line);
            let written = self.write_line(&line, self.count);
            self.line = line;
            written?;
            self.reported = self.count;
            self.last_report = Instant::now();
        }
        Ok(())
    }

    fn write_line(&mut self, line: &[u8], count: u64) -> io::Result<()> {
        let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Micros, false);
        let mut text = Vec::with_capacity(timestamp.len() + line.len() + 16);
        write!(text, "{timestamp} ")?;
        text.extend_from_slice(line.strip_suffix(b"\n").unwrap_or(line));
        if count > 1 {
            write!(text, " (x{count})")?;
        }
        text.push(b'\n');
        self.sink.write_all(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn repeated_lines_collapse_in_any_sink() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let sink = output(Box::new(Shared(written.clone())));
        let mut writer = CoalescingWriter(sink.clone());
        for line in ["a\n", "b\n", "b\n", "b\n", "c\n"] {
            writer.write_all(line.as_bytes()).unwrap();
        }
        let text = String::from_utf8(written.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = text
            .lines()
            .map(|line| line.split_once(' ').unwrap().1)
            .collect();
        assert_eq!(lines, ["a", "b", "b (x3)", "c"]);
    }

    fn shared() -> (Arc<Mutex<Vec<u8>>>, Arc<Mutex<Output>>) {
        let written = Arc::new(Mutex::new(Vec::new()));
        let sink = output(Box::new(Shared(written.clone())));
        (written, sink)
    }

    fn text(written: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(written.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn file_lines_show_spans_without_color_codes() {
        let (_, console) = shared();
        let (file, sink) = shared();
        let subscriber = subscriber(console, EnvFilter::new("info"), Some(sink));
        tracing::subscriber::with_default(subscriber, || {
            tracing::error_span!("session", id = 3).in_scope(|| tracing::info!("signed in"));
        });
        let text = text(&file);
        assert!(text.contains("INFO session{id=3}: "), "{text}");
        assert!(!text.contains('\x1b'), "{text:?}");
    }

    #[test]
    fn game_traces_are_logged_only_in_debug_builds() {
        let (console, console_sink) = shared();
        let (file, file_sink) = shared();
        let subscriber = subscriber(
            console_sink,
            EnvFilter::new(CONSOLE_FILTER),
            Some(file_sink),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "avm_trace", "[Net] [Sending - STR]: %xt%zm%mv%1%223%446%10%");
            tracing::info!(target: "ruffle_core::context", "Loaded SWF version 43");
            tracing::debug!(target: "mira::bridge", "unhandled loader call");
        });
        let (console, file) = (text(&console), text(&file));
        let packet = "[Net] [Sending - STR]: %xt%zm%mv%1%223%446%10%";
        assert_eq!(
            console.contains(packet),
            cfg!(debug_assertions),
            "{console}"
        );
        assert_eq!(file.contains(packet), cfg!(debug_assertions), "{file}");
        assert!(file.contains("unhandled loader call"), "{file}");
        assert!(!file.contains("Loaded SWF"), "{file}");
    }

    #[test]
    fn panics_reach_the_log_file() {
        let (_, console) = shared();
        let (file, sink) = shared();
        let subscriber = subscriber(console, EnvFilter::new("info"), Some(sink));
        tracing::subscriber::with_default(subscriber, || {
            let previous = panic::take_hook();
            panic::set_hook(Box::new(log_panic));
            let caught = tracing::error_span!("session", id = 7)
                .in_scope(|| panic::catch_unwind(|| panic!("the len is 0")));
            panic::set_hook(previous);
            assert!(caught.is_err());
        });
        let text = text(&file);
        assert!(
            text.contains("ERROR session{id=7}: mira::panic: panicked thread="),
            "{text}"
        );
        assert!(text.contains(concat!("location=", file!())), "{text}");
        assert!(
            text.contains(r#"panic="the len is 0" backtrace="#),
            "{text}"
        );
    }

    #[test]
    fn the_log_keeps_the_previous_run_and_rotates_at_its_size_limit() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let directory =
            std::env::temp_dir().join(format!("mira-logs-{:x}", u64::from_ne_bytes(random)));
        let read = |name: &str| fs::read_to_string(directory.join(name)).unwrap();

        let mut first = LogFile::open(directory.clone(), 16).unwrap();
        first.write_all(b"first run\n").unwrap();
        drop(first);
        let mut second = LogFile::open(directory.clone(), 16).unwrap();
        second.write_all(b"second run\n").unwrap();
        assert_eq!(read("mira.log"), "second run\n");
        assert_eq!(read("mira.previous.log"), "first run\n");

        second.write_all(b"line 2\n").unwrap();
        second.write_all(b"line 3\n").unwrap();
        assert_eq!(read("mira.previous.log"), "second run\n");
        assert_eq!(read("mira.log"), "line 2\nline 3\n");
        second.write_all(b"a line longer than the limit\n").unwrap();
        assert_eq!(read("mira.previous.log"), "line 2\nline 3\n");
        assert_eq!(read("mira.log"), "a line longer than the limit\n");
        fs::remove_dir_all(directory).unwrap();
    }
}
