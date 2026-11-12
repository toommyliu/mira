use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{
    sync::watch,
    time::{Instant, sleep},
};

use crate::{
    bridge::{self, CallError, Event, Loader, Packets, Subscription},
    domain::{self, drops::GetDropPacket, inventory::AddItemsPacket, quests},
};

const CHECK_EVERY: Duration = Duration::from_secs(5);
const BACKOFF: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Registered {
    pub(crate) quest_id: i32,
    pub(crate) reward_item_id: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Config {
    pub(crate) enabled: bool,
    pub(crate) accept_rewards: bool,
    pub(crate) accept_requirements: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            accept_rewards: false,
            accept_requirements: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct State {
    pub(crate) quests: Vec<Registered>,
    pub(crate) config: Config,
}

impl State {
    pub(crate) fn register(&mut self, quests: impl IntoIterator<Item = Registered>) {
        for quest in quests {
            if quest.quest_id <= 0 {
                continue;
            }
            let quest = Registered {
                reward_item_id: quest.reward_item_id.filter(|id| *id > 0),
                ..quest
            };
            match self
                .quests
                .iter_mut()
                .find(|registered| registered.quest_id == quest.quest_id)
            {
                Some(registered) => *registered = quest,
                None => self.quests.push(quest),
            }
        }
    }

    pub(crate) fn unregister(&mut self, quest_ids: &[i32]) {
        self.quests
            .retain(|registered| !quest_ids.contains(&registered.quest_id));
    }

    fn quest_ids(&self) -> Vec<i32> {
        self.quests
            .iter()
            .map(|registered| registered.quest_id)
            .collect()
    }

    pub(super) async fn drop_names(
        &self,
        loader: &impl Loader,
    ) -> Result<(Vec<String>, bool), CallError> {
        let Config {
            accept_rewards,
            accept_requirements,
            ..
        } = self.config;
        let mut names = Vec::new();
        if self.quests.is_empty() || (!accept_rewards && !accept_requirements) {
            return Ok((names, true));
        }
        let loaded = quests::load_batch(loader, &self.quest_ids(), true)
            .await?
            .into_iter()
            .all(|loaded| loaded);
        for registered in &self.quests {
            let Some(quest) = quests::quest(loader, registered.quest_id)? else {
                continue;
            };
            if accept_rewards {
                names.extend(quest.reward_names().map(str::to_owned));
            }
            if accept_requirements {
                names.extend(quest.requirement_names().map(str::to_owned));
            }
        }
        Ok((names, loaded))
    }
}

#[derive(Default)]
struct Memory {
    warned: Vec<i32>,
    failures: HashMap<i32, (Instant, usize)>,
}

impl Memory {
    fn cooling(&self, quest_id: i32) -> bool {
        self.failures.get(&quest_id).is_some_and(|(at, strikes)| {
            Instant::now() < *at + BACKOFF[(strikes - 1).min(BACKOFF.len() - 1)]
        })
    }

    fn settled(&mut self, quest_id: i32, done: bool) {
        if done {
            self.failures.remove(&quest_id);
            return;
        }
        let strikes = self
            .failures
            .get(&quest_id)
            .map_or(0, |(_, strikes)| *strikes);
        self.failures
            .insert(quest_id, (Instant::now(), strikes + 1));
    }
}

fn gained_items(event: &Event) -> bool {
    Packets::<AddItemsPacket>::wants(event) || Packets::<GetDropPacket>::wants(event)
}

pub(crate) async fn run(
    loader: impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    mut state: watch::Receiver<State>,
) {
    let gained = subscribe(gained_items);
    let mut memory = Memory::default();
    loop {
        let current = state.borrow_and_update().clone();
        let active = current.config.enabled && !current.quests.is_empty();
        if active && let Err(error) = settle(&loader, &subscribe, &state, &mut memory).await {
            tracing::debug!(%error, "registered quests skipped a check");
        }
        tokio::select! {
            _ = gained.next() => {}
            changed = state.changed() => if changed.is_err() {
                return;
            },
            () = sleep(CHECK_EVERY), if active => {}
        }
    }
}

async fn settle(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    state: &watch::Receiver<State>,
    memory: &mut Memory,
) -> Result<(), CallError> {
    if !domain::ready(loader)? {
        return Ok(());
    }
    let quest_ids = state.borrow().quest_ids();
    quests::load_batch(loader, &quest_ids, true).await?;
    while advance(loader, subscribe, state, memory).await? {}
    Ok(())
}

fn still_registered(state: &watch::Receiver<State>, quest_id: i32) -> Option<Registered> {
    let state = state.borrow();
    if !state.config.enabled {
        return None;
    }
    state
        .quests
        .iter()
        .find(|registered| registered.quest_id == quest_id)
        .copied()
}

async fn advance(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    state: &watch::Receiver<State>,
    memory: &mut Memory,
) -> Result<bool, CallError> {
    let mut acted = false;
    let quest_ids = state.borrow().quest_ids();
    for quest_id in quest_ids {
        let Some(Registered { reward_item_id, .. }) = still_registered(state, quest_id) else {
            continue;
        };
        if memory.cooling(quest_id) {
            continue;
        }
        if !bridge::quests::is_in_progress(loader, quest_id)? {
            if bridge::quests::is_available(loader, quest_id)? {
                let accept = quests::accept(loader, subscribe, quest_id, true);
                let Some(accepted) = unless_changed(state, accept).await? else {
                    return Ok(false);
                };
                memory.settled(quest_id, accepted);
                acted |= accepted;
            }
            continue;
        }
        if !bridge::quests::can_complete(loader, quest_id)? {
            continue;
        }
        if reward_item_id.is_none()
            && quests::quest(loader, quest_id)?.is_some_and(|quest| quest.needs_reward_choice())
        {
            if !memory.warned.contains(&quest_id) {
                memory.warned.push(quest_id);
                tracing::warn!(
                    quest_id,
                    "a registered quest needs a reward pick to complete"
                );
            }
            continue;
        }
        let turn_ins = bridge::quests::get_max_turn_ins(loader, quest_id)?.max(1);
        let complete = quests::complete(loader, subscribe, quest_id, turn_ins, reward_item_id);
        let Some(completed) = unless_changed(state, complete).await? else {
            return Ok(false);
        };
        memory.settled(quest_id, completed);
        acted |= completed;
    }
    Ok(acted)
}

async fn unless_changed(
    state: &watch::Receiver<State>,
    action: impl Future<Output = Result<bool, CallError>>,
) -> Result<Option<bool>, CallError> {
    let mut changes = state.clone();
    tokio::select! {
        biased;
        _ = changes.changed() => Ok(None),
        done = action => done.map(Some),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;
    use tokio::task::LocalSet;

    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    #[derive(Default)]
    struct Log {
        in_progress: bool,
        has_items: bool,
        choice: bool,
        unregister_on_complete: bool,
        refuse_complete: bool,
        action_blocked: bool,
        accepted: u32,
        completed: Vec<(f64, f64)>,
    }

    fn packet(data: serde_json::Value) -> Event {
        Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": data }).to_string(),
        }
    }

    fn quest_row(choice: bool) -> Value {
        let reward = Value::Object(
            [
                ("itemId", Value::Number(50.0)),
                ("name", Value::String("Prize".into())),
                ("quantity", Value::Number(1.0)),
                (
                    "kind",
                    Value::String(if choice { "choice" } else { "guaranteed" }.into()),
                ),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
        );
        Value::Object(
            [
                ("id", Value::Number(7.0)),
                ("name", Value::String("Quest".into())),
                ("rewards", Value::List(vec![reward])),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
        )
    }

    fn start(log: Log) -> (Rc<RefCell<Log>>, Rc<Events>, Rc<watch::Sender<State>>) {
        let log = Rc::new(RefCell::new(log));
        let events = Rc::new(Events::default());
        let (settings, receiver) = watch::channel(State::default());
        let settings = Rc::new(settings);
        let (state, publisher, script) = (log.clone(), events.clone(), settings.clone());
        let loader = FakeLoader::new(move |callback, args| {
            let mut log = state.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "world.isActionAvailable" => Value::Bool(!log.action_blocked),
                "quests.get" => quest_row(log.choice),
                "quests.isInProgress" => Value::Bool(log.in_progress),
                "quests.isAvailable" => Value::Bool(true),
                "quests.canComplete" => Value::Bool(log.in_progress && log.has_items),
                "quests.getMaxTurnIns" => Value::Number(3.0),
                "quests.accept" => {
                    log.in_progress = true;
                    log.accepted += 1;
                    publisher.publish(&packet(
                        json!({ "cmd": "acceptQuest", "QuestID": 7, "bSuccess": 1 }),
                    ));
                    Value::Bool(true)
                }
                "quests.complete" => {
                    let [_, Value::Number(turn_ins), Value::Number(reward)] = args else {
                        panic!("complete needs turn-ins and a reward: {args:?}");
                    };
                    log.completed.push((*turn_ins, *reward));
                    if log.refuse_complete {
                        publisher.publish(&packet(
                            json!({ "cmd": "ccqr", "QuestID": 7, "bSuccess": 0 }),
                        ));
                        return Ok(Value::Bool(true));
                    }
                    log.in_progress = false;
                    log.has_items = false;
                    if log.unregister_on_complete {
                        script.send_modify(|state| state.unregister(&[7]));
                    }
                    publisher.publish(&packet(
                        json!({ "cmd": "ccqr", "QuestID": 7, "bSuccess": 1 }),
                    ));
                    Value::Bool(true)
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        let subscriber = events.clone();
        tokio::task::spawn_local(run(
            loader,
            move |wants| subscriber.subscribe(wants),
            receiver,
        ));
        (log, events, settings)
    }

    fn gain_items(events: &Events) {
        events.publish(&packet(json!({ "cmd": "addItems", "items": {} })));
    }

    async fn settle() {
        sleep(Duration::from_millis(50)).await;
    }

    fn register(state: &watch::Sender<State>, reward_item_id: Option<i32>) {
        state.send_modify(|state| {
            state.register([Registered {
                quest_id: 7,
                reward_item_id,
            }])
        });
    }

    #[tokio::test(start_paused = true)]
    async fn a_quest_is_accepted_then_completed_with_every_turn_in_once_its_items_arrive() {
        LocalSet::new()
            .run_until(async {
                let (log, events, state) = start(Log::default());
                register(&state, None);
                settle().await;
                assert_eq!(log.borrow().accepted, 1);
                assert!(log.borrow().completed.is_empty());

                log.borrow_mut().has_items = true;
                gain_items(&events);
                settle().await;
                let log = log.borrow();
                assert_eq!(log.completed, [(3.0, -1.0)]);
                assert_eq!(log.accepted, 2, "the repeatable quest is accepted again");
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_choice_quest_completes_only_with_a_reward_pick() {
        LocalSet::new()
            .run_until(async {
                let (log, _events, state) = start(Log {
                    in_progress: true,
                    has_items: true,
                    choice: true,
                    ..Log::default()
                });
                register(&state, None);
                sleep(CHECK_EVERY * 2).await;
                assert!(log.borrow().completed.is_empty());

                register(&state, Some(50));
                settle().await;
                assert_eq!(log.borrow().completed, [(3.0, 50.0)]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_quest_unregistered_mid_check_is_not_accepted_again() {
        LocalSet::new()
            .run_until(async {
                let (log, _events, state) = start(Log {
                    in_progress: true,
                    has_items: true,
                    unregister_on_complete: true,
                    ..Log::default()
                });
                register(&state, None);
                sleep(CHECK_EVERY * 2).await;
                let log = log.borrow();
                assert_eq!(log.completed.len(), 1);
                assert_eq!(log.accepted, 0);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn paused_or_unregistered_quests_are_left_alone() {
        LocalSet::new()
            .run_until(async {
                let (log, _events, state) = start(Log::default());
                state.send_modify(|state| state.config.enabled = false);
                register(&state, None);
                sleep(CHECK_EVERY * 2).await;
                assert_eq!(log.borrow().accepted, 0);

                state.send_modify(|state| {
                    state.config.enabled = true;
                    state.unregister(&[7]);
                });
                sleep(CHECK_EVERY * 2).await;
                assert_eq!(log.borrow().accepted, 0);

                {
                    let mut log = log.borrow_mut();
                    log.in_progress = true;
                    log.has_items = true;
                    log.action_blocked = true;
                }
                register(&state, None);
                settle().await;
                state.send_modify(|state| state.unregister(&[7]));
                log.borrow_mut().action_blocked = false;
                sleep(CHECK_EVERY * 2).await;
                assert!(
                    log.borrow().completed.is_empty(),
                    "a turn-in waiting on the cooldown is dropped when its quest is unregistered"
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_turn_in_waits_before_trying_again() {
        LocalSet::new()
            .run_until(async {
                let (log, events, state) = start(Log {
                    in_progress: true,
                    has_items: true,
                    refuse_complete: true,
                    ..Log::default()
                });
                register(&state, None);
                settle().await;
                assert_eq!(log.borrow().completed.len(), 1);

                gain_items(&events);
                settle().await;
                assert_eq!(
                    log.borrow().completed.len(),
                    1,
                    "new items don't retry a refused turn-in right away"
                );

                sleep(CHECK_EVERY).await;
                assert_eq!(log.borrow().completed.len(), 2);

                log.borrow_mut().refuse_complete = false;
                sleep(Duration::from_secs(3)).await;
                gain_items(&events);
                settle().await;
                assert_eq!(log.borrow().completed.len(), 2, "the backoff grows");
                sleep(CHECK_EVERY).await;
                assert_eq!(log.borrow().completed.len(), 3);
            })
            .await;
    }
}
