use std::{collections::HashMap, sync::mpsc};

use serde::Deserialize;
use serde_json::json;

use super::{player, runtime::Command};
use crate::bridge::{self, Bridge, CallError, Loader, Value};
use crate::domain::{auth, combat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RunId(pub(super) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CallId(pub(super) u64);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "domain", rename_all = "camelCase")]
pub(crate) enum Request {
    Player(player::Request),
    Combat(CombatRequest),
    Inventory(InventoryRequest),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum CombatRequest {
    Kill { monster: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum InventoryRequest {
    Count { item: String },
}

pub(crate) type Reply = Result<serde_json::Value, CallError>;

pub(crate) enum ToPlayer {
    Call(Call),
    Cancel { run: RunId, id: CallId },
    Ended(RunId),
}

pub(crate) struct Call {
    pub(crate) run: RunId,
    pub(crate) id: CallId,
    pub(crate) request: Request,
    pub(crate) reply: Replier,
}

pub(crate) struct Replier {
    id: CallId,
    commands: Option<mpsc::Sender<Command>>,
}

impl Replier {
    pub(super) fn new(id: CallId, commands: mpsc::Sender<Command>) -> Self {
        Self {
            id,
            commands: Some(commands),
        }
    }

    pub(crate) fn send(mut self, reply: Reply) {
        self.deliver(reply);
    }

    fn deliver(&mut self, reply: Reply) {
        if let Some(commands) = self.commands.take() {
            let _ = commands.send(Command::Reply { id: self.id, reply });
        }
    }
}

impl Drop for Replier {
    fn drop(&mut self) {
        self.deliver(Err(CallError::Stopped));
    }
}

#[derive(Default)]
pub(crate) struct Calls(HashMap<RunId, HashMap<CallId, async_task::Task<()>>>);

impl Calls {
    pub(crate) fn handle(
        &mut self,
        message: ToPlayer,
        start: impl FnOnce(Call) -> async_task::Task<()>,
    ) {
        match message {
            ToPlayer::Call(call) => {
                let (run, id) = (call.run, call.id);
                let task = start(call);
                let calls = self.0.entry(run).or_default();
                calls.retain(|_, task| !task.is_finished());
                calls.insert(id, task);
            }
            ToPlayer::Cancel { run, id } => {
                if let Some(calls) = self.0.get_mut(&run) {
                    calls.remove(&id);
                }
            }
            ToPlayer::Ended(run) => {
                self.0.remove(&run);
            }
        }
    }
}

pub(crate) async fn answer(bridge: Bridge, call: Call) {
    let Call { request, reply, .. } = call;
    reply.send(serve(&bridge, request).await);
}

async fn serve(bridge: &Bridge, request: Request) -> Reply {
    match request {
        Request::Player(request) => {
            player::serve(bridge, |wants| bridge.subscribe(wants), request).await
        }
        Request::Combat(CombatRequest::Kill { monster }) => {
            Ok(json!(combat::kill(bridge, &monster).await?))
        }
        Request::Inventory(InventoryRequest::Count { item }) => {
            Ok(json!(if auth::in_game(bridge)? {
                count(bridge, &item)?
            } else {
                0
            }))
        }
    }
}

fn count(loader: &impl Loader, item: &str) -> Result<u32, CallError> {
    let name = Value::Object([("name".to_owned(), Value::String(item.to_owned()))].into());
    let Value::Object(item) = bridge::inventory::get_item(loader, name)? else {
        return Ok(0);
    };
    Ok(match item.get("iQty") {
        Some(Value::Number(quantity)) if *quantity > 0.0 => *quantity as u32,
        _ => 0,
    })
}

pub(super) fn encode(reply: Reply) -> String {
    match reply {
        Ok(value) => json!({ "ok": value }),
        Err(error) => json!({ "error": error.to_string() }),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;

    struct Dropped(Rc<Cell<bool>>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    fn call(run: u64, id: u64) -> (ToPlayer, mpsc::Receiver<Command>) {
        let (commands, replies) = mpsc::channel();
        let message = ToPlayer::Call(Call {
            run: RunId(run),
            id: CallId(id),
            request: Request::Player(player::Request::Hp),
            reply: Replier::new(CallId(id), commands),
        });
        (message, replies)
    }

    fn start(calls: &mut Calls, message: ToPlayer) -> Rc<Cell<bool>> {
        let dropped = Rc::new(Cell::new(false));
        let guard = Dropped(dropped.clone());
        calls.handle(message, |call| {
            let (runnable, task) = async_task::spawn_local(
                async move {
                    let _guard = guard;
                    let _call = call;
                    std::future::pending::<()>().await;
                },
                drop,
            );
            runnable.run();
            task
        });
        dropped
    }

    #[test]
    fn ending_a_run_cancels_only_that_runs_calls() {
        let mut calls = Calls::default();
        let first = start(&mut calls, call(1, 1).0);
        let second = start(&mut calls, call(1, 2).0);
        let other = start(&mut calls, call(2, 1).0);
        calls.handle(
            ToPlayer::Cancel {
                run: RunId(1),
                id: CallId(2),
            },
            |_| unreachable!(),
        );
        assert!(!first.get() && second.get() && !other.get());
        calls.handle(ToPlayer::Ended(RunId(1)), |_| unreachable!());
        assert!(first.get() && !other.get());
    }

    #[test]
    fn a_call_dropped_unanswered_replies_that_the_player_stopped() {
        let (message, replies) = call(1, 7);
        drop(message);
        let Ok(Command::Reply { id, reply }) = replies.try_recv() else {
            panic!("the dropped call replied");
        };
        assert_eq!(id, CallId(7));
        assert_eq!(reply, Err(CallError::Stopped));
    }
}
