use std::{cell::RefCell, os::unix::net::UnixStream, path::PathBuf, rc::Rc, sync::mpsc, thread};

use super::{worker::read, worker::write, *};
use crate::scripting::calls::RunId;
use crate::{
    bridge::{Events, Value as Flash},
    domain::testing::FakeLoader,
    scripting::{feeds, inputs::Definition, limits::Clock},
};

fn compiled() -> Compiled {
    Compiled {
        assembly: PathBuf::from("/cache/entry.dll"),
        core: None,
        class: "FreeDailyBoost".into(),
        definition: Definition::new(Vec::new()),
    }
}

struct Harness {
    session: Session,
    commands: mpsc::Receiver<Command>,
    inbox: mpsc::Sender<Command>,
    calls: Rc<RefCell<Vec<calls::Request>>>,
    logs: mpsc::Receiver<String>,
}

fn harness(worker: Arc<Worker>, grace: Duration) -> Harness {
    let (inbox, commands) = mpsc::channel();
    let run = RunId(7);
    assert!(worker.admit(run, &inbox));
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    let (logged, logs) = mpsc::channel();
    let link = Link {
        run,
        player: Rc::new(move |message| {
            if let ToPlayer::Call(call) = message {
                seen.borrow_mut().push(call.request.clone());
                call.reply.send(Ok(json!("pong")));
            }
        }),
        dialogs: Rc::new(|_| {}),
        commands: inbox.clone(),
    };
    Harness {
        session: Session {
            worker,
            link,
            log: Box::new(move |_, text| {
                let _ = logged.send(text.to_owned());
            }),
            grace,
        },
        commands,
        inbox,
        calls,
        logs,
    }
}

fn frame(stream: &mut UnixStream, value: Json) {
    write(stream, &value).unwrap();
}

fn next(stream: &mut UnixStream) -> Json {
    read(stream).unwrap().expect("a frame")
}

#[test]
fn a_run_starts_relays_calls_and_ends_when_the_worker_says() {
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    let harness = harness(Worker::connected(ours), STOP_GRACE);
    let fake = thread::spawn(move || {
        let start = next(&mut theirs);
        frame(
            &mut theirs,
            json!({ "type": "call", "run": 7, "id": 1, "request": {
                "domain": "skua", "op": "call", "function": "skua.availableMonsters"
            }}),
        );
        let reply = next(&mut theirs);
        frame(
            &mut theirs,
            json!({ "type": "call", "run": 7, "id": 2, "request": { "domain": "player", "op": "flyAway" } }),
        );
        let refused = next(&mut theirs);
        frame(
            &mut theirs,
            json!({ "type": "call", "run": 7, "id": 4, "request": { "domain": "player", "op": "cell" } }),
        );
        let _cell = next(&mut theirs);
        frame(
            &mut theirs,
            json!({ "type": "log", "run": 7, "level": "info", "text": "Bot Started" }),
        );
        frame(
            &mut theirs,
            json!({ "type": "ended", "run": 7, "outcome": { "kind": "completed" } }),
        );
        frame(
            &mut theirs,
            json!({ "type": "call", "run": 7, "id": 3, "request": {
                "domain": "skua", "op": "call", "function": "isNull", "args": ["late"]
            }}),
        );
        (start, reply, refused)
    });
    let options: Values = serde_json::from_value(json!({ "Mode": "Farm Mode" })).unwrap();
    let outcome = harness.session.run(
        &compiled(),
        Path::new("/skua/Scripts/Dailies/FreeDailyBoost.cs"),
        options,
        &Clock::new(),
        &harness.commands,
    );
    let (start, reply, refused) = fake.join().unwrap();
    assert_eq!(outcome, Outcome::Completed);
    assert_eq!(
        start,
        json!({
            "type": "start", "run": 7, "assembly": "/cache/entry.dll", "core": null,
            "class": "FreeDailyBoost", "script": "/skua/Scripts/Dailies/FreeDailyBoost.cs",
            "options": { "Mode": "Farm Mode" }
        })
    );
    assert_eq!(
        reply,
        json!({ "type": "reply", "run": 7, "id": 1, "ok": "pong" })
    );
    assert_eq!(refused["id"], 2);
    assert!(refused["error"].as_str().is_some());
    assert_eq!(
        *harness.calls.borrow(),
        [
            calls::Request::Skua(Request::Call {
                function: "skua.availableMonsters".into(),
                args: Vec::new(),
            }),
            calls::Request::Player(crate::scripting::player::Request::Cell),
        ]
    );
    assert_eq!(harness.logs.try_iter().collect::<Vec<_>>(), ["Bot Started"]);
}

#[test]
fn stopping_asks_the_worker_and_waits_for_it_to_end_the_run() {
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    let worker = Worker::connected(ours);
    let harness = harness(worker.clone(), STOP_GRACE);
    harness.inbox.send(Command::Stop).unwrap();
    let fake = thread::spawn(move || {
        let start = next(&mut theirs);
        let stop = next(&mut theirs);
        frame(
            &mut theirs,
            json!({ "type": "ended", "run": 7, "outcome": { "kind": "stopped" } }),
        );
        (start["type"].clone(), stop, theirs)
    });
    let outcome = harness.session.run(
        &compiled(),
        Path::new("/x.cs"),
        Values::new(),
        &Clock::new(),
        &harness.commands,
    );
    let (start, stop, _open) = fake.join().unwrap();
    assert_eq!(outcome, Outcome::Stopped);
    assert_eq!(start, "start");
    assert_eq!(stop, json!({ "type": "stop", "run": 7 }));
    let (other, _) = mpsc::channel();
    assert!(worker.admit(RunId(8), &other));
}

#[test]
fn a_run_that_ignores_stop_is_abandoned_and_its_worker_retired() {
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    theirs
        .set_write_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let worker = Worker::connected(ours);
    let mut harness = harness(worker.clone(), Duration::from_millis(50));
    harness.session.log = Box::new(|_, _| thread::sleep(Duration::from_millis(1)));
    let fake = thread::spawn(move || {
        let _start = next(&mut theirs);
        let started = Instant::now();
        let log = json!({ "type": "log", "run": 7, "level": "info", "text": "still going" });
        while started.elapsed() < Duration::from_secs(5) && write(&mut theirs, &log).is_ok() {}
    });
    let clock = Clock::new();
    let inbox = harness.inbox.clone();
    let started = Instant::now();
    let outcome = thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_millis(200));
            clock.request_stop();
            let _ = inbox.send(Command::Stop);
        });
        harness.session.run(
            &compiled(),
            Path::new("/x.cs"),
            Values::new(),
            &clock,
            &harness.commands,
        )
    });
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(outcome, Outcome::Stopped);
    fake.join().unwrap();
    let (other, _) = mpsc::channel();
    assert!(!worker.admit(RunId(8), &other));
}

#[test]
fn a_run_that_ended_while_its_stop_ran_out_keeps_its_worker() {
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    let worker = Worker::connected(ours);
    let mut harness = harness(worker.clone(), Duration::from_millis(50));
    harness.session.log = Box::new(|_, _| thread::sleep(Duration::from_millis(1)));
    harness.inbox.send(Command::Stop).unwrap();
    let fake = thread::spawn(move || {
        let _start = next(&mut theirs);
        let _stop = next(&mut theirs);
        let log = json!({ "type": "log", "run": 7, "level": "info", "text": "cleaning up" });
        for _ in 0..500 {
            write(&mut theirs, &log).unwrap();
        }
        frame(
            &mut theirs,
            json!({ "type": "ended", "run": 7, "outcome": { "kind": "stopped" } }),
        );
        theirs
    });
    let outcome = harness.session.run(
        &compiled(),
        Path::new("/x.cs"),
        Values::new(),
        &Clock::new(),
        &harness.commands,
    );
    let _open = fake.join().unwrap();
    assert_eq!(outcome, Outcome::Stopped);
    let (other, _) = mpsc::channel();
    assert!(worker.admit(RunId(8), &other));
}

#[test]
fn a_start_too_large_to_send_fails_the_run() {
    let (ours, _theirs) = UnixStream::pair().unwrap();
    let harness = harness(Worker::connected(ours), STOP_GRACE);
    let options: Values = serde_json::from_value(json!({ "Note": "x".repeat(17 << 20) })).unwrap();
    let outcome = harness.session.run(
        &compiled(),
        Path::new("/x.cs"),
        options,
        &Clock::new(),
        &harness.commands,
    );
    assert!(matches!(outcome, Outcome::Failed { .. }), "{outcome:?}");
}

#[test]
fn a_hard_stop_still_stops_the_run_and_keeps_the_worker_when_the_run_ends() {
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    theirs
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let worker = Worker::connected(ours);
    let harness = harness(worker.clone(), STOP_GRACE);
    let clock = Clock::new();
    clock.stop_now();
    harness.inbox.send(Command::Stop).unwrap();
    let outcome = harness.session.run(
        &compiled(),
        Path::new("/x.cs"),
        Values::new(),
        &clock,
        &harness.commands,
    );
    assert_eq!(outcome, Outcome::Stopped);
    let _start = next(&mut theirs);
    assert_eq!(next(&mut theirs), json!({ "type": "stop", "run": 7 }));
    frame(
        &mut theirs,
        json!({ "type": "call", "run": 7, "id": 5, "request": { "domain": "player", "op": "cell" } }),
    );
    let refused = next(&mut theirs);
    assert_eq!(refused["id"], 5);
    assert!(refused["error"].as_str().is_some());
    frame(
        &mut theirs,
        json!({ "type": "ended", "run": 7, "outcome": { "kind": "stopped" } }),
    );
    let (other, _) = mpsc::channel();
    assert!(worker.admit(RunId(8), &other));
}

#[test]
fn a_reply_too_large_to_send_becomes_an_error() {
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    let harness = harness(Worker::connected(ours), STOP_GRACE);
    harness
        .inbox
        .send(Command::Reply {
            id: CallId(3),
            reply: Ok(json!("x".repeat(17 << 20))),
        })
        .unwrap();
    let fake = thread::spawn(move || {
        let _start = next(&mut theirs);
        let reply = next(&mut theirs);
        frame(
            &mut theirs,
            json!({ "type": "ended", "run": 7, "outcome": { "kind": "completed" } }),
        );
        reply
    });
    let outcome = harness.session.run(
        &compiled(),
        Path::new("/x.cs"),
        Values::new(),
        &Clock::new(),
        &harness.commands,
    );
    let reply = fake.join().unwrap();
    assert_eq!(outcome, Outcome::Completed);
    assert_eq!(reply["id"], 3);
    assert!(reply["error"].as_str().is_some());
}

#[test]
fn losing_the_worker_fails_the_run() {
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    let harness = harness(Worker::connected(ours), STOP_GRACE);
    let fake = thread::spawn(move || {
        let _start = next(&mut theirs);
    });
    let outcome = harness.session.run(
        &compiled(),
        Path::new("/x.cs"),
        Values::new(),
        &Clock::new(),
        &harness.commands,
    );
    fake.join().unwrap();
    assert_eq!(
        outcome,
        Outcome::Failed {
            message: "The Skua runner quit.".into(),
            stack: None
        }
    );
}

#[tokio::test]
async fn calls_reach_only_skua_and_flash_exports_and_events_stream_raw() {
    let loader = FakeLoader::new(|callback, args| {
        assert!(
            matches!(callback, "skua.walkTo" | "flash.getGameObject"),
            "{callback}"
        );
        assert_eq!(args.len(), 1);
        Ok(Flash::String("{}".into()))
    });
    let events = Events::default();
    let feeds = Feeds::default();
    let subscribe = |wants| events.subscribe(wants);
    assert_eq!(
        serve(
            &loader,
            true,
            subscribe,
            &feeds,
            Request::Call {
                function: "skua.walkTo".into(),
                args: vec![json!(100)],
            }
        ),
        Ok(json!("{}"))
    );
    assert_eq!(
        serve(
            &loader,
            true,
            |wants| events.subscribe(wants),
            &feeds,
            Request::Call {
                function: "flash.getGameObject".into(),
                args: vec![json!("world.strFrame")],
            }
        ),
        Ok(json!("{}"))
    );
    for refused in ["auth.login", "skua.notAFunction", "getGameObject"] {
        assert_eq!(
            serve(
                &loader,
                true,
                |wants| events.subscribe(wants),
                &feeds,
                Request::Call {
                    function: refused.into(),
                    args: Vec::new(),
                }
            ),
            Err(CallError::NotExposed(refused.into()))
        );
    }
    assert_eq!(
        serve(
            &loader,
            true,
            |wants| events.subscribe(wants),
            &feeds,
            Request::Call {
                function: "world.jumpToCell".into(),
                args: Vec::new(),
            }
        ),
        Err(CallError::NotExposed("world.jumpToCell".into()))
    );
    let id: FeedId = serde_json::from_value(json!(1)).unwrap();
    assert_eq!(
        serve(
            &loader,
            true,
            |wants| events.subscribe(wants),
            &feeds,
            Request::Events { id }
        ),
        Ok(Json::Null)
    );
    events.publish(&LoaderEvent::Progress { percent: 40 });
    events.publish(&LoaderEvent::ExtensionResponse {
        packet: r#"{"cmd":"ct"}"#.into(),
    });
    events.publish(&LoaderEvent::SmartFoxDebug {
        line: "[Sending - STR]: %xt%zm%moveToCell%1%Enter%Spawn%".into(),
    });
    let next = || serde_json::from_value(json!({ "op": "next", "id": 1 })).unwrap();
    assert_eq!(
        feeds::serve(&feeds, next()).await,
        Ok(json!({ "lagged": false, "items": [{ "event": "loaded" }] }))
    );
    assert_eq!(
        feeds::serve(&feeds, next()).await,
        Ok(json!({ "lagged": false, "items": [
            { "event": "extensionResponse", "packet": r#"{"cmd":"ct"}"# },
            { "event": "smartFoxDebug", "line": "[Sending - STR]: %xt%zm%moveToCell%1%Enter%Spawn%" },
        ]}))
    );
}

#[test]
#[ignore = "needs dotnet, scripts/build-skua and MIRA_SKUA_TEST_HOME with a Scripts checkout"]
fn free_daily_boost_runs_through_the_real_worker() {
    let home = PathBuf::from(std::env::var("MIRA_SKUA_TEST_HOME").unwrap());
    let runner = std::env::current_dir()
        .unwrap()
        .join("target/Resources/skua/Mira.Skua.dll");
    init(Setup {
        runner,
        home: home.clone(),
        cache: home.join("../cache"),
    });
    let game: serde_json::Map<String, Json> = serde_json::from_value(json!({
        "skua.isLoggedIn": true, "skua.isKicked": false,
        "flash.getGameObject:world.myAvatar.items.length": "3",
        "flash.getGameObject:world.mapLoadInProgress": "false",
        "flash.callGameFunction0:world.myAvatar.pMC.artLoaded": "true",
        "flash.getGameObject:world.myAvatar.dataLeaf.intState": "1",
        "flash.getGameObject:world.myAvatar.dataLeaf.intHP": "2500",
        "flash.getGameObject:world.strMapName": "\"battleon\"",
        "flash.getGameObject:world.myAvatar.items": "[]",
        "flash.getGameObject:world.myAvatar.houseitems": "[]",
    }))
    .unwrap();
    let launch = Launch {
        entry: home.join("Scripts/Dailies/FreeDailyBoost[Mem].cs"),
        inputs: crate::scripting::Supplied::Values(Values::new()),
    };
    let (inbox, commands) = mpsc::channel();
    let link = Link {
        run: RunId(99),
        player: Rc::new(move |message| {
            let ToPlayer::Call(call) = message else {
                return;
            };
            let answer = match &call.request {
                calls::Request::Skua(Request::Call { function, args }) => {
                    let key = match args.first().and_then(Json::as_str) {
                        Some(path) => format!("{function}:{path}"),
                        None => function.clone(),
                    };
                    game.get(&key).cloned().unwrap_or(match function.as_str() {
                        "flash.isNull" => json!(false),
                        _ => Json::Null,
                    })
                }
                calls::Request::Feeds(_) => {
                    thread::sleep(Duration::from_millis(50));
                    json!({ "items": [], "lagged": false })
                }
                calls::Request::Auth(_) => json!(true),
                _ => Json::Null,
            };
            call.reply.send(Ok(answer));
        }),
        dialogs: Rc::new(|message| {
            if let ToDialogs::Open(dialog) = message {
                dialog.cancel();
            }
        }),
        commands: inbox,
    };
    let (logged, logs) = mpsc::channel();
    let outcome = execute(
        &launch,
        &Clock::new(),
        &commands,
        Box::new(move |_, text| {
            let _ = logged.send(text.to_owned());
        }),
        link,
    );
    let logs: Vec<String> = logs.try_iter().collect();
    assert_eq!(outcome, Outcome::Completed, "{logs:#?}");
    assert!(
        logs.iter().any(|line| line.contains("Bot Configured")),
        "{logs:#?}"
    );
}

#[test]
fn dotnet_is_found_on_path_first_then_through_the_login_shell() {
    use std::os::unix::fs::PermissionsExt;

    let mut random = [0; 8];
    getrandom::getrandom(&mut random).unwrap();
    let temp = std::env::temp_dir().join(format!("mira-dotnet-{:x}", u64::from_ne_bytes(random)));
    let (bin, mise) = (temp.join("bin"), temp.join("mise/dotnet-root"));
    for dir in [&bin, &mise] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("dotnet"), "").unwrap();
    }
    let path = std::env::join_paths([temp.join("empty"), bin.clone()]).unwrap();
    assert_eq!(on_path(Some(path)), Some(bin.join("dotnet")));
    assert_eq!(on_path(Some(temp.join("empty").into())), None);

    std::fs::write(
        temp.join(".zshrc"),
        "path=(\"$ZDOTDIR/mise/dotnet-root\" $path)\nalias dotnet='dotnet --nologo'\necho 'from the rc files'\n",
    )
    .unwrap();
    let shell = temp.join("zsh");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\nZDOTDIR='{}' exec /bin/zsh \"$@\"\n",
            temp.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(mise.join("dotnet"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        from_login_shell(shell.as_os_str()),
        Some(mise.join("dotnet"))
    );
    std::fs::remove_dir_all(temp).unwrap();
}
