use std::{cell::RefCell, rc::Rc, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{
    sync::watch,
    time::{Instant, sleep},
};

use crate::bridge::{self, CallError, Event, Lagged, Loader, Subscription};
use crate::domain::auth::{
    self, ConnectError, Credentials, LoginError, LoginOutcome, Logins, Refusal,
};

const DIALOG_POLL: Duration = Duration::from_secs(1);
const STUCK_DIALOG: Duration = Duration::from_secs(30);
const LOST_DIALOG: [&str; 3] = ["has been lost", "restart", "maintenance"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Settings {
    #[serde(skip)]
    pub(crate) enabled: bool,
    #[serde(skip)]
    pub(crate) server: Option<String>,
    pub(crate) delay_seconds: u32,
    pub(crate) retries: u32,
    pub(crate) backoff_seconds: u32,
    pub(crate) backoff_cap_seconds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            server: None,
            delay_seconds: 3,
            retries: 3,
            backoff_seconds: 5,
            backoff_cap_seconds: 60,
        }
    }
}

impl Settings {
    pub(crate) const MAX_DELAY_SECONDS: u32 = 300;
    pub(crate) const MAX_RETRIES: u32 = 10;
    pub(crate) const MAX_BACKOFF_SECONDS: u32 = 600;

    pub(crate) fn normalized(self) -> Self {
        let backoff_seconds = self.backoff_seconds.clamp(1, Self::MAX_BACKOFF_SECONDS);
        Self {
            enabled: self.enabled,
            server: self
                .server
                .map(|server| server.trim().to_owned())
                .filter(|server| !server.is_empty()),
            delay_seconds: self.delay_seconds.min(Self::MAX_DELAY_SECONDS),
            retries: self.retries.min(Self::MAX_RETRIES),
            backoff_seconds,
            backoff_cap_seconds: self
                .backoff_cap_seconds
                .clamp(backoff_seconds, Self::MAX_BACKOFF_SECONDS),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Phase {
    Off,
    Armed,
    Waiting,
    Attempt { attempt: u32, of: u32 },
    Backoff { attempt: u32, of: u32 },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Report {
    Phase(Phase),
    Servers(Vec<String>),
}

pub(crate) fn wants(event: &Event) -> bool {
    matches!(event, Event::Connection { .. })
}

struct Dialog {
    text: String,
    since: Instant,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AttemptError {
    #[error(transparent)]
    Login(#[from] LoginError),
    #[error(transparent)]
    Connect(#[from] ConnectError),
    #[error(transparent)]
    Call(#[from] CallError),
}

impl AttemptError {
    fn retryable(&self) -> bool {
        match self {
            Self::Connect(error) => error.retryable(),
            Self::Login(_) | Self::Call(_) => true,
        }
    }
}

enum Wake {
    Recovered(Result<(), AttemptError>),
    Event(Result<Event, Lagged>),
    Settings { open: bool },
    Dialog,
}

struct Reporter<F> {
    report: F,
    phase: RefCell<Option<Phase>>,
}

impl<F: Fn(Report)> Reporter<F> {
    fn phase(&self, phase: Phase) {
        if self.phase.borrow().as_ref() != Some(&phase) {
            self.phase.replace(Some(phase.clone()));
            (self.report)(Report::Phase(phase));
        }
    }
}

pub(crate) async fn run(
    loader: impl Loader,
    logins: Rc<Logins>,
    connections: Subscription,
    mut settings: watch::Receiver<Settings>,
    report: impl Fn(Report),
) {
    let reporter = Reporter {
        report,
        phase: RefCell::default(),
    };
    let mut connected = bridge::auth::is_logged_in(&loader).unwrap_or(false);
    let mut failure: Option<String> = None;
    let mut dialog: Option<Dialog> = None;
    loop {
        let current = settings.borrow_and_update().clone();
        let recovering = (current.enabled && !connected && failure.is_none())
            .then(|| logins.last())
            .flatten();
        let wake = if let Some(last) = recovering {
            let server = current.server.as_deref().unwrap_or(&last.server);
            let wake = tokio::select! {
                result = recover(&loader, &logins, &last.credentials, Some(server), &current, |phase| reporter.phase(phase)) => Wake::Recovered(result),
                changed = settings.changed() => Wake::Settings { open: changed.is_ok() },
            };
            connections.clear();
            dialog = None;
            connected = bridge::auth::is_logged_in(&loader).unwrap_or(false);
            wake
        } else {
            reporter.phase(match (&failure, current.enabled) {
                (_, false) => Phase::Off,
                (Some(failure), true) => Phase::Failed(failure.clone()),
                (None, true) => Phase::Armed,
            });
            let watching = current.enabled && connected;
            tokio::select! {
                event = connections.next() => Wake::Event(event),
                changed = settings.changed() => Wake::Settings { open: changed.is_ok() },
                () = sleep(DIALOG_POLL), if watching => Wake::Dialog,
            }
        };
        match wake {
            Wake::Settings { open: false } => return,
            Wake::Settings { open: true } => failure = None,
            Wake::Event(Ok(Event::Connection { status })) => match status.as_str() {
                "OnConnection" => {
                    connected = true;
                    failure = None;
                    dialog = None;
                    logins.connected(&loader);
                    if let Ok(servers) = auth::servers(&loader)
                        && !servers.is_empty()
                    {
                        (reporter.report)(Report::Servers(
                            servers.into_iter().map(|server| server.name).collect(),
                        ));
                    }
                }
                "OnConnectionLost" => {
                    connected = false;
                    failure = None;
                }
                _ => {}
            },
            Wake::Event(Ok(_)) => {}
            Wake::Event(Err(Lagged)) => {
                connected = bridge::auth::is_logged_in(&loader).unwrap_or(false);
            }
            Wake::Recovered(Ok(())) => connected = true,
            Wake::Recovered(Err(error)) => {
                tracing::warn!(%error, "auto relogin gave up");
                failure = Some(error.to_string());
            }
            Wake::Dialog => match auth::connection_dialog(&loader) {
                Ok(Some(text)) => {
                    let since = match dialog {
                        Some(Dialog { text: seen, since }) if seen == text => since,
                        _ => Instant::now(),
                    };
                    let lower = text.to_lowercase();
                    if LOST_DIALOG.iter().any(|phrase| lower.contains(phrase))
                        || since.elapsed() >= STUCK_DIALOG
                    {
                        tracing::info!(dialog = %text, "auto relogin found the game stuck on a connection dialog");
                        connected = false;
                        failure = None;
                        dialog = None;
                    } else {
                        dialog = Some(Dialog { text, since });
                    }
                }
                Ok(None) | Err(_) => dialog = None,
            },
        }
    }
}

pub(crate) async fn recover(
    loader: &impl Loader,
    logins: &Logins,
    credentials: &Credentials,
    server: Option<&str>,
    settings: &Settings,
    phase: impl Fn(Phase),
) -> Result<(), AttemptError> {
    phase(Phase::Waiting);
    sleep(Duration::from_secs(settings.delay_seconds.into())).await;
    let of = settings.retries + 1;
    let cap = Duration::from_secs(settings.backoff_cap_seconds.into());
    let mut backoff = Duration::from_secs(settings.backoff_seconds.into()).min(cap);
    let mut attempt = 1;
    loop {
        logins.space_out().await;
        phase(Phase::Attempt { attempt, of });
        let turn = logins.turn().await;
        let result = attempt_once(loader, credentials, server).await;
        drop(turn);
        match result {
            Ok(()) => return Ok(()),
            Err(error) if error.retryable() && attempt < of => {
                tracing::warn!(%error, attempt, "auto relogin attempt failed");
                phase(Phase::Backoff { attempt, of });
                sleep(backoff).await;
                backoff = (backoff * 2).min(cap);
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn attempt_once(
    loader: &impl Loader,
    credentials: &Credentials,
    server: Option<&str>,
) -> Result<(), AttemptError> {
    if auth::in_game(loader)? {
        return Ok(());
    }
    if bridge::auth::is_logged_in(loader)? || auth::connection_dialog(loader)?.is_some() {
        bridge::auth::logout(loader)?;
    }
    if !auth::server_selection_shown(loader)?
        && auth::login(loader, credentials).await? == LoginOutcome::InGame
    {
        return Ok(());
    }
    let Some(server) = server else {
        return Ok(());
    };
    match auth::connect(loader, server).await {
        Ok(()) => Ok(()),
        Err(
            error @ (ConnectError::Refused {
                refusal: Refusal::Full,
                ..
            }
            | ConnectError::PlayerUnready),
        ) => {
            bridge::auth::logout(loader)?;
            Err(error.into())
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    use tokio::{task::LocalSet, time::Instant};

    use super::*;
    use crate::bridge::{Events, Value};
    use crate::domain::testing::FakeLoader;

    #[derive(Default)]
    pub(crate) struct Game {
        pub(crate) connected: bool,
        pub(crate) listed: bool,
        pub(crate) dialog: Option<String>,
        pub(crate) failing_logins: u32,
        failing_connects: u32,
        refusals: HashMap<String, Vec<(&'static str, &'static str)>>,
        pub(crate) logins: Vec<Duration>,
        pub(crate) connects: Vec<String>,
        logouts: u32,
    }

    pub(crate) struct World {
        pub(crate) game: Rc<RefCell<Game>>,
        pub(crate) events: Rc<Events>,
        start: Instant,
    }

    impl World {
        pub(crate) fn new() -> (Self, FakeLoader) {
            let game = Rc::new(RefCell::new(Game::default()));
            let events = Rc::new(Events::default());
            let start = Instant::now();
            let state = game.clone();
            let publish = events.clone();
            let loader = FakeLoader::new(move |callback, args| {
                let mut game = state.borrow_mut();
                let text = |value: &str| Value::String(value.to_owned());
                let object = |fields: &[(&str, &str)]| {
                    Value::Object(
                        fields
                            .iter()
                            .map(|(key, value)| ((*key).to_owned(), text(value)))
                            .collect(),
                    )
                };
                Ok(match callback {
                    "auth.isLoggedIn" => Value::Bool(game.connected),
                    "world.isLoaded" => Value::Bool(game.connected && game.dialog.is_none()),
                    "auth.isTemporarilyKicked" => Value::Bool(false),
                    "flash.isNull" => Value::Bool(!game.listed),
                    "flash.getConnMcText" => text(game.dialog.as_deref().unwrap_or("null")),
                    "auth.getServers" => Value::List(vec![
                        object(&[("sName", "Twilly")]),
                        object(&[("sName", "Yorumi")]),
                    ]),
                    "flash.getGameObjectS" if args == [text("loginInfo.strUsername")] => {
                        text(r#""player1""#)
                    }
                    "flash.getGameObjectS" if args == [text("loginInfo.strPassword")] => {
                        text(r#""hunter2""#)
                    }
                    "flash.getGameObject" if args == [text("objServerInfo.sName")] => {
                        text(r#""Twilly""#)
                    }
                    "auth.login" => {
                        game.logins.push(start.elapsed());
                        if game.failing_logins > 0 {
                            game.failing_logins -= 1;
                        } else {
                            game.listed = true;
                        }
                        Value::Undefined
                    }
                    "auth.connectTo" => {
                        let Value::String(server) = &args[0] else {
                            panic!("connectTo needs a server");
                        };
                        game.connects.push(server.clone());
                        let refusal = game
                            .refusals
                            .get_mut(server)
                            .filter(|refusals| !refusals.is_empty())
                            .map(|refusals| refusals.remove(0));
                        if let Some((reason, message)) = refusal {
                            return Ok(object(&[
                                ("status", "blocked"),
                                ("reason", reason),
                                ("message", message),
                            ]));
                        }
                        game.listed = false;
                        if game.failing_connects > 0 {
                            game.failing_connects -= 1;
                            drop(game);
                            publish.publish(&connection("OnConnectionFailed"));
                            return Ok(object(&[("status", "selected")]));
                        }
                        game.connected = true;
                        drop(game);
                        publish.publish(&connection("OnConnection"));
                        object(&[("status", "selected")])
                    }
                    "auth.logout" => {
                        game.logouts += 1;
                        let was_connected = std::mem::take(&mut game.connected);
                        game.listed = false;
                        game.dialog = None;
                        drop(game);
                        if was_connected {
                            publish.publish(&connection("OnConnectionLost"));
                        }
                        Value::Undefined
                    }
                    other => panic!("unexpected call {other}"),
                })
            });
            (
                World {
                    game,
                    events,
                    start,
                },
                loader,
            )
        }

        pub(crate) fn connect(&self) {
            self.game.borrow_mut().connected = true;
            self.events.publish(&connection("OnConnection"));
        }

        pub(crate) fn lose_connection(&self) {
            self.game.borrow_mut().connected = false;
            self.events.publish(&connection("OnConnectionLost"));
        }

        fn since_start(&self) -> Duration {
            self.start.elapsed()
        }
    }

    fn connection(status: &str) -> Event {
        Event::Connection {
            status: status.into(),
        }
    }

    fn enabled() -> Settings {
        Settings {
            enabled: true,
            ..Settings::default()
        }
    }

    struct Running {
        world: World,
        loader: Rc<FakeLoader>,
        logins: Rc<Logins>,
        settings: watch::Sender<Settings>,
        reports: Rc<RefCell<Vec<Report>>>,
    }

    fn start(settings: Settings) -> Running {
        let (world, loader) = World::new();
        let loader = Rc::new(loader);
        let logins = Rc::new(Logins::default());
        let (sender, receiver) = watch::channel(settings);
        let reports = Rc::new(RefCell::new(Vec::new()));
        let seen = reports.clone();
        let connections = world.events.subscribe(wants);
        tokio::task::spawn_local(run(
            loader.clone(),
            logins.clone(),
            connections,
            receiver,
            move |report| seen.borrow_mut().push(report),
        ));
        Running {
            world,
            loader,
            logins,
            settings: sender,
            reports,
        }
    }

    async fn after(seconds: u64) {
        sleep(Duration::from_secs(seconds)).await;
    }

    fn last_phase(running: &Running) -> Option<Phase> {
        running
            .reports
            .borrow()
            .iter()
            .rev()
            .find_map(|report| match report {
                Report::Phase(phase) => Some(phase.clone()),
                Report::Servers(_) => None,
            })
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_connection_logs_back_in_to_the_captured_server() {
        LocalSet::new()
            .run_until(async {
                let running = start(enabled());
                after(1).await;
                running.world.connect();
                after(1).await;
                running.world.lose_connection();
                after(30).await;
                let game = running.world.game.borrow();
                assert_eq!(game.logins.len(), 1);
                assert_eq!(game.connects, ["Twilly"]);
                assert!(game.connected);
                drop(game);
                assert_eq!(last_phase(&running), Some(Phase::Armed));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_attempt_waits_for_the_configured_delay() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings {
                    delay_seconds: 10,
                    ..enabled()
                });
                running.world.connect();
                after(1).await;
                let lost_at = running.world.since_start();
                running.world.lose_connection();
                after(30).await;
                let login = running.world.game.borrow().logins[0];
                assert_eq!(login - lost_at, Duration::from_secs(10));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn failed_attempts_back_off_exponentially_up_to_the_cap() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings {
                    delay_seconds: 0,
                    retries: 4,
                    backoff_seconds: 5,
                    backoff_cap_seconds: 12,
                    ..enabled()
                });
                running.world.connect();
                after(1).await;
                running.world.game.borrow_mut().failing_logins = 4;
                running.world.lose_connection();
                after(300).await;
                let logins = running.world.game.borrow().logins.clone();
                let gaps: Vec<_> = logins.windows(2).map(|pair| pair[1] - pair[0]).collect();
                let outcome_wait = Duration::from_secs(15);
                assert_eq!(
                    gaps,
                    [5, 10, 12, 12].map(|seconds| outcome_wait + Duration::from_secs(seconds))
                );
                assert_eq!(running.world.game.borrow().connects, ["Twilly"]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn it_stops_after_its_retries_and_reports_why() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings {
                    delay_seconds: 0,
                    retries: 2,
                    ..enabled()
                });
                running.world.connect();
                after(1).await;
                running.world.game.borrow_mut().failing_logins = u32::MAX;
                running.world.lose_connection();
                after(600).await;
                assert_eq!(running.world.game.borrow().logins.len(), 3);
                assert_eq!(
                    last_phase(&running),
                    Some(Phase::Failed(LoginError::NoServerSelection.to_string()))
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_happens_while_it_is_off() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings::default());
                running.world.connect();
                after(1).await;
                running.world.lose_connection();
                after(60).await;
                assert!(running.world.game.borrow().logins.is_empty());
                assert_eq!(last_phase(&running), Some(Phase::Off));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn turning_it_on_while_disconnected_recovers() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings::default());
                running.world.connect();
                after(1).await;
                running.world.lose_connection();
                after(60).await;
                running.settings.send(enabled()).unwrap();
                after(30).await;
                assert_eq!(running.world.game.borrow().connects, ["Twilly"]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_settings_change_while_waiting_applies_the_new_delay() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings {
                    delay_seconds: 120,
                    ..enabled()
                });
                running.world.connect();
                after(1).await;
                running.world.lose_connection();
                after(5).await;
                let changed_at = running.world.since_start();
                running
                    .settings
                    .send(Settings {
                        delay_seconds: 2,
                        ..enabled()
                    })
                    .unwrap();
                after(30).await;
                let login = running.world.game.borrow().logins[0];
                assert_eq!(login - changed_at, Duration::from_secs(2));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_script_logs_back_in_to_another_server_with_the_captured_login() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings::default());
                running.world.connect();
                after(1).await;
                running.logins.log_out(&running.loader).await.unwrap();
                assert_eq!(running.logins.log_in(&running.loader).await, Ok(true));
                assert_eq!(
                    running.logins.connect(&running.loader, " Yorumi ").await,
                    Ok(())
                );
                let game = running.world.game.borrow();
                assert_eq!(game.logins.len(), 1);
                assert_eq!(game.connects, ["Yorumi"]);
                assert!(game.connected);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_captured_login_it_does_not_try() {
        LocalSet::new()
            .run_until(async {
                let running = start(enabled());
                running.world.game.borrow_mut().connected = false;
                running.world.lose_connection();
                after(60).await;
                assert!(running.world.game.borrow().logins.is_empty());
            })
            .await;
    }

    #[test]
    fn settings_outside_their_limits_are_clamped() {
        let settings = Settings {
            enabled: true,
            server: Some("  ".into()),
            delay_seconds: 9000,
            retries: 200,
            backoff_seconds: 0,
            backoff_cap_seconds: 0,
        }
        .normalized();
        assert_eq!(
            settings,
            Settings {
                enabled: true,
                server: None,
                delay_seconds: Settings::MAX_DELAY_SECONDS,
                retries: Settings::MAX_RETRIES,
                backoff_seconds: 1,
                backoff_cap_seconds: 1,
            }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_chosen_server_is_used_instead_of_the_captured_one() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings {
                    server: Some("Yorumi".into()),
                    ..enabled()
                });
                running.world.connect();
                after(1).await;
                running.world.lose_connection();
                after(30).await;
                assert_eq!(running.world.game.borrow().connects, ["Yorumi"]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_refusal_that_retrying_cannot_fix_stops_at_once() {
        LocalSet::new()
            .run_until(async {
                let running = start(enabled());
                running.world.connect();
                after(1).await;
                running.world.game.borrow_mut().refusals.insert(
                    "Twilly".into(),
                    vec![
                        (
                            "member-only",
                            "account is not authorized for member-only servers"
                        );
                        5
                    ],
                );
                running.world.lose_connection();
                after(300).await;
                assert_eq!(running.world.game.borrow().connects.len(), 1);
                assert_eq!(
                    last_phase(&running),
                    Some(Phase::Failed(
                        "account is not authorized for member-only servers".into()
                    ))
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_server_is_retried_after_logging_out() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings {
                    delay_seconds: 0,
                    ..enabled()
                });
                running.world.connect();
                after(1).await;
                running
                    .world
                    .game
                    .borrow_mut()
                    .refusals
                    .insert("Twilly".into(), vec![("full", "server is full")]);
                running.world.lose_connection();
                after(60).await;
                let game = running.world.game.borrow();
                assert_eq!(game.connects, ["Twilly", "Twilly"]);
                assert!(game.logouts >= 1);
                assert!(game.connected);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_lost_dialog_triggers_relogin_without_an_event() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings {
                    delay_seconds: 0,
                    ..enabled()
                });
                running.world.connect();
                after(1).await;
                {
                    let mut game = running.world.game.borrow_mut();
                    game.connected = false;
                    game.dialog = Some("Connection to the server has been lost.".into());
                }
                after(30).await;
                let game = running.world.game.borrow();
                assert!(game.logouts >= 1, "the dialog is cleared first");
                assert_eq!(game.logins.len(), 1);
                assert!(game.connected);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_dialog_stuck_on_the_same_text_triggers_relogin() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings {
                    delay_seconds: 0,
                    ..enabled()
                });
                running.world.connect();
                after(1).await;
                running.world.game.borrow_mut().dialog = Some("Loading Map...".into());
                after(20).await;
                assert!(running.world.game.borrow().logins.is_empty());
                after(40).await;
                let game = running.world.game.borrow();
                assert!(game.logouts >= 1);
                assert_eq!(game.logins.len(), 1);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_brief_loading_dialog_is_left_alone() {
        LocalSet::new()
            .run_until(async {
                let running = start(enabled());
                running.world.connect();
                after(1).await;
                running.world.game.borrow_mut().dialog = Some("Loading Map...".into());
                after(5).await;
                running.world.game.borrow_mut().dialog = None;
                after(60).await;
                let game = running.world.game.borrow();
                assert_eq!(game.logouts, 0);
                assert!(game.logins.is_empty());
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn servers_seen_at_login_are_reported() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings::default());
                running.world.connect();
                after(1).await;
                assert!(
                    running
                        .reports
                        .borrow()
                        .contains(&Report::Servers(vec!["Twilly".into(), "Yorumi".into()]))
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_connection_uses_an_attempt_instead_of_restarting() {
        LocalSet::new()
            .run_until(async {
                let running = start(Settings {
                    delay_seconds: 0,
                    retries: 1,
                    ..enabled()
                });
                running.world.connect();
                after(1).await;
                running.world.game.borrow_mut().failing_connects = u32::MAX;
                running.world.lose_connection();
                after(300).await;
                assert_eq!(running.world.game.borrow().connects.len(), 2);
                assert!(matches!(last_phase(&running), Some(Phase::Failed(_))));
            })
            .await;
    }
}
