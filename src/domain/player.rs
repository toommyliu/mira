use std::time::Duration;

use serde::{Deserialize, Serialize, de::IgnoredAny};
use tokio::time::{Instant, timeout_at};

use super::{
    POLL, State, WarningLine, arrive, combat,
    inventory::{self, ItemQuery, Row},
    players, poll, wait_for_action, wait_until_ready, world,
};
use crate::bridge::{
    self, CallError, Event, Format, GameAction, Interruption, Loader, Packet, Packets,
    Subscription, Value, packets::wire,
};

const JOIN_TIMEOUT: Duration = Duration::from_secs(20);
const JUMP_TIMEOUT: Duration = Duration::from_secs(5);
const WALK_TIMEOUT: Duration = Duration::from_secs(3);
const REST_TIMEOUT: Duration = Duration::from_secs(10);
const RESPAWN_TIMEOUT: Duration = Duration::from_secs(12);
const OUTFIT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CELL: &str = "Enter";
const DEFAULT_PAD: &str = "Spawn";
const OUTFIT_SLOTS: [&str; 8] = ["Weapon", "ar", "ba", "co", "he", "mi", "pe", "None"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Boost {
    Gold,
    Xp,
    Rep,
    ClassPoints,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Faction {
    id: f64,
    name: String,
    rank: f64,
    rep: f64,
}

pub(crate) fn state(loader: &impl Loader) -> Result<Option<State>, CallError> {
    Ok(State::from_game(bridge::player::get_state(loader)?))
}

pub(crate) fn alive(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(bridge::player::get_hp(loader)? > 0 && state(loader)? != Some(State::Dead))
}

pub(crate) fn position(loader: &impl Loader) -> Result<Option<(f64, f64)>, CallError> {
    if bridge::flash::is_null(loader, "world.myAvatar.pMC")? {
        return Ok(None);
    }
    Ok(match bridge::player::get_position(loader)? {
        Value::List(point) => match point.as_slice() {
            [Value::Number(x), Value::Number(y)] => Some((*x, *y)),
            _ => None,
        },
        _ => None,
    })
}

pub(crate) fn has_boost(loader: &impl Loader, boost: Boost) -> Result<bool, CallError> {
    let kind = match boost {
        Boost::Gold => "gold",
        Boost::Xp => "xp",
        Boost::Rep => "rep",
        Boost::ClassPoints => "class",
    };
    bridge::player::has_active_boost(loader, kind)
}

pub(crate) fn factions(loader: &impl Loader) -> Result<Vec<Faction>, CallError> {
    let Value::List(rows) = bridge::player::get_factions(loader)? else {
        return Ok(Vec::new());
    };
    let number = |row: &std::collections::BTreeMap<String, Value>, key: &str| match row.get(key) {
        Some(Value::Number(number)) => *number,
        _ => 0.0,
    };
    Ok(rows
        .into_iter()
        .filter_map(|row| match row {
            Value::Object(row) => match row.get("sName") {
                Some(Value::String(name)) => Some(Faction {
                    id: number(&row, "FactionID"),
                    name: name.clone(),
                    rank: number(&row, "iRank"),
                    rep: number(&row, "iRep"),
                }),
                _ => None,
            },
            _ => None,
        })
        .collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) name: String,
    pub(crate) guild: Option<String>,
    pub(crate) level: Option<u32>,
    pub(crate) class: Option<String>,
}

pub(crate) fn identity(loader: &impl Loader) -> Result<Option<Identity>, CallError> {
    Ok(identity_of(&bridge::player::get_data(loader)?))
}

fn identity_of(data: &Value) -> Option<Identity> {
    let Value::Object(data) = data else {
        return None;
    };
    let Some(Value::String(name)) = data.get("strUsername") else {
        return None;
    };
    let guild = match data.get("guild") {
        Some(Value::Object(guild)) => match guild.get("Name") {
            Some(Value::String(guild)) if !guild.is_empty() => Some(guild.clone()),
            _ => None,
        },
        _ => None,
    };
    let level = match data.get("intLevel") {
        Some(Value::Number(level)) if *level >= 1.0 => Some(*level as u32),
        Some(Value::String(level)) => level.parse().ok(),
        _ => None,
    };
    let class = match data.get("strClassName") {
        Some(Value::String(class)) if !class.trim().is_empty() => Some(class.trim().to_owned()),
        _ => None,
    };
    Some(Identity {
        name: name.clone(),
        guild,
        level,
        class,
    })
}

pub(crate) fn outfit_names(loader: &impl Loader) -> Result<Vec<String>, CallError> {
    let Value::List(outfits) = bridge::outfits::get_all(loader)? else {
        return Ok(Vec::new());
    };
    Ok(outfits
        .into_iter()
        .filter_map(|outfit| match outfit {
            Value::Object(mut outfit) => match outfit.remove("name") {
                Some(Value::String(name)) => Some(name),
                _ => None,
            },
            _ => None,
        })
        .collect())
}

#[derive(Deserialize)]
pub(crate) struct MoveToAreaPacket {
    #[serde(rename = "strMapName", default, deserialize_with = "wire::text")]
    pub(crate) map: String,
}

impl Packet for MoveToAreaPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "moveToArea";
}

pub(crate) async fn join(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    map: &str,
    cell: Option<&str>,
    pad: Option<&str>,
) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    let map = map.trim();
    let (name, room) = split_room(map);
    let cell = cell.unwrap_or(DEFAULT_CELL);
    let pad = pad.unwrap_or(DEFAULT_PAD);
    let here = bridge::player::get_map(loader)?;
    if here.is_some_and(|here| here.eq_ignore_ascii_case(name)) && in_room(loader, room)? {
        return jump(loader, cell, Some(pad)).await;
    }
    if !ready_to_transfer(loader).await? {
        return Ok(false);
    }
    let deadline = Instant::now() + JOIN_TIMEOUT;
    let moves = Packets::<MoveToAreaPacket>::subscribe(subscribe);
    let warnings = Packets::<WarningLine>::subscribe(subscribe);
    bridge::player::join_map(loader, map, Some(cell), Some(pad))?;
    let moved = tokio::select! {
        moved = timeout_at(deadline, moves.next()) => matches!(moved, Ok(Ok(_) | Err(Interruption::Lagged))),
        Some(()) = warnings.answer(JOIN_TIMEOUT, |warning| {
            (refused(&warning.message) == Some(Refusal::RoomFull)).then_some(())
        }) => false,
    };
    if !moved {
        return Ok(false);
    }
    arrive(loader, deadline - Instant::now(), || {
        let loaded = bridge::world::is_loaded(loader)?
            && bridge::player::get_map(loader)?.is_some_and(|here| here.eq_ignore_ascii_case(name));
        if !loaded {
            return Ok(None);
        }
        if !world::has_cell(loader, cell)? {
            return Ok(Some(false));
        }
        Ok((bridge::world::is_cell_ready(loader, cell, None)?
            && bridge::player::is_loaded(loader)?)
        .then_some(true))
    })
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrival {
    Arrived,
    Refused(Refusal),
    Missed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    LockedZone,
    RoomFull,
    IgnoringGoto,
}

fn refused(message: &str) -> Option<Refusal> {
    let message = message.to_lowercase();
    if message.contains("a locked zone.") || message.contains("is not available.") {
        Some(Refusal::LockedZone)
    } else if message.contains("is full") {
        Some(Refusal::RoomFull)
    } else if message.contains("ignoring goto") {
        Some(Refusal::IgnoringGoto)
    } else {
        None
    }
}

#[derive(Deserialize)]
struct ServerLine {
    _cmd: IgnoredAny,
    _room: IgnoredAny,
    message: String,
}

impl Packet for ServerLine {
    const FORMAT: Format = Format::Str;
    const CMD: &'static str = "server";
}

pub(crate) async fn go_to(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    username: &str,
) -> Result<Arrival, CallError> {
    let username = username.trim();
    if username.is_empty() || !wait_until_ready(loader).await? || !ready_to_transfer(loader).await?
    {
        return Ok(Arrival::Missed);
    }
    let warnings = Packets::<WarningLine>::subscribe(subscribe);
    let notices = Packets::<ServerLine>::subscribe(subscribe);
    bridge::player::go_to_player(loader, username)?;
    let arrived = async {
        let landed = arrive(loader, JOIN_TIMEOUT, || {
            if !bridge::world::is_loaded(loader)? || !bridge::player::is_loaded(loader)? {
                return Ok(None);
            }
            let (Some(_), Some(cell)) = (
                players::get(loader, username)?,
                bridge::player::get_cell(loader)?,
            ) else {
                return Ok(None);
            };
            Ok(bridge::world::is_cell_ready(loader, &cell, None)?.then_some(true))
        })
        .await?;
        if !landed {
            return Ok(false);
        }
        let Some(target) = players::get(loader, username)? else {
            return Ok(false);
        };
        let here = bridge::player::get_cell(loader)?.unwrap_or_default();
        if target.cell.eq_ignore_ascii_case(&here) {
            return Ok(true);
        }
        let pad = (!target.pad.is_empty()).then_some(target.pad.as_str());
        jump(loader, &target.cell, pad).await
    };
    tokio::select! {
        arrived = arrived => Ok(if arrived? { Arrival::Arrived } else { Arrival::Missed }),
        answer = answer(&warnings, &notices) => Ok(answer),
    }
}

async fn answer(warnings: &Packets<WarningLine>, notices: &Packets<ServerLine>) -> Arrival {
    loop {
        let message = tokio::select! {
            Ok(warning) = warnings.next() => warning.message,
            Ok(notice) = notices.next() => notice.message,
            else => std::future::pending().await,
        };
        if let Some(refusal) = refused(&message) {
            return Arrival::Refused(refusal);
        }
        if message.to_lowercase().contains("could not be found") {
            return Arrival::Missed;
        }
    }
}

async fn ready_to_transfer(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(
        poll(POLL, RESPAWN_TIMEOUT, || Ok(alive(loader)?.then_some(())))
            .await?
            .is_some()
            && combat::exit(loader).await?
            && wait_for_action(loader, GameAction::Tfer).await?,
    )
}

pub(crate) async fn jump(
    loader: &impl Loader,
    cell: &str,
    pad: Option<&str>,
) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    if !world::has_cell(loader, cell)? {
        return Ok(false);
    }
    let pad = pad.unwrap_or(DEFAULT_PAD);
    bridge::player::jump(loader, cell, Some(pad))?;
    let arrived = arrive(loader, JUMP_TIMEOUT, || {
        Ok(bridge::world::is_cell_ready(loader, cell, Some(pad))?.then_some(true))
    })
    .await?;
    if !arrived && tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!(cell, pad, state = %world::cell_state(loader), "the cell never became ready");
    }
    Ok(arrived)
}

fn split_room(map: &str) -> (&str, Option<&str>) {
    match map.split_once('-') {
        Some((name, room)) if !name.is_empty() && !room.is_empty() => (name, Some(room)),
        _ => (map, None),
    }
}

fn in_room(loader: &impl Loader, room: Option<&str>) -> Result<bool, CallError> {
    let Some(room) = room else {
        return Ok(true);
    };
    let here = world::room_number(loader)?;
    Ok(room.parse().is_ok_and(|room: u32| here == Some(room)))
}

pub(crate) fn walk_to(loader: &impl Loader, x: i32, y: i32) -> Result<bool, CallError> {
    if bridge::player::get_hp(loader)? <= 0 {
        return Ok(false);
    }
    bridge::player::walk_to(loader, x, y, None)
}

pub(crate) async fn walk_and_arrive(
    loader: &impl Loader,
    x: i32,
    y: i32,
    speed: Option<f64>,
) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await?
        || bridge::player::get_hp(loader)? <= 0
        || !bridge::player::walk_to(loader, x, y, speed)?
    {
        return Ok(false);
    }
    let arrived = poll(POLL, WALK_TIMEOUT, || {
        Ok(position(loader)?
            .is_some_and(|(at_x, at_y)| {
                (at_x - f64::from(x)).abs() < 1.0 && (at_y - f64::from(y)).abs() < 1.0
            })
            .then_some(()))
    })
    .await?;
    Ok(arrived.is_some())
}

pub(crate) async fn rest(loader: &impl Loader, until_full: bool) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    let full = || -> Result<bool, CallError> {
        Ok(
            bridge::player::get_hp(loader)? >= bridge::player::get_max_hp(loader)?
                && bridge::player::get_mp(loader)? >= bridge::player::get_max_mp(loader)?,
        )
    };
    if until_full && full()? {
        return Ok(true);
    }
    if !wait_for_action(loader, GameAction::Rest).await? {
        return Ok(false);
    }
    bridge::player::rest(loader)?;
    if !until_full {
        return Ok(true);
    }
    Ok(poll(POLL, REST_TIMEOUT, || Ok(full()?.then_some(())))
        .await?
        .is_some())
}

pub(crate) async fn equip_outfit(
    loader: &impl Loader,
    name: &str,
    keep_colors: Option<bool>,
) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    let Value::Object(outfit) = bridge::outfits::get(loader, name)? else {
        return Ok(false);
    };
    let outfit = Row(outfit);
    let items: Vec<i32> = OUTFIT_SLOTS
        .iter()
        .filter_map(|slot| outfit.positive(slot))
        .collect();
    if !wait_for_action(loader, GameAction::EquipLoadout).await?
        || !bridge::outfits::equip(loader, name, keep_colors)?
    {
        return Ok(false);
    }
    let equipped = poll(POLL, OUTFIT_TIMEOUT, || {
        for id in &items {
            if !equipped(loader, *id)? {
                return Ok(None);
            }
        }
        Ok(Some(()))
    })
    .await?;
    Ok(equipped.is_some())
}

fn equipped(loader: &impl Loader, id: i32) -> Result<bool, CallError> {
    Ok(inventory::item(loader, &ItemQuery::ItemId { item_id: id })?
        .is_some_and(|item| item.equipped))
}

#[derive(Deserialize)]
pub(crate) struct WearLoadoutPacket {
    uid: i64,
    #[serde(default)]
    success: bool,
}

impl Packet for WearLoadoutPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "wearLoadout";
}

pub(crate) async fn wear_outfit(
    loader: &impl Loader,
    replies: Packets<WearLoadoutPacket>,
    name: &str,
    keep_colors: Option<bool>,
) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? || !wait_for_action(loader, GameAction::WearLoadout).await?
    {
        return Ok(false);
    }
    replies.clear();
    if !bridge::outfits::wear(loader, name, keep_colors)? {
        return Ok(false);
    }
    let uid = i64::from(bridge::player::get_user_id(loader)?);
    Ok(replies
        .answer(OUTFIT_TIMEOUT, |packet| {
            (packet.uid == uid).then_some(packet.success)
        })
        .await
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::bridge::{Event, Events, Value};
    use crate::domain::testing::{FakeLoader, debug_logs};

    fn player(hp: i32) -> (FakeLoader, Rc<RefCell<Vec<(f64, f64)>>>) {
        let walks = Rc::new(RefCell::new(Vec::new()));
        let seen = walks.clone();
        let loader = FakeLoader::new(move |callback, args| {
            Ok(match (callback, args) {
                ("player.getHp", _) => Value::Number(f64::from(hp)),
                ("player.walkTo", [Value::Number(x), Value::Number(y), _]) => {
                    seen.borrow_mut().push((*x, *y));
                    Value::Bool(true)
                }
                (other, _) => panic!("unexpected call {other} {args:?}"),
            })
        });
        (loader, walks)
    }

    #[test]
    fn a_dead_player_does_not_walk() {
        let (dead, walks) = player(0);
        assert_eq!(walk_to(&dead, 10, 20), Ok(false));
        assert!(walks.borrow().is_empty());
        let (alive, walks) = player(500);
        assert_eq!(walk_to(&alive, 10, 20), Ok(true));
        assert_eq!(*walks.borrow(), [(10.0, 20.0)]);
    }

    struct Place {
        logged_in: bool,
        map: String,
        room: u32,
        cell: String,
        pad: String,
        loading: u32,
        art: u32,
        transfer_lock: u32,
        joins: Vec<Vec<Value>>,
        jumps: Vec<Vec<Value>>,
        gotos: Vec<Value>,
        friend: Option<(&'static str, &'static str)>,
        state: State,
    }

    fn in_map(map: &str, cell: &str) -> Place {
        Place {
            logged_in: true,
            map: map.into(),
            room: 1,
            cell: cell.into(),
            pad: "Spawn".into(),
            loading: 0,
            art: 0,
            transfer_lock: 0,
            joins: Vec::new(),
            jumps: Vec::new(),
            gotos: Vec::new(),
            friend: None,
            state: State::Idle,
        }
    }

    fn traveller(place: Place) -> (FakeLoader, Rc<RefCell<Place>>) {
        let place = Rc::new(RefCell::new(place));
        let state = place.clone();
        let text = |value: &Value| match value {
            Value::String(text) => text.clone(),
            other => panic!("expected text, got {other:?}"),
        };
        let loader = FakeLoader::new(move |callback, args| {
            let mut place = state.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn" => Value::Bool(place.logged_in),
                "world.isLoaded" => {
                    let loaded = place.loading == 0;
                    place.loading = place.loading.saturating_sub(1);
                    Value::Bool(place.logged_in && loaded)
                }
                "player.getMap" => Value::String(place.map.clone()),
                "flash.getGameObject" if args == ["world.strAreaName".into()] => {
                    Value::String(format!(r#""{}-{}""#, place.map, place.room))
                }
                "world.getCells" => Value::List(
                    ["Enter", "r2", "Room"]
                        .map(|cell| Value::String(cell.into()))
                        .to_vec(),
                ),
                "player.isLoaded" => {
                    let loaded = place.loading == 0 && place.art == 0;
                    place.art = place.art.saturating_sub(1);
                    Value::Bool(loaded)
                }
                "world.isActionAvailable" if args == [Value::String("tfer".into())] => {
                    let available = place.transfer_lock == 0;
                    place.transfer_lock = place.transfer_lock.saturating_sub(1);
                    Value::Bool(available)
                }
                "player.joinMap" => {
                    if place.transfer_lock == 0 && place.state == State::Idle {
                        place.joins.push(args.to_vec());
                    }
                    Value::Undefined
                }
                "player.jump" => {
                    place.jumps.push(args.to_vec());
                    place.cell = text(&args[0]);
                    place.pad = text(&args[1]);
                    if place.state == State::Combat {
                        place.state = State::Idle;
                    }
                    Value::Undefined
                }
                "player.goToPlayer" => {
                    if place.state == State::Idle {
                        place.gotos.push(args[0].clone());
                        if let Some((map, cell)) = place.friend.filter(|(map, _)| place.map != *map)
                        {
                            place.map = map.into();
                            place.cell = cell.into();
                            place.loading = 3;
                        }
                    }
                    Value::Undefined
                }
                "player.getState" => Value::Number(match place.state {
                    State::Dead => 0.0,
                    State::Idle => 1.0,
                    State::Combat => 2.0,
                }),
                "player.getHp" => Value::Number(if place.state == State::Dead {
                    0.0
                } else {
                    100.0
                }),
                "flash.isNull" => Value::Bool(false),
                "world.getMonsters" => Value::List(Vec::new()),
                "combat.cancelAutoAttack" | "combat.cancelTarget" => Value::Null,
                "player.getPad" => Value::String(place.pad.clone()),
                "player.getCell" => Value::String(place.cell.clone()),
                "players.get" => match place.friend {
                    Some((map, cell))
                        if args == [Value::String("player2".into())] && place.map == map =>
                    {
                        Value::Object(
                            [
                                ("username", Value::String("player2".into())),
                                ("userId", Value::Number(2.0)),
                                ("cell", Value::String(cell.into())),
                            ]
                            .into_iter()
                            .map(|(key, value)| (key.to_owned(), value))
                            .collect(),
                        )
                    }
                    _ => Value::Null,
                },
                "world.isCellReady" => Value::Bool(
                    place.loading == 0
                        && place.cell.eq_ignore_ascii_case(&text(&args[0]))
                        && (args[1] == Value::Null || place.pad == text(&args[1])),
                ),
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        (loader, place)
    }

    async fn transfer_later(
        place: &RefCell<Place>,
        events: &Events,
        map: &str,
        cell: &str,
        art: u32,
    ) {
        tokio::time::sleep(Duration::from_secs(1)).await;
        {
            let mut place = place.borrow_mut();
            place.loading = 3;
            place.art = art;
            place.map = map.into();
            place.room = 5;
            place.cell = cell.into();
        }
        events.publish(&Event::ExtensionResponse {
            packet: format!(
                r#"{{"type":"json","dataObj":{{"cmd":"moveToArea","areaName":"{map}-5"}}}}"#
            ),
        });
    }

    #[tokio::test(start_paused = true)]
    async fn joining_finishes_once_the_server_moves_the_player_and_the_map_loads() {
        let (loader, place) = traveller(in_map("battleon", "Enter"));
        let events = Events::default();
        let subscribe = |wants| events.subscribe(wants);
        let (joined, ()) = tokio::join!(
            join(&loader, &subscribe, "yulgar-1234", Some("Room"), None),
            transfer_later(&place, &events, "yulgar", "Room", 0),
        );
        assert_eq!(joined, Ok(true));
        {
            let place = place.borrow();
            assert_eq!(place.loading, 0);
            assert_eq!(
                place.joins,
                [vec!["yulgar-1234".into(), "Room".into(), "Spawn".into()]]
            );
        }

        let (loader, place) = traveller(Place {
            transfer_lock: 5,
            ..in_map("battleon", "Enter")
        });
        let (joined, ()) = tokio::join!(
            join(&loader, &subscribe, "yulgar-1234", None, None),
            transfer_later(&place, &events, "yulgar", "Enter", 0),
        );
        assert_eq!(joined, Ok(true));
        assert_eq!(place.borrow().joins.len(), 1);

        for state in [State::Dead, State::Combat] {
            let (loader, place) = traveller(Place {
                state,
                ..in_map("battleon", "Enter")
            });
            let (joined, (), ()) = tokio::join!(
                join(&loader, &subscribe, "yulgar-1234", None, None),
                async {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    if place.borrow().state == State::Dead {
                        place.borrow_mut().state = State::Idle;
                    }
                },
                async {
                    let sent = poll(POLL, JOIN_TIMEOUT, || {
                        Ok((!place.borrow().joins.is_empty()).then_some(()))
                    })
                    .await
                    .unwrap();
                    if sent.is_some() {
                        transfer_later(&place, &events, "yulgar", "Enter", 0).await;
                    }
                },
            );
            assert_eq!(joined, Ok(true), "{state:?}");
            assert_eq!(place.borrow().joins.len(), 1);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn going_to_a_player_finishes_in_their_room_and_cell() {
        let events = Events::default();
        let subscribe = |wants| events.subscribe(wants);
        let (loader, place) = traveller(Place {
            friend: Some(("yulgar", "Room")),
            transfer_lock: 5,
            ..in_map("battleon", "Enter")
        });
        assert_eq!(
            go_to(&loader, &subscribe, " player2 ").await,
            Ok(Arrival::Arrived)
        );
        assert_eq!(place.borrow().gotos, [Value::String("player2".into())]);

        let (loader, place) = traveller(Place {
            friend: Some(("yulgar", "Room")),
            state: State::Dead,
            ..in_map("battleon", "Enter")
        });
        let started = Instant::now();
        let (arrived, ()) = tokio::join!(go_to(&loader, &subscribe, "player2"), async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            place.borrow_mut().state = State::Idle;
        });
        assert_eq!(arrived, Ok(Arrival::Arrived));
        assert!(started.elapsed() >= Duration::from_secs(10));
        assert_eq!(place.borrow().gotos.len(), 1);

        let (loader, place) = traveller(Place {
            friend: Some(("yulgar", "Room")),
            state: State::Combat,
            ..in_map("battleon", "Enter")
        });
        assert_eq!(
            go_to(&loader, &subscribe, "player2").await,
            Ok(Arrival::Arrived)
        );
        {
            let place = place.borrow();
            assert_eq!(place.jumps, [vec!["Enter".into(), "Spawn".into()]]);
            assert_eq!(place.gotos.len(), 1);
        }

        let (loader, place) = traveller(Place {
            friend: Some(("battleon", "r2")),
            ..in_map("battleon", "Enter")
        });
        assert_eq!(
            go_to(&loader, &subscribe, "player2").await,
            Ok(Arrival::Arrived)
        );
        {
            let place = place.borrow();
            assert_eq!(place.gotos.len(), 1);
            assert_eq!(place.jumps, [vec!["r2".into(), "Spawn".into()]]);
        }

        let (loader, place) = traveller(in_map("battleon", "Enter"));
        assert_eq!(go_to(&loader, &subscribe, " ").await, Ok(Arrival::Missed));
        assert!(place.borrow().gotos.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn going_to_a_player_stops_when_the_server_refuses_or_cannot_find_them() {
        let events = Events::default();
        let subscribe = |wants| events.subscribe(wants);
        let answers = [
            (
                "warning",
                "Cannot goto to player in a Locked zone.",
                Arrival::Refused(Refusal::LockedZone),
            ),
            (
                "warning",
                "Room join failed, destination room is full.",
                Arrival::Refused(Refusal::RoomFull),
            ),
            (
                "server",
                "Player2 is ignoring goto requests.",
                Arrival::Refused(Refusal::IgnoringGoto),
            ),
            (
                "warning",
                "Player 'player2' could not be found.",
                Arrival::Missed,
            ),
        ];
        for (cmd, message, answer) in answers {
            let (loader, _) = traveller(in_map("battleon", "Enter"));
            let started = Instant::now();
            let (arrived, ()) = tokio::join!(go_to(&loader, &subscribe, "player2"), async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let other = serde_json::json!({ "type": "str", "dataObj": ["server", "-1", "You joined \"battleon-1\""] });
                let refused = serde_json::json!({ "type": "str", "dataObj": [cmd, "-1", message] });
                for packet in [other, refused] {
                    events.publish(&Event::ExtensionResponse {
                        packet: packet.to_string(),
                    });
                }
            });
            assert_eq!(arrived, Ok(answer));
            assert_eq!(started.elapsed(), Duration::from_secs(1));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn joining_another_room_of_the_current_map_waits_for_the_transfer() {
        let (loader, place) = traveller(in_map("battleon", "Enter"));
        let events = Events::default();
        let subscribe = |wants| events.subscribe(wants);
        let started = Instant::now();
        let ((joined, took), ()) = tokio::join!(
            async {
                let joined = join(&loader, &subscribe, "battleon-5", None, None).await;
                (joined, started.elapsed())
            },
            transfer_later(&place, &events, "battleon", "Enter", 0),
        );
        assert_eq!(joined, Ok(true));
        assert!(took >= Duration::from_secs(1), "joined after {took:?}");
        assert_eq!(place.borrow().joins.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn joining_waits_until_the_avatar_has_loaded() {
        let (loader, place) = traveller(in_map("battleon", "Enter"));
        let events = Events::default();
        let subscribe = |wants| events.subscribe(wants);
        let (joined, ()) = tokio::join!(
            join(&loader, &subscribe, "yulgar", None, None),
            transfer_later(&place, &events, "yulgar", "Enter", 5),
        );
        assert_eq!(joined, Ok(true));
        assert_eq!(place.borrow().art, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn joining_the_current_map_or_room_jumps() {
        for target in ["Battleon", "battleon-1"] {
            let (loader, place) = traveller(in_map("battleon", "Enter"));
            let events = Events::default();
            let subscribe = |wants| events.subscribe(wants);
            assert_eq!(
                join(&loader, &subscribe, target, Some("r2"), None).await,
                Ok(true),
                "{target}"
            );
            assert!(place.borrow().joins.is_empty(), "{target}");
            assert_eq!(
                place.borrow().jumps,
                [vec!["r2".into(), "Spawn".into()]],
                "{target}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_join_the_server_never_answers_or_refuses_returns_false() {
        let events = Events::default();
        let subscribe = |wants| events.subscribe(wants);
        for target in ["yulgar", "battleon-5"] {
            let (loader, _) = traveller(in_map("battleon", "Enter"));
            events.publish(&Event::ExtensionResponse {
                packet: r#"{"type":"json","dataObj":{"cmd":"moveToArea","areaName":"battleon-1"}}"#
                    .into(),
            });
            assert_eq!(
                join(&loader, &subscribe, target, None, None).await,
                Ok(false),
                "{target}"
            );
        }

        let (loader, _) = traveller(in_map("battleon", "Enter"));
        let started = Instant::now();
        let (joined, ()) = tokio::join!(
            join(&loader, &subscribe, "yulgar-9999", None, None),
            async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                events.publish(&Event::ExtensionResponse {
                packet: serde_json::json!({ "type": "str", "dataObj": ["warning", "-1", "Room join failed, destination room is full."] }).to_string(),
            });
            }
        );
        assert_eq!(joined, Ok(false));
        assert_eq!(started.elapsed(), Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn travel_to_a_cell_the_map_does_not_have_fails_without_waiting() {
        let (loader, place) = traveller(in_map("battleon", "Enter"));
        let started = Instant::now();
        assert_eq!(jump(&loader, "Nowhere", None).await, Ok(false));
        assert_eq!(jump(&loader, "enter", None).await, Ok(false));
        assert!(place.borrow().jumps.is_empty());

        let events = Events::default();
        let subscribe = |wants| events.subscribe(wants);
        let ((joined, took), ()) = tokio::join!(
            async {
                let joined = join(&loader, &subscribe, "yulgar", Some("Nowhere"), None).await;
                (joined, started.elapsed())
            },
            transfer_later(&place, &events, "yulgar", "Enter", 0),
        );
        assert_eq!(joined, Ok(false));
        assert!(took < Duration::from_secs(3), "gave up after {took:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn travel_fails_when_the_player_is_logged_out() {
        let (loader, place) = traveller(Place {
            logged_in: false,
            ..in_map("battleon", "Enter")
        });
        let events = Events::default();
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            join(&loader, &subscribe, "yulgar", None, None).await,
            Ok(false)
        );
        assert_eq!(jump(&loader, "r2", None).await, Ok(false));
        assert!(place.borrow().joins.is_empty() && place.borrow().jumps.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_diagnostic_never_changes_a_failed_jumps_result() {
        let _debug = debug_logs();
        let loader = FakeLoader::new(|callback, _| match callback {
            "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Ok(Value::Bool(true)),
            "world.getCells" => Ok(Value::List(vec![Value::String("r2".into())])),
            "player.jump" => Ok(Value::Undefined),
            "world.isCellReady" => Ok(Value::Bool(false)),
            "flash.getGameObject" | "flash.isNull" => Err(CallError::Threw {
                callback,
                message: "the map is null".into(),
            }),
            other => panic!("unexpected call {other}"),
        });
        assert_eq!(jump(&loader, "r2", None).await, Ok(false));
        assert!(loader.count("flash.getGameObject") > 0);
    }
    #[test]
    fn identity_reads_the_players_own_name_guild_level_and_class() {
        let data = |guild: Value| {
            Value::Object(
                [
                    ("strUsername".to_owned(), Value::String("player1".into())),
                    ("guild".to_owned(), guild),
                    ("intLevel".to_owned(), Value::Number(86.0)),
                    (
                        "strClassName".to_owned(),
                        Value::String("Blaze Binder".into()),
                    ),
                ]
                .into(),
            )
        };
        let guild = Value::Object([("Name".to_owned(), Value::String("Guild One".into()))].into());
        assert_eq!(
            identity_of(&data(guild)),
            Some(Identity {
                name: "player1".into(),
                guild: Some("Guild One".into()),
                level: Some(86),
                class: Some("Blaze Binder".into()),
            })
        );
        assert_eq!(
            identity_of(&data(Value::Null)),
            Some(Identity {
                name: "player1".into(),
                guild: None,
                level: Some(86),
                class: Some("Blaze Binder".into()),
            })
        );
        assert_eq!(identity_of(&Value::Null), None);
    }
}
