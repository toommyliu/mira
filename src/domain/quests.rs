use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{
    arrive,
    inventory::{ItemCount, Row, item_count, list, reply, row, rows},
    wait_for_action, wait_until_ready,
};
use crate::bridge::{
    self, CallError, Event, Format, GameAction, Loader, Packet, Packets, Subscription, Value,
    packets::wire,
};

const LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const BATCH: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Cadence {
    None,
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RewardKind {
    Guaranteed,
    Choice,
    Roll,
    Random,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuestReward {
    #[serde(flatten)]
    item: ItemCount,
    kind: RewardKind,
    drop_chance: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Quest {
    id: i32,
    name: String,
    cadence: Cadence,
    once: bool,
    requirements: Vec<ItemCount>,
    rewards: Vec<QuestReward>,
}

impl Quest {
    pub(crate) fn needs_reward_choice(&self) -> bool {
        self.rewards
            .iter()
            .any(|reward| reward.kind == RewardKind::Choice)
    }

    pub(crate) fn reward_names(&self) -> impl Iterator<Item = &str> {
        self.rewards.iter().map(|reward| reward.item.name())
    }

    pub(crate) fn requirement_names(&self) -> impl Iterator<Item = &str> {
        self.requirements.iter().map(ItemCount::name)
    }
}

fn reward_from(value: Value) -> Option<QuestReward> {
    let row = row(value)?;
    Some(QuestReward {
        item: item_count(&row)?,
        kind: match row.text("kind").as_str() {
            "choice" => RewardKind::Choice,
            "roll" => RewardKind::Roll,
            "random" => RewardKind::Random,
            _ => RewardKind::Guaranteed,
        },
        drop_chance: row.number("dropChance").unwrap_or(100.0),
    })
}

fn quest_from(value: Value) -> Option<Quest> {
    let Value::Object(mut fields) = value else {
        return None;
    };
    let requirements = list(&mut fields, "requirements")
        .into_iter()
        .filter_map(|value| item_count(&row(value)?))
        .collect();
    let rewards = list(&mut fields, "rewards")
        .into_iter()
        .filter_map(reward_from)
        .collect();
    let quest = Row(fields);
    Some(Quest {
        id: quest.positive("id")?,
        name: quest.text("name"),
        cadence: match quest.text("cadence").as_str() {
            "daily" => Cadence::Daily,
            "weekly" => Cadence::Weekly,
            "monthly" => Cadence::Monthly,
            _ => Cadence::None,
        },
        once: quest.flag("once"),
        requirements,
        rewards,
    })
}

pub(crate) fn quest(loader: &impl Loader, quest_id: i32) -> Result<Option<Quest>, CallError> {
    Ok(quest_from(bridge::quests::get(loader, quest_id)?))
}

pub(crate) fn accepted(loader: &impl Loader) -> Result<Vec<Quest>, CallError> {
    Ok(rows(bridge::quests::get_accepted(loader)?, quest_from))
}

fn loaded(loader: &impl Loader, quest_id: i32) -> Result<bool, CallError> {
    Ok(bridge::quests::get(loader, quest_id)? != Value::Null)
}

fn all_loaded(loader: &impl Loader, quest_ids: &[i32]) -> Result<bool, CallError> {
    for &quest_id in quest_ids {
        if !loaded(loader, quest_id)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) async fn load_batch(
    loader: &impl Loader,
    quest_ids: &[i32],
    silent: bool,
) -> Result<Vec<bool>, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(vec![false; quest_ids.len()]);
    }
    let mut missing = Vec::new();
    for &quest_id in quest_ids {
        if !missing.contains(&quest_id) && !loaded(loader, quest_id)? {
            missing.push(quest_id);
        }
    }
    for batch in missing.chunks(BATCH) {
        let list = batch
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let sent = arrive(loader, LOAD_TIMEOUT, || {
            if all_loaded(loader, batch)? {
                return Ok(Some(true));
            }
            Ok(
                match bridge::quests::load(loader, &list, silent)?.as_deref() {
                    Some("sent") => Some(true),
                    Some("wait") => None,
                    _ => Some(false),
                },
            )
        })
        .await?;
        if sent {
            arrive(loader, LOAD_TIMEOUT, || {
                Ok(all_loaded(loader, batch)?.then_some(true))
            })
            .await?;
        }
    }
    quest_ids
        .iter()
        .map(|&quest_id| loaded(loader, quest_id))
        .collect()
}

pub(crate) async fn load(
    loader: &impl Loader,
    quest_id: i32,
    silent: bool,
) -> Result<bool, CallError> {
    Ok(load_batch(loader, &[quest_id], silent).await?[0])
}

#[derive(Deserialize)]
pub(crate) struct AcceptQuestPacket {
    #[serde(rename = "QuestID", default, deserialize_with = "wire::integer")]
    quest_id: Option<i32>,
    #[serde(rename = "bSuccess", default, deserialize_with = "wire::flag")]
    success: bool,
}

impl Packet for AcceptQuestPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "acceptQuest";
}

#[derive(Deserialize)]
pub(crate) struct CompleteQuestPacket {
    #[serde(rename = "QuestID", default, deserialize_with = "wire::integer")]
    pub(crate) quest_id: Option<i32>,
    #[serde(rename = "sName", default, deserialize_with = "wire::text")]
    pub(crate) name: String,
    #[serde(rename = "bSuccess", default, deserialize_with = "wire::flag")]
    pub(crate) success: bool,
}

impl Packet for CompleteQuestPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "ccqr";
}

pub(crate) async fn accept(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    quest_id: i32,
    silent: bool,
) -> Result<bool, CallError> {
    if !load(loader, quest_id, silent).await? {
        return Ok(false);
    }
    if bridge::quests::is_in_progress(loader, quest_id)? {
        return Ok(true);
    }
    if !wait_for_action(loader, GameAction::AcceptQuest).await? {
        return Ok(false);
    }
    let replies = Packets::<AcceptQuestPacket>::subscribe(subscribe);
    if !bridge::quests::accept(loader, quest_id)? {
        return Ok(false);
    }
    Ok(reply(&replies, |packet| {
        (packet.quest_id == Some(quest_id)).then_some(packet.success)
    })
    .await)
}

pub(crate) async fn accept_batch(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    quest_ids: &[i32],
    silent: bool,
) -> Result<Vec<bool>, CallError> {
    let loaded = load_batch(loader, quest_ids, silent).await?;
    let mut accepted = Vec::with_capacity(quest_ids.len());
    for (&quest_id, loaded) in quest_ids.iter().zip(loaded) {
        accepted.push(loaded && accept(loader, &subscribe, quest_id, silent).await?);
    }
    Ok(accepted)
}

pub(crate) async fn complete(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    quest_id: i32,
    turn_ins: i32,
    reward_item_id: Option<i32>,
) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await?
        || !wait_for_action(loader, GameAction::TryQuestComplete).await?
    {
        return Ok(false);
    }
    let replies = Packets::<CompleteQuestPacket>::subscribe(subscribe);
    if !bridge::quests::complete(loader, quest_id, turn_ins, reward_item_id.unwrap_or(-1))? {
        return Ok(false);
    }
    Ok(reply(&replies, |packet| {
        (packet.quest_id == Some(quest_id)).then_some(packet.success)
    })
    .await)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;

    use super::*;
    use crate::{bridge::Events, domain::testing::FakeLoader};

    fn object(fields: &[(&str, Value)]) -> Value {
        Value::Object(
            fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    fn text(value: &str) -> Value {
        Value::String(value.into())
    }

    fn quest_row(id: i32) -> Value {
        object(&[
            ("id", Value::Number(f64::from(id))),
            ("name", text("Quest")),
        ])
    }

    fn packet(data: serde_json::Value) -> Event {
        Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": data }).to_string(),
        }
    }

    struct Log {
        loaded: Vec<i32>,
        in_progress: Vec<i32>,
        replies: Vec<serde_json::Value>,
        sent: Vec<(&'static str, Vec<Value>)>,
        loads_sent_at: Vec<tokio::time::Instant>,
        blocked: bool,
    }

    fn quest_log(
        loaded: Vec<i32>,
        in_progress: Vec<i32>,
        replies: Vec<serde_json::Value>,
    ) -> (FakeLoader, Rc<Events>, Rc<RefCell<Log>>) {
        let events = Rc::new(Events::default());
        let log = Rc::new(RefCell::new(Log {
            loaded,
            in_progress,
            replies,
            sent: Vec::new(),
            loads_sent_at: Vec::new(),
            blocked: false,
        }));
        let (publisher, state) = (events.clone(), log.clone());
        let loader = FakeLoader::new(move |callback, args| {
            let mut log = state.borrow_mut();
            let id = match args.first() {
                Some(Value::Number(id)) => *id as i32,
                _ => 0,
            };
            Ok(match callback {
                "auth.isLoggedIn"
                | "world.isLoaded"
                | "player.isLoaded"
                | "world.isActionAvailable" => Value::Bool(true),
                "quests.get" if log.loaded.contains(&id) => quest_row(id),
                "quests.get" => Value::Null,
                "quests.isInProgress" => Value::Bool(log.in_progress.contains(&id)),
                "quests.load" if log.blocked => Value::String("blocked".into()),
                "quests.load"
                    if log
                        .loads_sent_at
                        .last()
                        .is_some_and(|sent| sent.elapsed() < Duration::from_millis(1500)) =>
                {
                    Value::String("wait".into())
                }
                "quests.load" | "quests.accept" | "quests.complete" => {
                    log.sent.push((callback, args.to_vec()));
                    if let Some(Value::String(list)) = args.first() {
                        let ids = list.split(',').map(|id| id.parse::<i32>().unwrap());
                        log.loaded.extend(ids);
                        log.loads_sent_at.push(tokio::time::Instant::now());
                        return Ok(Value::String("sent".into()));
                    }
                    let upto = log
                        .replies
                        .iter()
                        .position(|reply| reply["QuestID"] == json!(id))
                        .map_or(log.replies.len(), |at| at + 1);
                    for reply in log.replies.drain(..upto) {
                        publisher.publish(&packet(reply));
                    }
                    Value::Bool(true)
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        (loader, events, log)
    }

    #[test]
    fn a_quest_lists_what_it_takes_and_each_reward_with_its_kind() {
        let quest = object(&[
            ("id", Value::Number(6552.0)),
            ("name", text("Power UP!")),
            ("cadence", text("daily")),
            ("once", Value::Bool(false)),
            (
                "requirements",
                Value::List(vec![object(&[
                    ("itemId", Value::Number(45277.0)),
                    ("name", text("Arcane Energy")),
                    ("quantity", Value::Number(6.0)),
                ])]),
            ),
            (
                "rewards",
                Value::List(vec![
                    object(&[
                        ("itemId", Value::Number(45230.0)),
                        ("name", text("Shadowscythe Templar")),
                        ("quantity", Value::Number(1.0)),
                        ("kind", text("choice")),
                        ("dropChance", Value::Number(100.0)),
                    ]),
                    object(&[
                        ("itemId", Value::Number(9.0)),
                        ("name", text("Gem")),
                        ("quantity", Value::Number(2.0)),
                        ("kind", text("random")),
                        ("dropChance", Value::Number(25.0)),
                    ]),
                ]),
            ),
        ]);
        assert_eq!(
            serde_json::to_value(quest_from(quest).unwrap()).unwrap(),
            json!({ "id": 6552, "name": "Power UP!", "cadence": "daily", "once": false,
                    "requirements": [{ "itemId": 45277, "name": "Arcane Energy", "quantity": 6 }],
                    "rewards": [
                        { "itemId": 45230, "name": "Shadowscythe Templar", "quantity": 1,
                          "kind": "choice", "dropChance": 100.0 },
                        { "itemId": 9, "name": "Gem", "quantity": 2,
                          "kind": "random", "dropChance": 25.0 }
                    ] })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn loading_opens_the_quest_log_unless_asked_to_stay_silent() {
        let (loader, _, log) = quest_log(vec![7], Vec::new(), Vec::new());
        assert_eq!(load(&loader, 5, false).await, Ok(true));
        assert_eq!(load(&loader, 6, true).await, Ok(true));
        assert_eq!(load(&loader, 7, false).await, Ok(true));
        assert_eq!(
            log.borrow().sent,
            [
                (
                    "quests.load",
                    vec![Value::String("5".into()), Value::Bool(false)]
                ),
                (
                    "quests.load",
                    vec![Value::String("6".into()), Value::Bool(true)]
                ),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn quest_loads_close_together_each_wait_their_turn() {
        let (loader, _, log) = quest_log(Vec::new(), Vec::new(), Vec::new());
        for id in [8152, 8153, 8154] {
            assert_eq!(load(&loader, id, false).await, Ok(true), "quest {id}");
        }
        let sent = log.borrow().loads_sent_at.clone();
        assert_eq!(sent.len(), 3);
        assert!(
            sent.windows(2)
                .all(|pair| pair[1] - pair[0] >= Duration::from_millis(1500))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_batch_loads_only_missing_quests_thirty_to_a_request() {
        let wanted: Vec<i32> = (1..=35).chain([3, 40]).collect();
        let (loader, _, log) = quest_log(vec![40], Vec::new(), Vec::new());
        assert_eq!(load_batch(&loader, &wanted, true).await, Ok(vec![true; 37]));
        let first: Vec<String> = (1..=30).map(|id| id.to_string()).collect();
        assert_eq!(
            log.borrow().sent,
            [
                (
                    "quests.load",
                    vec![Value::String(first.join(",")), Value::Bool(true)]
                ),
                (
                    "quests.load",
                    vec![Value::String("31,32,33,34,35".into()), Value::Bool(true)]
                ),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_batch_accept_loads_together_then_accepts_each_in_turn() {
        let accepted = |id: u32, success: u8| json!({ "cmd": "acceptQuest", "QuestID": id, "bSuccess": success, "msg": "" });
        let (loader, events, log) = quest_log(
            Vec::new(),
            vec![8153],
            vec![accepted(8152, 1), accepted(8154, 0)],
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            accept_batch(&loader, subscribe, &[8152, 8153, 8154], false).await,
            Ok(vec![true, true, false])
        );
        let sent: Vec<_> = log
            .borrow()
            .sent
            .iter()
            .map(|(call, args)| (*call, args[0].clone()))
            .collect();
        assert_eq!(
            sent,
            [
                ("quests.load", Value::String("8152,8153,8154".into())),
                ("quests.accept", Value::Number(8152.0)),
                ("quests.accept", Value::Number(8154.0)),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_blocked_quest_log_fails_without_waiting() {
        let (loader, _, log) = quest_log(Vec::new(), Vec::new(), Vec::new());
        log.borrow_mut().blocked = true;
        let started = tokio::time::Instant::now();
        assert_eq!(load(&loader, 5, false).await, Ok(false));
        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn accepting_finishes_on_the_servers_answer_for_that_quest() {
        let other = json!({ "cmd": "acceptQuest", "QuestID": 9, "bSuccess": 1, "msg": "success" });
        for (answer, expected) in [(1, true), (0, false)] {
            let ours =
                json!({ "cmd": "acceptQuest", "QuestID": 6552, "bSuccess": answer, "msg": "" });
            let (loader, events, _) = quest_log(Vec::new(), Vec::new(), vec![other.clone(), ours]);
            let subscribe = |wants| events.subscribe(wants);
            assert_eq!(
                accept(&loader, subscribe, 6552, false).await,
                Ok(expected),
                "bSuccess {answer}"
            );
        }

        let (loader, events, log) = quest_log(vec![6552], vec![6552], Vec::new());
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(accept(&loader, subscribe, 6552, false).await, Ok(true));
        assert!(log.borrow().sent.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn completing_finishes_on_the_servers_answer_for_that_quest() {
        let done = |id: u32, success: u8| json!({ "cmd": "ccqr", "QuestID": id, "bSuccess": success, "sName": "Power UP!", "msg": "" });
        let (loader, events, log) =
            quest_log(vec![6552], vec![6552], vec![done(9, 1), done(6552, 1)]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            complete(&loader, subscribe, 6552, 1, Some(45230)).await,
            Ok(true)
        );
        assert_eq!(
            log.borrow().sent,
            [(
                "quests.complete",
                vec![
                    Value::Number(6552.0),
                    Value::Number(1.0),
                    Value::Number(45230.0)
                ]
            )]
        );

        let (loader, events, log) = quest_log(vec![6552], vec![6552], vec![done(6552, 0)]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(complete(&loader, subscribe, 6552, 2, None).await, Ok(false));
        assert_eq!(
            log.borrow().sent[0].1,
            [
                Value::Number(6552.0),
                Value::Number(2.0),
                Value::Number(-1.0)
            ]
        );

        let (loader, events, _) = quest_log(vec![6552], vec![6552], vec![done(9, 1)]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(complete(&loader, subscribe, 6552, 1, None).await, Ok(false));
    }
}
