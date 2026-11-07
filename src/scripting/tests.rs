use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use serde_json::json;

use super::{
    Level, Limit, Limits, Outcome, Run, ToPlayer,
    calls::{Call, Reply, Request, RunId},
    player::Request as P,
    runtime::Player,
};
use crate::bridge::CallError;

struct Folder(PathBuf);

impl Folder {
    fn new(files: &[(&str, &str)]) -> Self {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let root =
            std::env::temp_dir().join(format!("lyra-script-{:x}", u64::from_ne_bytes(random)));
        for (path, text) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        Self(root)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Finished {
    outcome: Outcome,
    logs: Vec<String>,
    elapsed: Duration,
}

struct Started {
    run: Run,
    outcome: mpsc::Receiver<Outcome>,
    logs: Arc<Mutex<Vec<String>>>,
    started: Instant,
}

impl Started {
    fn finish(self) -> Finished {
        let outcome = self
            .outcome
            .recv_timeout(Duration::from_secs(20))
            .expect("the script finished");
        let elapsed = self.started.elapsed();
        drop(self.run);
        let logs = self.logs.lock().unwrap().clone();
        Finished {
            outcome,
            logs,
            elapsed,
        }
    }
}

fn start(folder: &Folder, entry: &str, limits: Limits, player: Player) -> Started {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let sink = logs.clone();
    let (finished, outcome) = mpsc::channel();
    let started = Instant::now();
    let run = Run::start(
        folder.0.join(entry),
        limits,
        Box::new(move |level, message| {
            let prefix = if level == Level::Error { "error: " } else { "" };
            sink.lock().unwrap().push(format!("{prefix}{message}"));
        }),
        player,
        move |outcome| {
            let _ = finished.send(outcome);
        },
    )
    .unwrap();
    Started {
        run,
        outcome,
        logs,
        started,
    }
}

fn run_in(folder: &Folder, entry: &str, limits: Limits, stop_after: Option<Duration>) -> Finished {
    let started = start(folder, entry, limits, Box::new(|_| {}));
    if let Some(delay) = stop_after {
        std::thread::sleep(delay);
        started.run.stop();
    }
    started.finish()
}

#[derive(Debug, PartialEq)]
enum Seen {
    Call(u64, Request),
    Cancel(u64),
    Ended,
}

#[derive(Clone, Default)]
struct FakePlayer {
    seen: Arc<Mutex<Vec<(RunId, Seen)>>>,
    held: Arc<Mutex<Vec<Call>>>,
}

impl FakePlayer {
    fn answering(
        &self,
        answer: impl Fn(&Request, &mut Vec<Call>) -> Option<Reply> + Send + 'static,
    ) -> Player {
        let fake = self.clone();
        Box::new(move |message| {
            let (run, seen) = match &message {
                ToPlayer::Call(call) => (call.run, Seen::Call(call.id.0, call.request.clone())),
                ToPlayer::Cancel { run, id } => (*run, Seen::Cancel(id.0)),
                ToPlayer::Ended(run) => (*run, Seen::Ended),
            };
            fake.seen.lock().unwrap().push((run, seen));
            if let ToPlayer::Call(call) = message {
                let mut held = fake.held.lock().unwrap();
                match answer(&call.request, &mut held) {
                    Some(reply) => call.reply.send(reply),
                    None => held.push(call),
                }
            }
        })
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .drain(..)
            .map(|(_, seen)| seen)
            .collect()
    }
}

fn run(files: &[(&str, &str)]) -> Finished {
    let folder = Folder::new(files);
    run_in(&folder, files[0].0, Limits::default(), None)
}

fn failure(outcome: &Outcome) -> (&str, &str) {
    match outcome {
        Outcome::Failed { message, stack } => (message, stack.as_deref().unwrap_or("")),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_typescript_generator_runs_with_its_imports() {
    let finished = run(&[
        (
            "main.ts",
            r#"import { Effect } from "effect";
import type { Name } from "./greet.ts";
import { greet } from "./greet.ts";

const name: Name = "player1";

export default function* () {
  yield* Effect.sleep("20 millis");
  console.log(greet(name));
}
"#,
        ),
        (
            "greet.ts",
            "export type Name = string;\nexport const greet = (name: Name): string => `hello ${name}`;\n",
        ),
    ]);
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["hello player1"]);
}

#[test]
fn plain_javascript_modules_run_as_written() {
    let finished = run(&[
        (
            "main.mjs",
            "import { Effect } from \"effect\";\nimport { twice } from \"./util.js\";\nexport default Effect.sync(() => console.log(twice(21)));\n",
        ),
        ("util.js", "export const twice = (value) => value * 2;\n"),
    ]);
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["42"]);
}

#[test]
fn an_import_used_only_for_its_side_effect_still_runs() {
    let finished = run(&[
        (
            "main.ts",
            "import { value } from \"./side.ts\";\nexport default function* () {}\n",
        ),
        (
            "side.ts",
            "console.log(\"side effect\");\nexport const value: number = 1;\n",
        ),
    ]);
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["side effect"]);
}

#[test]
fn a_failure_reports_the_typescript_line_that_threw() {
    let finished = run(&[(
        "main.ts",
        "type Quantity = number;\nconst needed: Quantity = 10;\n\nexport default function* () {\n  throw new Error(`need ${needed}`);\n}\n",
    )]);
    let (message, stack) = failure(&finished.outcome);
    assert_eq!(message, "need 10");
    assert!(stack.contains("main.ts:5:"), "stack was {stack}");
}

#[test]
fn a_syntax_error_names_the_file_and_position() {
    let finished = run(&[(
        "main.ts",
        "export default function* () {}\nconst broken: = 1;\n",
    )]);
    let (message, _) = failure(&finished.outcome);
    assert!(message.contains("main.ts:2:"), "message was {message}");
}

#[test]
fn the_default_export_must_be_runnable() {
    let finished = run(&[("main.js", "export default 42;\n")]);
    let (message, _) = failure(&finished.outcome);
    assert!(message.contains("generator function or an Effect"));
}

#[test]
fn scripts_can_only_import_effect_and_their_own_folder() {
    let folder = Folder::new(&[
        ("outside.ts", "export const secret = 1;\n"),
        ("script/inside.ts", "export const ok = 1;\n"),
        (
            "script/parent.ts",
            "import \"../outside.ts\";\nexport default function* () {}\n",
        ),
        (
            "script/package.ts",
            "import \"lodash\";\nexport default function* () {}\n",
        ),
        (
            "script/internal.ts",
            "import \"internal:/lyra/prelude.js\";\nexport default function* () {}\n",
        ),
        (
            "script/platform.ts",
            "import \"effect/http\";\nexport default function* () {}\n",
        ),
        (
            "script/guessed.ts",
            "import \"./inside\";\nexport default function* () {}\n",
        ),
        (
            "script/module.ts",
            "import * as Schema from \"effect/Schema\";\nexport default function* () { console.log(typeof Schema.decodeUnknownSync); }\n",
        ),
    ]);
    for entry in [
        "script/parent.ts",
        "script/package.ts",
        "script/internal.ts",
        "script/platform.ts",
        "script/guessed.ts",
    ] {
        let finished = run_in(&folder, entry, Limits::default(), None);
        assert!(
            matches!(finished.outcome, Outcome::Failed { .. }),
            "{entry} should fail to import, got {:?}",
            finished.outcome
        );
    }
    let finished = run_in(&folder, "script/module.ts", Limits::default(), None);
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["function"]);
}

#[test]
fn every_run_starts_with_fresh_globals() {
    let folder = Folder::new(&[
        (
            "leak.js",
            "globalThis.leaked = 1;\nexport default function* () {}\n",
        ),
        (
            "check.js",
            "export default function* () { if (typeof leaked !== \"undefined\") throw new Error(\"leaked\"); }\n",
        ),
    ]);
    assert_eq!(
        run_in(&folder, "leak.js", Limits::default(), None).outcome,
        Outcome::Completed
    );
    assert_eq!(
        run_in(&folder, "check.js", Limits::default(), None).outcome,
        Outcome::Completed
    );
}

#[test]
fn stopping_runs_effect_finalizers() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Effect } from "effect";
export default function* () {
  yield* Effect.never.pipe(Effect.ensuring(Effect.sync(() => console.log("cleaned up"))));
}
"#,
    )]);
    let finished = run_in(
        &folder,
        "main.ts",
        Limits::default(),
        Some(Duration::from_millis(200)),
    );
    assert_eq!(finished.outcome, Outcome::Stopped);
    assert_eq!(finished.logs, ["cleaned up"]);
}

#[test]
fn stopping_interrupts_a_loop_that_never_yields() {
    for script in [
        "export default function* () { while (true) {} }\n",
        "while (true) {}\nexport default function* () {}\n",
    ] {
        let folder = Folder::new(&[("main.js", script)]);
        let finished = run_in(
            &folder,
            "main.js",
            Limits::default(),
            Some(Duration::from_millis(200)),
        );
        assert_eq!(finished.outcome, Outcome::Stopped, "{script}");
        assert!(
            finished.elapsed < Duration::from_millis(1500),
            "took {:?}",
            finished.elapsed
        );
    }
}

#[test]
fn a_turn_that_never_yields_is_killed_at_the_limit() {
    let folder = Folder::new(&[(
        "main.js",
        "export default function* () { while (true) {} }\n",
    )]);
    let limits = Limits {
        turn: Duration::from_millis(300),
        ..Limits::default()
    };
    let finished = run_in(&folder, "main.js", limits, None);
    assert_eq!(finished.outcome, Outcome::Killed(Limit::Turn));
    assert!(finished.elapsed >= Duration::from_millis(300));
}

#[test]
fn a_script_that_waits_is_not_a_hang() {
    let folder = Folder::new(&[(
        "main.ts",
        "import { Effect } from \"effect\";\nexport default function* () { yield* Effect.sleep(\"600 millis\"); }\n",
    )]);
    let limits = Limits {
        turn: Duration::from_millis(300),
        ..Limits::default()
    };
    assert_eq!(
        run_in(&folder, "main.ts", limits, None).outcome,
        Outcome::Completed
    );
}

#[test]
fn running_out_of_memory_ends_the_run() {
    let folder = Folder::new(&[(
        "main.js",
        "export default function* () { const kept = []; while (true) kept.push(new Array(100000).fill(1)); }\n",
    )]);
    let limits = Limits {
        memory: 32 << 20,
        ..Limits::default()
    };
    assert_eq!(
        run_in(&folder, "main.js", limits, None).outcome,
        Outcome::Killed(Limit::Memory)
    );
}

#[test]
fn an_error_thrown_by_a_timer_ends_the_run() {
    let folder = Folder::new(&[
        (
            "throws.ts",
            "import { Effect } from \"effect\";\nsetTimeout(() => { throw new Error(\"timer boom\"); }, 10);\nexport default function* () { yield* Effect.never; }\n",
        ),
        (
            "null.js",
            "import { Effect } from \"effect\";\nsetTimeout(() => { throw null; }, 10);\nexport default function* () { yield* Effect.never; }\n",
        ),
        (
            "exhausts.js",
            "import { Effect } from \"effect\";\nsetTimeout(() => { const kept = []; while (true) kept.push(new Array(100000).fill(1)); }, 10);\nexport default function* () { yield* Effect.never; }\n",
        ),
    ]);
    let thrown = run_in(&folder, "throws.ts", Limits::default(), None);
    assert_eq!(failure(&thrown.outcome).0, "timer boom");
    assert!(matches!(
        run_in(&folder, "null.js", Limits::default(), None).outcome,
        Outcome::Failed { .. }
    ));
    let limits = Limits {
        memory: 32 << 20,
        ..Limits::default()
    };
    assert_eq!(
        run_in(&folder, "exhausts.js", limits, None).outcome,
        Outcome::Killed(Limit::Memory)
    );
}

#[test]
fn an_endless_promise_chain_is_killed_at_the_turn_limit() {
    let folder = Folder::new(&[(
        "main.js",
        "import { Effect } from \"effect\";\nconst again = () => {\n  Promise.resolve().then(again);\n};\nagain();\nexport default function* () { yield* Effect.never; }\n",
    )]);
    let limits = Limits {
        turn: Duration::from_millis(300),
        ..Limits::default()
    };
    let finished = run_in(&folder, "main.js", limits, None);
    assert_eq!(finished.outcome, Outcome::Killed(Limit::Turn));
    assert!(finished.elapsed < Duration::from_secs(5));
}

#[test]
fn a_script_drives_the_game_through_the_lyra_module() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Player } from "lyra";

export default function* () {
  yield* Player.joinMap("battleon", { cell: "r2" });
  console.log(`${yield* Player.getMap()} ${yield* Player.getHp()} ${yield* Player.getState()}`);
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|request, _| {
            Some(Ok(match request {
                Request::Player(P::JoinMap { .. }) => json!(true),
                Request::Player(P::Map) => json!("battleon"),
                Request::Player(P::Hp) => json!(1200),
                Request::Player(P::State) => json!("idle"),
                other => panic!("unexpected {other:?}"),
            }))
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["battleon 1200 idle"]);
    assert_eq!(
        player.seen(),
        [
            Seen::Call(
                1,
                Request::Player(P::JoinMap {
                    map: "battleon".into(),
                    cell: Some("r2".into()),
                    pad: None
                })
            ),
            Seen::Call(2, Request::Player(P::Map)),
            Seen::Call(3, Request::Player(P::Hp)),
            Seen::Call(4, Request::Player(P::State)),
            Seen::Ended,
        ]
    );
}

#[test]
fn a_failed_loader_call_ends_the_script_with_its_message() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Player } from "lyra";

export default function* () {
  console.log(String(yield* Player.jumpToCell("r2")));
  yield* Player.getHp();
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|request, _| {
            Some(match request {
                Request::Player(P::JumpToCell { .. }) => Ok(json!(false)),
                _ => Err(CallError::Threw {
                    callback: "player.getHp",
                    message: "boom".into(),
                }),
            })
        }),
    )
    .finish();
    assert_eq!(finished.logs, ["false"]);
    assert_eq!(failure(&finished.outcome).0, "player.getHp threw: boom");
}

#[test]
fn interrupting_a_fiber_cancels_its_pending_call() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Effect } from "effect";
import { Player } from "lyra";

export default function* () {
  const result = yield* Player.joinMap("yulgar").pipe(Effect.timeoutOption("50 millis"));
  console.log(result._tag);
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| None),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["None"]);
    assert_eq!(
        player.seen(),
        [
            Seen::Call(
                1,
                Request::Player(P::JoinMap {
                    map: "yulgar".into(),
                    cell: None,
                    pad: None
                })
            ),
            Seen::Cancel(1),
            Seen::Ended,
        ]
    );
}

#[test]
fn a_reply_after_a_run_stops_never_reaches_the_next_run() {
    let folder = Folder::new(&[
        (
            "travel.ts",
            "import { Player } from \"lyra\";\nexport default function* () { yield* Player.joinMap(\"yulgar\"); }\n",
        ),
        (
            "read.ts",
            "import { Player } from \"lyra\";\nexport default function* () { console.log(yield* Player.getMap()); }\n",
        ),
    ]);
    let player = FakePlayer::default();
    let travelling = start(
        &folder,
        "travel.ts",
        Limits::default(),
        player.answering(|_, _| None),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while player.held.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the join reached the player");
        std::thread::sleep(Duration::from_millis(5));
    }
    travelling.run.stop();
    assert_eq!(travelling.finish().outcome, Outcome::Stopped);
    assert_eq!(player.seen().last(), Some(&Seen::Ended));
    let reading = start(
        &folder,
        "read.ts",
        Limits::default(),
        player.answering(|_, held| {
            for late in held.drain(..) {
                late.reply.send(Ok(json!("late")));
            }
            Some(Ok(json!("battleon")))
        }),
    )
    .finish();
    assert_eq!(reading.outcome, Outcome::Completed);
    assert_eq!(reading.logs, ["battleon"]);
}
