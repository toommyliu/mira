use tokio::sync::watch;

use super::game_settings;
use crate::{
    bridge::{self, CallError, Loader, Subscription},
    domain::{
        combat::{self, Target},
        counter_attack::{self, CounterAttacks},
        events::Myself,
    },
};

pub(crate) async fn run(
    loader: impl Loader,
    events: Subscription,
    mut settings: watch::Receiver<game_settings::Settings>,
    attacks: CounterAttacks,
) {
    loop {
        attacks.set_enabled(settings.borrow_and_update().anti_counter);
        tokio::select! {
            changed = settings.changed() => if changed.is_err() {
                return;
            },
            event = events.next() => if let Ok(event) = event {
                for signal in counter_attack::signals(&event, &Myself(&loader)) {
                    if let Some(id) = attacks.apply(signal)
                        && let Err(error) = stop_attacking(&loader, id)
                    {
                        tracing::debug!(%error, "couldn't stop attacking a countering monster");
                    }
                }
            }
        }
    }
}

fn stop_attacking(loader: &impl Loader, mon_map_id: i32) -> Result<(), CallError> {
    if matches!(combat::target(loader)?, Some(Target::Monster(monster)) if monster.mon_map_id == f64::from(mon_map_id))
    {
        bridge::combat::cancel_auto_attack(loader)?;
        bridge::combat::cancel_target(loader)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use serde_json::json;
    use tokio::{task::LocalSet, time::sleep};

    use super::*;
    use crate::{
        bridge::{Event, Events, Value},
        domain::testing::FakeLoader,
    };

    struct Running {
        target: Rc<RefCell<Option<f64>>>,
        loader: Rc<FakeLoader>,
        events: Rc<Events>,
        attacks: CounterAttacks,
        settings: watch::Sender<game_settings::Settings>,
    }

    fn start(target: Option<f64>) -> Running {
        let target = Rc::new(RefCell::new(target));
        let state = target.clone();
        let loader = Rc::new(FakeLoader::new(move |callback, _| {
            let mut target = state.borrow_mut();
            Ok(match callback {
                "combat.getTarget" => match *target {
                    Some(id) => Value::Object(
                        [
                            ("type".to_owned(), Value::String("monster".into())),
                            ("monMapId".to_owned(), Value::Number(id)),
                        ]
                        .into(),
                    ),
                    None => Value::Null,
                },
                "combat.cancelTarget" => {
                    *target = None;
                    Value::Null
                }
                "combat.cancelAutoAttack" => Value::Null,
                other => panic!("unexpected call {other}"),
            })
        }));
        let events = Rc::new(Events::default());
        let attacks = CounterAttacks::default();
        let (settings, receiver) = watch::channel(game_settings::Settings::default());
        tokio::task::spawn_local(run(
            loader.clone(),
            events.subscribe(counter_attack::wants),
            receiver,
            attacks.clone(),
        ));
        Running {
            target,
            loader,
            events,
            attacks,
            settings,
        }
    }

    async fn publish(events: &Events, event: Event) {
        events.publish(&event);
        sleep(Duration::from_millis(10)).await;
    }

    async fn warn(events: &Events, id: i32) {
        let packet = json!({ "type": "json", "dataObj": { "cmd": "ct", "anims": [
            { "msg": "<mon> prepares a counter attack!", "cInf": format!("m:{id}"), "tInf": "p:1" },
        ] } });
        publish(
            events,
            Event::ExtensionResponse {
                packet: packet.to_string(),
            },
        )
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_warning_from_the_target_drops_it() {
        LocalSet::new()
            .run_until(async {
                let running = start(Some(2.0));
                warn(&running.events, 3).await;
                assert_eq!(*running.target.borrow(), Some(2.0));
                assert!(running.attacks.countering(3.0));
                warn(&running.events, 2).await;
                assert_eq!(*running.target.borrow(), None);
                assert_eq!(running.loader.count("combat.cancelAutoAttack"), 1);
                assert!(running.attacks.countering(2.0));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn reconnecting_forgets_every_counter() {
        LocalSet::new()
            .run_until(async {
                let running = start(None);
                warn(&running.events, 3).await;
                publish(
                    &running.events,
                    Event::Connection {
                        status: "OnConnection".into(),
                    },
                )
                .await;
                assert!(!running.attacks.any());
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn turning_it_off_forgets_counters_and_ignores_warnings() {
        LocalSet::new()
            .run_until(async {
                let running = start(Some(2.0));
                warn(&running.events, 3).await;
                running
                    .settings
                    .send_modify(|settings| settings.anti_counter = false);
                sleep(Duration::from_millis(10)).await;
                assert!(!running.attacks.any());
                warn(&running.events, 2).await;
                assert_eq!(*running.target.borrow(), Some(2.0));
                assert!(!running.attacks.any());
            })
            .await;
    }
}
