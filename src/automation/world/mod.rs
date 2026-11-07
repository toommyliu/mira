pub(crate) mod auto_zone;

use std::time::Duration;

use tokio::time::{Instant, timeout_at};

use super::{
    auth,
    bridge::{self, CallError, Event, Loader, Subscription},
    poll,
};

const TRAVEL_POLL: Duration = Duration::from_millis(100);
const JOIN_TIMEOUT: Duration = Duration::from_secs(20);
const JUMP_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CELL: &str = "Enter";
const DEFAULT_PAD: &str = "Spawn";

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
    if !auth::in_game(loader)? {
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
    bridge::player::join_map(loader, map, Some(cell), Some(pad))?;
    if timeout_at(deadline, moves.next()).await.is_err() {
        return Ok(false);
    }
    arrive(loader, deadline - Instant::now(), || {
        Ok(bridge::world::is_loaded(loader)?
            && bridge::player::get_map(loader)?.is_some_and(|here| here.eq_ignore_ascii_case(name))
            && bridge::world::is_cell_ready(loader, cell, None)?)
    })
    .await
}

pub(crate) async fn jump(
    loader: &impl Loader,
    cell: &str,
    pad: Option<&str>,
) -> Result<bool, CallError> {
    if !auth::in_game(loader)? {
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
    let ended = poll(TRAVEL_POLL, within, || {
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

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::automation::{
        bridge::{Events, Value},
        testing::FakeLoader,
    };

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

    async fn transfer_later(place: &RefCell<Place>, events: &Events, map: &str, cell: &str) {
        tokio::time::sleep(Duration::from_secs(1)).await;
        {
            let mut place = place.borrow_mut();
            place.loading = 3;
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
            transfer_later(&place, &events, "yulgar", "Room"),
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
            transfer_later(&place, &events, "battleon", "Enter"),
        );
        assert_eq!(joined, Ok(true));
        assert!(took >= Duration::from_secs(1), "joined after {took:?}");
        assert_eq!(place.borrow().joins.len(), 1);
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
            assert_eq!(
                join(&loader, events.subscribe(moved), target, None, None).await,
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
