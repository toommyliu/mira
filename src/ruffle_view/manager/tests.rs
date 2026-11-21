use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui::{AppContext as _, Context, Focusable as _, Size, Window, px, size};
use reqwest::header::HeaderValue;

use super::{
    Fake, Manager, View,
    run::{Runs, Start},
};
use crate::{
    accounts::{Account, AccountId},
    ruffle_view::{
        Login, ScriptPhase, ScriptState, Summary,
        account::{Character, Place},
        account_card::Presence,
        client,
    },
};

const ROSTER: [(&str, &str, u32, &str); 10] = [
    ("wildfox", "Twig", 100, "ArchPaladin"),
    ("xyrafin", "Twig", 100, "Legion Revenant"),
    ("kestrel", "Twig", 100, "Void Highlord"),
    ("marble", "Twig", 97, "Verus DoomKnight"),
    ("lamplight7", "Twig", 100, "Lord Of Order"),
    ("sorrel", "Twig", 88, "Chaos Avenger"),
    ("notwildfox", "Twig", 100, "Dragon of Time"),
    ("forest of tall pines", "Twig", 64, "Chrono ShadowHunter"),
    ("amber", "", 100, "Blaze Binder"),
    ("burner", "Twig", 12, "Rogue"),
];
const DAILY: [&str; 8] = [
    "wildfox",
    "kestrel",
    "sorrel",
    "notwildfox",
    "xyrafin",
    "lamplight7",
    "marble",
    "forestoftallpines",
];

fn script(name: &str) -> PathBuf {
    std::env::temp_dir().join("aqw/scripts").join(name)
}

fn id(username: &str) -> AccountId {
    AccountId::of(username)
}

fn ended(name: &str, phase: ScriptPhase, at: Instant) -> ScriptState {
    ScriptState {
        entry: script(name),
        name: name.into(),
        phase,
        since: Some(at),
    }
}

fn online(username: &str, area: &str, cell: &str, script: Option<ScriptState>) -> Presence {
    let (_, server, level, class) = ROSTER
        .iter()
        .find(|(name, ..)| name.replace(' ', "") == username)
        .copied()
        .unwrap();
    Presence::Open(Summary {
        login: Login::Online,
        place: Some(Place {
            server: Some(server.into()),
            area: Some(area.to_owned().into()),
            cell: Some(cell.to_owned().into()),
        }),
        character: Some(Character {
            level: Some(level),
            class: Some(class.into()),
        }),
        script,
    })
}

fn signing_in(login: Login) -> Presence {
    Presence::Open(Summary {
        login,
        place: None,
        character: None,
        script: None,
    })
}

#[derive(Clone, Copy)]
enum Scene {
    NotStarted,
    MidRun,
    Done,
}

fn fake(scene: Scene, now: Instant) -> (Fake, Option<Instant>) {
    let minutes = |count: u64| now - Duration::from_secs(count * 60);
    let mut presences = HashMap::new();
    let mut finished = HashMap::new();
    let started = match scene {
        Scene::NotStarted => {
            for username in DAILY {
                presences.insert(
                    id(username),
                    Presence::Closed(Some(ended(
                        "daily.ts",
                        ScriptPhase::Finished,
                        minutes(20 * 60),
                    ))),
                );
            }
            None
        }
        Scene::MidRun => {
            let running = |minutes_ago| ScriptState {
                since: Some(minutes(minutes_ago)),
                ..ended("daily.ts", ScriptPhase::Running, now)
            };
            presences.extend([
                (
                    id("wildfox"),
                    online("wildfox", "yulgar", "Enter", Some(running(14))),
                ),
                (
                    id("kestrel"),
                    online("kestrel", "darkoviaforest", "r1", Some(running(9))),
                ),
                (
                    id("notwildfox"),
                    online(
                        "notwildfox",
                        "battleon",
                        "Enter",
                        Some(ended(
                            "daily.ts",
                            ScriptPhase::Failed("Bank is full".into()),
                            minutes(1),
                        )),
                    ),
                ),
                (
                    id("xyrafin"),
                    online(
                        "xyrafin",
                        "tercessuinotlim",
                        "m2",
                        Some(ended("daily.ts", ScriptPhase::NeedsInput, minutes(11))),
                    ),
                ),
                (id("burner"), online("burner", "battleon", "Enter", None)),
                (
                    id("sorrel"),
                    signing_in(Login::Failed("login didn't reach server selection".into())),
                ),
                (id("marble"), signing_in(Login::Loading(Some(46)))),
                (
                    id("forestoftallpines"),
                    Presence::Waiting {
                        place: 1,
                        script: Some(script("daily.ts")),
                        open: None,
                    },
                ),
                (
                    id("lamplight7"),
                    Presence::Closed(Some(ended("daily.ts", ScriptPhase::Finished, minutes(20)))),
                ),
            ]);
            finished.insert(id("lamplight7"), minutes(20));
            Some(minutes(32))
        }
        Scene::Done => {
            for (index, username) in DAILY.into_iter().enumerate() {
                let at = minutes(3 + index as u64 * 5);
                finished.insert(id(username), at);
                let state = ended("daily.ts", ScriptPhase::Finished, at);
                presences.insert(
                    id(username),
                    if index % 2 == 0 {
                        online(username, "battleon", "Enter", Some(state))
                    } else {
                        Presence::Closed(Some(state))
                    },
                );
            }
            Some(minutes(50))
        }
    };
    (
        Fake {
            presences,
            finished,
        },
        started,
    )
}

fn write_accounts(dir: &std::path::Path) -> PathBuf {
    let accounts: Vec<_> = ROSTER
        .iter()
        .map(|(name, server, ..)| {
            let mut account = serde_json::json!({
                "name": name,
                "username": name.replace(' ', ""),
                "password": "saved",
            });
            if !server.is_empty() {
                account["server"] = (*server).into();
            }
            account
        })
        .collect();
    let members: Vec<_> = DAILY
        .iter()
        .map(|username| serde_json::json!({ "username": username }))
        .collect();
    let path = dir.join("accounts.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "accounts": accounts,
            "setups": [
                { "name": "Daily", "script": script("daily.ts"), "members": members },
                {
                    "name": "Warlord farm",
                    "script": script("warlord.ts"),
                    "members": [{ "username": "wildfox" }, { "username": "kestrel" }],
                    "launch": { "newWindow": true, "grid": true },
                },
            ],
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

fn shoot(
    name: &str,
    window_size: Size<gpui::Pixels>,
    scene: Scene,
    view: Option<&str>,
    act: impl Fn(&mut Manager, &mut Window, &mut Context<Manager>) + 'static,
) {
    shoot_with(name, window_size, scene, view, |_, _| {}, act);
}

fn hover_at(x: f32, y: f32) -> impl Fn(&mut gpui::HeadlessAppContext, gpui::AnyWindowHandle) {
    move |cx, owner| {
        for nudge in [0.0, 1.0] {
            cx.update_window(owner, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                        position: gpui::point(px(x + nudge), px(y)),
                        ..Default::default()
                    }),
                    cx,
                );
                window.refresh();
            })
            .unwrap();
            cx.run_until_parked();
        }
        cx.advance_clock(Duration::from_secs(1));
    }
}

fn shoot_with(
    name: &str,
    window_size: Size<gpui::Pixels>,
    scene: Scene,
    view: Option<&str>,
    interact: impl Fn(&mut gpui::HeadlessAppContext, gpui::AnyWindowHandle),
    act: impl Fn(&mut Manager, &mut Window, &mut Context<Manager>) + 'static,
) {
    let dir = std::env::temp_dir().join(format!("mira-manager-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = write_accounts(&dir);
    let view = view.map(str::to_owned);
    for written in mira_ui::snapshot_with(
        name,
        window_size,
        |window, cx| {
            crate::settings::init(dir.join("settings.json"), |_| {}, cx);
            let now = Instant::now();
            let (fake, started) = fake(scene, now);
            cx.new(|cx| {
                let mut manager =
                    Manager::new(path.clone(), HeaderValue::from_static("test"), window, cx);
                manager.fake = Some(fake);
                manager.recent = vec![script("daily.ts"), script("warlord.ts"), script("hunt.ts")];
                if let Some(started) = started {
                    cx.global_mut::<Runs>().start("Daily", started);
                }
                if let Some(view) = &view {
                    manager.view = View::Setup(view.clone());
                }
                act(&mut manager, window, cx);
                manager
            })
        },
        interact,
    ) {
        assert!(written.exists());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

fn opening() -> Size<gpui::Pixels> {
    size(px(1000.0), px(600.0))
}

fn accounts(manager: &Manager, usernames: &[&str]) -> Vec<Account> {
    manager.named(
        &usernames
            .iter()
            .map(|username| id(username))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn snapshot_a_setup_before_during_and_after_a_run() {
    shoot(
        "manager-not-started",
        opening(),
        Scene::NotStarted,
        Some("Daily"),
        |_, _, _| {},
    );
    shoot(
        "manager-mid-run",
        opening(),
        Scene::MidRun,
        Some("Daily"),
        |_, _, _| {},
    );
    shoot(
        "manager-done",
        opening(),
        Scene::Done,
        Some("Daily"),
        |_, _, _| {},
    );
}

#[test]
fn snapshot_manager_hover_states() {
    shoot_with(
        "manager-hover-card",
        opening(),
        Scene::MidRun,
        Some("Daily"),
        hover_at(330.0, 150.0),
        |_, _, _| {},
    );
    shoot_with(
        "manager-hover-account-script",
        size(px(720.0), px(420.0)),
        Scene::MidRun,
        None,
        hover_at(265.0, 129.0),
        |_, _, _| {},
    );
    shoot_with(
        "manager-hover-error",
        opening(),
        Scene::MidRun,
        Some("Daily"),
        hover_at(280.0, 269.0),
        |_, _, _| {},
    );
    shoot_with(
        "manager-hover-long-name",
        opening(),
        Scene::MidRun,
        Some("Daily"),
        hover_at(600.0, 400.0),
        |_, _, _| {},
    );
    shoot_with(
        "manager-hover-setup",
        opening(),
        Scene::MidRun,
        Some("Daily"),
        hover_at(100.0, 162.0),
        |_, _, _| {},
    );
    shoot_with(
        "manager-hover-setup-path",
        size(px(720.0), px(420.0)),
        Scene::NotStarted,
        Some("Daily"),
        hover_at(175.0, 136.0),
        |manager, _, cx| {
            let path = PathBuf::from("/Volumes/Shared/Scripts/farming/daily.ts");
            manager.set_setup_script("Daily", Some(path), cx);
            manager.set_setup_script("Warlord farm", None, cx);
        },
    );
    shoot_with(
        "manager-setup-actions",
        size(px(720.0), px(420.0)),
        Scene::NotStarted,
        None,
        |cx, owner| {
            let (x, y) = (184.0, 115.0);
            hover_at(x, y)(cx, owner);
            cx.update_window(owner, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                        position: gpui::point(px(x), px(y)),
                        button: gpui::MouseButton::Left,
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                );
            })
            .unwrap();
            cx.run_until_parked();
            cx.advance_clock(Duration::from_secs(1));
        },
        |_, _, _| {},
    );
    shoot_with(
        "manager-clear-search",
        opening(),
        Scene::MidRun,
        None,
        hover_at(975.0, 18.0),
        |manager, window, cx| {
            manager.search.update(cx, |search, cx| {
                search.set_text("wildfox", cx);
                search.focus_handle(cx).focus(window, cx);
            });
        },
    );
}

#[test]
fn snapshot_editing_a_setup() {
    shoot(
        "manager-edit",
        opening(),
        Scene::MidRun,
        None,
        |manager, window, cx| {
            manager.view = View::Setup("Daily".into());
            manager.cursor = Some(id("wildfox"));
            manager.checked.insert(id("wildfox"));
            manager.edit_setup("Daily", window, cx)
        },
    );
}

#[test]
fn snapshot_a_selection_at_opening_and_minimum_size() {
    let select = |manager: &mut Manager, _: &mut Window, _: &mut Context<Manager>| {
        manager
            .checked
            .extend(["wildfox", "kestrel", "burner"].map(id));
    };
    shoot("manager-selected", opening(), Scene::MidRun, None, select);
    shoot(
        "manager-selected-min",
        size(px(720.0), px(420.0)),
        Scene::MidRun,
        None,
        select,
    );
}

#[test]
fn snapshot_every_confirmation() {
    shoot(
        "manager-dialog-stop",
        opening(),
        Scene::MidRun,
        Some("Daily"),
        |manager, _, cx| manager.stop_setup(cx),
    );
    shoot(
        "manager-dialog-run",
        opening(),
        Scene::MidRun,
        Some("Daily"),
        |manager, _, cx| {
            let targets = accounts(manager, &["kestrel", "wildfox"]);
            manager.run(targets, script("hunt.ts"), cx)
        },
    );
    shoot(
        "manager-dialog-start",
        opening(),
        Scene::MidRun,
        Some("Daily"),
        |manager, _, cx| {
            if let Some(fake) = &mut manager.fake {
                fake.presences.insert(
                    id("wildfox"),
                    online(
                        "wildfox",
                        "yulgar",
                        "Enter",
                        Some(ended("hunt.ts", ScriptPhase::Running, Instant::now())),
                    ),
                );
            }
            manager.start_current(cx)
        },
    );
    shoot(
        "manager-dialog-close",
        opening(),
        Scene::MidRun,
        None,
        |manager, _, cx| {
            let targets = accounts(manager, &["kestrel", "xyrafin", "burner"]);
            manager.close(targets, cx)
        },
    );
    shoot(
        "manager-dialog-delete",
        opening(),
        Scene::NotStarted,
        Some("Daily"),
        |manager, _, cx| {
            manager.ask(
                super::Ask::Delete {
                    setup: "Daily".into(),
                },
                cx,
            )
        },
    );
    shoot(
        "manager-dialog-account",
        opening(),
        Scene::MidRun,
        None,
        |manager, _, cx| {
            let kestrel = accounts(manager, &["kestrel"]).remove(0);
            manager.servers.list = vec![
                crate::net::Server::sample("Twilly", 846, 1000, 59),
                crate::net::Server::sample("Twig", 451, 1000, 57),
                crate::net::Server::sample("Swordhaven (EU)", 172, 1000, 161),
            ];
            manager.servers.check = super::Check::Done(Instant::now());
            manager.edit(Some(&kestrel), cx)
        },
    );
}

#[test]
fn snapshot_every_card_state() {
    let now = Instant::now();
    let minutes = |count: u64| now - Duration::from_secs(count * 60);
    let playing = |script: Option<ScriptState>| {
        Presence::Open(Summary {
            login: Login::Online,
            place: Some(Place {
                server: Some("Twig".into()),
                area: Some("battleon".into()),
                cell: Some("Enter".into()),
            }),
            character: Some(Character {
                level: Some(100),
                class: Some("ArchPaladin".into()),
            }),
            script,
        })
    };
    let daily = |phase, at| Some(ended("daily.ts", phase, at));
    let states: Vec<(&str, Presence)> = vec![
        ("not-open", Presence::Closed(None)),
        (
            "not-open-ran-before",
            Presence::Closed(daily(ScriptPhase::Finished, minutes(120))),
        ),
        (
            "queued-to-log-in",
            Presence::Waiting {
                place: 2,
                script: Some(script("daily.ts")),
                open: None,
            },
        ),
        ("loading", signing_in(Login::Loading(Some(46)))),
        (
            "logging-in",
            signing_in(Login::LoggingIn { attempt: 1, of: 3 }),
        ),
        (
            "logging-in-again",
            signing_in(Login::LoggingIn { attempt: 2, of: 3 }),
        ),
        (
            "retrying",
            signing_in(Login::Retrying { attempt: 2, of: 3 }),
        ),
        ("reconnecting", signing_in(Login::Reconnecting)),
        ("script-queued", playing(daily(ScriptPhase::Queued, now))),
        ("running", playing(daily(ScriptPhase::Running, minutes(14)))),
        (
            "stopping",
            playing(daily(ScriptPhase::Stopping, minutes(14))),
        ),
        ("idle", playing(None)),
        (
            "finished-before-run",
            playing(daily(ScriptPhase::Finished, minutes(90))),
        ),
        ("stopped", playing(daily(ScriptPhase::Stopped, minutes(3)))),
        ("done", playing(daily(ScriptPhase::Finished, minutes(5)))),
        (
            "needs-input",
            playing(daily(ScriptPhase::NeedsInput, minutes(8))),
        ),
        ("choose-server", signing_in(Login::ChooseServer)),
        ("disconnected", signing_in(Login::Disconnected)),
        (
            "script-failed",
            playing(daily(
                ScriptPhase::Failed("couldn't join house-73675086, the room is full".into()),
                minutes(1),
            )),
        ),
        (
            "login-failed",
            signing_in(Login::Failed("login didn't reach server selection".into())),
        ),
        (
            "couldnt-start",
            signing_in(Login::Broken("the player couldn't start".into())),
        ),
        (
            "couldnt-open",
            Presence::Unopened("Couldn't read accounts.json".into()),
        ),
    ];
    let dir = std::env::temp_dir().join(format!("mira-manager-states-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("accounts.json");
    let accounts: Vec<_> = states
        .iter()
        .map(|(name, _)| {
            serde_json::json!({ "name": name, "username": name, "password": "x", "server": "Twig" })
        })
        .collect();
    let members: Vec<_> = states
        .iter()
        .map(|(name, _)| serde_json::json!({ "username": name }))
        .collect();
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "accounts": accounts,
            "setups": [{ "name": "Every state", "script": script("daily.ts"), "members": members }],
        }))
        .unwrap(),
    )
    .unwrap();
    for written in mira_ui::snapshot(
        "manager-states",
        size(px(1000.0), px(1040.0)),
        |window, cx| {
            crate::settings::init(dir.join("settings.json"), |_| {}, cx);
            let presences = states
                .iter()
                .map(|(name, presence)| (id(name), presence.clone()))
                .collect();
            cx.new(|cx| {
                let mut manager =
                    Manager::new(path.clone(), HeaderValue::from_static("test"), window, cx);
                manager.fake = Some(Fake {
                    presences,
                    finished: HashMap::from([
                        (id("done"), minutes(5)),
                        (id("finished-before-run"), minutes(90)),
                    ]),
                });
                cx.global_mut::<Runs>().start("Every state", minutes(30));
                manager.view = View::Setup("Every state".into());
                manager
            })
        },
    ) {
        assert!(written.exists());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

fn open_manager<'a>(
    name: &str,
    view: View,
    cx: &'a mut gpui::TestAppContext,
) -> (
    gpui::Entity<Manager>,
    &'a mut gpui::VisualTestContext,
    PathBuf,
) {
    let dir = std::env::temp_dir().join(format!("mira-manager-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = write_accounts(&dir);
    cx.update(|cx| {
        mira_ui::init(cx);
        crate::settings::init(dir.join("settings.json"), |_| {}, cx);
        crate::script_inputs::init(dir.join("script-inputs.json"), cx);
    });
    let (manager, cx) = cx.add_window_view(|window, cx| {
        let mut manager = Manager::new(path, HeaderValue::from_static("test"), window, cx);
        manager.fake = Some(fake(Scene::MidRun, Instant::now()).0);
        manager.view = view;
        manager
    });
    (manager, cx, dir)
}

#[gpui::test]
fn setup_rows_preview_paths_on_hover_and_switch_on_click(cx: &mut gpui::TestAppContext) {
    let (manager, cx, dir) = open_manager("setup-row-hover", View::All, cx);
    cx.simulate_resize(size(px(720.0), px(420.0)));
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.run_until_parked();
    let mods = gpui::Modifiers::none();
    let account_script = gpui::point(px(265.0), px(129.0));
    cx.simulate_mouse_move(account_script, None, mods);
    cx.run_until_parked();
    assert!(cx.debug_bounds("card-line-wildfox-card").is_none());
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("card-line-wildfox-card").is_some());
    cx.simulate_mouse_move(gpui::point(px(8.0), px(300.0)), None, mods);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    let daily_name = gpui::point(px(32.0), px(116.0));
    cx.simulate_mouse_move(daily_name, None, mods);
    cx.simulate_click(daily_name, mods);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(manager.read_with(cx, |m, _| m.view == View::Setup("Daily".into())));
    assert!(cx.debug_bounds("setup-path-0-card").is_none());

    let warlord_padding = gpui::point(px(175.0), px(180.0));
    cx.simulate_mouse_move(warlord_padding, None, mods);
    cx.run_until_parked();
    assert!(cx.debug_bounds("setup-path-1-card").is_none());
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("setup-path-1-card").is_some());
    cx.simulate_click(warlord_padding, mods);
    cx.run_until_parked();
    assert!(manager.read_with(cx, |m, _| m.view == View::Setup("Warlord farm".into())));
    assert!(cx.debug_bounds("setup-path-1-card").is_none());

    let daily_actions = gpui::point(px(184.0), px(115.0));
    cx.simulate_mouse_move(daily_actions, None, mods);
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("setup-path-0-card").is_some());
    cx.simulate_click(daily_actions, mods);
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("setup-path-0-card").is_none(),
        "opening the actions menu dismisses the preview"
    );
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("setup-path-0-card").is_none());
    let edit = gpui::point(px(225.0), px(148.0));
    cx.simulate_mouse_move(edit, None, mods);
    cx.simulate_click(edit, mods);
    cx.run_until_parked();
    assert!(manager.read_with(cx, |m, _| {
        m.editing
            .as_ref()
            .is_some_and(|editing| editing.setup == "Daily")
    }));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn reopening_mira_opens_the_configured_window_only_when_all_windows_are_closed() {
    let mut cx = gpui::TestAppContext::single();
    let dir = std::env::temp_dir().join(format!("mira-reopen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let settings = dir.join("settings.json");
    std::fs::write(&settings, r#"{"start":"manager"}"#).unwrap();
    let client = cx.update(|cx| {
        mira_ui::init(cx);
        crate::settings::init(settings, |_| {}, cx);
        client::init(
            dir.clone(),
            dir.join("accounts.json"),
            HeaderValue::from_static("test"),
            crate::army::Armies::new(dir.join("armies")),
            cx,
        );
        client::open(client::Home::Shared, false, cx).unwrap()
    });
    cx.update(client::open_launch_window);
    assert_eq!(cx.windows(), [client.into()]);
    cx.update(|cx| {
        crate::ruffle_view::open_packets(
            client.entity(cx).unwrap(),
            crate::ruffle_view::Target::Selected,
            cx,
        );
    });
    assert_eq!(cx.windows().len(), 2);
    client
        .update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    assert!(cx.windows().is_empty());

    cx.update(client::open_launch_window);
    let [manager] = cx.windows().try_into().unwrap();
    assert!(manager.downcast::<Manager>().is_some());
    cx.update(client::open_launch_window);
    assert_eq!(cx.windows(), [manager]);
    cx.quit();
    std::fs::remove_dir_all(dir).unwrap();
}

#[gpui::test]
fn start_with_remembers_the_script_only_once_the_start_goes_ahead(cx: &mut gpui::TestAppContext) {
    let (manager, cx, dir) = open_manager("start-with", View::All, cx);
    let remembered = |cx: &mut gpui::VisualTestContext| {
        manager.read_with(cx, |manager, _| {
            manager
                .setup_named("Daily")
                .and_then(|setup| setup.script.clone())
        })
    };
    manager.update(cx, |manager, cx| {
        manager.start_with("Daily", script("hunt.ts"), cx);
        manager.dismiss(cx);
    });
    assert_eq!(remembered(cx), Some(script("daily.ts")));
    cx.update(|window, cx| {
        manager.update(cx, |manager, cx| {
            manager.start_with("Daily", script("hunt.ts"), cx);
            manager.confirm(false, window, cx);
        })
    });
    assert_eq!(remembered(cx), Some(script("hunt.ts")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[gpui::test]
fn arrow_keys_in_a_setup_being_edited_scroll_the_cursor_into_view(cx: &mut gpui::TestAppContext) {
    let (manager, cx, dir) = open_manager("edit-keys", View::Setup("Daily".into()), cx);
    cx.simulate_resize(size(px(720.0), px(420.0)));
    cx.update(|window, cx| {
        manager.update(cx, |manager, cx| {
            manager.edit_setup("Daily", window, cx);
            window.focus(&manager.focus, cx);
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("down down down down");
    cx.run_until_parked();
    let cursor_in_view = |cx: &mut gpui::VisualTestContext| {
        manager.read_with(cx, |manager, _| {
            let setup = manager.setup_named("Daily").unwrap();
            let index = manager
                .members(setup)
                .iter()
                .position(|account| manager.cursor.as_ref() == Some(&account.id()))
                .unwrap();
            let scroll = &manager.grid_scroll;
            let (view, shift) = (scroll.bounds(), scroll.offset().y);
            scroll.bounds_for_item(index).is_some_and(|card| {
                card.top() + shift >= view.top() && card.bottom() + shift <= view.bottom()
            })
        })
    };
    let scrolled = manager.read_with(cx, |manager, _| manager.grid_scroll.offset().y);
    assert!(scrolled < px(0.0));
    assert!(cursor_in_view(cx));
    manager.update(cx, |manager, _| manager.cursor = None);
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert!(cursor_in_view(cx));
    std::fs::remove_dir_all(dir).unwrap();
}

#[gpui::test]
fn the_edit_menu_select_all_checks_every_shown_account(cx: &mut gpui::TestAppContext) {
    let (manager, cx, dir) = open_manager("select-all", View::All, cx);
    cx.update(|window, cx| {
        let focus = manager.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    let select_all = mira_ui::primitives::text_field::SelectAll;
    assert!(cx.update(|window, cx| window.is_action_available(&select_all, cx)));
    cx.dispatch_action(select_all);
    manager.read_with(cx, |manager, cx| {
        assert!(!manager.checked.is_empty());
        assert_eq!(manager.checked.len(), manager.shown(cx).len());
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[gpui::test]
fn a_setup_set_to_open_in_a_new_window_launches_there_and_leaves_open_accounts_where_they_are(
    cx: &mut gpui::TestAppContext,
) {
    let (manager, cx, dir) = open_manager("new-window", View::All, cx);
    cx.update(|_, cx| {
        client::init(
            dir.clone(),
            dir.join("accounts.json"),
            HeaderValue::from_static("test"),
            crate::army::Armies::new(dir.join("armies")),
            cx,
        )
    });
    let start = |cx: &mut gpui::VisualTestContext| {
        manager.update(cx, |manager, cx| {
            manager.start_setup("Warlord farm", Start::Fresh, None, None, cx)
        });
    };
    let window = |username: &str, cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| client::holding(&id(username), cx).map(|game| game.window_id()))
    };
    manager.update(cx, |manager, cx| {
        manager.fake = None;
        let alone = accounts(manager, &["wildfox", "burner"]);
        manager.launch(alone, cx);
    });
    start(cx);
    let shared = window("burner", cx).unwrap();
    let setup = window("kestrel", cx).unwrap();
    assert_ne!(setup, shared);
    assert_eq!(window("wildfox", cx), Some(shared));
    manager.update(cx, |manager, cx| {
        let kestrel = accounts(manager, &["kestrel"]);
        manager.close_now(kestrel, cx);
    });
    assert_eq!(window("kestrel", cx), None);
    start(cx);
    assert_eq!(window("kestrel", cx), Some(setup));
    assert_eq!(cx.update(|_, cx| client::clients(cx).len()), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn snapshot_an_update_in_the_sidebar() {
    shoot(
        "manager-update-ready",
        size(px(720.0), px(420.0)),
        Scene::NotStarted,
        Some("Daily"),
        |_, _, cx| {
            cx.set_global(crate::updates::Updates::ready_preview(
                semver::Version::new(0, 2, 0),
            ))
        },
    );
}

#[test]
fn snapshot_stress() {
    let now = Instant::now();
    let minutes = |count: u64| now - Duration::from_secs(count * 60);
    let long_script = "legion-revenant-fealty-and-exalted-crown-farm.ts";
    let playing = |class: &str, area: &str, script: Option<ScriptState>| {
        Presence::Open(Summary {
            login: Login::Online,
            place: Some(Place {
                server: Some("Swordhaven (EU)".into()),
                area: Some(area.to_owned().into()),
                cell: Some("Enter".into()),
            }),
            character: Some(Character {
                level: Some(100),
                class: Some(class.to_owned().into()),
            }),
            script,
        })
    };
    let state = |phase, at| Some(ended(long_script, phase, at));
    let states: Vec<(&str, &str, Presence)> = vec![
        (
            "The Legendary Hero Of Battleon And Beyond",
            "legendaryhero",
            playing(
                "Chrono ShadowSlayer of the Eternal Void",
                "tercessuinotlim-99999",
                state(ScriptPhase::Running, minutes(60 * 27)),
            ),
        ),
        (
            "xXx_SuperLongUsernameWithoutAnySpaces_xXx",
            "xxxsuper",
            playing(
                "Legion Revenant",
                "battleon",
                state(
                    ScriptPhase::Failed(
                        "couldn't join house-73675086, the room is full and the fallback room house-73675087 is full too".into(),
                    ),
                    minutes(1),
                ),
            ),
        ),
        (
            "a",
            "a",
            playing("Rogue", "battleon", state(ScriptPhase::NeedsInput, minutes(8))),
        ),
        (
            "Wwwwwwwwwwwwwwwwwwwwwwwwwwwwww",
            "wide",
            Presence::Waiting {
                place: 128,
                script: Some(script(long_script)),
                open: None,
            },
        ),
        (
            "needs a server",
            "needsserver",
            signing_in(Login::ChooseServer),
        ),
        (
            "Another Extremely Long Display Name Here",
            "anotherlong",
            signing_in(Login::Failed(
                "login didn't reach server selection after three attempts and the session expired".into(),
            )),
        ),
        (
            "finished long ago",
            "finishedlong",
            Presence::Closed(state(ScriptPhase::Finished, minutes(60 * 24 * 12))),
        ),
        (
            "stopping one",
            "stoppingone",
            playing("ArchPaladin", "battleon", state(ScriptPhase::Stopping, minutes(14))),
        ),
        ("disconnected one", "disconnectedone", signing_in(Login::Disconnected)),
        ("loading", "loadingone", signing_in(Login::Loading(Some(100)))),
    ];
    let dir = std::env::temp_dir().join(format!("mira-manager-stress-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("accounts.json");
    let accounts: Vec<_> = states
        .iter()
        .map(|(name, username, _)| {
            serde_json::json!({ "name": name, "username": username, "password": "x", "server": "Swordhaven (EU)" })
        })
        .collect();
    let members: Vec<_> = states
        .iter()
        .map(|(_, username, _)| serde_json::json!({ "username": username }))
        .collect();
    let long_setup = "Daily quests, weekly bosses and the seasonal event farm";
    let mut setups = vec![
        serde_json::json!({ "name": long_setup, "script": script(long_script), "members": members }),
        serde_json::json!({ "name": "No script", "members": [] }),
    ];
    for index in 0..14 {
        setups.push(serde_json::json!({
            "name": format!("Setup {index}"),
            "script": script("daily.ts"),
            "members": [{ "username": "a" }],
        }));
    }
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({ "accounts": accounts, "setups": setups })).unwrap(),
    )
    .unwrap();
    type Act = fn(&mut Manager, &mut Window, &mut Context<Manager>);
    let cases: [(&str, (f32, f32), Option<&str>, Act); 8] = [
        (
            "stress-manager-not-started",
            (1000.0, 600.0),
            Some("Setup 0"),
            |_, _, _| {},
        ),
        (
            "stress-manager-setup",
            (1000.0, 600.0),
            Some(long_setup),
            |_, _, _| {},
        ),
        (
            "stress-manager-setup-min",
            (720.0, 420.0),
            Some(long_setup),
            |_, _, _| {},
        ),
        ("stress-manager-all-min", (720.0, 420.0), None, |_, _, _| {}),
        (
            "stress-manager-selected-min",
            (720.0, 420.0),
            Some(long_setup),
            |manager, _, _| {
                manager
                    .checked
                    .extend(["legendaryhero", "xxxsuper", "a", "wide"].map(id));
            },
        ),
        (
            "stress-manager-edit-min",
            (720.0, 420.0),
            Some(long_setup),
            |manager, window, cx| {
                manager.edit_setup(
                    "Daily quests, weekly bosses and the seasonal event farm",
                    window,
                    cx,
                )
            },
        ),
        (
            "stress-manager-empty-setup",
            (720.0, 420.0),
            Some("No script"),
            |_, _, _| {},
        ),
        (
            "stress-manager-search-none",
            (720.0, 420.0),
            None,
            |manager, _, cx| {
                manager
                    .search
                    .update(cx, |search, cx| search.set_text("zzzz", cx));
            },
        ),
    ];
    for (name, (w, h), view, act) in cases {
        let view = view.map(str::to_owned);
        for written in mira_ui::snapshot(name, size(px(w), px(h)), |window, cx| {
            crate::settings::init(dir.join("settings.json"), |_| {}, cx);
            let presences = states
                .iter()
                .map(|(_, username, presence)| (id(username), presence.clone()))
                .collect();
            cx.new(|cx| {
                let mut manager =
                    Manager::new(path.clone(), HeaderValue::from_static("test"), window, cx);
                manager.fake = Some(Fake {
                    presences,
                    finished: HashMap::new(),
                });
                cx.global_mut::<Runs>().start(long_setup, minutes(60 * 30));
                if let Some(view) = &view {
                    manager.view = View::Setup(view.clone());
                }
                act(&mut manager, window, cx);
                manager
            })
        }) {
            assert!(written.exists());
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
