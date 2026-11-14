use std::time::Duration;

use tokio::{
    sync::watch,
    time::{Instant, sleep_until},
};

use super::game_settings;
use crate::{
    bridge::{self, Event, Loader, Subscription},
    domain::{
        events::{GameEvent, Kind, Myself},
        packets,
    },
};

const BACK: &str = "%xt%zm%afk%1%false%";
const DELAY: Duration = Duration::from_millis(1000);
const JITTER_MS: u64 = 500;

pub(crate) fn wants(event: &Event) -> bool {
    Kind::Afk.wants(event)
}

pub(crate) async fn run(
    loader: impl Loader,
    events: Subscription,
    mut settings: watch::Receiver<game_settings::Settings>,
) {
    let mut enabled = false;
    let mut due = None;
    loop {
        let wanted = settings.borrow_and_update().anti_afk;
        if !wanted {
            due = None;
        } else if !enabled && matches!(bridge::player::is_afk(&loader), Ok(true)) {
            due = Some(later());
        }
        enabled = wanted;
        tokio::select! {
            changed = settings.changed() => if changed.is_err() {
                return;
            },
            event = events.next() => if let Ok(event) = event
                && enabled
                && let Some(afk) = my_afk(&loader, &event)
            {
                due = afk.then(|| due.unwrap_or_else(later));
            },
            () = sleep_until(due.unwrap_or_else(Instant::now)), if due.is_some() => {
                due = None;
                come_back(&loader);
            }
        }
    }
}

fn later() -> Instant {
    let mut bytes = [0; 8];
    let jitter = match getrandom::getrandom(&mut bytes) {
        Ok(()) => u64::from_ne_bytes(bytes) % (JITTER_MS + 1),
        Err(_) => 0,
    };
    Instant::now() + DELAY + Duration::from_millis(jitter)
}

fn my_afk(loader: &impl Loader, event: &Event) -> Option<bool> {
    let mut read = Vec::new();
    Kind::Afk.read(event, &Myself(loader), &mut read);
    read.into_iter().rev().find_map(|event| match event {
        GameEvent::Afk {
            is_self: true, afk, ..
        } => Some(afk),
        _ => None,
    })
}

fn come_back(loader: &impl Loader) {
    if let Err(error) = packets::send_to_server(loader, BACK) {
        tracing::debug!(%error, "couldn't clear AFK");
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;
    use tokio::{task::LocalSet, time::sleep};

    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    struct Running {
        sent: Rc<RefCell<Vec<String>>>,
        afk: Rc<RefCell<bool>>,
        events: Rc<Events>,
        settings: watch::Sender<game_settings::Settings>,
    }

    fn start(anti_afk: bool) -> Running {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let afk = Rc::new(RefCell::new(false));
        let (log, state) = (sent.clone(), afk.clone());
        let loader = Rc::new(FakeLoader::new(move |callback, args| {
            Ok(match (callback, args) {
                ("auth.isLoggedIn", _) => Value::Bool(true),
                ("player.isAfk", _) => Value::Bool(*state.borrow()),
                ("flash.getGameObjectS", _) => Value::String(r#""Player1""#.into()),
                ("flash.callGameFunction", [_, Value::String(packet)]) => {
                    log.borrow_mut().push(packet.clone());
                    Value::Null
                }
                (other, _) => panic!("unexpected call {other}"),
            })
        }));
        let events = Rc::new(Events::default());
        let (settings, receiver) = watch::channel(game_settings::Settings {
            anti_afk,
            ..game_settings::Settings::default()
        });
        tokio::task::spawn_local(run(loader, events.subscribe(wants), receiver));
        Running {
            sent,
            afk,
            events,
            settings,
        }
    }

    async fn marked(events: &Events, username: &str, afk: bool) {
        let packet =
            json!({ "type": "str", "dataObj": ["uotls", "-1", username, format!("afk:{afk}")] });
        events.publish(&Event::ExtensionResponse {
            packet: packet.to_string(),
        });
        sleep(Duration::from_millis(10)).await;
    }

    async fn after(millis: u64) {
        sleep(Duration::from_millis(millis)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn going_afk_sends_you_back_after_one_to_one_and_a_half_seconds() {
        LocalSet::new()
            .run_until(async {
                let running = start(true);
                marked(&running.events, "player2", true).await;
                marked(&running.events, "player1", false).await;
                after(2000).await;
                assert!(running.sent.borrow().is_empty());
                marked(&running.events, "player1", true).await;
                after(980).await;
                assert!(running.sent.borrow().is_empty());
                after(520).await;
                assert_eq!(*running.sent.borrow(), [BACK]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn coming_back_or_turning_it_off_during_the_delay_sends_nothing() {
        LocalSet::new()
            .run_until(async {
                let running = start(true);
                marked(&running.events, "player1", true).await;
                after(500).await;
                marked(&running.events, "player1", false).await;
                after(2000).await;
                assert!(running.sent.borrow().is_empty());
                marked(&running.events, "player1", true).await;
                after(500).await;
                running
                    .settings
                    .send_modify(|settings| settings.anti_afk = false);
                after(2000).await;
                assert!(running.sent.borrow().is_empty());
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn turned_off_it_leaves_you_afk() {
        LocalSet::new()
            .run_until(async {
                let running = start(false);
                marked(&running.events, "player1", true).await;
                after(2000).await;
                assert!(running.sent.borrow().is_empty());
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn turning_it_on_while_afk_brings_you_back() {
        LocalSet::new()
            .run_until(async {
                let running = start(false);
                *running.afk.borrow_mut() = true;
                after(10).await;
                running
                    .settings
                    .send_modify(|settings| settings.anti_afk = true);
                after(990).await;
                assert!(running.sent.borrow().is_empty());
                after(510).await;
                assert_eq!(*running.sent.borrow(), [BACK]);
            })
            .await;
    }
}
