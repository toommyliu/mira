use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::{self, Event, Loader, Packets, Subscription},
    domain::{self, auras, player::Boost},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Ready,
    Map,
    Cell,
    Pad,
    Hp,
    MaxHp,
    Mp,
    MaxMp,
    Level,
    Gold,
    State,
    Alive,
    Position,
    ClassName,
    UserId,
    CharId,
    Member,
    Afk,
    Factions,
    HasBoost {
        kind: Boost,
    },
    JoinMap {
        map: String,
        cell: Option<String>,
        pad: Option<String>,
    },
    JumpToCell {
        cell: String,
        pad: Option<String>,
    },
    GoToPlayer {
        username: String,
    },
    WalkTo {
        x: f64,
        y: f64,
        speed: Option<f64>,
    },
    Rest {
        until_full: Option<bool>,
    },
    Outfits,
    Auras,
    Aura {
        name: String,
    },
    HasAura {
        name: String,
    },
    EquipOutfit {
        name: String,
        keep_colors: Option<bool>,
    },
    WearOutfit {
        name: String,
        keep_colors: Option<bool>,
    },
}

impl Request {
    fn without_avatar(&self) -> serde_json::Value {
        match self {
            Request::Hp
            | Request::MaxHp
            | Request::Mp
            | Request::MaxMp
            | Request::Level
            | Request::Gold
            | Request::UserId
            | Request::CharId => json!(0),
            Request::Map
            | Request::Cell
            | Request::Pad
            | Request::State
            | Request::Position
            | Request::ClassName
            | Request::Aura { .. } => serde_json::Value::Null,
            Request::Factions | Request::Outfits | Request::Auras => json!([]),
            Request::Ready
            | Request::Alive
            | Request::Member
            | Request::Afk
            | Request::HasBoost { .. }
            | Request::HasAura { .. }
            | Request::JoinMap { .. }
            | Request::JumpToCell { .. }
            | Request::GoToPlayer { .. }
            | Request::WalkTo { .. }
            | Request::Rest { .. }
            | Request::EquipOutfit { .. }
            | Request::WearOutfit { .. } => json!(false),
        }
    }
}

pub(super) async fn serve(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    request: Request,
) -> Reply {
    use bridge::player;
    Ok(match request {
        Request::Ready => json!(domain::ready(loader)?),
        Request::JoinMap { map, cell, pad } => json!(
            domain::player::join(
                loader,
                Packets::subscribe(&subscribe),
                &map,
                cell.as_deref(),
                pad.as_deref()
            )
            .await?
        ),
        Request::JumpToCell { cell, pad } => {
            json!(domain::player::jump(loader, &cell, pad.as_deref()).await?)
        }
        Request::GoToPlayer { username } => {
            json!(domain::player::go_to(loader, &username).await?)
        }
        Request::WalkTo { x, y, speed } => {
            json!(domain::player::walk_and_arrive(loader, x as i32, y as i32, speed).await?)
        }
        Request::Rest { until_full } => {
            json!(domain::player::rest(loader, until_full.unwrap_or(false)).await?)
        }
        Request::EquipOutfit { name, keep_colors } => {
            json!(domain::player::equip_outfit(loader, &name, keep_colors).await?)
        }
        Request::WearOutfit { name, keep_colors } => {
            json!(
                domain::player::wear_outfit(
                    loader,
                    Packets::subscribe(&subscribe),
                    &name,
                    keep_colors
                )
                .await?
            )
        }
        request if !domain::has_avatar(loader)? => request.without_avatar(),
        Request::Map => json!(player::get_map(loader)?),
        Request::Cell => json!(player::get_cell(loader)?),
        Request::Pad => json!(player::get_pad(loader)?),
        Request::Hp => json!(player::get_hp(loader)?),
        Request::MaxHp => json!(player::get_max_hp(loader)?),
        Request::Mp => json!(player::get_mp(loader)?),
        Request::MaxMp => json!(player::get_max_mp(loader)?),
        Request::Level => json!(player::get_level(loader)?),
        Request::Gold => json!(player::get_gold(loader)?),
        Request::State => json!(domain::player::state(loader)?),
        Request::Alive => json!(domain::player::alive(loader)?),
        Request::Position => {
            json!(domain::player::position(loader)?.map(|(x, y)| json!({ "x": x, "y": y })))
        }
        Request::ClassName => json!(player::get_class_name(loader)?),
        Request::UserId => json!(player::get_user_id(loader)?),
        Request::CharId => json!(player::get_char_id(loader)?),
        Request::Member => json!(player::is_member(loader)?),
        Request::Afk => json!(player::is_afk(loader)?),
        Request::Factions => json!(domain::player::factions(loader)?),
        Request::HasBoost { kind } => json!(domain::player::has_boost(loader, kind)?),
        Request::Outfits => json!(domain::player::outfit_names(loader)?),
        Request::Auras => json!(auras::player(loader)?),
        Request::Aura { name } => json!(auras::find(auras::player(loader)?, &name)),
        Request::HasAura { name } => json!(auras::has_any(&auras::player(loader)?, &[&name])),
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use super::*;
    use crate::bridge::{Events, Value};
    use crate::domain::testing::FakeLoader;

    async fn ask(loader: &FakeLoader, request: Request) -> Reply {
        let events = Events::default();
        serve(loader, |wants| events.subscribe(wants), request).await
    }

    fn object(fields: &[(&str, Value)]) -> Value {
        Value::Object(
            fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    #[tokio::test]
    async fn out_of_game_reads_are_empty_and_actions_do_nothing() {
        let loader = FakeLoader::new(|callback, _| match callback {
            "auth.isLoggedIn" => Ok(Value::Bool(false)),
            other => panic!("{other} was called while out of game"),
        });
        for (request, expected) in [
            (Request::Ready, json!(false)),
            (Request::Hp, json!(0)),
            (Request::Map, json!(null)),
            (Request::State, json!(null)),
            (Request::Factions, json!([])),
            (Request::Outfits, json!([])),
            (Request::Auras, json!([])),
            (
                Request::Aura {
                    name: "Spirit Power".into(),
                },
                json!(null),
            ),
            (
                Request::HasAura {
                    name: "Spirit Power".into(),
                },
                json!(false),
            ),
            (
                Request::JoinMap {
                    map: "battleon".into(),
                    cell: None,
                    pad: None,
                },
                json!(false),
            ),
            (
                Request::GoToPlayer {
                    username: "player2".into(),
                },
                json!(false),
            ),
            (Request::Alive, json!(false)),
            (Request::Rest { until_full: None }, json!(false)),
        ] {
            assert_eq!(
                ask(&loader, request.clone()).await,
                Ok(expected),
                "{request:?}"
            );
        }
    }

    #[tokio::test]
    async fn ready_means_the_avatar_has_loaded() {
        for (avatar_loaded, expected) in [(false, false), (true, true)] {
            let loader = FakeLoader::new(move |callback, _| {
                Ok(match callback {
                    "auth.isLoggedIn" | "world.isLoaded" => Value::Bool(true),
                    "player.isLoaded" => Value::Bool(avatar_loaded),
                    other => panic!("unexpected call {other}"),
                })
            });
            assert_eq!(ask(&loader, Request::Ready).await, Ok(json!(expected)));
        }
    }

    #[tokio::test]
    async fn reads_translate_the_loader_values() {
        let boosts = Rc::new(RefCell::new(Vec::new()));
        let asked = boosts.clone();
        let loader = FakeLoader::new(move |callback, args| {
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "flash.isNull" => Value::Bool(false),
                "player.getState" => Value::Number(2.0),
                "player.getPosition" => Value::List(vec![Value::Number(10.5), Value::Number(20.0)]),
                "player.getFactions" => Value::List(vec![
                    object(&[
                        ("FactionID", Value::Number(8.0)),
                        ("sName", Value::String("Good".into())),
                        ("iRank", Value::Number(10.0)),
                        ("iRep", Value::Number(302500.0)),
                    ]),
                    object(&[
                        ("FactionID", Value::Number(9.0)),
                        ("sName", Value::String("Evil".into())),
                    ]),
                ]),
                "player.hasActiveBoost" => {
                    asked.borrow_mut().push(args[0].clone());
                    Value::Bool(true)
                }
                "outfits.getAll" => Value::List(vec![
                    object(&[("name", Value::String("Farming".into()))]),
                    object(&[("name", Value::String("Boss".into()))]),
                ]),
                other => panic!("unexpected call {other}"),
            })
        });
        assert_eq!(ask(&loader, Request::State).await, Ok(json!("combat")));
        assert_eq!(
            ask(&loader, Request::Position).await,
            Ok(json!({ "x": 10.5, "y": 20.0 }))
        );
        assert_eq!(
            ask(&loader, Request::Factions).await,
            Ok(json!([
                { "id": 8.0, "name": "Good", "rank": 10.0, "rep": 302500.0 },
                { "id": 9.0, "name": "Evil", "rank": 0.0, "rep": 0.0 },
            ]))
        );
        assert_eq!(
            ask(
                &loader,
                Request::HasBoost {
                    kind: Boost::ClassPoints
                }
            )
            .await,
            Ok(json!(true))
        );
        assert_eq!(*boosts.borrow(), [Value::String("class".into())]);
        assert_eq!(
            ask(&loader, Request::Outfits).await,
            Ok(json!(["Farming", "Boss"]))
        );
    }

    fn walker(steps_to_arrive: Option<u32>) -> FakeLoader {
        let mut walked = false;
        let mut steps = 0;
        FakeLoader::new(move |callback, args| {
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "flash.isNull" => Value::Bool(false),
                "player.getHp" => Value::Number(500.0),
                "player.walkTo" => {
                    assert_eq!(&args[..2], [Value::Number(300.0), Value::Number(400.0)]);
                    walked = true;
                    Value::Bool(true)
                }
                "player.getPosition" => {
                    steps += 1;
                    let arrived = walked && steps_to_arrive.is_some_and(|needed| steps > needed);
                    let (x, y) = if arrived {
                        (300.0, 400.0)
                    } else {
                        (10.0, 20.0)
                    };
                    Value::List(vec![Value::Number(x), Value::Number(y)])
                }
                other => panic!("unexpected call {other}"),
            })
        })
    }

    #[tokio::test(start_paused = true)]
    async fn walking_waits_until_the_player_arrives() {
        let walk = || Request::WalkTo {
            x: 300.9,
            y: 400.0,
            speed: None,
        };
        let loader = walker(Some(5));
        assert_eq!(ask(&loader, walk()).await, Ok(json!(true)));
        assert_eq!(loader.count("player.getPosition"), 6);
        assert_eq!(ask(&walker(None), walk()).await, Ok(json!(false)));
    }

    struct Resting {
        allowed_after: u32,
        hp: i32,
        rests: u32,
    }

    fn rester(allowed_after: u32, hp: i32) -> (FakeLoader, Rc<RefCell<Resting>>) {
        let state = Rc::new(RefCell::new(Resting {
            allowed_after,
            hp,
            rests: 0,
        }));
        let seen = state.clone();
        let loader = FakeLoader::new(move |callback, _| {
            let mut rest = seen.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "flash.isNull" => Value::Bool(false),
                "world.isActionAvailable" => {
                    rest.allowed_after = rest.allowed_after.saturating_sub(1);
                    Value::Bool(rest.allowed_after == 0)
                }
                "player.rest" => {
                    rest.rests += 1;
                    Value::Undefined
                }
                "player.getHp" => {
                    if rest.rests > 0 {
                        rest.hp = (rest.hp + 100).min(1000);
                    }
                    Value::Number(f64::from(rest.hp))
                }
                "player.getMaxHp" => Value::Number(1000.0),
                "player.getMp" | "player.getMaxMp" => Value::Number(100.0),
                other => panic!("unexpected call {other}"),
            })
        });
        (loader, state)
    }

    #[tokio::test(start_paused = true)]
    async fn resting_waits_for_the_game_to_allow_it() {
        let rest = Request::Rest { until_full: None };
        let (loader, state) = rester(3, 500);
        assert_eq!(ask(&loader, rest.clone()).await, Ok(json!(true)));
        assert_eq!(state.borrow().rests, 1);
        let (loader, state) = rester(u32::MAX, 500);
        assert_eq!(ask(&loader, rest).await, Ok(json!(false)));
        assert_eq!(state.borrow().rests, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn resting_until_full_waits_for_full_health() {
        let rest = Request::Rest {
            until_full: Some(true),
        };
        let (loader, state) = rester(1, 500);
        assert_eq!(ask(&loader, rest.clone()).await, Ok(json!(true)));
        assert_eq!(state.borrow().hp, 1000);
        let (loader, state) = rester(1, 1000);
        assert_eq!(ask(&loader, rest).await, Ok(json!(true)));
        assert_eq!(state.borrow().rests, 0);
    }

    struct Wardrobe {
        equipped: Vec<f64>,
        sent: Vec<Vec<Value>>,
        lands_after: u32,
        consumable_in: Option<u32>,
    }

    fn dresser(outfit: Value, lands_after: u32) -> (FakeLoader, Rc<RefCell<Wardrobe>>) {
        let state = Rc::new(RefCell::new(Wardrobe {
            equipped: vec![50741.0],
            sent: Vec::new(),
            lands_after,
            consumable_in: None,
        }));
        let seen = state.clone();
        let loader = FakeLoader::new(move |callback, args| {
            let mut wardrobe = seen.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn"
                | "world.isLoaded"
                | "world.isActionAvailable"
                | "player.isLoaded" => Value::Bool(true),
                "outfits.get" => outfit.clone(),
                "outfits.equip" => {
                    wardrobe.sent.push(args.to_vec());
                    Value::Bool(true)
                }
                "inventory.getItem" => {
                    let Value::Object(selector) = &args[0] else {
                        panic!("getItem needs a selector: {args:?}");
                    };
                    let Some(Value::Number(id)) = selector.get("itemId") else {
                        panic!("the selector names an item: {selector:?}");
                    };
                    if !wardrobe.sent.is_empty() {
                        if wardrobe.lands_after == 0 && wardrobe.consumable_in.is_none() {
                            wardrobe.equipped.extend([7.0, 12.0]);
                            wardrobe.consumable_in = Some(3);
                        }
                        wardrobe.lands_after = wardrobe.lands_after.saturating_sub(1);
                        match wardrobe.consumable_in {
                            Some(0) => {
                                wardrobe.equipped.push(20.0);
                                wardrobe.consumable_in = Some(u32::MAX);
                            }
                            Some(left) if left != u32::MAX => {
                                wardrobe.consumable_in = Some(left - 1)
                            }
                            _ => {}
                        }
                    }
                    let flag = wardrobe.equipped.contains(id);
                    object(&[
                        ("itemId", Value::Number(*id)),
                        ("equipped", Value::Bool(flag)),
                    ])
                }
                other => panic!("unexpected call {other}"),
            })
        });
        (loader, state)
    }

    fn farming_outfit() -> Value {
        object(&[
            ("name", Value::String("Farming".into())),
            ("ar", Value::Number(50741.0)),
            ("co", Value::Number(7.0)),
            ("Weapon", Value::Number(12.0)),
            ("he", Value::Number(0.0)),
            ("None", Value::Number(20.0)),
        ])
    }

    #[tokio::test(start_paused = true)]
    async fn equipping_an_outfit_waits_until_its_items_are_equipped() {
        let equip = || Request::EquipOutfit {
            name: "Farming".into(),
            keep_colors: Some(true),
        };
        let (loader, wardrobe) = dresser(farming_outfit(), 4);
        assert_eq!(ask(&loader, equip()).await, Ok(json!(true)));
        assert_eq!(
            wardrobe.borrow().sent,
            [vec![Value::String("Farming".into()), Value::Bool(true)]]
        );
        assert!(wardrobe.borrow().equipped.contains(&20.0));

        let (loader, _) = dresser(farming_outfit(), u32::MAX);
        assert_eq!(ask(&loader, equip()).await, Ok(json!(false)));

        let (loader, wardrobe) = dresser(Value::Null, 0);
        assert_eq!(ask(&loader, equip()).await, Ok(json!(false)));
        assert!(wardrobe.borrow().sent.is_empty());
    }

    fn tailor(events: Rc<Events>, replies: Vec<(i32, bool)>) -> FakeLoader {
        let mut sent = false;
        FakeLoader::new(move |callback, args| {
            Ok(match callback {
                "auth.isLoggedIn"
                | "world.isLoaded"
                | "world.isActionAvailable"
                | "player.isLoaded" => Value::Bool(true),
                "outfits.wear" => {
                    assert_eq!(args[0], Value::String("Test".into()));
                    sent = true;
                    Value::Bool(true)
                }
                "player.getUserId" => {
                    assert!(sent, "the reply is matched after wearing");
                    let events = events.clone();
                    let replies = replies.clone();
                    tokio::task::spawn_local(async move {
                        for (uid, success) in replies {
                            tokio::time::sleep(Duration::from_millis(300)).await;
                            events.publish(&Event::ExtensionResponse {
                                packet: json!({ "type": "json", "dataObj": {
                                    "cmd": "wearLoadout", "uid": uid, "success": success, "msg": ""
                                }})
                                .to_string(),
                            });
                        }
                    });
                    Value::Number(1.0)
                }
                other => panic!("unexpected call {other}"),
            })
        })
    }

    #[tokio::test(start_paused = true)]
    async fn wearing_an_outfit_returns_the_servers_answer() {
        let wear = || Request::WearOutfit {
            name: "Test".into(),
            keep_colors: None,
        };
        tokio::task::LocalSet::new()
            .run_until(async {
                for (stale, replies, expected) in [
                    (false, vec![(2, false), (1, true)], true),
                    (false, vec![(1, false)], false),
                    (false, vec![(2, true)], false),
                    (true, vec![(1, false)], false),
                ] {
                    let events = Rc::new(Events::default());
                    let loader = tailor(events.clone(), replies.clone());
                    let subscribe = |wants| {
                        let replies = events.subscribe(wants);
                        if stale {
                            events.publish(&Event::ExtensionResponse {
                                packet: json!({ "type": "json", "dataObj": {
                                    "cmd": "wearLoadout", "uid": 1, "success": true
                                }})
                                .to_string(),
                            });
                        }
                        replies
                    };
                    let answer = serve(&loader, subscribe, wear()).await;
                    assert_eq!(answer, Ok(json!(expected)), "{stale} {replies:?}");
                }
            })
            .await;
    }
}
