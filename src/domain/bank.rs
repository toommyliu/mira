use std::time::Duration;

use serde::Deserialize;

use super::{
    arrive,
    inventory::{self, Item, ItemQuery, by_id, item_from, ready_item, reply, rows},
    wait_until_ready,
};
use crate::bridge::{
    self, CallError, Event, Format, Loader, Packet, Packets, Subscription, packets::wire,
};

const LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) async fn load(loader: &impl Loader) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    if bridge::bank::is_loaded(loader)? {
        return Ok(true);
    }
    bridge::bank::load_items(loader)?;
    arrive(loader, LOAD_TIMEOUT, || {
        Ok(bridge::bank::is_loaded(loader)?.then_some(true))
    })
    .await
}

pub(crate) async fn items(loader: &impl Loader) -> Result<Vec<Item>, CallError> {
    if !load(loader).await? {
        return Ok(Vec::new());
    }
    Ok(rows(bridge::bank::get_items(loader)?, item_from))
}

pub(crate) async fn item(
    loader: &impl Loader,
    query: &ItemQuery,
) -> Result<Option<Item>, CallError> {
    if !load(loader).await? {
        return Ok(None);
    }
    Ok(item_from(bridge::bank::get_item(loader, query.selector())?))
}

pub(crate) fn available_slots(loader: &impl Loader) -> Result<i32, CallError> {
    Ok((bridge::bank::get_slots(loader)? - bridge::bank::get_used_slots(loader)?).max(0))
}

pub(crate) async fn open(loader: &impl Loader) -> Result<bool, CallError> {
    if !load(loader).await? || !bridge::bank::open(loader)? {
        return Ok(false);
    }
    arrive(loader, OPEN_TIMEOUT, || {
        Ok(bridge::bank::is_open(loader)?.then_some(true))
    })
    .await
}

#[derive(Deserialize)]
pub(crate) struct BankFromInvPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::integer")]
    item_id: Option<i32>,
    #[serde(rename = "bSuccess", default, deserialize_with = "wire::flag")]
    success: bool,
}

impl Packet for BankFromInvPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "bankFromInv";
}

#[derive(Deserialize)]
pub(crate) struct BankToInvPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::integer")]
    item_id: Option<i32>,
}

impl Packet for BankToInvPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "bankToInv";
}

#[derive(Deserialize)]
pub(crate) struct BankSwapInvPacket {
    #[serde(rename = "invItemID", default, deserialize_with = "wire::integer")]
    inventory_item_id: Option<i32>,
    #[serde(rename = "bankItemID", default, deserialize_with = "wire::integer")]
    bank_item_id: Option<i32>,
}

impl Packet for BankSwapInvPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "bankSwapInv";
}

pub(crate) async fn deposit(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    query: &ItemQuery,
) -> Result<bool, CallError> {
    let Some(stored) = ready_item(loader, query).await? else {
        return Ok(false);
    };
    let replies = Packets::<BankFromInvPacket>::subscribe(subscribe);
    if !bridge::bank::deposit(loader, by_id(stored.item_id))? {
        return Ok(false);
    }
    Ok(reply(&replies, |packet| {
        (packet.item_id == Some(stored.item_id)).then_some(packet.success)
    })
    .await)
}

pub(crate) async fn withdraw(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    query: &ItemQuery,
) -> Result<bool, CallError> {
    let Some(banked) = item(loader, query).await? else {
        return Ok(false);
    };
    let replies = Packets::<BankToInvPacket>::subscribe(subscribe);
    if !bridge::bank::withdraw(loader, by_id(banked.item_id))? {
        return Ok(false);
    }
    Ok(reply(&replies, |packet| {
        (packet.item_id == Some(banked.item_id)).then_some(true)
    })
    .await)
}

pub(crate) async fn swap(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    inventory_query: &ItemQuery,
    bank_query: &ItemQuery,
) -> Result<bool, CallError> {
    let Some(banked) = item(loader, bank_query).await? else {
        return Ok(false);
    };
    let Some(stored) = inventory::item(loader, inventory_query)? else {
        return Ok(false);
    };
    let replies = Packets::<BankSwapInvPacket>::subscribe(subscribe);
    if !bridge::bank::swap(loader, by_id(stored.item_id), by_id(banked.item_id))? {
        return Ok(false);
    }
    Ok(reply(&replies, |packet| {
        (packet.inventory_item_id == Some(stored.item_id)
            && packet.bank_item_id == Some(banked.item_id))
        .then_some(true)
    })
    .await)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

    use serde_json::json;

    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    fn row(id: f64, name: &str) -> Value {
        Value::Object(BTreeMap::from([
            ("itemId".to_owned(), Value::Number(id)),
            ("name".to_owned(), Value::String(name.into())),
            ("quantity".to_owned(), Value::Number(1.0)),
        ]))
    }

    fn packet(data: serde_json::Value) -> Event {
        Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": data }).to_string(),
        }
    }

    struct Account {
        bag: Vec<(f64, &'static str)>,
        bank: Vec<(f64, &'static str)>,
        loaded: bool,
        replies: Vec<serde_json::Value>,
    }

    fn account(
        bag: Vec<(f64, &'static str)>,
        bank: Vec<(f64, &'static str)>,
        replies: Vec<serde_json::Value>,
    ) -> (FakeLoader, Rc<Events>) {
        let events = Rc::new(Events::default());
        let state = Rc::new(RefCell::new(Account {
            bag,
            bank,
            loaded: false,
            replies,
        }));
        let publisher = events.clone();
        let loader = FakeLoader::new(move |callback, args| {
            let mut account = state.borrow_mut();
            let find = |items: &[(f64, &str)], selector: &Value| {
                let Value::Object(selector) = selector else {
                    panic!("{callback} needs a selector: {args:?}");
                };
                items
                    .iter()
                    .find(|(id, name)| match selector.get("itemId") {
                        Some(Value::Number(wanted)) => id == wanted,
                        _ => matches!(selector.get("name"), Some(Value::String(wanted)) if wanted.eq_ignore_ascii_case(name)),
                    })
                    .map_or(Value::Null, |(id, name)| row(*id, name))
            };
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "bank.isLoaded" => Value::Bool(account.loaded),
                "bank.loadItems" => {
                    account.loaded = true;
                    Value::Null
                }
                "bank.getItems" => Value::List(
                    account
                        .bank
                        .iter()
                        .map(|(id, name)| row(*id, name))
                        .collect(),
                ),
                "bank.getItem" => find(&account.bank, &args[0]),
                "inventory.getItem" => find(&account.bag, &args[0]),
                "bank.deposit" | "bank.withdraw" | "bank.swap" => {
                    for reply in std::mem::take(&mut account.replies) {
                        publisher.publish(&packet(reply));
                    }
                    Value::Bool(true)
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        (loader, events)
    }

    fn by_name(name: &str) -> ItemQuery {
        ItemQuery::Name(name.into())
    }

    #[tokio::test(start_paused = true)]
    async fn reads_load_the_bank_once() {
        let (loader, _) = account(Vec::new(), vec![(1.0, "Gem"), (2.0, "Ore")], Vec::new());
        let names: Vec<_> = items(&loader)
            .await
            .unwrap()
            .into_iter()
            .map(|item| serde_json::to_value(item).unwrap()["name"].clone())
            .collect();
        assert_eq!(names, [json!("Gem"), json!("Ore")]);
        assert!(item(&loader, &by_name("ore")).await.unwrap().is_some());
        assert_eq!(item(&loader, &by_name("Sword")).await, Ok(None));
        assert_eq!(loader.count("bank.loadItems"), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn depositing_finishes_on_the_servers_answer_for_that_item() {
        let other = json!({ "cmd": "bankFromInv", "ItemID": 9, "bSuccess": 1 });
        for (answer, expected) in [(1, true), (0, false)] {
            let ours = json!({ "cmd": "bankFromInv", "ItemID": 5, "bSuccess": answer, "msg": "" });
            let (loader, events) =
                account(vec![(5.0, "Gem")], Vec::new(), vec![other.clone(), ours]);
            let subscribe = |wants| events.subscribe(wants);
            assert_eq!(
                deposit(&loader, subscribe, &by_name("Gem")).await,
                Ok(expected),
                "bSuccess {answer}"
            );
        }

        let (loader, events) = account(vec![(5.0, "Gem")], Vec::new(), vec![other]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            deposit(&loader, subscribe, &by_name("Gem")).await,
            Ok(false)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn withdrawing_finishes_when_the_server_moves_that_item() {
        let moved = |id: u32| json!({ "cmd": "bankToInv", "ItemID": id.to_string() });
        let (loader, events) = account(Vec::new(), vec![(5.0, "Gem")], vec![moved(9), moved(5)]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            withdraw(&loader, subscribe, &by_name("Gem")).await,
            Ok(true)
        );

        let (loader, events) = account(Vec::new(), vec![(5.0, "Gem")], vec![moved(9)]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            withdraw(&loader, subscribe, &by_name("Gem")).await,
            Ok(false)
        );

        let (loader, events) = account(Vec::new(), Vec::new(), Vec::new());
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            withdraw(&loader, subscribe, &by_name("Gem")).await,
            Ok(false)
        );
        assert_eq!(loader.count("bank.withdraw"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn swapping_finishes_when_the_server_trades_both_items() {
        let swapped = |inv: u32, bank: u32| json!({ "cmd": "bankSwapInv", "invItemID": inv, "bankItemID": bank.to_string() });
        let (loader, events) = account(
            vec![(5.0, "Gem")],
            vec![(6.0, "Ore")],
            vec![swapped(6, 5), swapped(5, 6)],
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            swap(&loader, subscribe, &by_name("Gem"), &by_name("Ore")).await,
            Ok(true)
        );

        let (loader, events) = account(vec![(5.0, "Gem")], vec![(6.0, "Ore")], vec![swapped(6, 5)]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            swap(&loader, subscribe, &by_name("Gem"), &by_name("Ore")).await,
            Ok(false)
        );
    }
}
