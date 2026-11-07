use serde::{Deserialize, Serialize};
use tokio::{sync::watch, time::sleep};

use super::{TICK, selector, use_next_skill};
use crate::automation::{
    auth,
    bridge::{self, CallError, Loader, Value},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) enabled: bool,
}

pub(crate) async fn run(loader: impl Loader, mut settings: watch::Receiver<Settings>) {
    let mut next = 0;
    loop {
        let enabled = settings.borrow_and_update().enabled;
        tokio::select! {
            changed = settings.changed() => if changed.is_err() {
                return;
            },
            () = sleep(TICK), if enabled => {
                if let Err(error) = attack(&loader, &mut next) {
                    tracing::debug!(%error, "auto attack skipped a tick");
                }
            }
        }
    }
}

fn attack(loader: &impl Loader, next: &mut usize) -> Result<(), CallError> {
    if !auth::in_game(loader)? {
        return Ok(());
    }
    if !bridge::combat::has_target(loader)? && !target_any_monster(loader)? {
        return Ok(());
    }
    use_next_skill(loader, next)
}

fn target_any_monster(loader: &impl Loader) -> Result<bool, CallError> {
    let Value::List(ids) = bridge::world::get_available_monster_map_ids(loader)? else {
        return Ok(false);
    };
    let Some(id) = ids.into_iter().find_map(|id| match id {
        Value::Number(id) => Some(id),
        _ => None,
    }) else {
        return Ok(false);
    };
    bridge::combat::attack_monster(loader, selector("monsterMapId", Value::Number(id)))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    use tokio::task::LocalSet;

    use super::*;
    use crate::automation::testing::FakeLoader;

    #[derive(Default)]
    struct Fight {
        in_game: bool,
        monsters: Vec<f64>,
        target: Option<f64>,
        cooling: HashMap<i32, i32>,
        targeted: Vec<f64>,
        cast: Vec<i32>,
    }

    fn start(fight: Fight, enabled: bool) -> (Rc<RefCell<Fight>>, watch::Sender<Settings>) {
        let fight = Rc::new(RefCell::new(fight));
        let state = fight.clone();
        let loader = FakeLoader::new(move |callback, args| {
            let mut fight = state.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" => Value::Bool(fight.in_game),
                "combat.hasTarget" => Value::Bool(fight.target.is_some()),
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
                "combat.getSkillCooldownRemaining" => {
                    let [Value::Number(skill)] = args else {
                        panic!("cooldown needs a skill: {args:?}");
                    };
                    Value::Number(f64::from(
                        fight.cooling.get(&(*skill as i32)).copied().unwrap_or(0),
                    ))
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
        let (settings, receiver) = watch::channel(Settings { enabled });
        tokio::task::spawn_local(run(loader, receiver));
        (fight, settings)
    }

    async fn ticks(count: u32) {
        sleep(TICK * count + TICK / 2).await;
    }

    #[tokio::test(start_paused = true)]
    async fn it_targets_a_monster_and_rotates_through_skills_one_to_four() {
        LocalSet::new()
            .run_until(async {
                let (fight, _settings) = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0, 9.0],
                        ..Fight::default()
                    },
                    true,
                );
                ticks(5).await;
                let fight = fight.borrow();
                assert_eq!(fight.targeted, [7.0]);
                assert_eq!(fight.cast, [1, 2, 3, 4, 1]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn skills_on_cooldown_are_skipped() {
        LocalSet::new()
            .run_until(async {
                let (fight, _settings) = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0],
                        cooling: HashMap::from([(2, 3000), (4, 1500)]),
                        ..Fight::default()
                    },
                    true,
                );
                ticks(4).await;
                assert_eq!(fight.borrow().cast, [1, 3, 1, 3]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_live_target_is_kept() {
        LocalSet::new()
            .run_until(async {
                let (fight, _settings) = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0],
                        target: Some(3.0),
                        ..Fight::default()
                    },
                    true,
                );
                ticks(3).await;
                assert!(fight.borrow().targeted.is_empty());
                assert_eq!(fight.borrow().cast.len(), 3);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn it_waits_while_off_out_of_game_or_with_nothing_to_fight() {
        LocalSet::new()
            .run_until(async {
                let (off, settings) = start(
                    Fight {
                        in_game: true,
                        monsters: vec![7.0],
                        ..Fight::default()
                    },
                    false,
                );
                let (loading, _loading) = start(
                    Fight {
                        monsters: vec![7.0],
                        ..Fight::default()
                    },
                    true,
                );
                let (empty, _empty) = start(
                    Fight {
                        in_game: true,
                        ..Fight::default()
                    },
                    true,
                );
                ticks(5).await;
                for fight in [&off, &loading, &empty] {
                    assert!(fight.borrow().cast.is_empty());
                }
                settings.send_replace(Settings { enabled: true });
                ticks(1).await;
                assert_eq!(off.borrow().cast, [1]);
            })
            .await;
    }
}
