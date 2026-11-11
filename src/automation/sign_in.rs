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
    Failed(String),
}

pub(crate) async fn run(
    loader: impl Loader,
    logins: Rc<Logins>,
    connections: Subscription,
    credentials: Credentials,
    server: Option<String>,
    settings: auto_relogin::Settings,
    report: impl Fn(Status),
) {
    let report = on_change(report);
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
            Phase::Attempt { attempt, of } => report(Status::LoggingIn { attempt, of }),
            Phase::Backoff { attempt, of } => report(Status::Retrying { attempt, of }),
            Phase::Off | Phase::Armed | Phase::Waiting | Phase::Failed(_) => {}
        },
    )
    .await;
    match signed_in {
        Ok(()) if auth::in_game(&loader).unwrap_or(false) => report(online(&loader)),
        Ok(()) => report(Status::ChooseServer),
        Err(error) => return report(Status::Failed(error.to_string())),
    }
    follow(&loader, &connections, &report).await
}

pub(crate) async fn watch(loader: impl Loader, connections: Subscription, report: impl Fn(Status)) {
    let report = on_change(report);
    if ready(&loader).await {
        report(online(&loader));
    } else {
        report(Status::Offline);
    }
    follow(&loader, &connections, &report).await
}

fn on_change(report: impl Fn(Status)) -> impl Fn(Status) {
    let last = RefCell::new(None);
    move |status: Status| {
        if last.borrow().as_ref() != Some(&status) {
            last.replace(Some(status.clone()));
            report(status);
        }
    }
}

async fn follow(loader: &impl Loader, connections: &Subscription, report: &impl Fn(Status)) {
    loop {
        match connections.next().await {
            Ok(Event::Connection { status }) if status == "OnConnection" => {
                if ready(loader).await {
                    report(online(loader));
                }
            }
            Ok(Event::Connection { status }) if status == "OnConnectionLost" => {
                report(Status::Offline)
            }
            Ok(_) => {}
            Err(Lagged) => {
                if !bridge::auth::is_logged_in(loader).unwrap_or(false) {
                    report(Status::Offline);
                }
            }
        }
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
    use crate::automation::auto_relogin::tests::World;

    struct Running {
        world: World,
        reports: Rc<RefCell<Vec<Status>>>,
    }

    fn start(server: Option<&str>) -> Running {
        let (world, loader) = World::new();
        let reports = Rc::new(RefCell::new(Vec::new()));
        let seen = reports.clone();
        tokio::task::spawn_local(run(
            loader,
            Rc::new(Logins::default()),
            world.events.subscribe(auto_relogin::wants),
            Credentials {
                username: "player1".into(),
                password: "hunter2".into(),
            },
            server.map(str::to_owned),
            auto_relogin::Settings::default(),
            move |status| seen.borrow_mut().push(status),
        ));
        Running { world, reports }
    }

    fn watching(connected: bool, dialog: Option<&str>) -> Running {
        let (world, loader) = World::new();
        world.game.borrow_mut().connected = connected;
        world.game.borrow_mut().dialog = dialog.map(str::to_owned);
        let reports = Rc::new(RefCell::new(Vec::new()));
        let seen = reports.clone();
        tokio::task::spawn_local(watch(
            loader,
            world.events.subscribe(auto_relogin::wants),
            move |status| seen.borrow_mut().push(status),
        ));
        Running { world, reports }
    }

    async fn settle() {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn an_account_with_a_server_logs_in_joins_it_and_follows_the_connection() {
        LocalSet::new()
            .run_until(async {
                let running = start(Some("Twilly"));
                settle().await;
                assert_eq!(running.world.game.borrow().connects, ["Twilly"]);
                running.world.lose_connection();
                settle().await;
                running.world.connect();
                settle().await;
                assert_eq!(
                    *running.reports.borrow(),
                    [
                        Status::LoggingIn { attempt: 1, of: 4 },
                        Status::Online(Some("Twilly".into())),
                        Status::Offline,
                        Status::Online(Some("Twilly".into())),
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
