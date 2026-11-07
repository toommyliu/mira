use std::time::Duration;

use tokio::time::sleep;

use super::auth;
use crate::bridge::{self, CallError, Loader, Value};

pub(crate) const TICK: Duration = Duration::from_millis(200);
const SKILLS: [i32; 4] = [1, 2, 3, 4];

pub(crate) async fn kill(loader: &impl Loader, monster: &str) -> Result<bool, CallError> {
    if !auth::in_game(loader)? {
        return Ok(false);
    }
    let cell = bridge::player::get_cell(loader)?;
    let named = selector("name", Value::String(monster.to_owned()));
    let id = if bridge::combat::attack_monster(loader, named)? {
        target(loader)?
    } else {
        None
    };
    let Some(id) = id else {
        return Ok(false);
    };
    let mut next = 0;
    loop {
        if !auth::in_game(loader)? || bridge::player::get_cell(loader)? != cell {
            return Ok(false);
        }
        if !bridge::world::is_monster_available(loader, id)? {
            return Ok(true);
        }
        if target(loader)? != Some(id) {
            bridge::combat::attack_monster(loader, selector("monsterMapId", Value::Number(id)))?;
        }
        use_next_skill(loader, &mut next)?;
        sleep(TICK).await;
    }
}

pub(crate) fn use_next_skill(loader: &impl Loader, next: &mut usize) -> Result<(), CallError> {
    for offset in 0..SKILLS.len() {
        let index = (*next + offset) % SKILLS.len();
        let skill = SKILLS[index];
        if bridge::combat::get_skill_cooldown_remaining(loader, skill)? > 0 {
            continue;
        }
        if bridge::combat::use_skill(loader, &skill.to_string(), None, None)? {
            *next = (index + 1) % SKILLS.len();
            break;
        }
    }
    Ok(())
}

fn target(loader: &impl Loader) -> Result<Option<f64>, CallError> {
    let Value::Object(target) = bridge::combat::get_target(loader)? else {
        return Ok(None);
    };
    Ok(match (target.get("type"), target.get("monsterMapId")) {
        (Some(Value::String(kind)), Some(Value::Number(id))) if kind == "monster" => Some(*id),
        _ => None,
    })
}

pub(crate) fn selector(key: &str, value: Value) -> Value {
    Value::Object([(key.to_owned(), value)].into())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::domain::testing::FakeLoader;

    struct Monster {
        id: f64,
        name: &'static str,
        hp: u32,
    }

    #[derive(Default)]
    struct Fight {
        cell: &'static str,
        monsters: Vec<Monster>,
        target: Option<f64>,
        drop_target_after_first_cast: bool,
        leave_after_first_cast: bool,
        cast: Vec<i32>,
    }

    fn fighting(fight: Fight) -> (FakeLoader, Rc<RefCell<Fight>>) {
        let fight = Rc::new(RefCell::new(fight));
        let state = fight.clone();
        let loader = FakeLoader::new(move |callback, args| {
            let mut fight = state.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" => Value::Bool(true),
                "player.getCell" => Value::String(fight.cell.into()),
                "combat.attackMonster" => {
                    let [Value::Object(selector)] = args else {
                        panic!("attackMonster needs a selector: {args:?}");
                    };
                    let found = fight.monsters.iter().find(|monster| {
                        monster.hp > 0
                            && match (selector.get("name"), selector.get("monsterMapId")) {
                                (Some(Value::String(name)), _) => {
                                    monster.name.eq_ignore_ascii_case(name)
                                }
                                (_, Some(Value::Number(id))) => monster.id == *id,
                                _ => false,
                            }
                    });
                    let id = found.map(|monster| monster.id);
                    if id.is_some() {
                        fight.target = id;
                    }
                    Value::Bool(id.is_some())
                }
                "combat.getTarget" => match fight.target {
                    Some(id) => Value::Object(
                        [
                            ("type".to_owned(), Value::String("monster".into())),
                            ("monsterMapId".to_owned(), Value::Number(id)),
                        ]
                        .into(),
                    ),
                    None => Value::Null,
                },
                "world.isMonsterAvailable" => {
                    let [Value::Number(id)] = args else {
                        panic!("isMonsterAvailable needs an id: {args:?}");
                    };
                    let alive = fight.monsters.iter().any(|m| m.id == *id && m.hp > 0);
                    Value::Bool(fight.cell == "r2" && alive)
                }
                "combat.getSkillCooldownRemaining" => Value::Number(0.0),
                "combat.useSkill" => {
                    let Value::String(skill) = &args[0] else {
                        panic!("useSkill needs a skill: {args:?}");
                    };
                    fight.cast.push(skill.parse().unwrap());
                    let target = fight.target;
                    if let Some(monster) = fight.monsters.iter_mut().find(|m| Some(m.id) == target)
                    {
                        monster.hp = monster.hp.saturating_sub(1);
                    }
                    if fight.cast.len() == 1 {
                        if fight.drop_target_after_first_cast {
                            fight.target = None;
                        }
                        if fight.leave_after_first_cast {
                            fight.cell = "Enter";
                        }
                    }
                    Value::Bool(true)
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        (loader, fight)
    }

    fn frogzards() -> Vec<Monster> {
        vec![
            Monster {
                id: 1.0,
                name: "Frogzard",
                hp: 0,
            },
            Monster {
                id: 2.0,
                name: "Frogzard",
                hp: 3,
            },
            Monster {
                id: 3.0,
                name: "Slime",
                hp: 5,
            },
        ]
    }

    #[tokio::test(start_paused = true)]
    async fn kill_fights_a_live_monster_of_that_name_until_it_dies() {
        let (loader, fight) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            drop_target_after_first_cast: true,
            ..Fight::default()
        });
        assert_eq!(kill(&loader, "frogzard").await, Ok(true));
        let fight = fight.borrow();
        assert_eq!(fight.cast, [1, 2, 3]);
        assert_eq!(fight.monsters[1].hp, 0);
        assert_eq!(fight.monsters[2].hp, 5);
    }

    #[tokio::test(start_paused = true)]
    async fn kill_fails_when_no_monster_of_that_name_is_alive() {
        let (loader, fight) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            ..Fight::default()
        });
        assert_eq!(kill(&loader, "Dragon").await, Ok(false));
        assert!(fight.borrow().cast.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn leaving_the_cell_before_the_monster_dies_fails_the_kill() {
        let (loader, fight) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            leave_after_first_cast: true,
            ..Fight::default()
        });
        assert_eq!(kill(&loader, "Frogzard").await, Ok(false));
        assert_eq!(fight.borrow().cast, [1]);
    }
}
