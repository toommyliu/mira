use std::{collections::HashMap, rc::Rc, sync::mpsc};

use serde::Deserialize;
use serde_json::json;

use super::{
    army, auth, automation, bank, combat, drops, environment, events,
    feeds::{self, Feeds},
    house, inventory, packets, player, players, quests,
    runtime::Command,
    shops, world,
};
use crate::{
    army::Armies,
    automation::settings::Automation,
    bridge::{Bridge, CallError},
    domain::{auth::Logins, counter_attack::CounterAttacks},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RunId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CallId(pub(super) u64);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "domain", rename_all = "camelCase")]
pub(crate) enum Request {
    Army(army::Request),
    Auth(auth::Request),
    AutoRelogin(automation::ReloginRequest),
    AutoZone(automation::ZoneRequest),
    Settings(automation::SettingsRequest),
    QuestRegistry(environment::QuestRequest),
    DropRegistry(environment::DropRequest),
    BoostRegistry(environment::BoostRequest),
    Player(player::Request),
    Players(players::Request),
    World(world::Request),
    Combat(combat::Request),
    Inventory(inventory::Request),
    TempInventory(inventory::TempRequest),
    Bank(bank::Request),
    House(house::Request),
    Quests(quests::Request),
    Drops(drops::Request),
    Shops(shops::Request),
    Packets(packets::Request),
    Events(events::Request),
    Feeds(feeds::Request),
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
pub(crate) struct Calls(HashMap<RunId, RunCalls>);

#[derive(Default)]
struct RunCalls {
    tasks: HashMap<CallId, async_task::Task<()>>,
    state: Rc<RunState>,
}

#[derive(Default)]
pub(crate) struct RunState {
    feeds: Feeds,
    army: army::Membership,
}

impl Calls {
    pub(crate) fn handle(
        &mut self,
        message: ToPlayer,
        start: impl FnOnce(Call, Rc<RunState>) -> async_task::Task<()>,
    ) {
        match message {
            ToPlayer::Call(call) => {
                let run = self.0.entry(call.run).or_default();
                run.tasks.retain(|_, task| !task.is_finished());
                let id = call.id;
                let task = start(call, run.state.clone());
                run.tasks.insert(id, task);
            }
            ToPlayer::Cancel { run, id } => {
                if let Some(run) = self.0.get_mut(&run) {
                    run.tasks.remove(&id);
                }
            }
            ToPlayer::Ended(run) => {
                self.0.remove(&run);
            }
        }
    }
}

pub(crate) async fn answer(
    bridge: Bridge,
    logins: Rc<Logins>,
    automation: Automation,
    armies: Armies,
    counter_attacks: CounterAttacks,
    state: Rc<RunState>,
    call: Call,
) {
    let Call {
        run,
        request,
        reply,
        ..
    } = call;
    reply.send(
        serve(
            &bridge,
            &logins,
            &automation,
            &armies,
            &counter_attacks,
            run,
            &state,
            request,
        )
        .await,
    );
}

async fn serve(
    bridge: &Bridge,
    logins: &Logins,
    automation: &Automation,
    armies: &Armies,
    counter_attacks: &CounterAttacks,
    run: RunId,
    state: &RunState,
    request: Request,
) -> Reply {
    let feeds = &state.feeds;
    match request {
        Request::Army(request) => army::serve(armies, bridge, run, &state.army, request).await,
        Request::Auth(request) => auth::serve(bridge, logins, request).await,
        Request::AutoRelogin(request) => automation::serve_relogin(automation, request),
        Request::AutoZone(request) => automation::serve_zone(automation, request),
        Request::Settings(request) => automation::serve_settings(automation, request),
        Request::QuestRegistry(request) => environment::serve_quests(automation, request),
        Request::DropRegistry(request) => environment::serve_drops(automation, request),
        Request::BoostRegistry(request) => environment::serve_boosts(automation, request),
        Request::Packets(request) => {
            packets::serve(bridge, |wants| bridge.subscribe(wants), feeds, request)
        }
        Request::Events(request) => {
            events::serve(bridge, |wants| bridge.subscribe(wants), feeds, request)
        }
        Request::Feeds(request) => feeds::serve(feeds, request).await,
        Request::Player(request) => {
            player::serve(bridge, |wants| bridge.subscribe(wants), request).await
        }
        Request::Players(request) => players::serve(bridge, request),
        Request::World(request) => {
            world::serve(bridge, |wants| bridge.subscribe(wants), request).await
        }
        Request::Combat(request) => {
            combat::serve(
                bridge,
                |wants| bridge.subscribe(wants),
                counter_attacks,
                request,
            )
            .await
        }
        Request::Inventory(request) => {
            inventory::serve(bridge, |wants| bridge.subscribe(wants), request).await
        }
        Request::TempInventory(request) => inventory::serve_temp(bridge, request),
        Request::Bank(request) => {
            bank::serve(bridge, |wants| bridge.subscribe(wants), request).await
        }
        Request::House(request) => house::serve(bridge, request),
        Request::Quests(request) => {
            quests::serve(bridge, |wants| bridge.subscribe(wants), request).await
        }
        Request::Drops(request) => {
            drops::serve(bridge, |wants| bridge.subscribe(wants), request).await
        }
        Request::Shops(request) => {
            shops::serve(bridge, |wants| bridge.subscribe(wants), request).await
        }
    }
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
        calls.handle(message, |call, _| {
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
            |_, _| unreachable!(),
        );
        assert!(!first.get() && second.get() && !other.get());
        calls.handle(ToPlayer::Ended(RunId(1)), |_, _| unreachable!());
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
