use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{pin, select, time::sleep};

use super::{
    auth, has_avatar,
    inventory::{self, TurnInPacket},
    players::{self, RoomPlayer},
    world::{self, Monster, MonsterQuery},
};
use crate::bridge::{
    self, CallError, Format, Interruption, Loader, Packet, Packets, Value, packets::wire,
};

pub(crate) const TICK: Duration = Duration::from_millis(200);
const CONSUMED_TIMEOUT: Duration = Duration::from_secs(5);
const SKILLS: [i32; 4] = [1, 2, 3, 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
pub(crate) struct Skill(u8);

impl Skill {
    pub(crate) fn index(self) -> i32 {
        i32::from(self.0)
    }

    fn consumable(self) -> bool {
        self.0 == 5
    }
}

impl TryFrom<u8> for Skill {
    type Error = String;

    fn try_from(slot: u8) -> Result<Self, Self::Error> {
        if slot <= 5 {
            Ok(Skill(slot))
        } else {
            Err(format!("skill {slot} is not a slot from 0 to 5"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum Target {
    Monster(Monster),
    Player(RoomPlayer),
}

pub(crate) fn target(loader: &impl Loader) -> Result<Option<Target>, CallError> {
    let target = bridge::combat::get_target(loader)?;
    let Value::Object(fields) = &target else {
        return Ok(None);
    };
    Ok(match fields.get("type") {
        Some(Value::String(kind)) if kind == "monster" => {
            world::monster_from(target).map(Target::Monster)
        }
        Some(Value::String(kind)) if kind == "player" => {
            players::player_from(target).map(Target::Player)
        }
        _ => None,
    })
}

pub(crate) fn attack(loader: &impl Loader, query: &MonsterQuery) -> Result<bool, CallError> {
    match world::available_mon_map_id(loader, query)? {
        Some(id) => attack_id(loader, id),
        None => Ok(false),
    }
}

fn attack_id(loader: &impl Loader, id: f64) -> Result<bool, CallError> {
    bridge::combat::attack_monster(loader, selector("monsterMapId", Value::Number(id)))
}

pub(crate) async fn use_skill(
    loader: &impl Loader,
    turn_ins: Packets<TurnInPacket>,
    skill: Skill,
    target: Option<&MonsterQuery>,
    force: bool,
    wait_until_ready: bool,
) -> Result<bool, CallError> {
    if wait_until_ready && !ready(loader, skill).await? {
        return Ok(false);
    }
    if !has_avatar(loader)? {
        return Ok(false);
    }
    let selector = match target {
        None => None,
        Some(query) => match world::available_mon_map_id(loader, query)? {
            Some(id) => Some(selector("monsterMapId", Value::Number(id))),
            None => return Ok(false),
        },
    };
    let consumed = if skill.consumable() {
        match slotted_item(loader)? {
            None => return Ok(false),
            item_id => item_id,
        }
    } else {
        None
    };
    turn_ins.clear();
    if !bridge::combat::use_skill(loader, &skill.index().to_string(), selector, Some(force))? {
        return Ok(false);
    }
    let Some(item_id) = consumed else {
        return Ok(true);
    };
    Ok(turn_ins
        .answer(CONSUMED_TIMEOUT, |packet| {
            packet.took(item_id).then_some(())
        })
        .await
        .is_some())
}

fn slotted_item(loader: &impl Loader) -> Result<Option<i32>, CallError> {
    Ok(
        inventory::row(bridge::combat::get_consumable_skill_item(loader)?)
            .and_then(|row| row.positive("itemId")),
    )
}

async fn ready(loader: &impl Loader, skill: Skill) -> Result<bool, CallError> {
    loop {
        if !has_avatar(loader)? {
            return Ok(false);
        }
        let left = bridge::combat::get_skill_cooldown_remaining(loader, skill.index())?;
        if left <= 0 {
            return Ok(true);
        }
        sleep(Duration::from_millis(u64::from(left.unsigned_abs()))).await;
    }
}

#[derive(Deserialize)]
pub(crate) struct CtPacket {
    #[serde(rename = "m", default)]
    monsters: HashMap<String, MonsterState>,
}

#[derive(Deserialize)]
struct MonsterState {
    #[serde(rename = "intState", default, deserialize_with = "wire::number")]
    state: Option<f64>,
}

impl Packet for CtPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "ct";
}

impl CtPacket {
    pub(crate) fn deaths(&self) -> Vec<i32> {
        let mut deaths: Vec<i32> = self
            .monsters
            .iter()
            .filter(|(_, monster)| monster.state == Some(0.0))
            .filter_map(|(id, _)| id.parse().ok())
            .collect();
        deaths.sort_unstable();
        deaths
    }

    fn killed(&self, id: f64) -> bool {
        self.deaths()
            .into_iter()
            .any(|death| f64::from(death) == id)
    }
}

pub(crate) async fn kill(
    loader: &impl Loader,
    fights: Packets<CtPacket>,
    query: &MonsterQuery,
) -> Result<bool, CallError> {
    if !auth::in_game(loader)? {
        return Ok(false);
    }
    let Some(id) = world::available_mon_map_id(loader, query)? else {
        return Ok(false);
    };
    fights.clear();
    if !attack_id(loader, id)? {
        return Ok(false);
    }
    let engaged = Engaged(loader);
    let killed = fight(loader, &fights, id).await;
    std::mem::forget(engaged);
    killed
}

async fn fight(
    loader: &impl Loader,
    fights: &Packets<CtPacket>,
    id: f64,
) -> Result<bool, CallError> {
    let cell = bridge::player::get_cell(loader)?;
    let mut next = 0;
    loop {
        if !auth::in_game(loader)? || bridge::player::get_cell(loader)? != cell {
            return Ok(false);
        }
        if !matches!(target(loader)?, Some(Target::Monster(monster)) if monster.mon_map_id == id) {
            attack_id(loader, id)?;
        }
        use_next_skill(loader, &mut next)?;
        let tick = sleep(TICK);
        pin!(tick);
        loop {
            select! {
                event = fights.next() => match event {
                    Ok(packet) if packet.killed(id) => return Ok(true),
                    Ok(_) => {}
                    Err(Interruption::Lagged) => if !bridge::world::is_monster_available(loader, id)? {
                        return Ok(true);
                    },
                    Err(Interruption::Disconnected) => return Ok(false),
                },
                () = &mut tick => break,
            }
        }
    }
}

struct Engaged<'a, L: Loader>(&'a L);

impl<L: Loader> Drop for Engaged<'_, L> {
    fn drop(&mut self) {
        if let Err(error) = bridge::combat::cancel_target(self.0) {
            tracing::debug!(%error, "couldn't cancel the target of an interrupted kill");
        }
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

pub(crate) fn selector(key: &str, value: Value) -> Value {
    Value::Object([(key.to_owned(), value)].into())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use tokio::time::{Instant, timeout};

    use super::*;
    use crate::{
        bridge::{Event, Events},
        domain::testing::FakeLoader,
    };

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
        in_game_until: Option<Instant>,
        cooling_until: Option<Instant>,
        drop_target_after_first_cast: bool,
        leave_after_first_cast: bool,
        flood_after_death: bool,
        slotted: Option<f64>,
        taken_on_cast: Option<&'static str>,
        cast: Vec<i32>,
    }

    impl Fight {
        fn in_game(&self) -> bool {
            self.in_game_until
                .is_none_or(|until| Instant::now() < until)
        }

        fn cooldown(&self) -> f64 {
            self.cooling_until.map_or(0.0, |until| {
                until.saturating_duration_since(Instant::now()).as_millis() as f64
            })
        }
    }

    fn number(value: f64) -> Value {
        Value::Number(value)
    }

    fn row(fields: &[(&str, Value)]) -> Value {
        Value::Object(
            fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    fn selected(selector: &Value) -> f64 {
        match selector {
            Value::Object(fields) => match fields.get("monsterMapId") {
                Some(Value::Number(id)) => *id,
                _ => panic!("the selector names a monster by id: {fields:?}"),
            },
            other => panic!("not a selector: {other:?}"),
        }
    }

    fn struck(id: f64, state: u8) -> Event {
        Event::ExtensionResponse {
            packet: format!(
                r#"{{"type":"json","dataObj":{{"cmd":"ct","m":{{"{id}":{{"intState":{state}}}}}}}}}"#
            ),
        }
    }

    fn fighting(fight: Fight) -> (FakeLoader, Rc<RefCell<Fight>>, Rc<Events>) {
        let fight = Rc::new(RefCell::new(fight));
        let events = Rc::new(Events::default());
        let state = fight.clone();
        let publisher = events.clone();
        let loader = FakeLoader::new(move |callback, args| {
            let mut fight = state.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" => Value::Bool(fight.in_game()),
                "flash.isNull" => Value::Bool(false),
                "player.getCell" => Value::String(fight.cell.into()),
                "world.getMonsters" => Value::List(
                    fight
                        .monsters
                        .iter()
                        .map(|monster| {
                            row(&[
                                ("monMapId", number(monster.id)),
                                ("name", Value::String(monster.name.into())),
                                ("cell", Value::String("r2".into())),
                                ("hp", number(f64::from(monster.hp))),
                                ("state", number(if monster.hp > 0 { 1.0 } else { 0.0 })),
                                ("available", Value::Bool(monster.hp > 0)),
                            ])
                        })
                        .collect(),
                ),
                "combat.attackMonster" => {
                    let id = selected(&args[0]);
                    let alive = fight.monsters.iter().any(|m| m.id == id && m.hp > 0);
                    if alive {
                        fight.target = Some(id);
                    }
                    Value::Bool(alive)
                }
                "combat.getTarget" => match fight.target {
                    Some(id) => row(&[
                        ("type", Value::String("monster".into())),
                        ("monMapId", number(id)),
                    ]),
                    None => Value::Null,
                },
                "combat.cancelTarget" => {
                    fight.target = None;
                    Value::Null
                }
                "world.isMonsterAvailable" => {
                    let [Value::Number(id)] = args else {
                        panic!("isMonsterAvailable needs an id: {args:?}");
                    };
                    let alive = fight.monsters.iter().any(|m| m.id == *id && m.hp > 0);
                    Value::Bool(fight.cell == "r2" && alive)
                }
                "combat.getSkillCooldownRemaining" => number(fight.cooldown()),
                "combat.getConsumableSkillItem" => fight.slotted.map_or(Value::Null, |id| {
                    row(&[("itemId", number(id)), ("ready", Value::Bool(true))])
                }),
                "combat.useSkill" => {
                    let Value::String(skill) = &args[0] else {
                        panic!("useSkill needs a skill: {args:?}");
                    };
                    if fight.cooldown() > 0.0 {
                        return Ok(Value::Bool(false));
                    }
                    if args[1] != Value::Null {
                        fight.target = Some(selected(&args[1]));
                    }
                    fight.cast.push(skill.parse().unwrap());
                    if let (Some(items), "5") = (fight.taken_on_cast, skill.as_str()) {
                        publisher.publish(&Event::ExtensionResponse {
                            packet: format!(
                                r#"{{"type":"json","dataObj":{{"cmd":"turnIn","sItems":"{items}"}}}}"#
                            ),
                        });
                    }
                    publisher.publish(&struck(1.0, 0));
                    let target = fight.target;
                    if let Some(monster) = fight.monsters.iter_mut().find(|m| Some(m.id) == target)
                    {
                        monster.hp = monster.hp.saturating_sub(1);
                        let (id, dead) = (monster.id, monster.hp == 0);
                        publisher.publish(&struck(id, u8::from(!dead)));
                        if dead && fight.flood_after_death {
                            for _ in 0..300 {
                                publisher.publish(&struck(3.0, 1));
                            }
                        }
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
        (loader, fight, events)
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

    fn named(name: &str) -> MonsterQuery {
        MonsterQuery::Name(name.into())
    }

    #[tokio::test(start_paused = true)]
    async fn kill_fights_a_live_monster_of_that_name_until_it_dies() {
        let (loader, fight, events) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            drop_target_after_first_cast: true,
            ..Fight::default()
        });
        assert_eq!(
            kill(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                &named("frogzard")
            )
            .await,
            Ok(true)
        );
        let fight = fight.borrow();
        assert_eq!(fight.cast, [1, 2, 3]);
        assert_eq!(fight.monsters[1].hp, 0);
        assert_eq!(fight.monsters[2].hp, 5);
        assert_eq!(loader.count("combat.cancelTarget"), 0);
        assert_eq!(loader.count("world.isMonsterAvailable"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_death_lost_to_a_flood_of_packets_still_ends_the_kill() {
        let (loader, fight, events) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            flood_after_death: true,
            ..Fight::default()
        });
        let frogzard = named("Frogzard");
        let killed = timeout(
            Duration::from_secs(5),
            kill(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                &frogzard,
            ),
        );
        assert_eq!(killed.await, Ok(Ok(true)));
        assert_eq!(fight.borrow().cast, [1, 2, 3]);
    }

    #[tokio::test(start_paused = true)]
    async fn kill_fails_when_no_monster_of_that_name_is_alive() {
        let (loader, fight, events) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            ..Fight::default()
        });
        assert_eq!(
            kill(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                &named("Dragon")
            )
            .await,
            Ok(false)
        );
        assert!(fight.borrow().cast.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn leaving_the_cell_before_the_monster_dies_fails_the_kill() {
        let (loader, fight, events) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            leave_after_first_cast: true,
            ..Fight::default()
        });
        assert_eq!(
            kill(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                &named("Frogzard")
            )
            .await,
            Ok(false)
        );
        assert_eq!(fight.borrow().cast, [1]);
    }

    #[tokio::test(start_paused = true)]
    async fn interrupting_a_kill_drops_its_target() {
        let (loader, fight, events) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            ..Fight::default()
        });
        let interrupted = timeout(
            TICK,
            kill(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                &MonsterQuery::MonMapId { mon_map_id: 3.0 },
            ),
        );
        assert!(interrupted.await.is_err());
        let fight = fight.borrow();
        assert!(!fight.cast.is_empty());
        assert_eq!(fight.target, None);
    }

    fn turn_ins() -> Packets<TurnInPacket> {
        Packets::subscribe(|wants| Events::default().subscribe(wants))
    }

    #[tokio::test(start_paused = true)]
    async fn waiting_until_ready_casts_once_the_cooldown_ends() {
        let cooldown = Duration::from_millis(2500);
        let (loader, fight, _) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            target: Some(2.0),
            cooling_until: Some(Instant::now() + cooldown),
            ..Fight::default()
        });
        let skill = Skill(2);
        assert_eq!(
            use_skill(&loader, turn_ins(), skill, None, false, false).await,
            Ok(false)
        );
        let started = Instant::now();
        assert_eq!(
            use_skill(&loader, turn_ins(), skill, None, false, true).await,
            Ok(true)
        );
        assert_eq!(started.elapsed(), cooldown);
        assert_eq!(fight.borrow().cast, [2]);
    }

    #[tokio::test(start_paused = true)]
    async fn logging_out_while_waiting_gives_up_without_casting() {
        let (loader, fight, _) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            target: Some(2.0),
            in_game_until: Some(Instant::now() + Duration::from_secs(1)),
            cooling_until: Some(Instant::now() + Duration::from_secs(2)),
            ..Fight::default()
        });
        assert_eq!(
            use_skill(&loader, turn_ins(), Skill(1), None, false, true).await,
            Ok(false)
        );
        assert!(fight.borrow().cast.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_skill_aimed_at_a_monster_targets_a_live_one_or_is_not_used() {
        let (loader, fight, _) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            ..Fight::default()
        });
        let frogzard = named("Frogzard");
        let dragon = named("Dragon");
        assert_eq!(
            use_skill(&loader, turn_ins(), Skill(1), Some(&dragon), false, false).await,
            Ok(false)
        );
        assert!(fight.borrow().cast.is_empty());
        assert_eq!(
            use_skill(&loader, turn_ins(), Skill(1), Some(&frogzard), false, false).await,
            Ok(true)
        );
        assert_eq!(fight.borrow().target, Some(2.0));
    }

    #[tokio::test(start_paused = true)]
    async fn the_consumable_slot_finishes_when_the_server_takes_its_item() {
        for (slotted, taken, expected, cast) in [
            (Some(7539.0), Some("4:1,7539:1"), true, vec![5]),
            (Some(7539.0), Some("4:1"), false, vec![5]),
            (None, None, false, vec![]),
        ] {
            let (loader, fight, events) = fighting(Fight {
                cell: "r2",
                slotted,
                taken_on_cast: taken,
                ..Fight::default()
            });
            assert_eq!(
                use_skill(
                    &loader,
                    Packets::subscribe(|wants| events.subscribe(wants)),
                    Skill(5),
                    None,
                    false,
                    false
                )
                .await,
                Ok(expected),
                "{slotted:?} {taken:?}"
            );
            assert_eq!(fight.borrow().cast, cast);
        }
    }

    #[test]
    fn only_slots_zero_to_five_are_skills() {
        assert_eq!(serde_json::from_str::<Skill>("5").ok(), Some(Skill(5)));
        assert!(serde_json::from_str::<Skill>("6").is_err());
    }
}
