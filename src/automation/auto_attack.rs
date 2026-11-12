use std::sync::Arc;

use tokio::{
    sync::watch,
    time::{Instant, sleep_until},
};

use crate::bridge::{Loader, Subscription};
use crate::domain::{
    combat::{self, TICK},
    counter_attack::CounterAttacks,
    profiles::{Library, Profile},
    rotation::Fighter,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) enabled: bool,
    pub(crate) profile: Option<String>,
}

pub(crate) async fn run(
    loader: impl Loader,
    mut settings: watch::Receiver<Settings>,
    counter_attacks: CounterAttacks,
    messages: Subscription,
    library: watch::Receiver<Arc<Library>>,
) {
    let mut fighter = Fighter::fixed(Profile::generic()).listening(messages);
    let mut chosen = None;
    let mut next = Instant::now() + TICK;
    loop {
        let Settings { enabled, profile } = settings.borrow_and_update().clone();
        if chosen.as_ref() != Some(&profile) {
            fighter.switch(Fighter::chosen(profile.as_deref(), &library));
            chosen = Some(profile);
        }
        tokio::select! {
            changed = settings.changed() => {
                if changed.is_err() {
                    return;
                }
                next = Instant::now() + TICK;
            }
            () = fighter.listen(&loader) => {}
            () = sleep_until(next), if enabled => {
                if let Err(error) = combat::fight_any(&loader, &counter_attacks, &mut fighter, &[]) {
                    tracing::debug!(%error, "auto attack skipped a tick");
                }
                next = Instant::now() + TICK;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    use serde_json::json;
    use tokio::{task::LocalSet, time::sleep};

    use super::*;
    use crate::{
        bridge::{Event, Events, Value},
        domain::testing::FakeLoader,
    };

    #[derive(Default)]
    struct Fight {
        in_game: bool,
        monsters: Vec<f64>,
        target: Option<f64>,
        cooling: HashMap<i32, i32>,
        targeted: Vec<f64>,
        cast: Vec<i32>,
    }

    struct Started {
        fight: Rc<RefCell<Fight>>,
        settings: watch::Sender<Settings>,
        library: watch::Sender<Arc<Library>>,
        events: Rc<Events>,
    }

    fn on(enabled: bool) -> Settings {
        Settings {
            enabled,
            profile: None,
        }
    }

    fn start(fight: Fight, settings: Settings, library: Library) -> Started {
        let fight = Rc::new(RefCell::new(fight));
        let state = fight.clone();
        let loader = FakeLoader::new(move |callback, args| {
            let mut fight = state.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" => Value::Bool(fight.in_game),
                "combat.hasTarget" => Value::Bool(fight.target.is_some()),
                "player.getAuras" => Value::List(Vec::new()),
                "world.getAvailableMonsterMapIds" => {
                    Value::List(fight.monsters.iter().map(|id| Value::Number(*id)).collect())
                }
                "combat.attackMonster" => {
                    let [Value::Object(selector)] = args else {
                        panic!("attackMonster needs a selector: {args:?}");
                    };
                    let Some(Value::Number(id)) = selector.get("monsterMapId") else {
                        panic!("the selector names a monster: {selector:?}");
                    };
                    fight.targeted.push(*id);
                    fight.target = Some(*id);
                    Value::Bool(true)
                }
                "combat.getSnapshot" => {
                    let skill = |slot: i32| {
                        Value::Object(
                            [
                                (
                                    "cooldown".to_owned(),
                                    Value::Number(f64::from(
                                        fight.cooling.get(&slot).copied().unwrap_or(0),
                                    )),
                                ),
                                ("castable".to_owned(), Value::Bool(fight.target.is_some())),
                            ]
                            .into(),
                        )
                    };
                    Value::Object(
                        [
                            ("hp".to_owned(), Value::Number(100.0)),
                            ("maxHp".to_owned(), Value::Number(100.0)),
                            (
                                "skills".to_owned(),
                                Value::List((0..6).map(skill).collect()),
                            ),
                        ]
                        .into(),
                    )
                }
                "combat.useSkill" => {
                    let Value::String(skill) = &args[0] else {
                        panic!("useSkill needs a skill: {args:?}");
                    };
                    fight.cast.push(skill.parse().unwrap());
                    Value::Bool(true)
                }
                other => panic!("unexpected call {other}"),
            })
        });
        let (settings, receiver) = watch::channel(settings);
        let (library, profiles) = watch::channel(Arc::new(library));
        let events = Rc::new(Events::default());
        tokio::task::spawn_local(run(
            loader,
            receiver,
            CounterAttacks::default(),
            events.subscribe(crate::domain::rotation::wants),
            profiles,
        ));
        Started {
            fight,
            settings,
            library,
            events,
        }
    }

    fn library(profiles: &[(&str, serde_json::Value)]) -> Library {
        let mut library = Library::default();
        for (name, profile) in profiles {
            library.put(
                None,
                (*name).to_owned(),
                serde_json::from_value(profile.clone()).unwrap(),
            );
        }
        library
    }

    async fn ticks(count: u32) {
        sleep(TICK * count + TICK / 2).await;
    }

    #[tokio::test(start_paused = true)]
    async fn it_targets_a_monster_and_rotates_through_skills_one_to_four() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0, 9.0],
                        ..Fight::default()
                    },
                    on(true),
                    Library::default(),
                );
                ticks(5).await;
                let fight = started.fight.borrow();
                assert_eq!(fight.targeted, [7.0]);
                assert_eq!(fight.cast, [1, 2, 3, 4, 1]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn skills_on_cooldown_are_skipped() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0],
                        cooling: HashMap::from([(2, 3000), (4, 1500)]),
                        ..Fight::default()
                    },
                    on(true),
                    Library::default(),
                );
                ticks(4).await;
                assert_eq!(started.fight.borrow().cast, [1, 3, 1, 3]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_live_target_is_kept() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0],
                        target: Some(3.0),
                        ..Fight::default()
                    },
                    on(true),
                    Library::default(),
                );
                ticks(3).await;
                assert!(started.fight.borrow().targeted.is_empty());
                assert_eq!(started.fight.borrow().cast.len(), 3);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn the_rotation_starts_over_on_a_new_target() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0],
                        ..Fight::default()
                    },
                    on(true),
                    Library::default(),
                );
                ticks(2).await;
                started.fight.borrow_mut().target = None;
                ticks(2).await;
                assert_eq!(started.fight.borrow().cast, [1, 2, 1, 2]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn it_waits_while_off_out_of_game_or_with_nothing_to_fight() {
        LocalSet::new()
            .run_until(async {
                let off = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0],
                        ..Fight::default()
                    },
                    on(false),
                    Library::default(),
                );
                let loading = start(
                    Fight {
                        monsters: vec![7.0],
                        ..Fight::default()
                    },
                    on(true),
                    Library::default(),
                );
                let empty = start(
                    Fight {
                        in_game: true,
                        ..Fight::default()
                    },
                    on(true),
                    Library::default(),
                );
                ticks(5).await;
                for started in [&off, &loading, &empty] {
                    assert!(started.fight.borrow().cast.is_empty());
                }
                off.settings.send_replace(on(true));
                ticks(1).await;
                assert_eq!(off.fight.borrow().cast, [1]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_chosen_profile_is_used_and_picks_up_saved_changes() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0],
                        ..Fight::default()
                    },
                    Settings {
                        enabled: true,
                        profile: Some("solo".into()),
                    },
                    library(&[(
                        "Solo",
                        json!({ "onCooldown": "skip", "steps": [{ "skill": 3 }] }),
                    )]),
                );
                ticks(2).await;
                started.library.send_replace(Arc::new(library(&[(
                    "Solo",
                    json!({ "onCooldown": "skip", "steps": [{ "skill": 2 }] }),
                )])));
                sleep(TICK * 2).await;
                started.settings.send_replace(Settings {
                    enabled: true,
                    profile: Some("Missing".into()),
                });
                ticks(2).await;
                assert_eq!(started.fight.borrow().cast, [3, 3, 2, 2, 1, 2]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_message_fires_its_trigger() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0],
                        ..Fight::default()
                    },
                    Settings {
                        enabled: true,
                        profile: Some("Boss".into()),
                    },
                    library(&[(
                        "Boss",
                        json!({
                            "onCooldown": "skip",
                            "triggers": [{ "includes": "prepares", "skill": 4, "within": 1000 }],
                            "steps": [{ "skill": 1 }]
                        }),
                    )]),
                );
                ticks(2).await;
                started.events.publish(&Event::ExtensionResponse {
                    packet: r#"{"type":"json","dataObj":{"cmd":"ct","anims":[{"msg":"The boss prepares a slam!","cInf":"m:7","tInf":"p:1"}]}}"#.into(),
                });
                ticks(2).await;
                assert_eq!(started.fight.borrow().cast, [1, 1, 4, 1]);
            })
            .await;
    }
}
