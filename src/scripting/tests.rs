use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use serde_json::json;

use super::{
    Launch, Level, Limit, Limits, Outcome, Run, Supplied, ToPlayer, Values,
    auth::Request as A,
    automation::{ReloginRequest as AR, SettingsRequest as GS},
    bank::Request as B,
    calls::{Call, Reply, Request, RunId},
    combat::Request as C,
    drops::Request as D,
    environment::{BoostRequest as BR, DropRequest as DR, QuestRequest as QR},
    events::Request as EV,
    feeds::{FeedId, Request as F},
    house::Request as H,
    inventory::{Request as I, TempRequest as T},
    packets::Request as PK,
    player::Request as P,
    players::Request as PL,
    quests::Request as Q,
    runtime::Player,
    shops::Request as S,
    world::Request as W,
};
use crate::{
    bridge::{CallError, packets::Sender},
    domain::events::Kind,
};

struct Folder(PathBuf);

impl Folder {
    fn new(files: &[(&str, &str)]) -> Self {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let root =
            std::env::temp_dir().join(format!("mira-script-{:x}", u64::from_ne_bytes(random)));
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
    start_with(
        folder,
        entry,
        Supplied::Values(Values::new()),
        limits,
        player,
    )
}

fn start_with(
    folder: &Folder,
    entry: &str,
    inputs: Supplied,
    limits: Limits,
    player: Player,
) -> Started {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let sink = logs.clone();
    let (finished, outcome) = mpsc::channel();
    let started = Instant::now();
    let run = Run::start(
        Launch {
            entry: folder.0.join(entry),
            inputs,
        },
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

fn run_with_inputs(files: &[(&str, &str)], inputs: Supplied) -> Finished {
    let folder = Folder::new(files);
    start_with(
        &folder,
        files[0].0,
        inputs,
        Limits::default(),
        Box::new(|_| {}),
    )
    .finish()
}

fn supplied(values: serde_json::Value) -> Supplied {
    Supplied::Values(values.as_object().unwrap().clone())
}

const HUNT_INPUTS: &str = r#"import { Script, type Inputs } from "mira";
import { describe } from "./describe.ts";

export const inputs = {
  monster: { type: "string", label: "Monster", required: true, default: "Frogzard" },
  kills: { type: "number", label: "Kills", integer: true, min: 1 },
  rate: { type: "number", label: "Rate", default: 0.5 },
  potions: { type: "boolean", label: "Potions" },
  daily: { type: "select", label: "Daily", options: ["All", "Class"], default: "All" },
  buy: { type: "select", label: "Buy", options: ["A", "B", "C"], multiple: true },
} as const satisfies Inputs;

console.log(`top level ${describe()}`);

export default function* () {
  const values = Script.inputs<typeof inputs>();
  console.log(`run ${values.monster} ${values.kills} ${values.rate} ${values.potions} ${values.daily} ${values.buy.join("+")}`);
}
"#;

const DESCRIBE: &str = r#"import { Script } from "mira";

export const describe = (): string => JSON.stringify(Script.inputs());
"#;

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
fn a_script_reads_the_inputs_it_was_launched_with() {
    let finished = run_with_inputs(
        &[("hunt.ts", HUNT_INPUTS), ("describe.ts", DESCRIBE)],
        supplied(json!({ "kills": 12, "buy": ["C", "A"], "removed": true })),
    );
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(
        finished.logs,
        [
            r#"top level {"monster":"Frogzard","kills":12,"rate":0.5,"potions":false,"daily":"All","buy":["A","C"]}"#,
            "run Frogzard 12 0.5 false All A+C",
        ]
    );
}

#[test]
fn an_effect_default_export_reads_inputs_too() {
    let finished = run_with_inputs(
        &[(
            "main.ts",
            r#"import { Effect } from "effect";
import { Script } from "mira";

export const inputs = { target: { type: "string", label: "Target" } };

export default Effect.sync(() => console.log(`target ${Script.inputs().target}`));
"#,
        )],
        supplied(json!({ "target": "Dummy" })),
    );
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["target Dummy"]);
}

#[test]
fn inputs_cannot_be_changed_by_the_script() {
    let finished = run_with_inputs(
        &[(
            "main.ts",
            r#"import { Script } from "mira";

export const inputs = {
  kills: { type: "number", label: "Kills", default: 5 },
  buy: { type: "select", label: "Buy", options: ["A", "B"], multiple: true, default: ["A"] },
};

export default function* () {
  const values = Script.inputs() as { kills: number; buy: string[] };
  for (const change of [() => { values.kills = 1; }, () => { values.buy.push("B"); }]) {
    try {
      change();
    } catch (error) {
      console.log(String(error));
    }
  }
  console.log(`${values.kills} ${values.buy.join("+")}`);
}
"#,
        )],
        supplied(json!({})),
    );
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs.len(), 3, "{:?}", finished.logs);
    assert!(
        finished.logs[..2]
            .iter()
            .all(|log| log.starts_with("TypeError"))
    );
    assert_eq!(finished.logs[2], "5 A");
}

#[test]
fn missing_or_invalid_inputs_stop_the_script_before_any_of_it_runs() {
    let files = [("hunt.ts", HUNT_INPUTS), ("describe.ts", DESCRIBE)];
    let missing = run_with_inputs(&files, supplied(json!({})));
    assert_eq!(
        failure(&missing.outcome).0,
        "hunt.ts needs inputs: Kills. Open Scripts to fill them in."
    );
    assert!(missing.logs.is_empty(), "{:?}", missing.logs);

    let invalid = run_with_inputs(&files, supplied(json!({ "kills": 0, "daily": "Gold" })));
    assert_eq!(
        failure(&invalid.outcome).0,
        r#"hunt.ts: Kills must be at least 1 (got 0); Daily isn't one of the options (got "Gold"). Open Scripts to review its inputs."#
    );
    assert!(invalid.logs.is_empty(), "{:?}", invalid.logs);

    let broken = run_with_inputs(
        &[(
            "hunt.ts",
            "console.log(\"ran\");\nexport const inputs = { kills: { type: \"number\", label: \"Kills\", min: 1, default: 0 } };\nexport default function* () {}\n",
        )],
        supplied(json!({})),
    );
    assert_eq!(
        failure(&broken.outcome).0,
        "hunt.ts:2:83: inputs.kills.default must be at least 1"
    );
    assert!(broken.logs.is_empty(), "{:?}", broken.logs);
}

#[test]
fn unreadable_saved_inputs_only_stop_scripts_that_declare_inputs() {
    let declared = run_with_inputs(
        &[("hunt.ts", HUNT_INPUTS), ("describe.ts", DESCRIBE)],
        Supplied::StoreUnreadable,
    );
    assert_eq!(
        failure(&declared.outcome).0,
        "hunt.ts didn't start: saved script inputs couldn't be read. Open Scripts to recover them."
    );
    let plain = run_with_inputs(
        &[(
            "main.ts",
            "import { Script } from \"mira\";\nexport default function* () { console.log(JSON.stringify(Script.inputs())); }\n",
        )],
        Supplied::StoreUnreadable,
    );
    assert_eq!(plain.outcome, Outcome::Completed);
    assert_eq!(plain.logs, ["{}"]);
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
            "import \"internal:/mira/prelude.js\";\nexport default function* () {}\n",
        ),
        (
            "script/runtime.ts",
            "import \"mira/prelude\";\nexport default function* () {}\n",
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
        "script/runtime.ts",
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
fn effect_promises_get_a_signal_that_aborts_when_interrupted() {
    let finished = run(&[(
        "main.ts",
        r#"import { Effect } from "effect";

export default function* () {
  const aborted = yield* Effect.promise((signal) => Promise.resolve(signal.aborted));
  const timedOut = yield* Effect.promise(
    (signal) => new Promise<void>(() => signal.addEventListener("abort", () => console.log("aborted"))),
  ).pipe(Effect.timeoutOption("20 millis"));
  console.log(aborted, timedOut._tag);
}
"#,
    )]);
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["aborted", "false None"]);
}

#[test]
fn an_abort_listener_removed_during_abort_does_not_run() {
    let finished = run(&[(
        "main.js",
        r#"export default function* () {
  const controller = new AbortController();
  const later = () => console.log("removed listener ran");
  controller.signal.addEventListener("abort", () => controller.signal.removeEventListener("abort", later));
  controller.signal.addEventListener("abort", later);
  controller.signal.onabort = () =>
    controller.signal.addEventListener("abort", () => console.log("added listener ran"));
  controller.abort();
  console.log(`aborted ${controller.signal.aborted}`);
}
"#,
    )]);
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["aborted true"]);
}

#[test]
fn stopping_aborts_the_script_signal_before_cleanup() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Effect } from "effect";
import { Script } from "mira";

export default function* () {
  console.log(`running, aborted ${Script.signal.aborted}`);
  Script.signal.addEventListener("abort", () => {
    throw new Error("listener boom");
  });
  Script.signal.addEventListener("abort", () => console.log("abort event"));
  yield* Effect.never.pipe(
    Effect.ensuring(Effect.sync(() => console.log(`cleanup, aborted ${Script.signal.aborted}`))),
  );
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
    let [running, thrown, event, cleanup] = finished.logs.as_slice() else {
        panic!("logs were {:?}", finished.logs);
    };
    assert_eq!(running, "running, aborted false");
    assert!(
        thrown.starts_with("error: Error: listener boom"),
        "{thrown}"
    );
    assert_eq!(event, "abort event");
    assert_eq!(cleanup, "cleanup, aborted true");
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
fn exiting_runs_cleanup_and_cannot_be_caught() {
    let finished = run(&[(
        "main.ts",
        r#"import { Effect } from "effect";
import { Script } from "mira";

export default function* () {
  yield* Script.exit().pipe(
    Effect.catchCause(() => Effect.sync(() => console.log("caught"))),
    Effect.ensuring(Effect.sync(() => console.log(`cleanup, aborted ${Script.signal.aborted}`))),
  );
  console.log("after exit");
}
"#,
    )]);
    assert_eq!(finished.outcome, Outcome::Exited { close_tab: false });
    assert_eq!(finished.logs, ["cleanup, aborted true"]);
}

#[test]
fn exiting_from_a_fork_logs_out_after_cleanup() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Effect } from "effect";
import { Auth, Script } from "mira";

export default function* () {
  yield* Effect.forkChild(
    Effect.andThen(Effect.sleep("10 millis"), Script.exit({ logout: true, closeTab: true })),
  );
  yield* Effect.never.pipe(Effect.ensuring(Auth.isLoggedIn()));
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| Some(Ok(json!(true)))),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Exited { close_tab: true });
    assert!(
        finished.elapsed >= Duration::from_secs(1),
        "{:?}",
        finished.elapsed
    );
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    assert_eq!(requests, [A::LoggedIn, A::Logout].map(Request::Auth));
}

#[test]
fn exiting_wins_over_a_root_the_abort_ends() {
    for resumed in ["Effect.void", r#"Effect.die(new Error("aborted"))"#] {
        let folder = Folder::new(&[(
            "main.ts",
            &format!(
                r#"import {{ Effect }} from "effect";
import {{ Script }} from "mira";

export default function* () {{
  yield* Effect.forkChild(
    Effect.andThen(Effect.sleep("10 millis"), Script.exit({{ logout: true, closeTab: true }})),
  );
  yield* Effect.callback<void>((resume) => {{
    Script.signal.addEventListener("abort", () => resume({resumed}));
  }});
}}
"#
            ),
        )]);
        let player = FakePlayer::default();
        let finished = start(
            &folder,
            "main.ts",
            Limits::default(),
            player.answering(|_, _| Some(Ok(json!(null)))),
        )
        .finish();
        assert_eq!(
            finished.outcome,
            Outcome::Exited { close_tab: true },
            "{resumed}"
        );
        assert!(
            player
                .seen()
                .contains(&Seen::Call(1, Request::Auth(A::Logout))),
            "{resumed} logged out"
        );
    }
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
fn waiting_until_a_condition_holds_checks_it_right_away_and_then_on_each_interval() {
    let finished = run(&[(
        "main.ts",
        r#"import { Effect } from "effect";
import { Wait } from "mira";

export default function* () {
  const already = yield* Wait.until(Effect.succeed(true), { interval: "1 hour" });
  let checks = 0;
  const met = yield* Wait.until(Effect.sync(() => ++checks === 3), { interval: "10 millis" });
  const timedOut = yield* Wait.until(Effect.succeed(false), { interval: "10 millis", timeout: "50 millis" });
  console.log(already, met, checks, timedOut);
}
"#,
    )]);
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["true true 3 false"]);
    assert!(finished.elapsed >= Duration::from_millis(70));
}

#[test]
fn a_bank_batch_moves_one_item_at_a_time_like_the_game() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Bank } from "mira";

export default function* () {
  console.log(JSON.stringify(yield* Bank.depositBatch(["Gem", "Ore", { itemId: 3 }])));
}
"#,
    )]);
    let player = FakePlayer::default();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let sends = sent.clone();
    let started = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(move |request, _| match request {
            Request::Bank(B::Deposit { query }) => {
                let mut sent = sends.lock().unwrap();
                sent.push(Instant::now());
                (sent.len() > 1).then(|| {
                    Ok(json!(
                        *query != crate::domain::inventory::ItemQuery::Name("Ore".into())
                    ))
                })
            }
            _ => Some(Ok(json!(null))),
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while player.held.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the first deposit never arrived");
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(300));
    let first = player.held.lock().unwrap().pop().unwrap();
    let answered = Instant::now();
    first.reply.send(Ok(json!(true)));
    let finished = started.finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["[true,false,true]"]);
    let sent = sent.lock().unwrap();
    let [_, second, third] = sent.as_slice() else {
        panic!("deposits were sent {} times", sent.len());
    };
    assert!(*second >= answered + Duration::from_millis(150));
    assert!(*third >= *second + Duration::from_millis(150));
}

fn batch(items: serde_json::Value) -> Option<Reply> {
    Some(Ok(json!({ "items": items, "lagged": false })))
}

fn server(packet: serde_json::Value) -> serde_json::Value {
    json!({ "from": "server", "packet": packet.to_string() })
}

#[test]
fn waiting_for_a_packet_subscribes_before_the_trigger_and_returns_the_first_match() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Packets, Wait } from "mira";

export default function* () {
  const reply = yield* Wait.forPacket(
    { from: "server", cmd: "loadShop" },
    { trigger: Packets.sendToServer("%xt%zm%loadShop%1%5%"), timeout: "5 seconds" },
  );
  console.log(JSON.stringify(reply));
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|request, _| match request {
            Request::Feeds(F::Next { .. }) => batch(json!([
                server(json!({ "type": "json", "dataObj": { "cmd": "loadShops" } })),
                server(json!({ "type": "json", "dataObj": { "cmd": "loadShop", "shopId": 5 } })),
            ])),
            Request::Packets(PK::SendToServer { .. }) => Some(Ok(json!(true))),
            _ => Some(Ok(json!(null))),
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(
        finished.logs,
        [
            r#"{"from":"server","format":"json","cmd":"loadShop","data":{"cmd":"loadShop","shopId":5}}"#
        ]
    );
    assert_eq!(
        player.seen(),
        [
            Seen::Call(
                1,
                Request::Packets(PK::Subscribe {
                    id: FeedId(1),
                    from: Some(Sender::Server),
                    cmds: Some(vec!["loadShop".into()]),
                })
            ),
            Seen::Call(
                2,
                Request::Packets(PK::SendToServer {
                    packet: "%xt%zm%loadShop%1%5%".into()
                })
            ),
            Seen::Call(3, Request::Feeds(F::Next { id: FeedId(1) })),
            Seen::Call(4, Request::Feeds(F::Close { id: FeedId(1) })),
            Seen::Ended,
        ]
    );
}

#[test]
fn a_packet_wait_ends_empty_when_the_connection_drops_the_time_runs_out_or_the_trigger_fails() {
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Effect } from "effect";
import { Wait } from "mira";

export default function* () {
  const client = yield* Wait.forPacket({ from: "client", cmd: "afk" });
  const str = yield* Wait.forPacket({ from: "server", cmd: "respawnMon" });
  const dropped = yield* Wait.forPacket({ cmd: "ct" });
  const late = yield* Wait.forPacket({ cmd: "ct" }, { timeout: "30 millis" });
  const skipped = yield* Wait.forPacket({ cmd: "ct" }, { trigger: Effect.succeed(false) });
  console.log(JSON.stringify([client, str, dropped, late, skipped]));
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        FakePlayer::default().answering(|request, _| match request {
            Request::Feeds(F::Next { id: FeedId(1) }) => batch(json!([
                { "from": "client", "packet": "[Sending - STR]: %xt%zm%afk%1%true%, (len: 24)" },
            ])),
            Request::Feeds(F::Next { id: FeedId(2) }) => batch(json!([server(
                json!({ "type": "str", "dataObj": ["respawnMon", "-1", "2,3"] })
            ),])),
            Request::Feeds(F::Next { id: FeedId(3) }) => batch(json!([{ "lost": true }])),
            Request::Feeds(F::Next { .. }) => None,
            _ => Some(Ok(json!(null))),
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(
        finished.logs,
        [
            r#"[{"from":"client","cmd":"afk","args":["true"]},{"from":"server","format":"str","cmd":"respawnMon","args":["2,3"]},null,null,null]"#
        ]
    );
}

#[test]
fn a_failing_packet_handler_fails_the_script() {
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Effect } from "effect";
import { Packets } from "mira";

export default function* () {
  yield* Packets.on({ from: "server", cmd: "ct" }, function* (packet) {
    console.log(packet.cmd);
    if (packet.format === "json" && packet.data["boom"]) {
      yield* Effect.fail(new Error("handler boom"));
    }
  });
  yield* Effect.never;
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        FakePlayer::default().answering(|request, _| match request {
            Request::Feeds(F::Next { .. }) => Some(Ok(json!({ "lagged": true, "items": [
                server(json!({ "type": "json", "dataObj": { "cmd": "ct" } })),
                server(json!({ "type": "json", "dataObj": { "cmd": "ct", "boom": true } })),
            ] }))),
            _ => Some(Ok(json!(null))),
        }),
    )
    .finish();
    assert_eq!(failure(&finished.outcome).0, "handler boom");
    assert_eq!(
        finished.logs,
        [
            "A subscription fell behind and dropped what it was holding.",
            "ct",
            "ct"
        ]
    );
}

#[test]
fn a_subscription_keeps_packets_that_arrive_after_a_wait_on_it_gives_up() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Effect } from "effect";
import { Packets } from "mira";

export default function* () {
  const subscription = yield* Packets.subscribe({ cmd: "ct" });
  console.log((yield* Effect.timeoutOption(subscription.next, "30 millis"))._tag);
  console.log((yield* subscription.next).cmd);
}
"#,
    )]);
    let player = FakePlayer::default();
    let started = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|request, _| match request {
            Request::Feeds(F::Next { .. }) => None,
            _ => Some(Ok(json!(null))),
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while started.logs.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the first wait gave up");
        std::thread::sleep(Duration::from_millis(5));
    }
    for held in player.held.lock().unwrap().drain(..) {
        held.reply.send(
            batch(json!([server(
                json!({ "type": "json", "dataObj": { "cmd": "ct" } })
            )]))
            .unwrap(),
        );
    }
    let finished = started.finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["None", "ct"]);
    let nexts = player
        .seen()
        .into_iter()
        .filter(|seen| matches!(seen, Seen::Call(_, Request::Feeds(F::Next { .. }))))
        .count();
    assert_eq!(nexts, 1);
}

#[test]
fn waiting_for_an_event_returns_the_first_one_whose_fields_match() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Packets, Wait } from "mira";

export default function* () {
  const death = yield* Wait.forEvent(
    { type: "monsterDeath", monMapId: 4 },
    { trigger: Packets.sendToServer("%xt%zm%aggroMon%1%4%") },
  );
  console.log(JSON.stringify(death));
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|request, _| match request {
            Request::Feeds(F::Next { .. }) => batch(json!([
                { "type": "monsterDeath", "monMapId": 3 },
                { "type": "monsterDeath", "monMapId": 4 },
            ])),
            Request::Packets(PK::SendToServer { .. }) => Some(Ok(json!(true))),
            _ => Some(Ok(json!(null))),
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, [r#"{"type":"monsterDeath","monMapId":4}"#]);
    assert_eq!(
        player.seen()[..2],
        [
            Seen::Call(
                1,
                Request::Events(EV::Subscribe {
                    id: FeedId(1),
                    kind: Kind::MonsterDeath,
                })
            ),
            Seen::Call(
                2,
                Request::Packets(PK::SendToServer {
                    packet: "%xt%zm%aggroMon%1%4%".into()
                })
            ),
        ]
    );
}

#[test]
fn event_handlers_run_until_the_script_returns() {
    let player = FakePlayer::default();
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Effect } from "effect";
import { Events, Packets } from "mira";

export default function* () {
  yield* Events.on({ type: "afk", self: true, afk: true }, () =>
    Packets.sendToServer("%xt%zm%afk%1%false%"),
  );
  yield* Effect.sleep("50 millis");
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        player.answering({
            let delivered = std::sync::atomic::AtomicBool::new(false);
            move |request, _| match request {
                Request::Feeds(F::Next { .. })
                    if !delivered.swap(true, std::sync::atomic::Ordering::SeqCst) =>
                {
                    batch(json!([
                        { "type": "afk", "username": "player1", "self": true, "afk": false },
                        { "type": "afk", "username": "player2", "self": false, "afk": true },
                        { "type": "afk", "username": "player1", "self": true, "afk": true },
                    ]))
                }
                Request::Feeds(F::Next { .. }) => None,
                Request::Packets(PK::SendToServer { .. }) => Some(Ok(json!(true))),
                _ => Some(Ok(json!(null))),
            }
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    let seen = player.seen();
    let sent: Vec<_> = seen
        .iter()
        .filter(|seen| {
            matches!(
                seen,
                Seen::Call(_, Request::Packets(PK::SendToServer { .. }))
            )
        })
        .collect();
    assert_eq!(
        sent,
        [&Seen::Call(
            3,
            Request::Packets(PK::SendToServer {
                packet: "%xt%zm%afk%1%false%".into()
            })
        )]
    );
    assert!(
        seen.iter()
            .any(|seen| matches!(seen, Seen::Call(_, Request::Feeds(F::Close { .. })))),
        "{seen:?}"
    );
}

#[test]
fn a_script_drives_the_game_through_the_mira_module() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Player } from "mira";

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
        r#"import { Player } from "mira";

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
    let (message, stack) = failure(&finished.outcome);
    assert_eq!(message, "player.getHp threw: boom");
    let main = fs::canonicalize(folder.0.join("main.ts")).unwrap();
    assert_eq!(stack, format!("    at default ({}:5:17)", main.display()));
}

#[test]
fn a_failed_call_with_no_script_frame_keeps_the_runtime_stack() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Army } from "mira";

export default function* () {
  yield* Army.joinMap("bossmap");
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| {
            Some(Err(CallError::Threw {
                callback: "army.sync",
                message: "boom".into(),
            }))
        }),
    )
    .finish();
    let (message, stack) = failure(&finished.outcome);
    assert_eq!(message, "army.sync threw: boom");
    assert!(stack.contains("internal:/"), "stack was {stack}");
}

#[test]
fn interrupting_a_fiber_cancels_its_pending_call() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Effect } from "effect";
import { Player } from "mira";

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
            "import { Player } from \"mira\";\nexport default function* () { yield* Player.joinMap(\"yulgar\"); }\n",
        ),
        (
            "read.ts",
            "import { Player } from \"mira\";\nexport default function* () { console.log(yield* Player.getMap()); }\n",
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

#[test]
fn auth_requests_reach_the_player_typed() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Auth } from "mira";

export default function* () {
  yield* Auth.isLoggedIn();
  yield* Auth.getUsername();
  yield* Auth.getServer();
  yield* Auth.getServers();
  yield* Auth.logout();
  yield* Auth.login();
  yield* Auth.connect("Twilly");
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| Some(Ok(json!(null)))),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    assert_eq!(
        requests,
        [
            A::LoggedIn,
            A::Username,
            A::Server,
            A::Servers,
            A::Logout,
            A::Login,
            A::Connect {
                server: "Twilly".into()
            },
        ]
        .map(Request::Auth)
    );
}

#[test]
fn automation_requests_reach_the_player_typed() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Settings } from "mira";
import * as AutoRelogin from "mira/autorelogin";

export default function* () {
  yield* AutoRelogin.get();
  yield* AutoRelogin.update({ enabled: true, server: null, delaySeconds: 10 });
  yield* Settings.get();
  yield* Settings.update({ hidePlayers: true, walkSpeed: 12, customGuild: null, antiCounter: false, autoZone: true });
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| Some(Ok(json!(null)))),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    assert_eq!(
        requests,
        [
            Request::AutoRelogin(AR::Get),
            Request::AutoRelogin(AR::Update {
                enabled: Some(true),
                server: Some(None),
                delay_seconds: Some(10),
                retries: None,
                backoff_seconds: None,
                backoff_cap_seconds: None,
            }),
            Request::Settings(GS::Get),
            Request::Settings(GS::Update {
                infinite_range: None,
                provoke_cell: None,
                enemy_magnet: None,
                skip_cutscenes: None,
                hide_players: Some(true),
                rendering: None,
                animations: None,
                collisions: None,
                ads: None,
                walk_speed: Some(12),
                frame_rate: None,
                custom_name: None,
                custom_guild: Some(None),
                anti_counter: Some(false),
                auto_zone: Some(true),
            }),
        ]
    );
}

#[test]
fn environment_requests_reach_the_player_typed() {
    use crate::{automation::quests::Registered, domain::inventory::ItemQuery};

    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Drops, Player, Quests } from "mira";

export default function* () {
  yield* Quests.register([4432, { questId: 4433, rewardItemId: 50 }]);
  yield* Quests.unregister([4432]);
  yield* Quests.unregisterAll();
  yield* Quests.getRegistered();
  yield* Quests.getConfig();
  yield* Quests.configure({ acceptRewards: true });
  yield* Drops.register(["Gem", { itemId: 5 }]);
  yield* Drops.unregister(["Gem"]);
  yield* Drops.unregisterAll();
  yield* Drops.getRegistered();
  yield* Drops.getConfig();
  yield* Drops.configure({ acceptAc: true, rejectOthers: true });
  yield* Player.boosts.isActive("xp");
  yield* Player.boosts.register(["XP Boost"]);
  yield* Player.boosts.unregister(["XP Boost"]);
  yield* Player.boosts.unregisterAll();
  yield* Player.boosts.getRegistered();
  yield* Player.boosts.getConfig();
  yield* Player.boosts.configure({ enabled: false });
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| Some(Ok(json!(null)))),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    let gem = || ItemQuery::Name("Gem".into());
    let boost = || vec![ItemQuery::Name("XP Boost".into())];
    assert_eq!(
        requests,
        [
            Request::QuestRegistry(QR::Register {
                quests: vec![
                    Registered {
                        quest_id: 4432,
                        reward_item_id: None,
                    },
                    Registered {
                        quest_id: 4433,
                        reward_item_id: Some(50),
                    },
                ],
            }),
            Request::QuestRegistry(QR::Unregister {
                quest_ids: vec![4432],
            }),
            Request::QuestRegistry(QR::UnregisterAll),
            Request::QuestRegistry(QR::Registered),
            Request::QuestRegistry(QR::Config),
            Request::QuestRegistry(QR::Configure {
                enabled: None,
                accept_rewards: Some(true),
                accept_requirements: None,
            }),
            Request::DropRegistry(DR::Register {
                items: vec![gem(), ItemQuery::ItemId { item_id: 5 }],
            }),
            Request::DropRegistry(DR::Unregister { items: vec![gem()] }),
            Request::DropRegistry(DR::UnregisterAll),
            Request::DropRegistry(DR::Registered),
            Request::DropRegistry(DR::Config),
            Request::DropRegistry(DR::Configure {
                enabled: None,
                accept_ac: Some(true),
                accept_ac_member_only: None,
                accept_non_ac: None,
                accept_non_ac_member_only: None,
                reject_others: Some(true),
            }),
            Request::Player(P::HasBoost {
                kind: crate::domain::player::Boost::Xp,
            }),
            Request::BoostRegistry(BR::Register { items: boost() }),
            Request::BoostRegistry(BR::Unregister { items: boost() }),
            Request::BoostRegistry(BR::UnregisterAll),
            Request::BoostRegistry(BR::Registered),
            Request::BoostRegistry(BR::Config),
            Request::BoostRegistry(BR::Configure {
                enabled: Some(false),
            }),
        ]
    );
}

#[test]
fn world_requests_reach_the_player_typed() {
    use crate::domain::world::MonsterQuery;

    let folder = Folder::new(&[(
        "main.ts",
        r#"import { World } from "mira";

export default function* () {
  yield* World.getAreaId();
  yield* World.getRoomNumber();
  yield* World.getCells();
  yield* World.getCellPads();
  yield* World.getMonsters({ cell: "r2" });
  yield* World.getMonster("Frogzard");
  yield* World.isMonsterAvailable({ monMapId: 5 });
  yield* World.getMonsterDrops({ monMapId: 5 });
  yield* World.setSpawnPoint({ pad: "Left" });
  yield* World.getMapItem(3);
  yield* World.loadSwf("town.swf");
  yield* World.reload();
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| Some(Ok(json!(null)))),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    let five = || MonsterQuery::MonMapId { mon_map_id: 5.0 };
    assert_eq!(
        requests,
        [
            W::AreaId,
            W::RoomNumber,
            W::Cells,
            W::CellPads,
            W::Monsters {
                cell: Some("r2".into())
            },
            W::Monster {
                query: MonsterQuery::Name("Frogzard".into())
            },
            W::MonsterAvailable { query: five() },
            W::MonsterDrops { query: five() },
            W::SetSpawnPoint {
                cell: None,
                pad: Some("Left".into())
            },
            W::MapItem { item_id: 3 },
            W::LoadSwf {
                swf: "town.swf".into()
            },
            W::Reload,
        ]
        .map(Request::World)
    );
}

#[test]
fn combat_player_and_aura_requests_reach_the_player_typed() {
    use crate::domain::{profiles::ProfileChoice, world::MonsterQuery};

    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Combat, Player, Players } from "mira";

export default function* () {
  yield* Combat.getTarget();
  yield* Combat.hasTarget();
  yield* Combat.attack("Frogzard");
  yield* Combat.useSkill(2, { target: { monMapId: 5 }, waitUntilReady: true });
  yield* Combat.canUseSkill(0);
  yield* Combat.getSkillCooldown(5);
  yield* Combat.cancelTarget();
  yield* Combat.cancelAutoAttack();
  yield* Combat.exit();
  yield* Combat.kill("*");
  yield* Combat.kill("Boss", { skills: [3, 1, 5], targetPriority: [{ monMapId: 1 }, "Orb"] });
  yield* Combat.kill("Boss", { skillInterval: "1.5 seconds" });
  yield* Combat.kill("Boss", { profile: "Boss healer" });
  yield* Combat.kill("Boss", {
    profile: {
      onCooldown: "wait",
      triggers: [{ includes: "prepares", skill: 3 }],
      priority: [{ skill: 2, pauseAfter: "0.5 seconds" }],
      steps: [{ skill: 1, when: [{ type: "hp", of: "anyAlly", atMost: "60%" }] }, { skill: 4 }],
    },
  });
  yield* Combat.hunt("Frogzard");
  yield* Combat.hunt({ monMapId: 4 }, { preferMostMatches: true });
  yield* Player.getAuras();
  yield* Player.getAura("Spirit Power");
  yield* Player.hasAura("Spirit Power");
  yield* Player.goToPlayer("player2");
  yield* Players.getAll();
  yield* Players.get("player1");
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| Some(Ok(json!(null)))),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    let skill = |slot: u8| serde_json::from_value(json!(slot)).unwrap();
    let mut expected: Vec<Request> = [
        C::Target,
        C::HasTarget,
        C::Attack {
            query: MonsterQuery::Name("Frogzard".into()),
        },
        C::UseSkill {
            skill: skill(2),
            target: Some(MonsterQuery::MonMapId { mon_map_id: 5.0 }),
            force: None,
            wait_until_ready: Some(true),
        },
        C::CanUseSkill { skill: skill(0) },
        C::SkillCooldown { skill: skill(5) },
        C::CancelTarget,
        C::CancelAutoAttack,
        C::Exit,
        C::Kill {
            query: MonsterQuery::Name("*".into()),
            skills: None,
            skill_interval_ms: None,
            target_priority: None,
            profile: None,
        },
        C::Kill {
            query: MonsterQuery::Name("Boss".into()),
            skills: Some(vec![skill(3), skill(1), skill(5)]),
            skill_interval_ms: None,
            target_priority: Some(vec![
                MonsterQuery::MonMapId { mon_map_id: 1.0 },
                MonsterQuery::Name("Orb".into()),
            ]),
            profile: None,
        },
        C::Kill {
            query: MonsterQuery::Name("Boss".into()),
            skills: None,
            skill_interval_ms: Some(1500),
            target_priority: None,
            profile: None,
        },
        C::Kill {
            query: MonsterQuery::Name("Boss".into()),
            skills: None,
            skill_interval_ms: None,
            target_priority: None,
            profile: Some(ProfileChoice::Named("Boss healer".into())),
        },
        C::Kill {
            query: MonsterQuery::Name("Boss".into()),
            skills: None,
            skill_interval_ms: None,
            target_priority: None,
            profile: Some(ProfileChoice::Inline(
                serde_json::from_value(serde_json::json!({
                    "onCooldown": "wait",
                    "triggers": [{ "includes": "prepares", "skill": 3 }],
                    "priority": [{ "skill": 2, "pauseAfter": 500 }],
                    "steps": [
                        { "skill": 1, "when": [{ "type": "hp", "of": "anyAlly", "atMost": "60%" }] },
                        { "skill": 4 }
                    ]
                }))
                .unwrap(),
            )),
        },
        C::Hunt {
            query: MonsterQuery::Name("Frogzard".into()),
            prefer_most_matches: None,
        },
        C::Hunt {
            query: MonsterQuery::MonMapId { mon_map_id: 4.0 },
            prefer_most_matches: Some(true),
        },
    ]
    .map(Request::Combat)
    .into();
    expected.extend(
        [
            P::Auras,
            P::Aura {
                name: "Spirit Power".into(),
            },
            P::HasAura {
                name: "Spirit Power".into(),
            },
            P::GoToPlayer {
                username: "player2".into(),
            },
        ]
        .map(Request::Player),
    );
    expected.extend([
        Request::Players(PL::All),
        Request::Players(PL::Get {
            username: "player1".into(),
        }),
    ]);
    assert_eq!(requests, expected);
}

#[test]
fn fractional_item_ids_and_counts_never_reach_the_player() {
    let folder = Folder::new(&[
        (
            "count.ts",
            "import { Bank } from \"mira\";\nexport default function* () { yield* Bank.contains(\"Gem\", 2.5); }\n",
        ),
        (
            "id.ts",
            "import { House } from \"mira\";\nexport default function* () { yield* House.getItem({ itemId: 3.5 }); }\n",
        ),
    ]);
    for entry in ["count.ts", "id.ts"] {
        let player = FakePlayer::default();
        let finished = start(
            &folder,
            entry,
            Limits::default(),
            player.answering(|_, _| Some(Ok(json!(null)))),
        )
        .finish();
        assert!(
            matches!(finished.outcome, Outcome::Failed { .. }),
            "{entry}: {:?}",
            finished.outcome
        );
        assert!(
            !player
                .seen()
                .iter()
                .any(|seen| matches!(seen, Seen::Call(..))),
            "{entry}"
        );
    }
}

#[test]
fn item_requests_reach_the_player_typed() {
    use crate::domain::{
        enhancements::Slot,
        inventory::{ItemQuery, Pool},
        shops::ShopItemQuery,
    };

    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Bank, Drops, House, Inventory, Shops, TempInventory } from "mira";

export default function* () {
  yield* Inventory.getItems();
  yield* Inventory.getItem("Potion");
  yield* Inventory.getQuantity({ itemId: 7 });
  yield* Inventory.contains("Potion");
  yield* Inventory.contains("Potion", 5);
  yield* Inventory.isEquipped("Helm");
  yield* Inventory.getSlots();
  yield* Inventory.getUsedSlots();
  yield* Inventory.getAvailableSlots("misc");
  yield* Inventory.equip("Armor");
  yield* Inventory.equip("Armor", { wear: true });
  yield* Inventory.equipByEnhancement({ enhancement: "forge", slot: "helm" }, { wear: true });
  yield* Inventory.unequip("Helm");
  yield* Inventory.wear({ itemId: 8 });
  yield* Inventory.use("Tonic");
  yield* TempInventory.getItems();
  yield* TempInventory.getItem("Scale");
  yield* TempInventory.getQuantity("Scale");
  yield* TempInventory.contains("Scale", 10);
  yield* Bank.load();
  yield* Bank.getItems();
  yield* Bank.getItem("Gem");
  yield* Bank.getQuantity({ itemId: 3 });
  yield* Bank.contains("Gem", 2);
  yield* Bank.getSlots();
  yield* Bank.getUsedSlots();
  yield* Bank.getAvailableSlots();
  yield* Bank.isOpen();
  yield* Bank.open();
  yield* Bank.deposit("Gem");
  yield* Bank.withdraw({ itemId: 3 });
  yield* Bank.swap("Gem", "Ore");
  yield* Bank.depositBatch(["Gem", { itemId: 3 }]);
  yield* Bank.withdrawBatch(["Ore"]);
  yield* House.getItems();
  yield* House.getItem("Chair");
  yield* House.getQuantity({ itemId: 4 });
  yield* House.contains("Chair", 2);
  yield* House.getSlots();
  yield* House.getUsedSlots();
  yield* House.getAvailableSlots();
  yield* Drops.getItems();
  yield* Drops.getItem({ itemId: 45230 });
  yield* Drops.contains("Gem", 2);
  yield* Drops.accept("Gem");
  yield* Drops.reject({ itemId: 45230 });
  yield* Shops.load(2114);
  yield* Shops.getCurrent();
  yield* Shops.getItems();
  yield* Shops.getItem({ shopItemId: 9740 });
  yield* Shops.isOpen();
  yield* Shops.close(2114);
  yield* Shops.canBuy("Gem", { quantity: 10 });
  yield* Shops.getMaxBuyQuantity({ itemId: 11482 });
  yield* Shops.buy({ shopItemId: 9740 });
  yield* Shops.sell("Gem");
  yield* Shops.sell({ itemId: 3 }, { quantity: 2 });
  yield* Shops.enhanceItem("Sword", { enhancement: "lucky", special: "spiral carve" });
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| Some(Ok(json!(null)))),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    let name = |name: &str| ItemQuery::Name(name.into());
    let id = |item_id: i32| ItemQuery::ItemId { item_id };
    let mut expected: Vec<Request> = [
        I::Items,
        I::Item {
            query: name("Potion"),
        },
        I::Quantity { query: id(7) },
        I::Contains {
            query: name("Potion"),
            quantity: None,
        },
        I::Contains {
            query: name("Potion"),
            quantity: Some(5),
        },
        I::Equipped {
            query: name("Helm"),
        },
        I::Slots,
        I::UsedSlots {
            pool: Some(Pool::Bag),
        },
        I::AvailableSlots {
            pool: Some(Pool::Misc),
        },
        I::Equip {
            query: name("Armor"),
            wear: None,
        },
        I::Equip {
            query: name("Armor"),
            wear: Some(true),
        },
        I::EquipByEnhancement {
            enhancement: "forge".into(),
            special: None,
            slot: Some(Slot::Helm),
            wear: Some(true),
        },
        I::Unequip {
            query: name("Helm"),
        },
        I::Wear { query: id(8) },
        I::Use {
            query: name("Tonic"),
        },
    ]
    .map(Request::Inventory)
    .to_vec();
    expected.extend(
        [
            T::Items,
            T::Item {
                query: name("Scale"),
            },
            T::Quantity {
                query: name("Scale"),
            },
            T::Contains {
                query: name("Scale"),
                quantity: Some(10),
            },
        ]
        .map(Request::TempInventory),
    );
    expected.extend(
        [
            B::Load,
            B::Items,
            B::Item { query: name("Gem") },
            B::Quantity { query: id(3) },
            B::Contains {
                query: name("Gem"),
                quantity: Some(2),
            },
            B::Slots,
            B::UsedSlots,
            B::AvailableSlots,
            B::IsOpen,
            B::Open,
            B::Deposit { query: name("Gem") },
            B::Withdraw { query: id(3) },
            B::Swap {
                inventory_query: name("Gem"),
                bank_query: name("Ore"),
            },
            B::Deposit { query: name("Gem") },
            B::Deposit { query: id(3) },
            B::Withdraw { query: name("Ore") },
        ]
        .map(Request::Bank),
    );
    expected.extend(
        [
            H::Items,
            H::Item {
                query: name("Chair"),
            },
            H::Quantity { query: id(4) },
            H::Contains {
                query: name("Chair"),
                quantity: Some(2),
            },
            H::Slots,
            H::UsedSlots,
            H::AvailableSlots,
        ]
        .map(Request::House),
    );
    expected.extend(
        [
            D::Items,
            D::Item { query: id(45230) },
            D::Contains {
                query: name("Gem"),
                quantity: Some(2),
            },
            D::Accept { query: name("Gem") },
            D::Reject { query: id(45230) },
        ]
        .map(Request::Drops),
    );
    let listing = ShopItemQuery::ShopItemId { shop_item_id: 9740 };
    expected.extend(
        [
            S::Load { shop_id: 2114 },
            S::Current,
            S::Items,
            S::Item {
                query: listing.clone(),
            },
            S::IsOpen { shop_id: None },
            S::Close {
                shop_id: Some(2114),
            },
            S::CanBuy {
                query: ShopItemQuery::Item(name("Gem")),
                quantity: Some(10),
            },
            S::MaxBuyQuantity {
                query: ShopItemQuery::Item(id(11482)),
            },
            S::Buy {
                query: listing,
                quantity: None,
            },
            S::Sell {
                query: name("Gem"),
                quantity: None,
            },
            S::Sell {
                query: id(3),
                quantity: Some(2),
            },
            S::EnhanceItem {
                query: name("Sword"),
                enhancement: "lucky".into(),
                special: Some("spiral carve".into()),
            },
        ]
        .map(Request::Shops),
    );
    assert_eq!(requests, expected);
}

#[test]
fn quest_requests_reach_the_player_typed() {
    let folder = Folder::new(&[(
        "main.ts",
        r#"import { Quests } from "mira";

export default function* () {
  yield* Quests.load(6552);
  yield* Quests.load(6553, { silent: true });
  yield* Quests.loadBatch([8152, 8153], { silent: true });
  yield* Quests.get(6552);
  yield* Quests.getAccepted();
  yield* Quests.isInProgress(6552);
  yield* Quests.isAvailable(6552);
  yield* Quests.canComplete(6552);
  yield* Quests.getMaxTurnIns(6552);
  yield* Quests.accept(6552);
  yield* Quests.accept(6553, { silent: true });
  yield* Quests.acceptBatch([8152, 8153]);
  yield* Quests.complete(6552);
  yield* Quests.complete(6552, { turnIns: 3, rewardItemId: 45230 });
  yield* Quests.abandon(6552);
}
"#,
    )]);
    let player = FakePlayer::default();
    let finished = start(
        &folder,
        "main.ts",
        Limits::default(),
        player.answering(|_, _| Some(Ok(json!(null)))),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    assert_eq!(
        requests,
        [
            Q::Load {
                quest_id: 6552,
                silent: None,
            },
            Q::Load {
                quest_id: 6553,
                silent: Some(true),
            },
            Q::LoadBatch {
                quest_ids: vec![8152, 8153],
                silent: Some(true),
            },
            Q::Get { quest_id: 6552 },
            Q::Accepted,
            Q::InProgress { quest_id: 6552 },
            Q::Available { quest_id: 6552 },
            Q::CanComplete { quest_id: 6552 },
            Q::MaxTurnIns { quest_id: 6552 },
            Q::Accept {
                quest_id: 6552,
                silent: None,
            },
            Q::Accept {
                quest_id: 6553,
                silent: Some(true),
            },
            Q::AcceptBatch {
                quest_ids: vec![8152, 8153],
                silent: None,
            },
            Q::Complete {
                quest_id: 6552,
                turn_ins: None,
                reward_item_id: None,
            },
            Q::Complete {
                quest_id: 6552,
                turn_ins: Some(3),
                reward_item_id: Some(45230),
            },
            Q::Abandon { quest_id: 6552 },
        ]
        .map(Request::Quests)
    );
}

fn army_info() -> serde_json::Value {
    json!({ "name": "ultras", "number": 2, "size": 2, "room": 4321, "players": ["player1", "player2"] })
}

fn army_requests(seen: Vec<Seen>) -> Vec<super::army::Request> {
    seen.into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, Request::Army(request)) => Some(request),
            _ => None,
        })
        .collect()
}

#[test]
fn army_requests_reach_the_player_typed() {
    use super::army::Request as AY;
    let player = FakePlayer::default();
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Army } from "mira";

export default function* () {
  const army = yield* Army.start("ultras", { timeout: "2 seconds" });
  console.log(`${army?.number} of ${army?.size} in ${army?.room}`);
  yield* Army.sync("ready");
  console.log(JSON.stringify(yield* Army.gather("count", { kills: 2 }, { timeout: "1.5 millis" })));
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        player.answering(|request, _| {
            Some(Ok(match request {
                Request::Army(AY::Start { .. }) => army_info(),
                Request::Army(AY::Sync { .. }) => json!(null),
                Request::Army(AY::Gather { .. }) => json!([{ "kills": 1 }, { "kills": 2 }]),
                other => panic!("unexpected {other:?}"),
            }))
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(
        finished.logs,
        ["2 of 2 in 4321", r#"[{"kills":1},{"kills":2}]"#]
    );
    assert_eq!(
        army_requests(player.seen()),
        [
            AY::Start {
                name: "ultras".into(),
                timeout_ms: Some(2000)
            },
            AY::Sync {
                label: "ready".into(),
                timeout_ms: None
            },
            AY::Gather {
                label: "count".into(),
                value: json!({ "kills": 2 }),
                timeout_ms: Some(2)
            },
        ]
    );
}

#[test]
fn until_works_in_rounds_and_reports_done_until_the_army_settles() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::army::Request as AY;
    let player = FakePlayer::default();
    let kills = Arc::new(AtomicUsize::new(0));
    let counted = kills.clone();
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Army, Combat, Inventory } from "mira";

export default function* () {
  yield* Army.start("ultras");
  yield* Army.until(
    "trophy",
    {
      done: () => Inventory.contains("Trophy"),
      work: function* () {
        yield* Combat.kill("Boss");
      },
    },
    { interval: "1 millis", timeout: "1 minute" },
  );
  console.log("settled");
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        player.answering(move |request, held| {
            Some(Ok(match request {
                Request::Army(AY::Start { .. }) => army_info(),
                Request::Army(AY::Until { .. }) => json!(false),
                Request::Army(AY::Settle { .. }) => return None,
                Request::Army(AY::Report { .. }) => {
                    let settle = held
                        .iter()
                        .position(|call| matches!(call.request, Request::Army(AY::Settle { .. })))
                        .expect("settle is waiting");
                    held.remove(settle).reply.send(Ok(json!(null)));
                    json!(true)
                }
                Request::Combat(C::Kill { .. }) => {
                    counted.fetch_add(1, Ordering::SeqCst);
                    json!(true)
                }
                Request::Inventory(I::Contains { .. }) => {
                    json!(counted.load(Ordering::SeqCst) >= 2)
                }
                other => panic!("unexpected {other:?}"),
            }))
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["settled"]);
    let seen = player.seen();
    let position = |wanted: fn(&Request) -> bool| {
        seen.iter()
            .position(|seen| matches!(seen, Seen::Call(_, request) if wanted(request)))
            .unwrap()
    };
    let until = position(|request| matches!(request, Request::Army(AY::Until { .. })));
    let first_kill = position(|request| matches!(request, Request::Combat(C::Kill { .. })));
    let report = position(|request| matches!(request, Request::Army(AY::Report { .. })));
    assert!(until < first_kill && first_kill < report, "{seen:?}");
    let kills_before_report = seen[..report]
        .iter()
        .filter(|seen| matches!(seen, Seen::Call(_, Request::Combat(C::Kill { .. }))))
        .count();
    assert_eq!(kills_before_report, 2);
    assert_eq!(
        army_requests(seen),
        [
            AY::Start {
                name: "ultras".into(),
                timeout_ms: None
            },
            AY::Until {
                label: "trophy".into(),
                done: false
            },
            AY::Settle {
                timeout_ms: Some(60_000)
            },
            AY::Report { done: true },
        ]
    );
}

#[test]
fn until_skips_the_work_when_the_army_is_already_done() {
    use super::army::Request as AY;
    let player = FakePlayer::default();
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Army, Combat, Inventory } from "mira";

export default function* () {
  yield* Army.start("ultras");
  yield* Army.until("trophy", {
    done: () => Inventory.contains("Trophy"),
    work: () => Combat.kill("Boss"),
  });
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        player.answering(|request, _| {
            Some(Ok(match request {
                Request::Army(AY::Start { .. }) => army_info(),
                Request::Army(AY::Until { .. }) => json!(true),
                Request::Inventory(I::Contains { .. }) => json!(true),
                other => panic!("unexpected {other:?}"),
            }))
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(
        army_requests(player.seen()),
        [
            AY::Start {
                name: "ultras".into(),
                timeout_ms: None
            },
            AY::Until {
                label: "trophy".into(),
                done: true
            },
        ]
    );
}

#[test]
fn join_map_joins_the_army_room_and_agrees_on_the_result() {
    use super::army::Request as AY;
    let player = FakePlayer::default();
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Army } from "mira";

export default function* () {
  yield* Army.start("ultras");
  console.log(yield* Army.joinMap("bossmap", { cell: "Enter", timeout: "1 second" }));
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        player.answering(|request, _| {
            Some(Ok(match request {
                Request::Army(AY::Start { .. }) => army_info(),
                Request::Army(AY::Sync { .. }) => json!(null),
                Request::Player(P::JoinMap { .. }) => json!(true),
                Request::World(W::RoomNumber) => json!(4321),
                Request::Army(AY::Gather { .. }) => json!([true, false]),
                other => panic!("unexpected {other:?}"),
            }))
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["false"]);
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    assert_eq!(
        requests,
        [
            Request::Army(AY::Start {
                name: "ultras".into(),
                timeout_ms: None
            }),
            Request::Army(AY::Sync {
                label: "joinMap bossmap".into(),
                timeout_ms: Some(1000)
            }),
            Request::Player(P::JoinMap {
                map: "bossmap-4321".into(),
                cell: Some("Enter".into()),
                pad: None
            }),
            Request::World(W::RoomNumber),
            Request::Army(AY::Gather {
                label: "joinMap bossmap".into(),
                value: json!(true),
                timeout_ms: Some(1000)
            }),
        ]
    );
}

#[test]
fn equip_set_puts_on_the_gear_in_order_and_drinks_the_pots() {
    use super::army::Request as AY;
    use crate::domain::inventory::ItemQuery;
    let player = FakePlayer::default();
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Army } from "mira";

export default function* () {
  yield* Army.start("ultras");
  console.log(yield* Army.equipSet("boss"));
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        player.answering(|request, _| {
            let item = |name: &str| ItemQuery::Name(name.into());
            Some(Ok(match request {
                Request::Army(AY::Start { .. }) => army_info(),
                Request::Army(AY::Gear { .. }) => json!({
                    "safeClass": "Safe Class",
                    "safePot": "Safe Pot",
                    "class": "Main Class",
                    "weapon": "Sword",
                    "cape": null,
                    "helm": null,
                    "armor": null,
                    "pet": null,
                    "pots": ["Potion", "Elixir"],
                    "scroll": "Scroll",
                }),
                Request::Inventory(I::Equip { .. }) => json!(true),
                Request::Inventory(I::Item { query }) if *query == item("Potion") => {
                    json!({ "itemId": 1, "name": "Potion", "quantity": 3, "link": "" })
                }
                Request::Inventory(I::Item { query }) if *query == item("Elixir") => {
                    json!({ "itemId": 2, "name": "Elixir", "quantity": 1, "link": "Elixir" })
                }
                Request::Combat(C::UseSkill { .. }) => json!(true),
                Request::Inventory(I::Quantity { .. }) => json!(2),
                Request::Inventory(I::Use { .. }) => json!(true),
                Request::Army(AY::Gather { .. }) => json!([true, true]),
                other => panic!("unexpected {other:?}"),
            }))
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["true"]);
    let requests: Vec<Request> = player
        .seen()
        .into_iter()
        .filter_map(|seen| match seen {
            Seen::Call(_, request) => Some(request),
            _ => None,
        })
        .collect();
    let item = |name: &str| ItemQuery::Name(name.into());
    let equip = |name: &str| {
        Request::Inventory(I::Equip {
            query: item(name),
            wear: None,
        })
    };
    assert_eq!(
        requests,
        [
            Request::Army(AY::Start {
                name: "ultras".into(),
                timeout_ms: None
            }),
            Request::Army(AY::Gear {
                name: "boss".into()
            }),
            equip("Safe Class"),
            equip("Safe Pot"),
            equip("Main Class"),
            equip("Safe Pot"),
            equip("Sword"),
            Request::Inventory(I::Item {
                query: item("Potion")
            }),
            equip("Potion"),
            Request::Combat(C::UseSkill {
                skill: serde_json::from_value(json!(5)).unwrap(),
                target: None,
                force: Some(true),
                wait_until_ready: Some(true),
            }),
            Request::Inventory(I::Quantity {
                query: item("Potion")
            }),
            Request::Inventory(I::Item {
                query: item("Elixir")
            }),
            Request::Inventory(I::Use {
                query: item("Elixir")
            }),
            equip("Scroll"),
            Request::Army(AY::Gather {
                label: "equipSet boss".into(),
                value: json!(true),
                timeout_ms: None
            }),
        ]
    );
}

#[test]
fn take_turns_passes_each_turn_on_and_leaves_when_stopped() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::army::Request as AY;
    let player = FakePlayer::default();
    let turns = AtomicUsize::new(0);
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Army, Script } from "mira";

export default function* () {
  yield* Army.start("ultras");
  let turns = 0;
  const rotation = yield* Army.takeTurns("taunt", {
    players: [2, 1],
    turn: function* () {
      turns += 1;
      return turns % 2 === 0;
    },
  });
  yield* Script.sleep("50 millis");
  yield* rotation.stop;
  console.log(turns);
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        player.answering(move |request, _| {
            Some(Ok(match request {
                Request::Army(AY::Start { .. }) => army_info(),
                Request::Army(AY::Rotate { .. }) => json!(true),
                Request::Army(AY::Turn { .. }) if turns.fetch_add(1, Ordering::SeqCst) < 3 => {
                    json!(null)
                }
                Request::Army(AY::Turn { .. }) => return None,
                Request::Army(AY::Leave { .. }) => json!(null),
                other => panic!("unexpected {other:?}"),
            }))
        }),
    )
    .finish();
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.logs, ["3"]);
    let seen = player.seen();
    let held = seen
        .iter()
        .rev()
        .find_map(|seen| match seen {
            Seen::Call(id, Request::Army(AY::Turn { .. })) => Some(*id),
            _ => None,
        })
        .unwrap();
    assert!(seen.contains(&Seen::Cancel(held)), "{seen:?}");
    let turn = |acted: Option<bool>| AY::Turn {
        label: "taunt".into(),
        acted,
    };
    assert_eq!(
        army_requests(seen),
        [
            AY::Start {
                name: "ultras".into(),
                timeout_ms: None
            },
            AY::Rotate {
                label: "taunt".into(),
                players: vec![2, 1]
            },
            turn(None),
            turn(Some(false)),
            turn(Some(true)),
            turn(Some(false)),
            AY::Leave {
                label: "taunt".into()
            },
        ]
    );
}

#[test]
fn a_turn_that_fails_fails_the_script_and_leaves_the_rotation() {
    use super::army::Request as AY;
    let player = FakePlayer::default();
    let finished = start(
        &Folder::new(&[(
            "main.ts",
            r#"import { Army, Script } from "mira";

export default function* () {
  yield* Army.start("ultras");
  yield* Army.takeTurns("taunt", {
    players: [1, 2],
    turn: function* () {
      throw new Error("no scroll");
    },
  });
  yield* Script.sleep("1 minute");
}
"#,
        )]),
        "main.ts",
        Limits::default(),
        player.answering(|request, _| {
            Some(Ok(match request {
                Request::Army(AY::Start { .. }) => army_info(),
                Request::Army(AY::Rotate { .. }) => json!(true),
                Request::Army(AY::Turn { .. } | AY::Leave { .. }) => json!(null),
                other => panic!("unexpected {other:?}"),
            }))
        }),
    )
    .finish();
    assert_eq!(failure(&finished.outcome).0, "no scroll");
    assert!(army_requests(player.seen()).contains(&AY::Leave {
        label: "taunt".into()
    }));
}
