use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::{Instant, timeout_at};

use super::{
    POLL, State, arrive,
    inventory::{self, ItemQuery, Row},
    poll, wait_for_action, wait_until_ready, world,
};
use crate::bridge::{
    self, CallError, Format, GameAction, Interruption, Loader, Packet, Packets, Value,
    packets::wire,
};

const JOIN_TIMEOUT: Duration = Duration::from_secs(20);
const JUMP_TIMEOUT: Duration = Duration::from_secs(5);
const WALK_TIMEOUT: Duration = Duration::from_secs(3);
const REST_TIMEOUT: Duration = Duration::from_secs(10);
const OUTFIT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CELL: &str = "Enter";
const DEFAULT_PAD: &str = "Spawn";
const OUTFIT_SLOTS: [&str; 8] = ["Weapon", "ar", "ba", "co", "he", "mi", "pe", "None"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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
    moves: Packets<MoveToAreaPacket>,
    map: &str,
    cell: Option<&str>,
    pad: Option<&str>,
) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    let map = map.trim();
    let name = map_name(map);
    let cell = cell.unwrap_or(DEFAULT_CELL);
    let pad = pad.unwrap_or(DEFAULT_PAD);
    let here = bridge::player::get_map(loader)?;
    if name == map && here.is_some_and(|here| here.eq_ignore_ascii_case(name)) {
        return jump(loader, cell, Some(pad)).await;
    }
    if !wait_for_action(loader, GameAction::Tfer).await? {
        return Ok(false);
    }
    let deadline = Instant::now() + JOIN_TIMEOUT;
    moves.clear();
    bridge::player::join_map(loader, map, Some(cell), Some(pad))?;
    if !matches!(
        timeout_at(deadline, moves.next()).await,
        Ok(Ok(_) | Err(Interruption::Lagged))
    ) {
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

fn map_name(map: &str) -> &str {
    map.find('-')
        .filter(|index| *index > 0 && *index < map.len() - 1)
        .map_or(map, |index| &map[..index])
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
        cell: String,
        pad: String,
        loading: u32,
        art: u32,
        transfer_lock: u32,
        joins: Vec<Vec<Value>>,
        jumps: Vec<Vec<Value>>,
    }

    fn in_map(map: &str, cell: &str) -> Place {
        Place {
            logged_in: true,
            map: map.into(),
            cell: cell.into(),
            pad: "Spawn".into(),
            loading: 0,
            art: 0,
            transfer_lock: 0,
            joins: Vec::new(),
            jumps: Vec::new(),
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
                    if place.transfer_lock == 0 {
                        place.joins.push(args.to_vec());
                    }
                    Value::Undefined
                }
                "player.jump" => {
                    place.jumps.push(args.to_vec());
                    place.cell = text(&args[0]);
                    place.pad = text(&args[1]);
                    Value::Undefined
                }
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
        let (joined, ()) = tokio::join!(
            join(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                "yulgar-1234",
                Some("Room"),
                None
            ),
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
            join(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                "yulgar-1234",
                None,
                None
            ),
            transfer_later(&place, &events, "yulgar", "Enter", 0),
        );
        assert_eq!(joined, Ok(true));
        assert_eq!(place.borrow().joins.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn joining_another_room_of_the_current_map_waits_for_the_transfer() {
        let (loader, place) = traveller(in_map("battleon", "Enter"));
        let events = Events::default();
        let started = Instant::now();
        let ((joined, took), ()) = tokio::join!(
            async {
                let joined = join(
                    &loader,
                    Packets::subscribe(|wants| events.subscribe(wants)),
                    "battleon-5",
                    None,
                    None,
                )
                .await;
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
        let (joined, ()) = tokio::join!(
            join(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                "yulgar",
                None,
                None
            ),
            transfer_later(&place, &events, "yulgar", "Enter", 5),
        );
        assert_eq!(joined, Ok(true));
        assert_eq!(place.borrow().art, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn joining_the_current_map_without_a_room_jumps() {
        let (loader, place) = traveller(in_map("battleon", "Enter"));
        let events = Events::default();
        assert_eq!(
            join(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                "Battleon",
                Some("r2"),
                None
            )
            .await,
            Ok(true)
        );
        assert!(place.borrow().joins.is_empty());
        assert_eq!(place.borrow().jumps, [vec!["r2".into(), "Spawn".into()]]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_join_the_server_never_answers_returns_false() {
        let events = Events::default();
        for target in ["yulgar", "battleon-5"] {
            let (loader, _) = traveller(in_map("battleon", "Enter"));
            let moves = Packets::subscribe(|wants| events.subscribe(wants));
            events.publish(&Event::ExtensionResponse {
                packet: r#"{"type":"json","dataObj":{"cmd":"moveToArea","areaName":"battleon-1"}}"#
                    .into(),
            });
            assert_eq!(
                join(&loader, moves, target, None, None).await,
                Ok(false),
                "{target}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn travel_to_a_cell_the_map_does_not_have_fails_without_waiting() {
        let (loader, place) = traveller(in_map("battleon", "Enter"));
        let started = Instant::now();
        assert_eq!(jump(&loader, "Nowhere", None).await, Ok(false));
        assert_eq!(jump(&loader, "enter", None).await, Ok(false));
        assert!(place.borrow().jumps.is_empty());

        let events = Events::default();
        let ((joined, took), ()) = tokio::join!(
            async {
                let joined = join(
                    &loader,
                    Packets::subscribe(|wants| events.subscribe(wants)),
                    "yulgar",
                    Some("Nowhere"),
                    None,
                )
                .await;
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
        assert_eq!(
            join(
                &loader,
                Packets::subscribe(|wants| events.subscribe(wants)),
                "yulgar",
                None,
                None
            )
            .await,
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
}
