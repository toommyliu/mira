use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tracing_subscriber::{
    EnvFilter, Layer as _,
    fmt::{
        self,
        format::Writer,
        time::{FormatTime, SystemTime},
    },
    layer::SubscriberExt as _,
    util::SubscriberInitExt as _,
};

const FILE_FILTER: &str = "info,lyra=debug";
const LOG_FILE: &str = "~/Library/Logs/Lyra/lyra.log";

pub(crate) struct Logging(Vec<Arc<Mutex<Output>>>);

pub(crate) fn init() -> Logging {
    let console = output(Box::new(io::stdout()));
    let mut outputs = vec![console.clone()];
    let console_layer = fmt::layer()
        .without_time()
        .with_writer(move || CoalescingWriter(console.clone()))
        .with_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")));
    let (opened, file_layer) = match log_file() {
        Ok(file) => {
            let sink = output(Box::new(file));
            outputs.push(sink.clone());
            let layer = fmt::layer()
                .without_time()
                .with_ansi(false)
                .with_writer(move || CoalescingWriter(sink.clone()))
                .with_filter(EnvFilter::new(FILE_FILTER));
            (Ok(()), Some(layer))
        }
        Err(error) => (Err(error), None),
    };
    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .init();
    match opened {
        Ok(()) => tracing::info!("writing the log file to {LOG_FILE}"),
        Err(error) => tracing::warn!(%error, "couldn't open the log file"),
    }
    Logging(outputs)
}

fn log_file() -> io::Result<File> {
    let directory = dirs::home_dir()
        .ok_or_else(|| io::Error::other("no home directory"))?
        .join("Library/Logs/Lyra");
    log_file_in(&directory)
}

fn log_file_in(directory: &Path) -> io::Result<File> {
    fs::create_dir_all(directory)?;
    let path = directory.join("lyra.log");
    if path.exists() {
        fs::rename(&path, directory.join("lyra.previous.log"))?;
    }
    File::create(path)
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
        let mut timestamp = String::new();
        SystemTime
            .format_time(&mut Writer::new(&mut timestamp))
            .map_err(io::Error::other)?;
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

    #[test]
    fn each_run_keeps_the_previous_log() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let directory =
            std::env::temp_dir().join(format!("lyra-logs-{:x}", u64::from_ne_bytes(random)));
        let path = directory.join("lyra.log");
        let mut first = log_file_in(&directory).unwrap();
        writeln!(first, "first run").unwrap();
        drop(first);
        let mut second = log_file_in(&directory).unwrap();
        writeln!(second, "second run").unwrap();
        drop(second);
        assert_eq!(fs::read_to_string(&path).unwrap(), "second run\n");
        assert_eq!(
            fs::read_to_string(directory.join("lyra.previous.log")).unwrap(),
            "first run\n"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
