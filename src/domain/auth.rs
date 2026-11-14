use std::{cell::RefCell, time::Duration};

use serde::Serialize;
use tokio::{
    sync::{Mutex, MutexGuard},
    time::{Instant, sleep},
};

use super::poll;
use crate::bridge::{self, CallError, Loader, Value};

const POLL: Duration = Duration::from_millis(500);
const KICK_DEFAULT: Duration = Duration::from_secs(60);
const KICK_LIMIT: Duration = Duration::from_secs(70);
const KICK_RECHECK: Duration = Duration::from_secs(5);
const LOGIN_OUTCOME: Duration = Duration::from_secs(15);
const SELECTION_RETRY: Duration = Duration::from_secs(5);
const PLAYER_READY: Duration = Duration::from_secs(10);

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Credentials {
    pub(crate) username: String,
    pub(crate) password: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginOutcome {
    ServerSelection,
    InGame,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub(crate) enum LoginError {
    #[error("the login screen stayed locked by a temporary kick")]
    StillKicked,
    #[error("login didn't reach server selection")]
    NoServerSelection,
    #[error(transparent)]
    Call(#[from] CallError),
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub(crate) enum ConnectError {
    #[error("{message}")]
    Refused { refusal: Refusal, message: String },
    #[error("server selection wasn't ready")]
    SelectionNotReady,
    #[error("the player didn't become ready")]
    PlayerUnready,
    #[error(transparent)]
    Call(#[from] CallError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Refusal {
    Full,
    Offline,
    NotFound,
    MembersOnly,
    ChatRestricted,
    EmailUnconfirmed,
    TestClientOnly,
    Other,
}

impl Refusal {
    fn parse(reason: Option<&str>, status: Option<&str>) -> Self {
        match (reason, status) {
            (Some("full"), _) => Self::Full,
            (Some("offline"), _) => Self::Offline,
            (Some("member-only"), _) => Self::MembersOnly,
            (Some("chat-restricted" | "underage-chat"), _) => Self::ChatRestricted,
            (Some("email-unconfirmed"), _) => Self::EmailUnconfirmed,
            (Some("test-client-required"), _) => Self::TestClientOnly,
            (_, Some("not-found")) => Self::NotFound,
            _ => Self::Other,
        }
    }
}

impl ConnectError {
    pub(crate) fn retryable(&self) -> bool {
        match self {
            Self::Refused { refusal, .. } => matches!(refusal, Refusal::Full | Refusal::Offline),
            Self::SelectionNotReady | Self::PlayerUnready | Self::Call(_) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Server {
    pub(crate) name: String,
    players: f64,
    max_players: f64,
    online: bool,
    members_only: bool,
}

impl Server {
    fn parse(row: Value) -> Option<Self> {
        let Value::Object(fields) = row else {
            return None;
        };
        let name = match fields.get("sName") {
            Some(Value::String(name)) if !name.trim().is_empty() => name.clone(),
            _ => return None,
        };
        let number = |key: &str| match fields.get(key) {
            Some(Value::Number(number)) => *number,
            Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
            _ => 0.0,
        };
        Some(Self {
            name,
            players: number("iCount"),
            max_players: number("iMax"),
            online: number("bOnline") != 0.0,
            members_only: number("bUpg") != 0.0,
        })
    }
}

pub(crate) fn servers(loader: &impl Loader) -> Result<Vec<Server>, CallError> {
    let Value::List(rows) = bridge::auth::get_servers(loader)? else {
        return Ok(Vec::new());
    };
    Ok(rows.into_iter().filter_map(Server::parse).collect())
}

pub(crate) fn connection_dialog(loader: &impl Loader) -> Result<Option<String>, CallError> {
    Ok(bridge::flash::get_conn_mc_text(loader)?.filter(|text| text != "null"))
}

pub(crate) fn username(loader: &impl Loader) -> Result<Option<String>, CallError> {
    Ok(game_class_string(loader, "loginInfo.strUsername")?
        .map(|username| username.trim().to_owned())
        .filter(|username| !username.is_empty()))
}

pub(crate) fn credentials(loader: &impl Loader) -> Result<Option<Credentials>, CallError> {
    let username = username(loader)?;
    let password =
        game_class_string(loader, "loginInfo.strPassword")?.filter(|password| !password.is_empty());
    Ok(username
        .zip(password)
        .map(|(username, password)| Credentials { username, password }))
}

pub(crate) fn current_server(loader: &impl Loader) -> Result<Option<String>, CallError> {
    let json = bridge::flash::get_game_object(loader, "objServerInfo.sName")?;
    Ok(json_string(json).filter(|server| !server.is_empty()))
}

pub(crate) fn in_game(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(bridge::auth::is_logged_in(loader)? && bridge::world::is_loaded(loader)?)
}

pub(crate) fn server_selection_shown(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(!bridge::flash::is_null(loader, "mcLogin.sl.iList")?)
}

pub(crate) async fn login(
    loader: &impl Loader,
    credentials: &Credentials,
) -> Result<LoginOutcome, LoginError> {
    wait_out_kick(loader).await?;
    bridge::auth::login(loader, &credentials.username, &credentials.password)?;
    poll(POLL, LOGIN_OUTCOME, || {
        if in_game(loader)? {
            return Ok(Some(LoginOutcome::InGame));
        }
        Ok(server_selection_shown(loader)?.then_some(LoginOutcome::ServerSelection))
    })
    .await?
    .ok_or(LoginError::NoServerSelection)
}

pub(crate) async fn connect(loader: &impl Loader, server: &str) -> Result<(), ConnectError> {
    let deadline = Instant::now() + SELECTION_RETRY;
    loop {
        match Selection::parse(bridge::auth::connect_to(loader, server.trim())?) {
            Selection::Selected => break,
            Selection::NotReady if Instant::now() < deadline => sleep(POLL).await,
            Selection::NotReady => return Err(ConnectError::SelectionNotReady),
            Selection::Refused(refusal, message) => {
                return Err(ConnectError::Refused { refusal, message });
            }
        }
    }
    player_ready(loader).await
}

pub(crate) async fn player_ready(loader: &impl Loader) -> Result<(), ConnectError> {
    poll(POLL, PLAYER_READY, || Ok(in_game(loader)?.then_some(())))
        .await?
        .ok_or(ConnectError::PlayerUnready)
}

#[derive(Clone)]
pub(crate) struct LastLogin {
    pub(crate) credentials: Credentials,
    pub(crate) server: String,
}

const LOGIN_GAP: Duration = Duration::from_secs(1);
const LOGIN_JITTER: Duration = Duration::from_millis(500);

pub(crate) static LOGIN_GATE: LoginGate = LoginGate::new();

pub(crate) struct LoginGate {
    next: std::sync::Mutex<Option<Instant>>,
}

impl LoginGate {
    const fn new() -> Self {
        Self {
            next: std::sync::Mutex::new(None),
        }
    }

    fn reserve(&self, now: Instant, gap: Duration) -> Duration {
        let mut next = self
            .next
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let at = next.map_or(now, |next| next.max(now));
        *next = Some(at + gap);
        at - now
    }
}

#[derive(Default)]
pub(crate) struct Logins {
    turn: Mutex<()>,
    last: RefCell<Option<LastLogin>>,
    gate: Option<&'static LoginGate>,
}

impl Logins {
    pub(crate) fn gated(gate: &'static LoginGate) -> Self {
        Self {
            gate: Some(gate),
            ..Self::default()
        }
    }

    pub(crate) async fn turn(&self) -> MutexGuard<'_, ()> {
        self.turn.lock().await
    }

    pub(crate) async fn space_out(&self) {
        if let Some(gate) = self.gate {
            let gap = LOGIN_GAP + crate::jitter::up_to(LOGIN_JITTER);
            sleep(gate.reserve(Instant::now(), gap)).await;
        }
    }

    pub(crate) fn connected(&self, loader: &impl Loader) {
        let credentials = credentials(loader).ok().flatten();
        let server = current_server(loader).ok().flatten();
        if let Some((credentials, server)) = credentials.zip(server) {
            self.last.replace(Some(LastLogin {
                credentials,
                server,
            }));
        }
    }

    pub(crate) fn last(&self) -> Option<LastLogin> {
        self.last.borrow().clone()
    }

    pub(crate) async fn log_out(&self, loader: &impl Loader) -> Result<(), CallError> {
        let _turn = self.turn().await;
        bridge::auth::logout(loader)
    }

    pub(crate) async fn log_in(&self, loader: &impl Loader) -> Result<bool, CallError> {
        self.space_out().await;
        let _turn = self.turn().await;
        if bridge::auth::is_logged_in(loader)? || server_selection_shown(loader)? {
            return Ok(true);
        }
        let Some(last) = self.last() else {
            return Ok(false);
        };
        match login(loader, &last.credentials).await {
            Ok(_) => Ok(true),
            Err(LoginError::Call(error)) => Err(error),
            Err(LoginError::StillKicked | LoginError::NoServerSelection) => Ok(false),
        }
    }

    pub(crate) async fn connect(
        &self,
        loader: &impl Loader,
        server: &str,
    ) -> Result<(), ConnectError> {
        let _turn = self.turn().await;
        connect(loader, server).await
    }
}

async fn wait_out_kick(loader: &impl Loader) -> Result<(), LoginError> {
    if !kicked(loader) {
        return Ok(());
    }
    let until = Instant::now() + KICK_LIMIT;
    let remaining = json_number(bridge::flash::get_game_object(loader, "mcLogin.warning.n")?)
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map_or(KICK_DEFAULT, |seconds| {
            Duration::from_secs_f64(seconds.ceil())
        });
    sleep((remaining + Duration::from_secs(1)).min(KICK_LIMIT)).await;
    loop {
        if !kicked(loader) {
            return Ok(());
        }
        let now = Instant::now();
        if now >= until {
            return Err(LoginError::StillKicked);
        }
        sleep(KICK_RECHECK.min(until - now)).await;
    }
}

fn kicked(loader: &impl Loader) -> bool {
    matches!(bridge::auth::is_temporarily_kicked(loader), Ok(true))
}

fn game_class_string(loader: &impl Loader, path: &str) -> Result<Option<String>, CallError> {
    Ok(json_string(bridge::flash::get_game_object_s(loader, path)?))
}

fn json_string(json: Option<String>) -> Option<String> {
    serde_json::from_str::<Option<String>>(&json?)
        .ok()
        .flatten()
}

fn json_number(json: Option<String>) -> Option<f64> {
    serde_json::from_str::<f64>(&json?).ok()
}

enum Selection {
    Selected,
    NotReady,
    Refused(Refusal, String),
}

impl Selection {
    fn parse(value: Value) -> Self {
        let Value::Object(fields) = value else {
            return Self::NotReady;
        };
        let text = |key: &str| match fields.get(key) {
            Some(Value::String(text)) => Some(text.as_str()),
            _ => None,
        };
        let message = text("message")
            .unwrap_or("server selection failed")
            .to_owned();
        match text("status") {
            Some("selected") => Self::Selected,
            Some("not-ready") => Self::NotReady,
            status => Self::Refused(Refusal::parse(text("reason"), status), message),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::BTreeMap};

    use super::*;
    use crate::domain::testing::FakeLoader;

    #[test]
    fn logins_across_tabs_wait_their_turn_at_the_gate() {
        let gate = LoginGate::new();
        let now = Instant::now();
        let gap = Duration::from_secs(1);
        let waits: Vec<_> = (0..3).map(|_| gate.reserve(now, gap)).collect();
        assert_eq!(waits, [Duration::ZERO, gap, gap * 2]);
        assert_eq!(gate.reserve(now + gap * 5, gap), Duration::ZERO);
        assert_eq!(gate.reserve(now + gap * 5, gap), gap);
    }

    fn credentials() -> Credentials {
        Credentials {
            username: "player1".into(),
            password: "hunter2".into(),
        }
    }

    fn selection(status: &str, reason: Option<&str>, message: &str) -> Value {
        let mut fields = BTreeMap::from([
            ("status".to_owned(), Value::from(status.to_owned())),
            ("message".to_owned(), Value::from(message.to_owned())),
        ]);
        if let Some(reason) = reason {
            fields.insert("reason".into(), Value::from(reason.to_owned()));
        }
        Value::Object(fields)
    }

    struct LoginScreen {
        start: Instant,
        kicked_for: Duration,
        list_after: Option<Duration>,
        submitted_at: RefCell<Option<Duration>>,
    }

    impl LoginScreen {
        fn loader(self) -> (FakeLoader, std::rc::Rc<Self>) {
            let screen = std::rc::Rc::new(self);
            let state = screen.clone();
            let loader = FakeLoader::new(move |callback, args| {
                let elapsed = state.start.elapsed();
                Ok(match callback {
                    "auth.isTemporarilyKicked" => Value::Bool(elapsed < state.kicked_for),
                    "flash.getGameObject" if args == ["mcLogin.warning.n".into()] => {
                        Value::from(format!("{}", state.kicked_for.as_secs()))
                    }
                    "auth.login" => {
                        assert_eq!(args, ["player1".into(), "hunter2".into()]);
                        state.submitted_at.replace(Some(elapsed));
                        Value::Undefined
                    }
                    "auth.isLoggedIn" | "world.isLoaded" => Value::Bool(false),
                    "flash.isNull" => {
                        let shown = state.submitted_at.borrow().is_some()
                            && state.list_after.is_some_and(|after| elapsed >= after);
                        Value::Bool(!shown)
                    }
                    other => panic!("unexpected call {other}"),
                })
            });
            (loader, screen)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn login_submits_and_waits_for_the_server_list() {
        let (loader, screen) = LoginScreen {
            start: Instant::now(),
            kicked_for: Duration::ZERO,
            list_after: Some(Duration::from_secs(3)),
            submitted_at: RefCell::default(),
        }
        .loader();
        assert_eq!(
            login(&loader, &credentials()).await,
            Ok(LoginOutcome::ServerSelection)
        );
        assert_eq!(*screen.submitted_at.borrow(), Some(Duration::ZERO));
    }

    #[tokio::test(start_paused = true)]
    async fn login_waits_out_a_temporary_kick_before_submitting() {
        let (loader, screen) = LoginScreen {
            start: Instant::now(),
            kicked_for: Duration::from_secs(30),
            list_after: Some(Duration::ZERO),
            submitted_at: RefCell::default(),
        }
        .loader();
        assert_eq!(
            login(&loader, &credentials()).await,
            Ok(LoginOutcome::ServerSelection)
        );
        let submitted = screen.submitted_at.borrow().unwrap();
        assert!(
            (Duration::from_secs(30)..Duration::from_secs(32)).contains(&submitted),
            "submitted at {submitted:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn login_gives_up_when_the_kick_outlasts_its_limit() {
        let (loader, screen) = LoginScreen {
            start: Instant::now(),
            kicked_for: Duration::from_secs(600),
            list_after: Some(Duration::ZERO),
            submitted_at: RefCell::default(),
        }
        .loader();
        assert_eq!(
            login(&loader, &credentials()).await,
            Err(LoginError::StillKicked)
        );
        assert_eq!(*screen.submitted_at.borrow(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn login_fails_when_server_selection_never_appears() {
        let (loader, _) = LoginScreen {
            start: Instant::now(),
            kicked_for: Duration::ZERO,
            list_after: None,
            submitted_at: RefCell::default(),
        }
        .loader();
        let started = Instant::now();
        assert_eq!(
            login(&loader, &credentials()).await,
            Err(LoginError::NoServerSelection)
        );
        assert!(started.elapsed() >= LOGIN_OUTCOME);
    }

    fn server_list(
        replies: Vec<Value>,
        ready_after: Option<Duration>,
    ) -> (FakeLoader, std::rc::Rc<RefCell<Vec<Value>>>) {
        let start = Instant::now();
        let replies = std::rc::Rc::new(RefCell::new(replies));
        let remaining = replies.clone();
        let loader = FakeLoader::new(move |callback, args| {
            Ok(match callback {
                "auth.connectTo" => {
                    assert_eq!(args, ["Twilly".into()]);
                    let mut replies = remaining.borrow_mut();
                    if replies.len() > 1 {
                        replies.remove(0)
                    } else {
                        replies[0].clone()
                    }
                }
                "auth.isLoggedIn" | "world.isLoaded" => {
                    Value::Bool(ready_after.is_some_and(|after| start.elapsed() >= after))
                }
                other => panic!("unexpected call {other}"),
            })
        });
        (loader, replies)
    }

    #[tokio::test(start_paused = true)]
    async fn connect_retries_until_the_list_is_ready_then_waits_for_the_world() {
        let (loader, _) = server_list(
            vec![
                selection("not-ready", None, "server selection is not ready"),
                selection("not-ready", None, "server selection is not ready"),
                selection("selected", None, "server selected"),
            ],
            Some(Duration::from_secs(4)),
        );
        assert_eq!(connect(&loader, "Twilly").await, Ok(()));
        assert_eq!(loader.count("auth.connectTo"), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn connect_reports_why_a_server_refused() {
        let (loader, _) = server_list(
            vec![selection("blocked", Some("full"), "server is full")],
            None,
        );
        assert_eq!(
            connect(&loader, "Twilly").await,
            Err(ConnectError::Refused {
                refusal: Refusal::Full,
                message: "server is full".into()
            })
        );
        let (loader, _) = server_list(
            vec![selection("blocked", Some("member-only"), "member-only")],
            None,
        );
        assert_eq!(
            connect(&loader, "Twilly").await,
            Err(ConnectError::Refused {
                refusal: Refusal::MembersOnly,
                message: "member-only".into()
            })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn connect_fails_when_the_player_never_becomes_ready() {
        let (loader, _) = server_list(vec![selection("selected", None, "server selected")], None);
        assert_eq!(
            connect(&loader, "Twilly").await,
            Err(ConnectError::PlayerUnready)
        );
    }

    #[test]
    fn servers_report_their_population_and_access() {
        let row = |fields: &[(&str, Value)]| {
            Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), value.clone()))
                    .collect(),
            )
        };
        let loader = FakeLoader::new(move |callback, _| {
            assert_eq!(callback, "auth.getServers");
            Ok(Value::List(vec![
                row(&[
                    ("sName", Value::from("Twilly".to_owned())),
                    ("iCount", Value::Number(412.0)),
                    ("iMax", Value::Number(1000.0)),
                    ("bOnline", Value::Number(1.0)),
                    ("bUpg", Value::Number(0.0)),
                ]),
                row(&[
                    ("sName", Value::from("Yorumi".to_owned())),
                    ("iCount", Value::Number(0.0)),
                    ("iMax", Value::Number(500.0)),
                    ("bOnline", Value::Number(0.0)),
                    ("bUpg", Value::Number(1.0)),
                ]),
                row(&[("sName", Value::from(" ".to_owned()))]),
            ]))
        });
        assert_eq!(
            serde_json::to_value(servers(&loader).unwrap()).unwrap(),
            serde_json::json!([
                { "name": "Twilly", "players": 412.0, "maxPlayers": 1000.0, "online": true, "membersOnly": false },
                { "name": "Yorumi", "players": 0.0, "maxPlayers": 500.0, "online": false, "membersOnly": true },
            ])
        );
    }

    #[test]
    fn credentials_come_from_the_game_login_record() {
        let loader = FakeLoader::new(|callback, args| {
            assert_eq!(callback, "flash.getGameObjectS");
            Ok(match args.first() {
                Some(Value::String(path)) if path == "loginInfo.strUsername" => {
                    Value::from(r#"" player1 ""#.to_owned())
                }
                Some(Value::String(path)) if path == "loginInfo.strPassword" => {
                    Value::from(r#""hunter2""#.to_owned())
                }
                _ => panic!("unexpected path {args:?}"),
            })
        });
        assert!(credentials_of(&loader) == Some(("player1".into(), "hunter2".into())));
        let empty = FakeLoader::new(|_, _| Ok(Value::from(r#""""#.to_owned())));
        assert!(credentials_of(&empty).is_none());
    }

    fn credentials_of(loader: &FakeLoader) -> Option<(String, String)> {
        super::credentials(loader)
            .unwrap()
            .map(|credentials| (credentials.username, credentials.password))
    }
}
