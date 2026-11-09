use std::{cell::RefCell, rc::Rc, time::Duration};

use serde::Deserialize;
use serde_json::{Value, json};

use super::calls::{Reply, RunId};
use crate::{
    army::{Armies, ArmyError, Member},
    bridge::Loader,
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
}

#[derive(Default)]
pub(crate) struct Membership(RefCell<Slot>);

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
        let mut slot = self.0.0.borrow_mut();
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
    run: RunId,
    membership: &Membership,
    request: Request,
) -> Reply {
    if let Request::Start { name, timeout_ms } = request {
        return start(armies, loader, run, membership, &name, timeout(timeout_ms)).await;
    }
    let member = match &*membership.0.borrow() {
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
        let mut slot = membership.0.borrow_mut();
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
    *starting.0.0.borrow_mut() = Slot::Joined(Rc::new(member));
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bridge::{CallError, Value as LoaderValue},
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
                RunId(1),
                &membership,
                sync.clone()
            )
            .await,
            refused("Call Army.start before other army calls.")
        );
        assert_eq!(
            serve(&armies, &logged_in(""), RunId(1), &membership, start()).await,
            refused("Log in before starting an army.")
        );
        assert_eq!(
            serve(
                &armies,
                &logged_in("player1"),
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
}
