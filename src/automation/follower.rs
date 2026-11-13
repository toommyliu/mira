use std::{sync::Arc, time::Duration};

use tokio::{
    sync::watch,
    time::{Instant, sleep_until},
};

use crate::bridge::{self, CallError, Event, Loader, Subscription};
use crate::domain::{
    self, auth,
    combat::{self, TICK},
    counter_attack::CounterAttacks,
    events::{self, GameEvent, Kind, Myself},
    player::{self, Arrival, Refusal},
    players,
    profiles::Library,
    rotation::{self, Fighter},
    world::MonsterQuery,
};

const CHECK: Duration = Duration::from_secs(1);
const RETRY: Duration = Duration::from_secs(5);
const MOVES: [Kind; 3] = [Kind::CellChanged, Kind::PlayerLeft, Kind::MapJoined];

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Settings {
    pub(crate) enabled: bool,
    pub(crate) target: String,
    pub(crate) copy_walk: bool,
    pub(crate) fallbacks: Vec<String>,
    pub(crate) fight: bool,
    pub(crate) profile: Option<String>,
    pub(crate) attack_first: Vec<MonsterQuery>,
}

impl Settings {
    fn active(&self) -> bool {
        self.enabled && !self.target.trim().is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Status {
    #[default]
    Off,
    Waiting,
    Travelling,
    Following,
    Lost(Lost),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lost {
    Yourself,
    Refused(Refusal),
    Missed,
    Blocked,
}

enum Plan {
    Wait,
    Stay { at: Option<(f64, f64)> },
    Travel(Trip),
    Lost(Lost),
}

enum Trip {
    GoTo,
    Jump { cell: String, pad: String },
}

enum Heard {
    Nothing,
    Me,
    Moved,
    Jumped,
    Walked(f64, f64),
}

pub(crate) async fn run(
    loader: impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    mut settings: watch::Receiver<Settings>,
    counter_attacks: CounterAttacks,
    library: watch::Receiver<Arc<Library>>,
    report: impl Fn(Status),
) {
    loop {
        report(Status::Off);
        if settings.wait_for(Settings::active).await.is_err()
            || !follow(
                &loader,
                &subscribe,
                &mut settings,
                &counter_attacks,
                &library,
                &report,
            )
            .await
        {
            return;
        }
    }
}

pub(crate) fn add_monsters(queries: &mut Vec<MonsterQuery>, text: &str) {
    for entry in text
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let id = entry
            .get(..2)
            .filter(|prefix| prefix.eq_ignore_ascii_case("id"))
            .and_then(|_| entry[2..].strip_prefix(['.', ':', '\'', '-']))
            .and_then(|id| id.trim().parse::<u32>().ok())
            .filter(|id| *id > 0);
        let query = match id {
            Some(id) => MonsterQuery::MonMapId {
                mon_map_id: f64::from(id),
            },
            None => MonsterQuery::Name(entry.to_owned()),
        };
        let known = queries.iter().any(|known| match (known, &query) {
            (MonsterQuery::Name(known), MonsterQuery::Name(name)) => {
                known.eq_ignore_ascii_case(name)
            }
            (known, query) => known == query,
        });
        if !known {
            queries.push(query);
        }
    }
}

pub(crate) fn monster_label(query: &MonsterQuery) -> String {
    match query {
        MonsterQuery::Name(name) => name.clone(),
        MonsterQuery::MonMapId { mon_map_id } => format!("id:{mon_map_id}"),
    }
}

pub(crate) fn add_maps(maps: &mut Vec<String>, text: &str) {
    for map in text
        .split([',', '\n'])
        .map(str::trim)
        .filter(|map| !map.is_empty())
    {
        if !maps.iter().any(|known| known.eq_ignore_ascii_case(map)) {
            maps.push(map.to_owned());
        }
    }
}

async fn follow(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    settings: &mut watch::Receiver<Settings>,
    counter_attacks: &CounterAttacks,
    library: &watch::Receiver<Arc<Library>>,
    report: &impl Fn(Status),
) -> bool {
    let moves = subscribe(moved);
    let mut fighter = Fighter::chosen(None, library).listening(subscribe(rotation::wants));
    let mut last: Option<Settings> = None;
    let mut next = Instant::now();
    let mut stayed: Option<Instant> = None;
    let mut walk: Option<(f64, f64)> = None;
    let mut catch_up = false;
    let mut retrying = false;
    loop {
        let current = settings.borrow_and_update().clone();
        if !current.active() {
            return true;
        }
        if last.as_ref() != Some(&current) {
            if last.as_ref().map(|last| &last.profile) != Some(&current.profile) {
                fighter.switch(Fighter::chosen(current.profile.as_deref(), library));
            }
            catch_up |= current.copy_walk && !last.as_ref().is_some_and(|last| last.copy_walk);
            let retargeted = last
                .as_ref()
                .is_none_or(|last| !same_target(&last.target, &current.target));
            if retargeted {
                stayed = None;
                walk = None;
                retrying = false;
                catch_up |= current.copy_walk;
            }
            if retargeted || stayed.is_some() {
                next = Instant::now();
            }
            last = Some(current.clone());
        }
        let target = current.target.trim();
        tokio::select! {
            changed = settings.changed() => {
                if changed.is_err() {
                    return false;
                }
            }
            () = fighter.listen(loader) => {}
            event = moves.next() => {
                let heard = match event {
                    Ok(event) => heard(loader, &event, target),
                    Err(_) => Heard::Me,
                };
                match heard {
                    Heard::Nothing => continue,
                    Heard::Me if retrying => continue,
                    Heard::Me | Heard::Moved => {}
                    Heard::Jumped => walk = None,
                    Heard::Walked(x, y) => walk = Some((x, y)),
                }
                next = Instant::now();
                stayed = None;
            }
            () = sleep_until(next) => {
                let plan = match stayed {
                    Some(at) if at.elapsed() < CHECK => Plan::Stay { at: None },
                    _ => {
                        let plan = plan(loader, target).unwrap_or_else(|error| {
                            tracing::debug!(%error, "the follower couldn't find its target");
                            Plan::Wait
                        });
                        stayed = matches!(plan, Plan::Stay { .. }).then(Instant::now);
                        plan
                    }
                };
                let (status, wait) = match plan {
                    Plan::Wait => (Status::Waiting, CHECK),
                    Plan::Lost(lost) => (Status::Lost(lost), RETRY),
                    Plan::Stay { at } => {
                        let to = walk.take().or(at.filter(|_| catch_up));
                        catch_up = false;
                        if current.copy_walk
                            && let Some((x, y)) = to
                            && let Err(error) = player::walk_to(loader, x.round() as i32, y.round() as i32)
                        {
                            tracing::debug!(%error, "the follower couldn't copy a walk");
                        }
                        if current.fight {
                            if let Err(error) = combat::fight_any(loader, counter_attacks, &mut fighter, &current.attack_first) {
                                tracing::debug!(%error, "the follower skipped a combat tick");
                            }
                            (Status::Following, TICK)
                        } else {
                            (Status::Following, CHECK)
                        }
                    }
                    Plan::Travel(trip) => {
                        report(Status::Travelling);
                        fighter.restart();
                        let travelling = travel(loader, subscribe, target, &current.fallbacks, trip);
                        tokio::pin!(travelling);
                        let travelled = loop {
                            tokio::select! {
                                changed = settings.changed() => {
                                    if changed.is_err() {
                                        return false;
                                    }
                                    let now = settings.borrow();
                                    if !now.active() || !same_target(&now.target, target) {
                                        break None;
                                    }
                                }
                                travelled = &mut travelling => break Some(travelled),
                            }
                        };
                        moves.clear();
                        match travelled {
                            None => continue,
                            Some(Ok(None)) => {
                                catch_up = true;
                                (Status::Travelling, Duration::ZERO)
                            }
                            Some(Ok(Some(lost))) => {
                                tracing::debug!(?lost, target, "the follower couldn't reach its target");
                                (Status::Lost(lost), RETRY)
                            }
                            Some(Err(error)) => {
                                tracing::debug!(%error, "the follower couldn't reach its target");
                                (Status::Lost(Lost::Missed), RETRY)
                            }
                        }
                    }
                };
                retrying = matches!(status, Status::Lost(_));
                report(status);
                next = Instant::now() + wait;
            }
        }
    }
}

fn same_target(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn moved(event: &Event) -> bool {
    MOVES.iter().any(|kind| kind.wants(event))
}

fn heard(loader: &impl Loader, event: &Event, target: &str) -> Heard {
    if let Event::ExtensionResponse { packet } = event
        && let Some(update) = events::player_update(packet)
        && update.username.eq_ignore_ascii_case(target)
    {
        return if update.pad.is_some() {
            Heard::Jumped
        } else if let Some((x, y)) = update.walk {
            Heard::Walked(x, y)
        } else if update.cell.is_some() || update.joined {
            Heard::Moved
        } else {
            Heard::Nothing
        };
    }
    let mut read = Vec::new();
    for kind in MOVES {
        kind.read(event, &Myself(loader), &mut read);
    }
    read.iter()
        .find_map(|event| match event {
            GameEvent::CellChanged { is_self: true, .. } | GameEvent::MapJoined { .. } => {
                Some(Heard::Me)
            }
            GameEvent::PlayerLeft { username, .. } if username.eq_ignore_ascii_case(target) => {
                Some(Heard::Moved)
            }
            _ => None,
        })
        .unwrap_or(Heard::Nothing)
}

fn plan(loader: &impl Loader, target: &str) -> Result<Plan, CallError> {
    if !domain::ready(loader)? || !player::alive(loader)? {
        return Ok(Plan::Wait);
    }
    if auth::username(loader)?.is_some_and(|me| me.eq_ignore_ascii_case(target)) {
        return Ok(Plan::Lost(Lost::Yourself));
    }
    let Some(them) = players::get(loader, target)? else {
        return Ok(Plan::Travel(Trip::GoTo));
    };
    if them.cell.is_empty() {
        return Ok(Plan::Wait);
    }
    let here = bridge::player::get_cell(loader)?.unwrap_or_default();
    Ok(if them.cell.eq_ignore_ascii_case(&here) {
        Plan::Stay {
            at: them.position.map(|position| (position.x, position.y)),
        }
    } else {
        Plan::Travel(Trip::Jump {
            cell: them.cell,
            pad: them.pad,
        })
    })
}

async fn travel(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    target: &str,
    fallbacks: &[String],
    trip: Trip,
) -> Result<Option<Lost>, CallError> {
    match trip {
        Trip::GoTo => {
            let refusal = match player::go_to(loader, subscribe, target).await? {
                Arrival::Arrived => return Ok(None),
                Arrival::Refused(refusal) => refusal,
                Arrival::Missed => return Ok(Some(Lost::Missed)),
            };
            for map in fallbacks {
                if player::join(loader, subscribe, map, None, None).await?
                    && players::get(loader, target)?.is_some()
                {
                    return Ok(None);
                }
            }
            Ok(Some(Lost::Refused(refusal)))
        }
        Trip::Jump { cell, pad } => {
            let pad = (!pad.is_empty()).then_some(pad.as_str());
            Ok((!player::jump(loader, &cell, pad).await?).then_some(Lost::Blocked))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;
    use tokio::{
        task::{LocalSet, spawn_local},
        time::sleep,
    };

    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    #[derive(Clone)]
    struct Spot {
        map: String,
        cell: String,
        pad: String,
        x: f64,
        y: f64,
    }

    fn at(map: &str, cell: &str) -> Spot {
        Spot {
            map: map.into(),
            cell: cell.into(),
            pad: "Spawn".into(),
            x: 100.0,
            y: 200.0,
        }
    }

    struct World {
        me: Spot,
        lead: Option<Spot>,
        refusal: Option<(&'static str, &'static str)>,
        monsters: Vec<(f64, &'static str)>,
        target: Option<f64>,
        jumps: Vec<String>,
        joins: Vec<String>,
        walks: Vec<(f64, f64)>,
        attacked: Vec<f64>,
        gotos: usize,
        scans: usize,
    }

    fn world(me: Spot, lead: Option<Spot>) -> World {
        World {
            me,
            lead,
            refusal: None,
            monsters: Vec::new(),
            target: None,
            jumps: Vec::new(),
            joins: Vec::new(),
            walks: Vec::new(),
            attacked: Vec::new(),
            gotos: 0,
            scans: 0,
        }
    }

    struct Started {
        world: Rc<RefCell<World>>,
        events: Rc<Events>,
        settings: watch::Sender<Settings>,
        statuses: Rc<RefCell<Vec<Status>>>,
    }

    impl Started {
        fn status(&self) -> Status {
            self.statuses.borrow().last().copied().unwrap_or_default()
        }

        fn publish(&self, packet: serde_json::Value) {
            self.events.publish(&Event::ExtensionResponse {
                packet: packet.to_string(),
            });
        }
    }

    fn following(fight: bool) -> Settings {
        Settings {
            enabled: true,
            target: " Lead ".into(),
            fight,
            ..Settings::default()
        }
    }

    fn text(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            other => panic!("expected text, got {other:?}"),
        }
    }

    fn number(value: &Value) -> f64 {
        match value {
            Value::Number(number) => *number,
            other => panic!("expected a number, got {other:?}"),
        }
    }

    fn object(fields: Vec<(&str, Value)>) -> Value {
        Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    fn start(world: World, settings: Settings) -> Started {
        let world = Rc::new(RefCell::new(world));
        let events = Rc::new(Events::default());
        let (state, publisher) = (world.clone(), events.clone());
        let publish = move |packet: serde_json::Value| {
            publisher.publish(&Event::ExtensionResponse {
                packet: packet.to_string(),
            });
        };
        let loader = FakeLoader::new(move |callback, args| {
            let mut world = state.borrow_mut();
            let monster = |id: f64, name: &str, cell: &str| {
                object(vec![
                    ("type", Value::String("monster".into())),
                    ("monMapId", Value::Number(id)),
                    ("name", Value::String(name.into())),
                    ("cell", Value::String(cell.into())),
                    ("available", Value::Bool(true)),
                ])
            };
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "player.getHp" => Value::Number(100.0),
                "player.getState" => Value::Number(1.0),
                "flash.getGameObjectS" => Value::String(r#""me""#.into()),
                "world.isActionAvailable" => Value::Bool(true),
                "player.getMap" => Value::String(world.me.map.clone()),
                "player.getCell" => Value::String(world.me.cell.clone()),
                "players.get" => match &world.lead {
                    Some(lead) if lead.map == world.me.map => object(vec![
                        ("username", Value::String("lead".into())),
                        ("userId", Value::Number(2.0)),
                        ("cell", Value::String(lead.cell.clone())),
                        ("pad", Value::String(lead.pad.clone())),
                        ("x", Value::Number(lead.x)),
                        ("y", Value::Number(lead.y)),
                    ]),
                    _ => Value::Null,
                },
                "player.goToPlayer" => {
                    world.gotos += 1;
                    if let Some((cmd, message)) = world.refusal {
                        publish(json!({ "type": "str", "dataObj": [cmd, "-1", message] }));
                    } else if let Some(lead) = world.lead.clone() {
                        world.me = lead;
                    }
                    Value::Undefined
                }
                "player.joinMap" => {
                    let map = text(&args[0]);
                    world.joins.push(map.clone());
                    let name = map.split('-').next().unwrap_or_default().to_owned();
                    world.me = at(&name, "Enter");
                    publish(
                        json!({ "type": "json", "dataObj": { "cmd": "moveToArea", "strMapName": name } }),
                    );
                    Value::Undefined
                }
                "world.getCells" => Value::List(
                    ["Enter", "r2", "r3", "Room"]
                        .map(|cell| Value::String(cell.into()))
                        .to_vec(),
                ),
                "player.jump" => {
                    world.me.cell = text(&args[0]);
                    world.me.pad = text(&args[1]);
                    let jumped = format!("{}/{}", world.me.cell, world.me.pad);
                    world.jumps.push(jumped);
                    Value::Undefined
                }
                "player.walkTo" => {
                    let walked = (number(&args[0]), number(&args[1]));
                    world.walks.push(walked);
                    Value::Bool(true)
                }
                "world.isCellReady" => Value::Bool(world.me.cell == text(&args[0])),
                "player.getAuras" => Value::List(Vec::new()),
                "combat.hasTarget" => Value::Bool(world.target.is_some()),
                "combat.getTarget" => match world.target {
                    Some(id) => monster(id, "", &world.me.cell),
                    None => Value::Null,
                },
                "world.getMonsters" => Value::List(
                    world
                        .monsters
                        .iter()
                        .map(|(id, name)| monster(*id, name, &world.me.cell))
                        .collect(),
                ),
                "world.getAvailableMonsterMapIds" => {
                    world.scans += 1;
                    Value::List(
                        world
                            .monsters
                            .iter()
                            .map(|(id, _)| Value::Number(*id))
                            .collect(),
                    )
                }
                "combat.attackMonster" => {
                    let [Value::Object(selector)] = args else {
                        panic!("attackMonster needs a selector: {args:?}");
                    };
                    let id = number(&selector["monsterMapId"]);
                    world.attacked.push(id);
                    world.target = Some(id);
                    Value::Bool(true)
                }
                "combat.getSnapshot" => Value::Null,
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        let (settings, watched) = watch::channel(settings);
        let (_, library) = watch::channel(Arc::new(Library::default()));
        let statuses = Rc::new(RefCell::new(Vec::new()));
        let reported = statuses.clone();
        let subscriber = events.clone();
        spawn_local(run(
            loader,
            move |wants| subscriber.subscribe(wants),
            watched,
            CounterAttacks::default(),
            library,
            move |status| reported.borrow_mut().push(status),
        ));
        Started {
            world,
            events,
            settings,
            statuses,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn it_jumps_to_its_target_s_cell_as_soon_as_they_move() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    world(
                        at("battleon", "Enter"),
                        Some(Spot {
                            pad: "Left".into(),
                            ..at("battleon", "r2")
                        }),
                    ),
                    following(false),
                );
                sleep(Duration::from_millis(500)).await;
                assert_eq!(started.world.borrow().jumps, ["r2/Left"]);
                assert_eq!(started.status(), Status::Following);

                started.world.borrow_mut().lead = Some(Spot {
                    pad: "Right".into(),
                    ..at("battleon", "r3")
                });
                started.publish(json!({ "type": "json", "dataObj": {
                    "cmd": "uotls", "unm": "Lead", "o": { "strFrame": "r3", "strPad": "Right" }
                }}));
                sleep(Duration::from_millis(500)).await;
                assert_eq!(started.world.borrow().jumps, ["r2/Left", "r3/Right"]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn it_goes_to_a_target_in_another_map() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    world(at("battleon", "Enter"), Some(at("yulgar", "Room"))),
                    following(false),
                );
                sleep(Duration::from_millis(500)).await;
                let world = started.world.borrow();
                assert_eq!((world.gotos, world.me.map.as_str()), (1, "yulgar"));
                assert_eq!(started.status(), Status::Following);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_goto_is_reported_and_tried_again_later() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    World {
                        refusal: Some(("warning", "Cannot goto to player in a Locked zone.")),
                        ..world(at("battleon", "Enter"), Some(at("yulgar", "Room")))
                    },
                    following(false),
                );
                sleep(Duration::from_millis(500)).await;
                assert_eq!(started.world.borrow().gotos, 1);
                assert_eq!(
                    started.status(),
                    Status::Lost(Lost::Refused(Refusal::LockedZone))
                );

                started.world.borrow_mut().refusal = None;
                sleep(RETRY).await;
                assert_eq!(started.world.borrow().gotos, 2);
                assert_eq!(started.status(), Status::Following);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn it_fights_only_beside_its_target_and_only_when_asked() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    world(at("battleon", "Enter"), Some(at("battleon", "Enter"))),
                    following(false),
                );
                sleep(Duration::from_secs(2)).await;
                assert_eq!(started.world.borrow().scans, 0);

                started.settings.send_replace(following(true));
                sleep(Duration::from_secs(1)).await;
                assert!(started.world.borrow().scans >= 4);

                started.settings.send_replace(Settings {
                    enabled: false,
                    ..following(true)
                });
                sleep(Duration::from_millis(1)).await;
                let scans = started.world.borrow().scans;
                sleep(Duration::from_secs(2)).await;
                assert_eq!(started.world.borrow().scans, scans);
                assert_eq!(started.status(), Status::Off);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn following_yourself_is_reported_without_moving() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    world(at("battleon", "Enter"), None),
                    Settings {
                        target: "ME".into(),
                        ..following(true)
                    },
                );
                sleep(Duration::from_secs(2)).await;
                let world = started.world.borrow();
                assert_eq!((world.gotos, world.jumps.len(), world.scans), (0, 0, 0));
                assert_eq!(started.status(), Status::Lost(Lost::Yourself));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn copying_walks_catches_up_then_follows_each_walk_and_cell_change() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    world(at("battleon", "Enter"), Some(at("battleon", "r2"))),
                    Settings {
                        copy_walk: true,
                        ..following(false)
                    },
                );
                sleep(Duration::from_millis(500)).await;
                assert_eq!(started.world.borrow().jumps, ["r2/Spawn"]);
                assert_eq!(started.world.borrow().walks, [(100.0, 200.0)]);

                started.publish(json!({ "type": "str", "dataObj": ["uotls", "-1", "Lead", "tx:793,ty:353,sp:8,strFrame:r2"] }));
                sleep(Duration::from_millis(50)).await;
                assert_eq!(
                    started.world.borrow().walks,
                    [(100.0, 200.0), (793.0, 353.0)]
                );

                started.world.borrow_mut().lead.as_mut().unwrap().cell = "r3".into();
                started.publish(json!({ "type": "str", "dataObj": ["uotls", "-1", "Lead", "tx:40,ty:300,sp:8,strFrame:r3"] }));
                sleep(Duration::from_millis(50)).await;
                let world = started.world.borrow();
                assert_eq!(world.jumps, ["r2/Spawn", "r3/Spawn"]);
                assert_eq!(world.walks.last(), Some(&(40.0, 300.0)));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn walks_are_left_alone_unless_copying_is_on() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    world(at("battleon", "r2"), Some(at("battleon", "r2"))),
                    following(false),
                );
                sleep(Duration::from_millis(100)).await;
                started.publish(json!({ "type": "str", "dataObj": ["uotls", "-1", "Lead", "tx:793,ty:353,sp:8,strFrame:r2"] }));
                sleep(Duration::from_millis(100)).await;
                assert!(started.world.borrow().walks.is_empty());
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn fallback_maps_are_joined_in_order_until_the_target_is_there() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    World {
                        refusal: Some(("warning", "Cannot goto to player in a Locked zone.")),
                        ..world(at("battleon", "Enter"), Some(at("yulgar", "Room")))
                    },
                    Settings {
                        fallbacks: vec!["darkovia".into(), "yulgar-9999".into()],
                        ..following(false)
                    },
                );
                sleep(Duration::from_millis(500)).await;
                let world = started.world.borrow();
                assert_eq!(world.joins, ["darkovia", "yulgar-9999"]);
                assert_eq!(
                    (world.me.map.as_str(), world.me.cell.as_str()),
                    ("yulgar", "Room")
                );
                assert_eq!(started.status(), Status::Following);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn listed_monsters_are_attacked_first_even_mid_fight() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    World {
                        monsters: vec![(1.0, "Escherion"), (2.0, "Staff of Inversion")],
                        target: Some(1.0),
                        ..world(at("battleon", "Enter"), Some(at("battleon", "Enter")))
                    },
                    Settings {
                        attack_first: vec![MonsterQuery::Name("staff of inversion".into())],
                        ..following(true)
                    },
                );
                sleep(Duration::from_secs(1)).await;
                assert_eq!(started.world.borrow().attacked, [2.0]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn only_a_new_target_sends_another_goto_early() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    World {
                        refusal: Some(("warning", "Cannot goto to player in a Locked zone.")),
                        ..world(at("battleon", "Enter"), Some(at("doomvaultb", "Enter")))
                    },
                    following(false),
                );
                sleep(Duration::from_millis(100)).await;
                for typed in ["d", "do", "doo", "doom"] {
                    started.settings.send_modify(|settings| {
                        settings.fallbacks = vec![typed.into()];
                        settings.attack_first = vec![MonsterQuery::Name(typed.into())];
                    });
                    sleep(Duration::from_millis(100)).await;
                }
                started.publish(json!({ "type": "json", "dataObj": { "cmd": "moveToArea", "strMapName": "battleon" } }));
                started.publish(json!({ "type": "str", "dataObj": ["uotls", "-1", "me", "strFrame:r2,strPad:Left"] }));
                sleep(Duration::from_millis(100)).await;
                assert_eq!(started.world.borrow().gotos, 1);
                sleep(RETRY).await;
                assert_eq!(started.world.borrow().gotos, 2);

                started.settings.send_modify(|settings| settings.target = "Other".into());
                sleep(Duration::from_millis(100)).await;
                assert_eq!(started.world.borrow().gotos, 3);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_target_is_caught_up_with_when_copying_walks() {
        LocalSet::new()
            .run_until(async {
                let started = start(
                    world(at("battleon", "Enter"), Some(at("battleon", "Enter"))),
                    Settings {
                        copy_walk: true,
                        ..following(false)
                    },
                );
                sleep(Duration::from_millis(100)).await;
                assert_eq!(started.world.borrow().walks.len(), 1);
                started.world.borrow_mut().lead.as_mut().unwrap().x = 640.0;
                started
                    .settings
                    .send_modify(|settings| settings.target = "Other".into());
                sleep(Duration::from_millis(100)).await;
                assert_eq!(started.world.borrow().walks.last(), Some(&(640.0, 200.0)));
            })
            .await;
    }

    #[test]
    fn typed_entries_are_split_trimmed_and_added_once() {
        let mut monsters = vec![MonsterQuery::Name("Staff".into())];
        add_monsters(&mut monsters, " staff, id.3 ,Escherion,, ID:3, id:x");
        assert_eq!(
            monsters,
            [
                MonsterQuery::Name("Staff".into()),
                MonsterQuery::MonMapId { mon_map_id: 3.0 },
                MonsterQuery::Name("Escherion".into()),
                MonsterQuery::Name("id:x".into()),
            ]
        );
        assert_eq!(
            monsters.iter().map(monster_label).collect::<Vec<_>>(),
            ["Staff", "id:3", "Escherion", "id:x"]
        );
        let mut maps = vec!["ultra-9999".to_owned()];
        add_maps(&mut maps, "Ultra-9999,\n tercess , TERCESS");
        assert_eq!(maps, ["ultra-9999", "tercess"]);
    }
}
