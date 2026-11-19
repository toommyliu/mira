use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Child, ExitStatus, Stdio},
    sync::{Mutex, TryLockError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::Value as Json;

use super::super::inputs::{Definition, Field, Kind};
use crate::packages::SKUA_FOLDER;

const POLL: Duration = Duration::from_millis(25);
const TIMEOUT: Duration = Duration::from_secs(120);
const LISTED_DIAGNOSTICS: usize = 8;
const OUTSIDE: &str = "Skua scripts run from the Skua scripts package.";

static COMPILING: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Compiled {
    pub(super) assembly: PathBuf,
    pub(super) core: Option<PathBuf>,
    pub(super) class: String,
    pub(crate) definition: Definition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Halt {
    Stopped,
    Failed(String),
}

pub(crate) fn compile(entry: &Path, stopped: &dyn Fn() -> bool) -> Result<Compiled, Halt> {
    let _turn = loop {
        match COMPILING.try_lock() {
            _ if stopped() => return Err(Halt::Stopped),
            Ok(turn) => break turn,
            Err(TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => thread::sleep(POLL),
        }
    };
    let setup = super::setup().map_err(Halt::Failed)?;
    let scripts = setup.home.join(SKUA_FOLDER);
    let relative = entry
        .strip_prefix(&scripts)
        .ok()
        .and_then(Path::to_str)
        .ok_or_else(|| Halt::Failed(OUTSIDE.into()))?;
    let mut process = super::runner(setup).map_err(Halt::Failed)?;
    process
        .arg("compile")
        .arg("--scripts")
        .arg(&scripts)
        .arg("--cache")
        .arg(&setup.cache)
        .arg("--entry")
        .arg(relative)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process
        .spawn()
        .map_err(|error| Halt::Failed(super::not_started(&error)))?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let status = finish(&mut child, stopped, Instant::now() + TIMEOUT)?;
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        return Err(Halt::Failed(super::quit(status, &stderr)));
    }
    parse(&stdout).map_err(Halt::Failed)
}

pub(crate) fn describe(
    entry: &Path,
    stopped: &dyn Fn() -> bool,
) -> Result<Option<Definition>, Halt> {
    let definition = compile(entry, stopped)?.definition;
    Ok((!definition.fields().is_empty()).then_some(definition))
}

fn finish(
    child: &mut Child,
    stopped: &dyn Fn() -> bool,
    deadline: Instant,
) -> Result<ExitStatus, Halt> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if stopped() || Instant::now() >= deadline => {
                let halt = if stopped() {
                    Halt::Stopped
                } else {
                    Halt::Failed(format!(
                        "The Skua compiler didn't finish within {} seconds.",
                        TIMEOUT.as_secs()
                    ))
                };
                let _ = child.kill();
                let _ = child.wait();
                return Err(halt);
            }
            Ok(None) => thread::sleep(POLL),
            Err(error) => {
                return Err(Halt::Failed(format!(
                    "Lost track of the Skua compiler: {error}."
                )));
            }
        }
    }
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Printed {
    Ok(Raw),
    Error(Failure),
}

#[derive(Deserialize)]
struct Raw {
    assembly: PathBuf,
    core: Option<PathBuf>,
    class: String,
    options: RawOptions,
}

#[derive(Deserialize)]
struct RawOptions {
    fields: Vec<RawField>,
}

#[derive(Deserialize)]
struct RawField {
    key: String,
    group: Option<String>,
    label: String,
    description: Option<String>,
    kind: RawKind,
    default: Json,
    #[serde(default)]
    options: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum RawKind {
    Boolean,
    Text,
    Integer,
    Number,
    Select,
}

#[derive(Deserialize)]
struct Failure {
    message: String,
    #[serde(default)]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Deserialize)]
struct Diagnostic {
    file: String,
    line: u32,
    column: u32,
    message: String,
}

fn parse(stdout: &str) -> Result<Compiled, String> {
    let line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or("The Skua compiler didn't say what happened.")?;
    let printed: Printed = serde_json::from_str(line)
        .map_err(|error| format!("The Skua compiler said something Mira can't read: {error}."))?;
    let raw = match printed {
        Printed::Ok(raw) => raw,
        Printed::Error(failure) => return Err(explain(failure)),
    };
    let fields = raw
        .options
        .fields
        .into_iter()
        .map(field)
        .collect::<Result<_, _>>()?;
    Ok(Compiled {
        assembly: raw.assembly,
        core: raw.core,
        class: raw.class,
        definition: Definition::new(fields),
    })
}

fn field(raw: RawField) -> Result<Field, String> {
    let number = |integer| Kind::Number {
        integer,
        min: None,
        max: None,
        default: Some(raw.default.as_f64().unwrap_or_default()),
    };
    let kind = match raw.kind {
        RawKind::Boolean => Kind::Boolean {
            default: raw.default.as_bool().unwrap_or_default(),
        },
        RawKind::Text => Kind::Text {
            required: false,
            default: raw.default.as_str().unwrap_or_default().to_owned(),
        },
        RawKind::Integer => number(true),
        RawKind::Number => number(false),
        RawKind::Select if raw.options.is_empty() => {
            return Err(format!("The Skua option {} has no choices.", raw.label));
        }
        RawKind::Select => Kind::Select {
            default: raw
                .default
                .as_str()
                .filter(|default| raw.options.iter().any(|option| option == default))
                .or(raw.options.first().map(String::as_str))
                .map(str::to_owned),
            options: raw.options,
        },
    };
    Ok(Field {
        key: raw.key,
        group: raw.group,
        label: raw.label,
        description: raw.description,
        kind,
    })
}

fn explain(failure: Failure) -> String {
    let mut message = failure.message;
    for diagnostic in failure.diagnostics.iter().take(LISTED_DIAGNOSTICS) {
        message.push_str(&format!(
            "\n{}:{}:{}: {}",
            diagnostic.file, diagnostic.line, diagnostic.column, diagnostic.message
        ));
    }
    let more = failure.diagnostics.len().saturating_sub(LISTED_DIAGNOSTICS);
    if more > 0 {
        message.push_str(&format!("\n…and {more} more"));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_output_becomes_an_entry_and_its_inputs() {
        let compiled = parse(concat!(
            "warming up\n",
            r#"{"ok":{"assembly":"/c/e.dll","core":"/c/core.dll","class":"FreeDailyBoost","options":{"configure":true,"fields":["#,
            r#"{"key":"Collect","group":null,"label":"Collect","description":"Pick it up","kind":"boolean","default":true},"#,
            r#"{"key":"Select:Sword","group":"Select","label":"Sword","description":null,"kind":"boolean","default":false},"#,
            r#"{"key":"Name","group":null,"label":"Name","description":null,"kind":"text","default":null},"#,
            r#"{"key":"Count","group":null,"label":"Count","description":null,"kind":"integer","default":3},"#,
            r#"{"key":"Mode","group":null,"label":"Mode","description":null,"kind":"select","default":"Gone","options":["Solo Mode","Farm Mode"]}"#,
            "]}}}\n"
        ))
        .unwrap();
        assert_eq!(compiled.assembly, PathBuf::from("/c/e.dll"));
        assert_eq!(compiled.core, Some(PathBuf::from("/c/core.dll")));
        assert_eq!(compiled.class, "FreeDailyBoost");
        let fields = compiled.definition.fields();
        let kinds: Vec<(&str, Option<&str>, &Kind)> = fields
            .iter()
            .map(|field| (field.key.as_str(), field.group.as_deref(), &field.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("Collect", None, &Kind::Boolean { default: true }),
                (
                    "Select:Sword",
                    Some("Select"),
                    &Kind::Boolean { default: false }
                ),
                (
                    "Name",
                    None,
                    &Kind::Text {
                        required: false,
                        default: String::new()
                    }
                ),
                (
                    "Count",
                    None,
                    &Kind::Number {
                        integer: true,
                        min: None,
                        max: None,
                        default: Some(3.0)
                    }
                ),
                (
                    "Mode",
                    None,
                    &Kind::Select {
                        options: vec!["Solo Mode".into(), "Farm Mode".into()],
                        default: Some("Solo Mode".into())
                    }
                ),
            ]
        );
        assert_eq!(
            compiled.definition.resolve(&serde_json::Map::new()),
            Ok(serde_json::from_str(
                r#"{"Collect":true,"Select:Sword":false,"Name":"","Count":3,"Mode":"Solo Mode"}"#
            )
            .unwrap())
        );
    }

    #[test]
    fn stopping_while_another_compile_runs_returns_at_once() {
        let _busy = COMPILING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (done, finished) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = done.send(compile(Path::new("/x.cs"), &|| true));
        });
        assert_eq!(
            finished.recv_timeout(Duration::from_secs(2)),
            Ok(Err(Halt::Stopped))
        );
    }

    #[test]
    fn a_compiler_that_never_finishes_is_killed_at_the_deadline() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let started = Instant::now();
        let halt = finish(&mut child, &|| false, started + Duration::from_millis(100));
        assert!(matches!(halt, Err(Halt::Failed(_))), "{halt:?}");
        assert!(started.elapsed() >= Duration::from_millis(100));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn compile_errors_name_the_file_and_line() {
        let diagnostics: Vec<String> = (1..=10)
            .map(|line| {
                format!(r#"{{"file":"CoreBots.cs","line":{line},"column":2,"message":"bad"}}"#)
            })
            .collect();
        let printed = format!(
            r#"{{"error":{{"message":"Couldn't compile Dailies/X.cs","diagnostics":[{}]}}}}"#,
            diagnostics.join(",")
        );
        let message = parse(&printed).unwrap_err();
        let lines: Vec<&str> = message.lines().collect();
        assert_eq!(lines.first(), Some(&"Couldn't compile Dailies/X.cs"));
        assert_eq!(lines.get(1), Some(&"CoreBots.cs:1:2: bad"));
        assert_eq!(lines.len(), 1 + LISTED_DIAGNOSTICS + 1);
        assert_eq!(lines.last(), Some(&"…and 2 more"));
        assert!(parse("").is_err());
        assert!(parse("not json").is_err());
    }
}
