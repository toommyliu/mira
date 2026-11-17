use std::time::Duration;

use tokio::time::sleep;

use super::{POLL, auth, house, packets, player, poll, ready, rooms, rooms::Rooms};
use crate::bridge::{self, CallError, Event, Loader, Subscription};

const ATTEMPTS: u32 = 3;
const ARRIVAL_TIMEOUT: Duration = Duration::from_secs(5);
const RETURN_INFO: &str = "world.returnInfo";

enum Spot {
    House { username: String },
    Buyhouse,
}

pub(crate) async fn retreat(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    rooms: Rooms,
) -> Result<bool, CallError> {
    if !super::wait_until_ready(loader).await? {
        return Ok(false);
    }
    let spot = if has_house(loader)? {
        if in_own_house(loader)? {
            return Ok(true);
        }
        let Some(username) = auth::username(loader)? else {
            return Ok(false);
        };
        Spot::House { username }
    } else {
        Spot::Buyhouse
    };
    let _cleared = match spot {
        Spot::House { .. } => Some(ReturnInfo::clear(loader)?),
        Spot::Buyhouse => None,
    };
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            sleep(Duration::from_secs(1 << (attempt - 1))).await;
        }
        let moved = match &spot {
            Spot::House { .. } if home(loader)? => true,
            Spot::House { username } => visit_house(loader, username).await?,
            Spot::Buyhouse => {
                let map = rooms::destination(loader, "buyhouse", rooms)?;
                player::join(loader, subscribe, &map, None, None).await?
            }
        };
        if moved {
            return Ok(true);
        }
        if !bridge::auth::is_logged_in(loader)? {
            return Ok(false);
        }
    }
    Ok(false)
}

fn has_house(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(house::items(loader)?
        .iter()
        .any(|item| item.equipped && item.category == "House"))
}

fn in_own_house(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(bridge::flash::call_game_function0(loader, "world.isMyHouse")?.as_deref() == Some("true"))
}

async fn visit_house(loader: &impl Loader, username: &str) -> Result<bool, CallError> {
    if !player::ready_to_transfer(loader).await? {
        return Ok(false);
    }
    if !packets::send_to_server(loader, &format!("%xt%zm%house%1%{username}%"))? {
        return Ok(false);
    }
    Ok(
        poll(POLL, ARRIVAL_TIMEOUT, || Ok(home(loader)?.then_some(())))
            .await?
            .is_some(),
    )
}

fn home(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(in_own_house(loader)? && ready(loader)?)
}

// On entering a house the game moves the avatar to `world.returnInfo.strCell` instead of the
// spawn cell, so a stale value from another map leaves the avatar in a cell the house lacks.
struct ReturnInfo<'a, L: Loader> {
    loader: &'a L,
    saved: Option<bridge::Value>,
}

impl<'a, L: Loader> ReturnInfo<'a, L> {
    fn clear(loader: &'a L) -> Result<Self, CallError> {
        let saved = if bridge::flash::is_null(loader, RETURN_INFO)? {
            None
        } else {
            let saved = bridge::flash::get_game_object(loader, RETURN_INFO)?
                .and_then(|json| serde_json::from_str(&json).ok())
                .map(bridge::from_json);
            bridge::flash::set_game_object(loader, RETURN_INFO, bridge::Value::Null)?;
            saved
        };
        Ok(Self { loader, saved })
    }
}

impl<L: Loader> Drop for ReturnInfo<'_, L> {
    fn drop(&mut self) {
        if let Some(saved) = self.saved.take()
            && let Err(error) = bridge::flash::set_game_object(self.loader, RETURN_INFO, saved)
        {
            tracing::debug!(%error, "couldn't restore the game's return info");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;

    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::{FakeLoader, value},
    };

    struct Client {
        logged_in: bool,
        house: bool,
        in_house: bool,
        return_info: Option<serde_json::Value>,
        return_info_while_moving: Vec<Option<serde_json::Value>>,
        sent: Vec<String>,
        joins: Vec<String>,
    }

    fn client(house: bool, in_house: bool) -> Rc<RefCell<Client>> {
        Rc::new(RefCell::new(Client {
            logged_in: true,
            house,
            in_house,
            return_info: Some(json!({ "strMap": "battleon", "strCell": "Enter" })),
            return_info_while_moving: Vec::new(),
            sent: Vec::new(),
            joins: Vec::new(),
        }))
    }

    fn loader(client: &Rc<RefCell<Client>>) -> FakeLoader {
        let client = client.clone();
        FakeLoader::new(move |callback, args| {
            let mut client = client.borrow_mut();
            let path = args.first().cloned();
            Ok(match callback {
                "auth.isLoggedIn" => Value::Bool(client.logged_in),
                "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "house.getItems" => Value::List(if client.house {
                    vec![value(json!({
                        "itemId": 1, "name": "Cozy Cottage", "category": "House", "equipped": true,
                    }))]
                } else {
                    Vec::new()
                }),
                "flash.callGameFunction0" => Value::String(client.in_house.to_string()),
                "flash.isNull" if path == Some("world.returnInfo".into()) => {
                    Value::Bool(client.return_info.is_none())
                }
                "flash.getGameObject" if path == Some("world.returnInfo".into()) => {
                    Value::String(client.return_info.as_ref().unwrap().to_string())
                }
                "flash.setGameObject" => {
                    client.return_info = match &args[1] {
                        Value::Null => None,
                        Value::Object(fields) => Some(json!({
                            "strMap": text(&fields["strMap"]),
                            "strCell": text(&fields["strCell"]),
                        })),
                        other => panic!("unexpected return info {other:?}"),
                    };
                    Value::Undefined
                }
                "flash.getGameObject" => Value::String(r#""battleon-1""#.into()),
                "flash.getGameObjectS" => Value::String(r#""Alice""#.into()),
                "player.getMap" => Value::String("battleon".into()),
                "player.getHp" => Value::Number(100.0),
                "player.getState" => Value::Number(1.0),
                "world.isActionAvailable" => Value::Bool(true),
                "flash.callGameFunction" => {
                    let snapshot = client.return_info.clone();
                    client.return_info_while_moving.push(snapshot);
                    let Some(Value::String(packet)) = args.get(1) else {
                        panic!("unexpected packet {args:?}");
                    };
                    client.sent.push(packet.clone());
                    client.in_house = true;
                    Value::Undefined
                }
                "player.joinMap" => {
                    let snapshot = client.return_info.clone();
                    client.return_info_while_moving.push(snapshot);
                    let Value::String(map) = &args[0] else {
                        panic!("unexpected map {args:?}");
                    };
                    client.joins.push(map.clone());
                    Value::Undefined
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        })
    }

    fn text(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            other => panic!("expected text, got {other:?}"),
        }
    }

    async fn retreat_with(client: &Rc<RefCell<Client>>, rooms: Rooms) -> Result<bool, CallError> {
        let events = Events::default();
        retreat(&loader(client), &|wants| events.subscribe(wants), rooms).await
    }

    #[tokio::test(start_paused = true)]
    async fn a_player_with_a_house_goes_home_with_the_return_info_cleared_on_the_way() {
        let client = client(true, false);
        let before = client.borrow().return_info.clone();
        assert_eq!(retreat_with(&client, Rooms::default()).await, Ok(true));
        let client = client.borrow();
        assert_eq!(client.sent, ["%xt%zm%house%1%Alice%"]);
        assert!(client.joins.is_empty());
        assert_eq!(client.return_info_while_moving, [None]);
        assert_eq!(client.return_info, before);
    }

    #[tokio::test(start_paused = true)]
    async fn a_player_already_home_stays() {
        let client = client(true, true);
        assert_eq!(retreat_with(&client, Rooms::default()).await, Ok(true));
        assert!(client.borrow().sent.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_player_without_a_house_tries_buyhouse_in_a_private_room_three_times() {
        let client = client(false, false);
        let pinned = Rooms::Private {
            room: Some(4242.try_into().unwrap()),
        };
        assert_eq!(retreat_with(&client, pinned).await, Ok(false));
        let client = client.borrow();
        assert_eq!(client.joins, ["buyhouse-4242"; 3]);
        assert!(client.sent.is_empty());
        assert!(client.return_info_while_moving.iter().all(Option::is_some));
    }

    #[tokio::test(start_paused = true)]
    async fn a_logged_out_player_is_left_alone() {
        let client = client(true, false);
        client.borrow_mut().logged_in = false;
        assert_eq!(retreat_with(&client, Rooms::default()).await, Ok(false));
        assert!(client.borrow().sent.is_empty());
    }
}
