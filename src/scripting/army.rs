use std::{cell::RefCell, rc::Rc, time::Duration};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    calls::{Reply, RunId},
    loop_taunt::{self, Answered, Taunting},
};
use crate::{
    army::{Armies, ArmyError, Member},
    bridge::{Event, Loader, Subscription},
    domain::auth,
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Start {
        name: String,
        timeout_ms: Option<u64>,
    },
    Sync {
        label: String,
        timeout_ms: Option<u64>,
    },
    Gather {
        label: String,
        value: Value,
        timeout_ms: Option<u64>,
    },
    Until {
        label: String,
        done: bool,
    },
    Report {
        done: bool,
    },
    Settle {
        timeout_ms: Option<u64>,
    },
    Gear {
        name: String,
    },
    Rotate {
        label: String,
        players: Vec<usize>,
    },
    Turn {
        label: String,
        acted: Option<bool>,
    },
    Leave {
        label: String,
    },
    LoopTaunt {
        plan: Vec<Vec<loop_taunt::Target>>,
        timeout_ms: Option<u64>,
    },
    TauntTurn {
        answered: Option<Answered>,
    },
    TauntLeave,
}

#[derive(Default)]
pub(crate) struct Membership {
    slot: RefCell<Slot>,
    taunting: RefCell<Option<Rc<Taunting>>>,
}

#[derive(Default)]
enum Slot {
    #[default]
    Idle,
    Starting,
    Joined(Rc<Member>),
}

struct Starting<'a>(&'a Membership);

impl Drop for Starting<'_> {
    fn drop(&mut self) {
        let mut slot = self.0.slot.borrow_mut();
        if matches!(*slot, Slot::Starting) {
            *slot = Slot::Idle;
        }
    }
}

fn refuse(message: &str) -> ArmyError {
    ArmyError::from(message.to_owned())
}

fn timeout(timeout_ms: Option<u64>) -> Option<Duration> {
    timeout_ms.map(Duration::from_millis)
}

pub(super) async fn serve(
    armies: &Armies,
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    run: RunId,
    membership: &Membership,
    request: Request,
) -> Reply {
    if let Request::Start { name, timeout_ms } = request {
        return start(armies, loader, run, membership, &name, timeout(timeout_ms)).await;
    }
    let member = match &*membership.slot.borrow() {
        Slot::Joined(member) => member.clone(),
        Slot::Idle | Slot::Starting => {
            return Err(refuse("Call Army.start before other army calls.").into());
        }
    };
    Ok(match request {
        Request::Start { .. } => unreachable!("handled above"),
        Request::Sync { label, timeout_ms } => {
            member.sync(&label, timeout(timeout_ms)).await?;
            Value::Null
        }
        Request::Gather {
            label,
            value,
            timeout_ms,
        } => json!(member.gather(&label, value, timeout(timeout_ms)).await?),
        Request::Until { label, done } => json!(member.until(&label, done)?),
        Request::Report { done } => json!(member.report(done)?),
        Request::Settle { timeout_ms } => {
            member.settle(timeout(timeout_ms)).await?;
            Value::Null
        }
        Request::Gear { name } => json!(member.gear(&name)?),
        Request::Rotate { label, players } => json!(member.rotate(&label, &players)?),
        Request::Turn { label, acted } => {
            member.turn(&label, acted).await?;
            Value::Null
        }
        Request::Leave { label } => {
            member.leave_rotation(&label);
            Value::Null
        }
        Request::LoopTaunt { plan, timeout_ms } => {
            if membership.taunting.borrow().is_some() {
                return Err(refuse("This player is already loop taunting.").into());
            }
            let taunting =
                loop_taunt::join(&member, loader, &subscribe, plan, timeout(timeout_ms)).await?;
            *membership.taunting.borrow_mut() = Some(Rc::new(taunting));
            Value::Null
        }
        Request::TauntTurn { answered } => {
            let Some(taunting) = membership.taunting.borrow().clone() else {
                return Ok(Value::Null);
            };
            match taunting.turn(loader, &subscribe, answered).await? {
                Some(offer) => json!({ "lease": offer.lease, "assignment": offer.assignment }),
                None => {
                    membership.taunting.borrow_mut().take();
                    Value::Null
                }
            }
        }
        Request::TauntLeave => {
            membership.taunting.borrow_mut().take();
            Value::Null
        }
    })
}

async fn start(
    armies: &Armies,
    loader: &impl Loader,
    run: RunId,
    membership: &Membership,
    name: &str,
    timeout: Option<Duration>,
) -> Reply {
    {
        let mut slot = membership.slot.borrow_mut();
        if !matches!(*slot, Slot::Idle) {
            return Err(refuse("This script already started an army.").into());
        }
        *slot = Slot::Starting;
    }
    let starting = Starting(membership);
    let username =
        auth::username(loader)?.ok_or_else(|| refuse("Log in before starting an army."))?;
    let Some(member) = armies.start(name, run, &username, timeout).await? else {
        return Ok(Value::Null);
    };
    let info = json!(member.info());
    *starting.0.slot.borrow_mut() = Slot::Joined(Rc::new(member));
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bridge::{CallError, Events, Value as LoaderValue},
        domain::testing::FakeLoader,
    };

    fn logged_in(username: &'static str) -> FakeLoader {
        FakeLoader::new(move |callback, _| {
            assert_eq!(callback, "flash.getGameObjectS");
            Ok(LoaderValue::from(format!("\"{username}\"")))
        })
    }

    fn refused(message: &str) -> Reply {
        Err(CallError::Army(refuse(message)))
    }

    #[tokio::test]
    async fn army_calls_need_a_logged_in_player_and_one_started_army() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let folder =
            std::env::temp_dir().join(format!("mira-army-calls-{:x}", u64::from_ne_bytes(random)));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("solo.yaml"), "players: [player1]\nroom: 7\n").unwrap();
        let armies = Armies::new(folder.clone());
        let membership = Membership::default();
        let events = Events::default();
        let start = || Request::Start {
            name: "solo".into(),
            timeout_ms: None,
        };
        let sync = Request::Sync {
            label: "ready".into(),
            timeout_ms: None,
        };

        assert_eq!(
            serve(
                &armies,
                &logged_in("player1"),
                |wants| events.subscribe(wants),
                RunId(1),
                &membership,
                sync.clone()
            )
            .await,
            refused("Call Army.start before other army calls.")
        );
        assert_eq!(
            serve(
                &armies,
                &logged_in(""),
                |wants| events.subscribe(wants),
                RunId(1),
                &membership,
                start()
            )
            .await,
            refused("Log in before starting an army.")
        );
        assert_eq!(
            serve(
                &armies,
                &logged_in("player1"),
                |wants| events.subscribe(wants),
                RunId(1),
                &membership,
                start()
            )
            .await,
            Ok(json!({
                "name": "solo",
                "number": 1,
                "size": 1,
                "room": 7,
                "players": ["player1"],
            }))
        );
        assert_eq!(
            serve(
                &armies,
                &logged_in("player1"),
                |wants| events.subscribe(wants),
                RunId(1),
                &membership,
                start()
            )
            .await,
            refused("This script already started an army.")
        );
        assert_eq!(
            serve(
                &armies,
                &logged_in("player1"),
                |wants| events.subscribe(wants),
                RunId(1),
                &membership,
                Request::Gather {
                    label: "count".into(),
                    value: json!(5),
                    timeout_ms: None,
                },
            )
            .await,
            Ok(json!([5]))
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    fn armies(players: &str) -> (Armies, std::path::PathBuf) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let folder =
            std::env::temp_dir().join(format!("mira-army-taunt-{:x}", u64::from_ne_bytes(random)));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("army.yaml"),
            format!("players: [{players}]\nroom: 7\n"),
        )
        .unwrap();
        (Armies::new(folder.clone()), folder)
    }

    fn facing_the_boss(username: &'static str) -> FakeLoader {
        let row = |fields: serde_json::Value| crate::domain::testing::value(fields);
        FakeLoader::new(move |callback, _| {
            Ok(match callback {
                "flash.getGameObjectS" => LoaderValue::from(format!("\"{username}\"")),
                "auth.isLoggedIn" => LoaderValue::Bool(true),
                "flash.isNull" => LoaderValue::Bool(false),
                "player.getCell" => LoaderValue::String("r1".into()),
                "player.getHp" => LoaderValue::Number(100.0),
                "player.getState" => LoaderValue::Number(1.0),
                "world.getMonsters" => row(json!([
                    { "monMapId": 7, "name": "Boss", "cell": "r1", "hp": 100, "state": 1 },
                ])),
                "combat.getConsumableSkillItem" => row(json!({ "itemId": 12917, "ready": true })),
                "combat.getSkillCooldownRemaining" => LoaderValue::Number(0.0),
                _ => LoaderValue::Null,
            })
        })
    }

    fn server(packet: serde_json::Value) -> Event {
        Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": packet }).to_string(),
        }
    }

    fn taunt_the_boss(on: serde_json::Value) -> Request {
        let plan = json!([[{ "target": "Boss", "players": [1, 2], "on": on, "skips": true }]]);
        Request::LoopTaunt {
            plan: serde_json::from_value(plan).unwrap(),
            timeout_ms: None,
        }
    }

    #[tokio::test]
    async fn a_player_that_changes_maps_while_deciding_to_skip_does_not_cast() {
        let (armies, folder) = armies("player1");
        let membership = Membership::default();
        let events = Events::default();
        let loader = facing_the_boss("player1");
        let serve = |request| {
            serve(
                &armies,
                &loader,
                |wants| events.subscribe(wants),
                RunId(1),
                &membership,
                request,
            )
        };
        serve(Request::Start {
            name: "army".into(),
            timeout_ms: None,
        })
        .await
        .unwrap();
        let plan = json!([[{ "target": "Boss", "players": [1], "on": { "type": "focus" }, "skips": true }]]);
        serve(Request::LoopTaunt {
            plan: serde_json::from_value(plan).unwrap(),
            timeout_ms: None,
        })
        .await
        .unwrap();
        let offer = serve(Request::TauntTurn { answered: None }).await.unwrap();
        events.publish(&server(
            json!({ "cmd": "moveToArea", "strMapName": "battleon" }),
        ));
        let answered = json!({ "lease": offer["lease"], "cast": true });
        assert_eq!(
            serve(Request::TauntTurn {
                answered: Some(serde_json::from_value(answered).unwrap()),
            })
            .await,
            Ok(Value::Null)
        );
        assert_eq!(loader.count("combat.castConsumableOnMonster"), 0);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[tokio::test(start_paused = true)]
    async fn a_boss_that_respawns_before_everyone_starts_is_on_the_same_life_for_all() {
        let (armies, folder) = armies("player1, player2");
        let players = [
            (
                Membership::default(),
                Events::default(),
                facing_the_boss("player1"),
            ),
            (
                Membership::default(),
                Events::default(),
                facing_the_boss("player2"),
            ),
        ];
        let serve = |player: usize, request| {
            let (membership, events, loader) = &players[player];
            serve(
                &armies,
                loader,
                |wants| events.subscribe(wants),
                RunId(1),
                membership,
                request,
            )
        };
        let start = |player| {
            serve(
                player,
                Request::Start {
                    name: "army".into(),
                    timeout_ms: None,
                },
            )
        };
        let (first, second) = tokio::join!(start(0), start(1));
        first.unwrap();
        second.unwrap();
        let on = json!({ "type": "message", "includes": "prepares" });
        let mut early = std::pin::pin!(serve(0, taunt_the_boss(on.clone())));
        assert!(
            tokio::time::timeout(Duration::ZERO, &mut early)
                .await
                .is_err()
        );
        players[0].1.publish(&server(
            json!({ "cmd": "ct", "m": { "7": { "intState": 0 } } }),
        ));
        players[0].1.publish(&Event::ExtensionResponse {
            packet: json!({ "type": "str", "dataObj": ["respawnMon", "-1", "7"] }).to_string(),
        });
        let (early, late) = tokio::join!(early, serve(1, taunt_the_boss(on)));
        early.unwrap();
        late.unwrap();
        let waiting = Duration::from_secs(10);
        let turn = || Request::TauntTurn { answered: None };
        assert!(
            tokio::time::timeout(waiting, serve(0, turn()))
                .await
                .is_err()
        );
        serve(0, Request::TauntLeave).await.unwrap();
        let message = json!({ "cmd": "umsg", "s": "The boss prepares a heavy attack!" });
        players[1].1.publish(&server(message));
        let offer = tokio::time::timeout(waiting, serve(1, turn())).await;
        assert!(
            matches!(offer, Ok(Ok(Value::Object(_)))),
            "player 2 heard the message, so it is their turn: {offer:?}"
        );
        let _ = std::fs::remove_dir_all(folder);
    }
}
