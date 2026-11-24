use std::{cell::RefCell, rc::Rc};

use super::auto_relogin::{self, Phase};
use crate::{
    bridge::{self, Event, Lagged, Loader, Subscription},
    domain::auth::{self, Credentials, Logins},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    LoggingIn { attempt: u32, of: u32 },
    Retrying { attempt: u32, of: u32 },
    ChooseServer,
    Online(Option<String>),
    Offline,
    LoggedOut,
    Failed(String),
}

pub(crate) struct Statuses {
    state: RefCell<State>,
    report: Box<dyn Fn(Status)>,
}

#[derive(Default)]
struct State {
    connection: Option<Status>,
    attempt: Attempt,
    reported: Option<Status>,
}

#[derive(Default)]
enum Attempt {
    #[default]
    Idle,
    Running(Option<Status>),
    Joining(Option<Status>),
    Ended(Status),
}

impl State {
    fn online(&self) -> bool {
        matches!(self.connection, Some(Status::Online(_)))
    }

    fn current(&self) -> Option<&Status> {
        if self.online() {
            return self.connection.as_ref();
        }
        match &self.attempt {
            Attempt::Idle => self.connection.as_ref(),
            Attempt::Running(progress) | Attempt::Joining(progress) => progress.as_ref(),
            Attempt::Ended(outcome) => Some(outcome),
        }
    }
}

impl Statuses {
    pub(crate) fn new(report: impl Fn(Status) + 'static) -> Self {
        Self {
            state: RefCell::default(),
            report: Box::new(report),
        }
    }

    fn begin(&self) {
        self.update(|state| {
            state.attempt = Attempt::Running(None);
            state.reported = None;
        });
    }

    fn progress(&self, status: Status) {
        self.update(|state| state.attempt = Attempt::Running(Some(status)));
    }

    fn end(&self, outcome: Option<Status>) {
        self.update(|state| {
            state.attempt = match (outcome, std::mem::take(&mut state.attempt)) {
                _ if state.online() => Attempt::Idle,
                (Some(outcome), _) => Attempt::Ended(outcome),
                (None, Attempt::Running(progress)) => Attempt::Joining(progress),
                (None, attempt) => attempt,
            }
        });
    }

    fn connection(&self, status: Status) {
        self.update(|state| {
            if matches!(status, Status::Online(_)) || matches!(state.attempt, Attempt::Joining(_)) {
                state.attempt = Attempt::Idle;
            }
            state.connection = Some(status);
        });
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        let changed = {
            let mut state = self.state.borrow_mut();
            change(&mut state);
            let current = state.current().cloned();
            match current {
                Some(current) if state.reported.as_ref() != Some(&current) => {
                    state.reported = Some(current.clone());
                    Some(current)
                }
                _ => None,
            }
        };
        if let Some(status) = changed {
            (self.report)(status);
        }
    }
}

pub(crate) fn run(
    loader: impl Loader,
    logins: Rc<Logins>,
    credentials: Credentials,
    server: Option<String>,
    settings: auto_relogin::Settings,
    statuses: Rc<Statuses>,
) -> impl Future<Output = ()> {
    statuses.begin();
    async move {
        let settings = auto_relogin::Settings {
            delay_seconds: 0,
            ..settings
        };
        let signed_in = auto_relogin::recover(
            &loader,
            &logins,
            &credentials,
            server.as_deref(),
            &settings,
            |phase| match phase {
                Phase::Attempt { attempt, of } => {
                    statuses.progress(Status::LoggingIn { attempt, of })
                }
                Phase::Backoff { attempt, of, .. } => {
                    statuses.progress(Status::Retrying { attempt, of })
                }
                Phase::Off | Phase::Armed | Phase::Waiting(_) | Phase::Failed(_) => {}
            },
        )
        .await;
        statuses.end(match signed_in {
            Ok(()) if auth::in_game(&loader).unwrap_or(false) => None,
            Ok(()) => Some(Status::ChooseServer),
            Err(error) => Some(Status::Failed(error.to_string())),
        });
    }
}

pub(crate) async fn watch(
    loader: impl Loader,
    logins: Rc<Logins>,
    connections: Subscription,
    statuses: Rc<Statuses>,
) {
    statuses.connection(check(&loader, &logins).await);
    loop {
        match connections.next().await {
            Ok(Event::Connection { status }) if status == "OnConnection" => {
                if ready(&loader).await {
                    statuses.connection(online(&loader));
                }
            }
            Ok(Event::Connection { status }) if status == "OnConnectionLost" => {
                statuses.connection(offline(&logins))
            }
            Ok(_) => {}
            Err(Lagged) => statuses.connection(check(&loader, &logins).await),
        }
    }
}

async fn check(loader: &impl Loader, logins: &Logins) -> Status {
    if ready(loader).await {
        online(loader)
    } else {
        offline(logins)
    }
}

fn offline(logins: &Logins) -> Status {
    if logins.take_logout() {
        Status::LoggedOut
    } else {
        Status::Offline
    }
}

async fn ready(loader: &impl Loader) -> bool {
    while bridge::auth::is_logged_in(loader).unwrap_or(false) {
        if auth::player_ready(loader).await.is_ok() {
            return true;
        }
    }
    false
}

fn online(loader: &impl Loader) -> Status {
    Status::Online(auth::current_server(loader).ok().flatten())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::task::LocalSet;

    use super::*;
    use crate::{automation::auto_relogin::tests::World, domain::testing::FakeLoader};

    struct Running {
        world: World,
        reports: Rc<RefCell<Vec<Status>>>,
        loader: Rc<FakeLoader>,
        logins: Rc<Logins>,
        statuses: Rc<Statuses>,
    }

    impl Running {
        fn sign_in(&self, server: Option<&str>) {
            tokio::task::spawn_local(run(
                self.loader.clone(),
                self.logins.clone(),
                Credentials {
                    username: "player1".into(),
                    password: "hunter2".into(),
                },
                server.map(str::to_owned),
                auto_relogin::Settings::default(),
                self.statuses.clone(),
            ));
        }
    }

    fn start(server: Option<&str>) -> Running {
        let running = watching(false, None);
        running.sign_in(server);
        running
    }

    fn watching(connected: bool, dialog: Option<&str>) -> Running {
        let (world, loader) = World::new();
        world.game.borrow_mut().connected = connected;
        world.game.borrow_mut().dialog = dialog.map(str::to_owned);
        let loader = Rc::new(loader);
        let logins = Rc::new(Logins::default());
        let reports = Rc::new(RefCell::new(Vec::new()));
        let seen = reports.clone();
        let statuses = Rc::new(Statuses::new(move |status| seen.borrow_mut().push(status)));
        tokio::task::spawn_local(watch(
            loader.clone(),
            logins.clone(),
            world.events.subscribe(auto_relogin::wants),
            statuses.clone(),
        ));
        Running {
            world,
            reports,
            loader,
            logins,
            statuses,
        }
    }

    async fn settle() {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn an_account_with_a_server_logs_in_joins_it_and_tells_its_own_logouts_from_drops() {
        LocalSet::new()
            .run_until(async {
                let running = start(Some("Twilly"));
                settle().await;
                assert_eq!(running.world.game.borrow().connects, ["Twilly"]);
                running.world.lose_connection();
                settle().await;
                running.world.connect();
                settle().await;
                running.logins.log_out(&running.loader).await.unwrap();
                running.logins.log_out(&running.loader).await.unwrap();
                settle().await;
                running.world.connect();
                settle().await;
                running.world.lose_connection();
                running.logins.log_out(&running.loader).await.unwrap();
                settle().await;
                running.world.connect();
                running.logins.log_out(&running.loader).await.unwrap();
                settle().await;
                assert_eq!(
                    *running.reports.borrow(),
                    [
                        Status::LoggingIn { attempt: 1, of: 4 },
                        Status::Online(Some("Twilly".into())),
                        Status::Offline,
                        Status::Online(Some("Twilly".into())),
                        Status::LoggedOut,
                        Status::Online(Some("Twilly".into())),
                        Status::Offline,
                        Status::LoggedOut,
                    ]
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_attempt_is_retried_before_the_account_goes_online() {
        LocalSet::new()
            .run_until(async {
                let running = start(Some("Twilly"));
                running.world.game.borrow_mut().failing_logins = 1;
                tokio::time::sleep(Duration::from_secs(60)).await;
                let reports = running.reports.borrow();
                assert_eq!(reports[0], Status::LoggingIn { attempt: 1, of: 4 });
                assert_eq!(reports[1], Status::Retrying { attempt: 1, of: 4 });
                assert_eq!(reports[2], Status::LoggingIn { attempt: 2, of: 4 });
                assert_eq!(reports.last(), Some(&Status::Online(Some("Twilly".into()))));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_server_a_failed_login_is_retried_before_server_selection() {
        LocalSet::new()
            .run_until(async {
                let running = start(None);
                running.world.game.borrow_mut().failing_logins = 1;
                tokio::time::sleep(Duration::from_secs(60)).await;
                let reports = running.reports.borrow();
                assert_eq!(reports[1], Status::Retrying { attempt: 1, of: 4 });
                assert_eq!(reports.last(), Some(&Status::ChooseServer));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_world_that_loads_slowly_after_choosing_a_server_still_comes_online() {
        LocalSet::new()
            .run_until(async {
                let running = start(None);
                settle().await;
                running.world.game.borrow_mut().dialog = Some("Loading".into());
                running.world.connect();
                tokio::time::sleep(Duration::from_secs(30)).await;
                assert_eq!(running.reports.borrow().last(), Some(&Status::ChooseServer));
                running.world.game.borrow_mut().dialog = None;
                settle().await;
                assert_eq!(
                    running.reports.borrow().last(),
                    Some(&Status::Online(Some("Twilly".into())))
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_server_the_account_waits_for_the_player_to_choose_one() {
        LocalSet::new()
            .run_until(async {
                let running = start(None);
                settle().await;
                assert_eq!(running.reports.borrow().last(), Some(&Status::ChooseServer));
                assert!(running.world.game.borrow().connects.is_empty());
                running.world.connect();
                settle().await;
                assert_eq!(
                    running.reports.borrow().last(),
                    Some(&Status::Online(Some("Twilly".into())))
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_player_who_logs_in_by_hand_is_followed_without_logging_in_again() {
        LocalSet::new()
            .run_until(async {
                let running = watching(false, None);
                settle().await;
                running.world.connect();
                settle().await;
                running.world.lose_connection();
                settle().await;
                assert!(running.world.game.borrow().logins.is_empty());
                assert_eq!(
                    *running.reports.borrow(),
                    [
                        Status::Offline,
                        Status::Online(Some("Twilly".into())),
                        Status::Offline,
                    ]
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_player_who_logs_in_by_hand_after_a_failed_login_is_followed() {
        LocalSet::new()
            .run_until(async {
                let running = start(Some("Twilly"));
                running.world.game.borrow_mut().failing_logins = u32::MAX;
                tokio::time::sleep(Duration::from_secs(600)).await;
                let failed = running.reports.borrow().len();
                assert!(matches!(
                    running.reports.borrow().last(),
                    Some(Status::Failed(_))
                ));
                running.world.connect();
                settle().await;
                running.world.lose_connection();
                settle().await;
                assert_eq!(
                    running.reports.borrow()[failed..],
                    [Status::Online(Some("Twilly".into())), Status::Offline]
                );
            })
            .await;
    }

    #[test]
    fn a_sign_in_that_succeeds_just_before_the_connection_drops_shows_offline() {
        let reports = Rc::new(RefCell::new(Vec::new()));
        let seen = reports.clone();
        let statuses = Statuses::new(move |status| seen.borrow_mut().push(status));
        statuses.connection(Status::Offline);
        statuses.begin();
        statuses.progress(Status::LoggingIn { attempt: 1, of: 4 });
        statuses.end(None);
        statuses.connection(Status::Offline);
        assert_eq!(
            *reports.borrow(),
            [
                Status::Offline,
                Status::LoggingIn { attempt: 1, of: 4 },
                Status::Offline,
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_player_watched_mid_room_load_comes_online_once_it_loads() {
        LocalSet::new()
            .run_until(async {
                let running = watching(true, Some("Loading"));
                settle().await;
                running.world.game.borrow_mut().dialog = None;
                settle().await;
                assert_eq!(
                    *running.reports.borrow(),
                    [Status::Online(Some("Twilly".into()))]
                );
            })
            .await;
    }
}
