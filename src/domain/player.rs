use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::{Instant, timeout_at};

use super::{auth, poll};
use crate::bridge::{self, CallError, Event, GameAction, Loader, Subscription, Value};

const POLL: Duration = Duration::from_millis(100);
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const ACTION_TIMEOUT: Duration = Duration::from_secs(2);
const JOIN_TIMEOUT: Duration = Duration::from_secs(20);
const JUMP_TIMEOUT: Duration = Duration::from_secs(5);
const WALK_TIMEOUT: Duration = Duration::from_secs(3);
const REST_TIMEOUT: Duration = Duration::from_secs(10);
const OUTFIT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CELL: &str = "Enter";
const DEFAULT_PAD: &str = "Spawn";
const OUTFIT_SLOTS: [&str; 8] = ["Weapon", "ar", "ba", "co", "he", "mi", "pe", "None"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum State {
    Dead,
    Idle,
    Combat,
}

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

pub(crate) fn has_avatar(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(bridge::auth::is_logged_in(loader)?
        && !bridge::flash::is_null(loader, "world.myAvatar.dataLeaf")?
        && !bridge::flash::is_null(loader, "world.myAvatar.objData")?)
}

pub(crate) fn ready(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(auth::in_game(loader)? && bridge::player::is_loaded(loader)?)
}

async fn wait_until_ready(loader: &impl Loader) -> Result<bool, CallError> {
    if !bridge::auth::is_logged_in(loader)? {
        return Ok(false);
    }
    Ok(
        poll(POLL, READY_TIMEOUT, || Ok(ready(loader)?.then_some(())))
            .await?
            .is_some(),
    )
}

async fn wait_for_action(loader: &impl Loader, action: GameAction) -> Result<bool, CallError> {
    Ok(poll(POLL, ACTION_TIMEOUT, || {
        Ok(bridge::world::is_action_available(loader, action)?.then_some(()))
    })
    .await?
    .is_some())
}

pub(crate) fn state(loader: &impl Loader) -> Result<Option<State>, CallError> {
    Ok(match bridge::player::get_state(loader)? {
        0 => Some(State::Dead),
        1 => Some(State::Idle),
        2 => Some(State::Combat),
        _ => None,
    })
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

pub(crate) fn moved(event: &Event) -> bool {
    matches!(event, Event::ExtensionResponse { packet } if packet.contains("\"cmd\":\"moveToArea\""))
}

pub(crate) async fn join(
    loader: &impl Loader,
    moves: Subscription,
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
    let deadline = Instant::now() + JOIN_TIMEOUT;
    moves.clear();
    bridge::player::join_map(loader, map, Some(cell), Some(pad))?;
    if timeout_at(deadline, moves.next()).await.is_err() {
        return Ok(false);
    }
    arrive(loader, deadline - Instant::now(), || {
        Ok(bridge::world::is_loaded(loader)?
            && bridge::player::get_map(loader)?.is_some_and(|here| here.eq_ignore_ascii_case(name))
            && bridge::world::is_cell_ready(loader, cell, None)?
            && bridge::player::is_loaded(loader)?)
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
    let pad = pad.unwrap_or(DEFAULT_PAD);
    bridge::player::jump(loader, cell, Some(pad))?;
    arrive(loader, JUMP_TIMEOUT, || {
        bridge::world::is_cell_ready(loader, cell, Some(pad))
    })
    .await
}

fn map_name(map: &str) -> &str {
    map.find('-')
        .filter(|index| *index > 0 && *index < map.len() - 1)
        .map_or(map, |index| &map[..index])
}

async fn arrive(
    loader: &impl Loader,
    within: Duration,
    mut arrived: impl FnMut() -> Result<bool, CallError>,
) -> Result<bool, CallError> {
    let ended = poll(POLL, within, || {
        if !bridge::auth::is_logged_in(loader)? {
            return Ok(Some(false));
        }
        Ok(arrived()?.then_some(true))
    })
    .await?;
    Ok(ended.unwrap_or(false))
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
    let items: Vec<f64> = OUTFIT_SLOTS
        .iter()
        .filter_map(|slot| match outfit.get(*slot) {
            Some(Value::Number(id)) if *id > 0.0 => Some(*id),
            _ => None,
        })
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

fn equipped(loader: &impl Loader, id: f64) -> Result<bool, CallError> {
    let selector = Value::Object([("itemId".to_owned(), Value::Number(id))].into());
    let Value::Object(item) = bridge::inventory::get_item(loader, selector)? else {
        return Ok(false);
    };
    Ok(match item.get("bEquip") {
        Some(Value::Number(flag)) => *flag == 1.0,
        Some(Value::String(flag)) => flag == "1",
        Some(Value::Bool(flag)) => *flag,
        _ => false,
    })
}

pub(crate) fn worn(event: &Event) -> bool {
    matches!(event, Event::ExtensionResponse { packet } if packet.contains("\"cmd\":\"wearLoadout\""))
}

pub(crate) async fn wear_outfit(
    loader: &impl Loader,
    replies: Subscription,
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
    let uid = bridge::player::get_user_id(loader)?;
    let deadline = Instant::now() + OUTFIT_TIMEOUT;
    while let Ok(reply) = timeout_at(deadline, replies.next()).await {
        if let Ok(Event::ExtensionResponse { packet }) = reply
            && let Some(success) = wear_result(&packet, uid)
        {
            return Ok(success);
        }
    }
    Ok(false)
}

fn wear_result(packet: &str, uid: i32) -> Option<bool> {
    let reply: serde_json::Value = serde_json::from_str(packet).ok()?;
    let data = reply.get("dataObj")?;
    (data.get("cmd")?.as_str()? == "wearLoadout" && data.get("uid")?.as_i64()? == i64::from(uid))
        .then(|| data.get("success").and_then(serde_json::Value::as_bool) == Some(true))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::bridge::{Events, Value};
    use crate::domain::testing::FakeLoader;

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
                "player.isLoaded" => {
                    let loaded = place.loading == 0 && place.art == 0;
                    place.art = place.art.saturating_sub(1);
                    Value::Bool(loaded)
                }
                "player.joinMap" => {
                    place.joins.push(args.to_vec());
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
            packet: format!(r#"{{"dataObj":{{"cmd":"moveToArea","areaName":"{map}-5"}}}}"#),
        });
    }

    #[tokio::test(start_paused = true)]
    async fn joining_finishes_once_the_server_moves_the_player_and_the_map_loads() {
        let (loader, place) = traveller(in_map("battleon", "Enter"));
        let events = Events::default();
        let (joined, ()) = tokio::join!(
            join(
                &loader,
                events.subscribe(moved),
                "yulgar-1234",
                Some("Room"),
                None
            ),
            transfer_later(&place, &events, "yulgar", "Room", 0),
        );
        assert_eq!(joined, Ok(true));
        let place = place.borrow();
        assert_eq!(place.loading, 0);
        assert_eq!(
            place.joins,
            [vec!["yulgar-1234".into(), "Room".into(), "Spawn".into()]]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn joining_another_room_of_the_current_map_waits_for_the_transfer() {
        let (loader, place) = traveller(in_map("battleon", "Enter"));
        let events = Events::default();
        let started = Instant::now();
        let ((joined, took), ()) = tokio::join!(
            async {
                let joined = join(&loader, events.subscribe(moved), "battleon-5", None, None).await;
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
            join(&loader, events.subscribe(moved), "yulgar", None, None),
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
                events.subscribe(moved),
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
            let moves = events.subscribe(moved);
            events.publish(&Event::ExtensionResponse {
                packet: r#"{"dataObj":{"cmd":"moveToArea","areaName":"battleon-1"}}"#.into(),
            });
            assert_eq!(
                join(&loader, moves, target, None, None).await,
                Ok(false),
                "{target}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn travel_fails_when_the_player_is_logged_out() {
        let (loader, place) = traveller(Place {
            logged_in: false,
            ..in_map("battleon", "Enter")
        });
        let events = Events::default();
        assert_eq!(
            join(&loader, events.subscribe(moved), "yulgar", None, None).await,
            Ok(false)
        );
        assert_eq!(jump(&loader, "r2", None).await, Ok(false));
        assert!(place.borrow().joins.is_empty() && place.borrow().jumps.is_empty());
    }
}
