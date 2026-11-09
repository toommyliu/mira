use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{
    pin, select,
    time::{Instant, sleep},
};

use super::{
    POLL, State, auth, has_avatar,
    inventory::{self, TurnInPacket},
    player,
    players::{self, RoomPlayer},
    poll,
    world::{self, Monster, MonsterQuery},
};
use crate::bridge::{
    self, CallError, Format, Interruption, Loader, Packet, Packets, Value, packets::wire,
};

pub(crate) const TICK: Duration = Duration::from_millis(200);
pub(crate) const MIN_SKILL_INTERVAL: Duration = Duration::from_millis(50);
const CONSUMED_TIMEOUT: Duration = Duration::from_secs(5);
const EXIT_TIMEOUT: Duration = Duration::from_millis(2_500);
const STAY_IDLE: Duration = Duration::from_millis(500);
pub(crate) const DEFAULT_ROTATION: [Skill; 4] = [Skill(1), Skill(2), Skill(3), Skill(4)];

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

pub(crate) async fn hunt(
    loader: &impl Loader,
    query: &MonsterQuery,
    most_matches: bool,
) -> Result<Option<Monster>, CallError> {
    let here = bridge::player::get_cell(loader)?.unwrap_or_default();
    let Some(prey) = world::hunt_target(loader, &here, query, most_matches)? else {
        return Ok(None);
    };
    let reached =
        prey.cell.eq_ignore_ascii_case(&here) || player::jump(loader, &prey.cell, None).await?;
    Ok(reached.then_some(prey))
}

pub(crate) async fn exit(loader: &impl Loader) -> Result<bool, CallError> {
    match player::state(loader)? {
        Some(State::Idle) => return Ok(true),
        Some(State::Combat) => {}
        Some(State::Dead) | None => return Ok(false),
    }
    let here = bridge::player::get_cell(loader)?.unwrap_or_default();
    let pad = bridge::player::get_pad(loader)?;
    let cells = exit_cells(
        world::cells(loader)?,
        &world::monsters(loader, None)?,
        &here,
    );
    for cell in cells.iter().cycle() {
        bridge::combat::cancel_auto_attack(loader)?;
        bridge::combat::cancel_target(loader)?;
        let pad = pad.as_deref().filter(|_| cell.eq_ignore_ascii_case(&here));
        player::jump(loader, cell, pad).await?;
        if let Some(left) = settle(loader).await? {
            return Ok(left);
        }
    }
    Ok(false)
}

fn exit_cells(cells: Vec<String>, monsters: &[Monster], here: &str) -> Vec<String> {
    let occupied = |cell: &str, aggressive: bool| {
        monsters.iter().any(|monster| {
            monster.cell.eq_ignore_ascii_case(cell) && (monster.aggressive || !aggressive)
        })
    };
    let mut cells: Vec<String> = cells
        .into_iter()
        .filter(|cell| {
            !matches!(cell.to_ascii_lowercase().as_str(), "" | "blank" | "wait")
                && !occupied(cell, true)
        })
        .collect();
    cells.sort_by_key(|cell| (occupied(cell, false), !cell.eq_ignore_ascii_case(here)));
    cells
}

async fn settle(loader: &impl Loader) -> Result<Option<bool>, CallError> {
    let mut idle_since = None;
    poll(POLL, EXIT_TIMEOUT, || {
        if !has_avatar(loader)? || !player::alive(loader)? {
            return Ok(Some(false));
        }
        if player::state(loader)? != Some(State::Idle) {
            idle_since = None;
            return Ok(None);
        }
        let since = *idle_since.get_or_insert_with(Instant::now);
        Ok((since.elapsed() >= STAY_IDLE).then_some(true))
    })
    .await
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

#[derive(Deserialize)]
pub(crate) struct FightMessagesPacket {
    #[serde(default)]
    pub(crate) anims: Vec<Option<Animation>>,
}

impl Packet for FightMessagesPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "ct";
}

#[derive(Deserialize)]
pub(crate) struct Animation {
    #[serde(default, deserialize_with = "wire::text")]
    pub(crate) msg: String,
    #[serde(rename = "cInf", default, deserialize_with = "wire::text")]
    pub(crate) caster: String,
    #[serde(rename = "tInf", default, deserialize_with = "wire::text")]
    pub(crate) target: String,
}

#[derive(Deserialize)]
pub(crate) struct UserMessagePacket {
    #[serde(rename = "s", default, deserialize_with = "wire::text")]
    pub(crate) text: String,
}

impl Packet for UserMessagePacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "umsg";
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
    skills: &[Skill],
    interval: Duration,
    priority: &[MonsterQuery],
) -> Result<bool, CallError> {
    if !auth::in_game(loader)? {
        return Ok(false);
    }
    let Some(id) = world::available_mon_map_id(loader, query)? else {
        return Ok(false);
    };
    fights.clear();
    let first = world::first_available_mon_map_id(loader, priority)?.unwrap_or(id);
    if !attack_id(loader, first)? {
        return Ok(false);
    }
    let engaged = Engaged(loader);
    let killed = fight(loader, &fights, id, skills, interval, priority).await;
    std::mem::forget(engaged);
    killed
}

async fn fight(
    loader: &impl Loader,
    fights: &Packets<CtPacket>,
    id: f64,
    skills: &[Skill],
    interval: Duration,
    priority: &[MonsterQuery],
) -> Result<bool, CallError> {
    let cell = bridge::player::get_cell(loader)?;
    let mut next = 0;
    loop {
        if !auth::in_game(loader)? || bridge::player::get_cell(loader)? != cell {
            return Ok(false);
        }
        let wanted = world::first_available_mon_map_id(loader, priority)?.unwrap_or(id);
        if !matches!(target(loader)?, Some(Target::Monster(monster)) if monster.mon_map_id == wanted)
        {
            attack_id(loader, wanted)?;
        }
        use_next_skill(loader, skills, &mut next)?;
        let tick = sleep(interval);
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

pub(crate) fn use_next_skill(
    loader: &impl Loader,
    skills: &[Skill],
    next: &mut usize,
) -> Result<(), CallError> {
    for offset in 0..skills.len() {
        let index = (*next + offset) % skills.len();
        let skill = skills[index].index();
        if bridge::combat::get_skill_cooldown_remaining(loader, skill)? > 0 {
            continue;
        }
        if bridge::combat::use_skill(loader, &skill.to_string(), None, None)? {
            *next = (index + 1) % skills.len();
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
        attacked: Vec<f64>,
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
                    fight.attacked.push(id);
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
        let started = Instant::now();
        assert_eq!(
            kill(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                &named("frogzard"),
                &DEFAULT_ROTATION,
                Duration::from_millis(700),
                &[],
            )
            .await,
            Ok(true)
        );
        assert_eq!(started.elapsed(), Duration::from_millis(1400));
        let fight = fight.borrow();
        assert_eq!(fight.cast, [1, 2, 3]);
        assert_eq!(fight.monsters[1].hp, 0);
        assert_eq!(fight.monsters[2].hp, 5);
        assert_eq!(loader.count("combat.cancelTarget"), 0);
        assert_eq!(loader.count("world.isMonsterAvailable"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn kill_fights_priority_targets_first_with_the_given_skills() {
        let (loader, fight, events) = fighting(Fight {
            cell: "r2",
            monsters: frogzards(),
            ..Fight::default()
        });
        assert_eq!(
            kill(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                &named("Frogzard"),
                &[Skill(3), Skill(1)],
                TICK,
                &[named("Dragon"), named("Slime")],
            )
            .await,
            Ok(true)
        );
        let fight = fight.borrow();
        assert_eq!(fight.attacked, [3.0, 2.0]);
        assert_eq!(fight.cast, [3, 1, 3, 1, 3, 1, 3, 1]);
        assert_eq!((fight.monsters[1].hp, fight.monsters[2].hp), (0, 0));
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
                &DEFAULT_ROTATION,
                TICK,
                &[],
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
                &named("Dragon"),
                &DEFAULT_ROTATION,
                TICK,
                &[],
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
                &named("Frogzard"),
                &DEFAULT_ROTATION,
                TICK,
                &[],
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
                &DEFAULT_ROTATION,
                TICK,
                &[],
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

    #[tokio::test(start_paused = true)]
    async fn hunting_jumps_to_a_cell_with_a_live_match() {
        let lair = [
            ("Frogzard", "r2", 0.0),
            ("Slime", "r2", 1.0),
            ("Frogzard", "r3", 1.0),
            ("Frogzard", "r4", 2.0),
            ("Frogzard", "r4", 1.0),
            ("Slime", "r3", 1.0),
        ];
        for (cell, query, most_matches, expected) in [
            ("r2", named("Frogzard"), false, Some(("r3", 3.0))),
            ("r3", named("Frogzard"), false, Some(("r3", 3.0))),
            ("r2", named("Frogzard"), true, Some(("r4", 4.0))),
            ("r3", named("Frogzard"), true, Some(("r4", 4.0))),
            ("r4", named("*"), true, Some(("r4", 4.0))),
            ("r3", named("*"), true, Some(("r3", 3.0))),
            (
                "r2",
                MonsterQuery::MonMapId { mon_map_id: 1.0 },
                false,
                None,
            ),
            ("r2", named("Dragon"), true, None),
        ] {
            let here = Rc::new(RefCell::new(cell.to_owned()));
            let state = here.clone();
            let loader = FakeLoader::new(move |callback, args| {
                Ok(match callback {
                    "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                    "player.getCell" => Value::String(state.borrow().clone()),
                    "world.getCells" => Value::List(
                        ["Enter", "r2", "r3", "r4"]
                            .map(|cell| Value::String(cell.into()))
                            .to_vec(),
                    ),
                    "world.getMonsters" => Value::List(
                        lair.iter()
                            .zip(1..)
                            .map(|((name, cell, state), id)| {
                                row(&[
                                    ("monMapId", number(f64::from(id))),
                                    ("name", Value::String((*name).into())),
                                    ("cell", Value::String((*cell).into())),
                                    ("state", number(*state)),
                                ])
                            })
                            .collect(),
                    ),
                    "player.jump" => {
                        let Value::String(cell) = &args[0] else {
                            panic!("jump needs a cell: {args:?}");
                        };
                        *state.borrow_mut() = cell.clone();
                        Value::Undefined
                    }
                    "world.isCellReady" => {
                        Value::Bool(args[0] == Value::String(state.borrow().clone()))
                    }
                    other => panic!("unexpected call {other} {args:?}"),
                })
            });
            let hunted = hunt(&loader, &query, most_matches).await.unwrap();
            assert_eq!(
                hunted.map(|monster| (here.borrow().clone(), monster.mon_map_id)),
                expected.map(|(cell, id)| (cell.to_owned(), id)),
                "from {cell} hunting {query:?}, most matches {most_matches}"
            );
            let jumped = expected.is_some_and(|(to, _)| to != cell);
            assert_eq!(
                loader.count("player.jump"),
                usize::from(jumped),
                "{query:?}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn exiting_combat_jumps_to_cells_without_aggressive_monsters_until_idle() {
        struct Case {
            here: &'static str,
            state: f64,
            aggressive: &'static [&'static str],
            calm: &'static [&'static str],
            reaggro: &'static [&'static str],
            dies_after: Option<usize>,
            logs_out_after: Option<usize>,
            left: bool,
            jumps: &'static [(&'static str, &'static str)],
        }
        let (dead, idle, fighting) = (0.0, 1.0, 2.0);
        let fight = Case {
            here: "r2",
            state: fighting,
            aggressive: &["r1"],
            calm: &[],
            reaggro: &[],
            dies_after: None,
            logs_out_after: None,
            left: true,
            jumps: &[],
        };
        for case in [
            Case {
                state: idle,
                ..fight
            },
            Case {
                state: dead,
                left: false,
                ..fight
            },
            Case {
                here: "r3",
                calm: &["r3"],
                jumps: &[("r3", "Left")],
                ..fight
            },
            Case {
                calm: &["r2"],
                jumps: &[("Enter", "Spawn"), ("r3", "Spawn"), ("r2", "Left")],
                ..fight
            },
            Case {
                here: "r1",
                calm: &["Enter", "r1"],
                jumps: &[("Enter", "Spawn")],
                ..fight
            },
            Case {
                calm: &["Enter", "r3"],
                reaggro: &["Enter"],
                jumps: &[("Enter", "Spawn"), ("r3", "Spawn")],
                ..fight
            },
            Case {
                dies_after: Some(5),
                left: false,
                jumps: &[
                    ("Enter", "Spawn"),
                    ("r3", "Spawn"),
                    ("r2", "Left"),
                    ("r4", "Spawn"),
                    ("Enter", "Spawn"),
                ],
                ..fight
            },
            Case {
                logs_out_after: Some(2),
                left: false,
                jumps: &[("Enter", "Spawn"), ("r3", "Spawn")],
                ..fight
            },
            Case {
                aggressive: &["Enter", "r1", "r2", "r3", "r4"],
                left: false,
                ..fight
            },
        ] {
            let Case {
                here,
                state,
                aggressive,
                calm,
                reaggro,
                dies_after,
                logs_out_after,
                left,
                jumps,
            } = case;
            let at = Rc::new(RefCell::new((here.to_owned(), None::<Instant>, 0)));
            let jumped = Rc::new(RefCell::new(Vec::new()));
            let (state_at, jumps_seen) = (at.clone(), jumped.clone());
            let loader = FakeLoader::new(move |callback, args| {
                let mut at = state_at.borrow_mut();
                let died = dies_after.is_some_and(|after| at.2 >= after);
                let logged_out = logs_out_after.is_some_and(|after| at.2 >= after);
                if logged_out && matches!(callback, "player.getState" | "player.getHp") {
                    return Err(CallError::Threw {
                        callback,
                        message: "myAvatar is null".into(),
                    });
                }
                Ok(match callback {
                    "auth.isLoggedIn" => Value::Bool(!logged_out),
                    "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                    "flash.isNull" => Value::Bool(false),
                    "player.getCell" => Value::String(at.0.clone()),
                    "player.getPad" => Value::String("Left".into()),
                    "player.getHp" => number(if died { 0.0 } else { 100.0 }),
                    "player.getState" => number(match at.1 {
                        _ if died => dead,
                        None => state,
                        Some(since)
                            if reaggro.contains(&at.0.as_str())
                                && since.elapsed() >= Duration::from_millis(200) =>
                        {
                            fighting
                        }
                        Some(_) if calm.contains(&at.0.as_str()) => idle,
                        Some(_) => fighting,
                    }),
                    "world.getCells" => Value::List(
                        ["Enter", "Blank", "r1", "r2", "r3", "r4"]
                            .map(|cell| Value::String(cell.into()))
                            .to_vec(),
                    ),
                    "world.getMonsters" => Value::List(
                        ["r2", "r4"]
                            .into_iter()
                            .map(|cell| (cell, false))
                            .chain(aggressive.iter().map(|cell| (*cell, true)))
                            .zip(1..)
                            .map(|((cell, aggressive), id)| {
                                row(&[
                                    ("monMapId", number(f64::from(id))),
                                    ("cell", Value::String(cell.into())),
                                    ("aggressive", Value::Bool(aggressive)),
                                ])
                            })
                            .collect(),
                    ),
                    "combat.cancelAutoAttack" | "combat.cancelTarget" => Value::Null,
                    "player.jump" => {
                        let [Value::String(cell), Value::String(pad)] = args else {
                            panic!("jump needs a cell and pad: {args:?}");
                        };
                        jumps_seen.borrow_mut().push((cell.clone(), pad.clone()));
                        *at = (cell.clone(), Some(Instant::now()), at.2 + 1);
                        Value::Undefined
                    }
                    "world.isCellReady" => Value::Bool(args[0] == Value::String(at.0.clone())),
                    other => panic!("unexpected call {other} {args:?}"),
                })
            });
            let expected: Vec<_> = jumps
                .iter()
                .map(|(cell, pad)| ((*cell).to_owned(), (*pad).to_owned()))
                .collect();
            assert_eq!(exit(&loader).await, Ok(left), "{expected:?}");
            assert_eq!(*jumped.borrow(), expected);
        }
    }

    #[test]
    fn only_slots_zero_to_five_are_skills() {
        assert_eq!(serde_json::from_str::<Skill>("5").ok(), Some(Skill(5)));
        assert!(serde_json::from_str::<Skill>("6").is_err());
    }
}
