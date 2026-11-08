use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::time::{Instant, timeout_at};

use super::{combat::selector, wait_for_action, wait_until_ready};
use crate::bridge::{
    self, CallError, Event, Format, GameAction, Loader, Packet, Packets, Subscription, Value,
    packets::wire,
};

const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum ItemQuery {
    Name(String),
    ItemId {
        #[serde(rename = "itemId")]
        item_id: f64,
    },
}

impl ItemQuery {
    fn selector(&self) -> Value {
        match self {
            ItemQuery::Name(name) => selector("name", Value::String(name.trim().to_owned())),
            ItemQuery::ItemId { item_id } => by_id(*item_id),
        }
    }
}

fn by_id(item_id: f64) -> Value {
    selector("itemId", Value::Number(item_id))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Pool {
    #[default]
    Bag,
    Misc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ItemPool {
    Bag,
    Misc,
    Class,
    House,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Item {
    item_id: f64,
    char_item_id: Option<f64>,
    name: String,
    description: String,
    pub(crate) quantity: f64,
    max_stack: f64,
    category: String,
    slot: String,
    link: String,
    meta: String,
    file: String,
    pub(crate) equipped: bool,
    wearable: bool,
    worn: bool,
    member: bool,
    coins: bool,
    pool: ItemPool,
    class_rank: Option<f64>,
    enhancement: Option<Enhancement>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Enhancement {
    id: f64,
    level: f64,
    pattern_id: Option<f64>,
    proc_id: Option<f64>,
    dps: Option<f64>,
    range: Option<f64>,
    rarity: Option<f64>,
}

impl Item {
    fn usable(&self) -> bool {
        self.category == "Item"
    }

    fn consumable_action(&self) -> Option<f64> {
        let link = self.link.trim().to_lowercase();
        if !self.usable() || matches!(link.as_str(), "" | "none" | "elixir" | "tonic") {
            return None;
        }
        self.meta.trim().parse().ok()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TempItem {
    item_id: f64,
    name: String,
    pub(crate) quantity: f64,
    max_stack: f64,
}

struct Row(BTreeMap<String, Value>);

impl Row {
    fn number(&self, key: &str) -> Option<f64> {
        match self.0.get(key) {
            Some(Value::Number(number)) if number.is_finite() => Some(*number),
            _ => None,
        }
    }

    fn text(&self, key: &str) -> String {
        match self.0.get(key) {
            Some(Value::String(text)) => text.clone(),
            _ => String::new(),
        }
    }

    fn flag(&self, key: &str) -> bool {
        matches!(self.0.get(key), Some(Value::Bool(true)))
    }

    fn item_id(&self) -> Option<f64> {
        self.number("itemId").filter(|id| *id > 0.0)
    }
}

fn row(value: Value) -> Option<Row> {
    match value {
        Value::Object(fields) => Some(Row(fields)),
        _ => None,
    }
}

fn item_from(value: Value) -> Option<Item> {
    let row = row(value)?;
    let positive = |key: &str| row.number(key).filter(|number| *number > 0.0);
    Some(Item {
        item_id: row.item_id()?,
        char_item_id: positive("charItemId"),
        name: row.text("name"),
        description: row.text("description"),
        quantity: row.number("quantity").unwrap_or(0.0),
        max_stack: row.number("maxStack").unwrap_or(1.0),
        category: row.text("category"),
        slot: row.text("slot"),
        link: row.text("link"),
        meta: row.text("meta"),
        file: row.text("file"),
        equipped: row.flag("equipped"),
        wearable: row.flag("wearable"),
        worn: row.flag("worn"),
        member: row.flag("member"),
        coins: row.flag("coins"),
        pool: match row.text("pool").as_str() {
            "misc" => ItemPool::Misc,
            "class" => ItemPool::Class,
            "house" => ItemPool::House,
            _ => ItemPool::Bag,
        },
        class_rank: row.number("classRank"),
        enhancement: positive("enhId").map(|id| Enhancement {
            id,
            level: row.number("enhLevel").unwrap_or(0.0),
            pattern_id: positive("enhPatternId"),
            proc_id: positive("enhProcId"),
            dps: row.number("enhDps"),
            range: row.number("enhRange"),
            rarity: row.number("enhRarity"),
        }),
    })
}

fn temp_item_from(value: Value) -> Option<TempItem> {
    let row = row(value)?;
    Some(TempItem {
        item_id: row.item_id()?,
        name: row.text("name"),
        quantity: row.number("quantity").unwrap_or(0.0),
        max_stack: row.number("maxStack").unwrap_or(1.0),
    })
}

fn rows<T>(value: Value, parse: fn(Value) -> Option<T>) -> Vec<T> {
    match value {
        Value::List(rows) => rows.into_iter().filter_map(parse).collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn items(loader: &impl Loader) -> Result<Vec<Item>, CallError> {
    Ok(rows(bridge::inventory::get_items(loader)?, item_from))
}

pub(crate) fn item(loader: &impl Loader, query: &ItemQuery) -> Result<Option<Item>, CallError> {
    Ok(item_from(bridge::inventory::get_item(
        loader,
        query.selector(),
    )?))
}

pub(crate) fn temp_items(loader: &impl Loader) -> Result<Vec<TempItem>, CallError> {
    Ok(rows(
        bridge::temp_inventory::get_items(loader)?,
        temp_item_from,
    ))
}

pub(crate) fn temp_item(
    loader: &impl Loader,
    query: &ItemQuery,
) -> Result<Option<TempItem>, CallError> {
    Ok(temp_item_from(bridge::temp_inventory::get_item(
        loader,
        query.selector(),
    )?))
}

pub(crate) fn used_slots(loader: &impl Loader, pool: Pool) -> Result<i32, CallError> {
    let pool = match pool {
        Pool::Bag => "bag",
        Pool::Misc => "misc",
    };
    bridge::inventory::get_used_slots(loader, pool)
}

pub(crate) fn available_slots(loader: &impl Loader, pool: Pool) -> Result<i32, CallError> {
    Ok((bridge::inventory::get_slots(loader)? - used_slots(loader, pool)?).max(0))
}

#[derive(Deserialize)]
pub(crate) struct EquipItemPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::number")]
    item_id: Option<f64>,
    #[serde(default, deserialize_with = "wire::number")]
    uid: Option<f64>,
}

impl Packet for EquipItemPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "equipItem";
}

#[derive(Deserialize)]
pub(crate) struct UnequipItemPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::number")]
    item_id: Option<f64>,
    #[serde(default, deserialize_with = "wire::number")]
    uid: Option<f64>,
}

impl Packet for UnequipItemPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "unequipItem";
}

#[derive(Deserialize)]
pub(crate) struct WearItemPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::number")]
    item_id: Option<f64>,
    #[serde(default, deserialize_with = "wire::number")]
    uid: Option<f64>,
    #[serde(default, deserialize_with = "wire::flag")]
    success: bool,
}

impl Packet for WearItemPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "wearItem";
}

#[derive(Deserialize)]
pub(crate) struct SeiaPacket {
    #[serde(rename = "iRes", default, deserialize_with = "wire::flag")]
    accepted: bool,
    #[serde(default)]
    o: Option<SeiaAction>,
}

#[derive(Deserialize)]
struct SeiaAction {
    #[serde(default, deserialize_with = "wire::number")]
    id: Option<f64>,
}

impl Packet for SeiaPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "seia";
}

#[derive(Deserialize)]
pub(crate) struct TurnInPacket {
    #[serde(rename = "sItems", default, deserialize_with = "wire::text")]
    items: String,
}

impl Packet for TurnInPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "turnIn";
}

impl TurnInPacket {
    pub(crate) fn took(&self, item_id: f64) -> bool {
        self.items.split(',').any(|entry| {
            entry
                .split(':')
                .next()
                .and_then(|id| id.trim().parse::<f64>().ok())
                == Some(item_id)
        })
    }
}

async fn reply<T: Packet>(replies: &Packets<T>, mut answer: impl FnMut(T) -> Option<bool>) -> bool {
    let deadline = Instant::now() + REPLY_TIMEOUT;
    while let Ok(reply) = timeout_at(deadline, replies.next()).await {
        if let Some(result) = reply.ok().and_then(&mut answer) {
            return result;
        }
    }
    false
}

fn own_uid(loader: &impl Loader) -> Result<f64, CallError> {
    Ok(f64::from(bridge::player::get_user_id(loader)?))
}

async fn ready_item(loader: &impl Loader, query: &ItemQuery) -> Result<Option<Item>, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(None);
    }
    item(loader, query)
}

pub(crate) async fn equip(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    query: &ItemQuery,
    wear: bool,
) -> Result<bool, CallError> {
    let Some(item) = ready_item(loader, query).await? else {
        return Ok(false);
    };
    // The client marks a consumable equipped before seia loads its action and keeps the mark when
    // seia refuses, so only a fresh seia shows that a consumable is ready.
    if item.usable() || !item.equipped {
        let equipped = if item.usable() {
            equip_consumable(loader, &subscribe, &item).await?
        } else {
            equip_gear(loader, &subscribe, &item).await?
        };
        if !equipped {
            return Ok(false);
        }
    }
    if wear && item.wearable && !item.worn {
        return wear_item(loader, &subscribe, &item).await;
    }
    Ok(true)
}

async fn equip_gear(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    item: &Item,
) -> Result<bool, CallError> {
    if !wait_for_action(loader, GameAction::EquipItem).await? {
        return Ok(false);
    }
    let replies = Packets::<EquipItemPacket>::subscribe(subscribe);
    if !bridge::inventory::equip(loader, by_id(item.item_id))? {
        return Ok(false);
    }
    let uid = own_uid(loader)?;
    Ok(reply(&replies, |packet| {
        (packet.item_id == Some(item.item_id) && packet.uid == Some(uid)).then_some(true)
    })
    .await)
}

async fn equip_consumable(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    item: &Item,
) -> Result<bool, CallError> {
    let Some(action) = item.consumable_action() else {
        return Ok(false);
    };
    let replies = Packets::<SeiaPacket>::subscribe(subscribe);
    if !bridge::inventory::equip(loader, by_id(item.item_id))? {
        return Ok(false);
    }
    let loaded = reply(&replies, |packet| {
        (packet.o?.id == Some(action)).then_some(packet.accepted)
    })
    .await;
    // seia carries only the action id, which items with the same sMeta share.
    Ok(loaded
        && self::item(
            loader,
            &ItemQuery::ItemId {
                item_id: item.item_id,
            },
        )?
        .is_some_and(|now| now.equipped))
}

async fn wear_item(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    item: &Item,
) -> Result<bool, CallError> {
    if !wait_for_action(loader, GameAction::WearItem).await? {
        return Ok(false);
    }
    let replies = Packets::<WearItemPacket>::subscribe(subscribe);
    if !bridge::inventory::wear(loader, by_id(item.item_id))? {
        return Ok(false);
    }
    let uid = own_uid(loader)?;
    Ok(reply(&replies, |packet| {
        (packet.item_id == Some(item.item_id) && packet.uid == Some(uid)).then_some(packet.success)
    })
    .await)
}

pub(crate) async fn wear(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    query: &ItemQuery,
) -> Result<bool, CallError> {
    match ready_item(loader, query).await? {
        Some(item) if item.worn => Ok(true),
        Some(item) => wear_item(loader, &subscribe, &item).await,
        None => Ok(false),
    }
}

pub(crate) async fn unequip(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    query: &ItemQuery,
) -> Result<bool, CallError> {
    let Some(item) = ready_item(loader, query).await? else {
        return Ok(false);
    };
    if !item.equipped {
        return Ok(true);
    }
    if item.usable() {
        return bridge::inventory::unequip(loader, by_id(item.item_id));
    }
    if !wait_for_action(loader, GameAction::UnequipItem).await? {
        return Ok(false);
    }
    let replies = Packets::<UnequipItemPacket>::subscribe(&subscribe);
    if !bridge::inventory::unequip(loader, by_id(item.item_id))? {
        return Ok(false);
    }
    let uid = own_uid(loader)?;
    Ok(reply(&replies, |packet| {
        (packet.item_id == Some(item.item_id) && packet.uid == Some(uid)).then_some(true)
    })
    .await)
}

pub(crate) async fn use_item(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    query: &ItemQuery,
) -> Result<bool, CallError> {
    let Some(item) = ready_item(loader, query).await? else {
        return Ok(false);
    };
    let replies = Packets::<TurnInPacket>::subscribe(&subscribe);
    if !bridge::inventory::r#use(loader, by_id(item.item_id))? {
        return Ok(false);
    }
    Ok(reply(&replies, |packet| packet.took(item.item_id).then_some(true)).await)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;

    use super::*;
    use crate::{bridge::Events, domain::testing::FakeLoader};

    const UID: f64 = 7.0;

    fn text(value: &str) -> Value {
        Value::String(value.into())
    }

    fn item(id: f64, name: &str, category: &str) -> BTreeMap<String, Value> {
        [
            ("itemId", Value::Number(id)),
            ("charItemId", Value::Number(id * 10.0)),
            ("name", text(name)),
            ("quantity", Value::Number(1.0)),
            ("maxStack", Value::Number(1.0)),
            ("category", text(category)),
            ("slot", text("co")),
            ("link", text("")),
            ("meta", text("")),
            ("equipped", Value::Bool(false)),
            ("wearable", Value::Bool(false)),
            ("worn", Value::Bool(false)),
            ("member", Value::Bool(false)),
            ("coins", Value::Bool(false)),
            ("pool", text("bag")),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
    }

    fn with(mut row: BTreeMap<String, Value>, fields: &[(&str, Value)]) -> BTreeMap<String, Value> {
        for (key, value) in fields {
            row.insert((*key).to_owned(), value.clone());
        }
        row
    }

    fn armor() -> BTreeMap<String, Value> {
        with(
            item(5518.0, "Armor", "Armor"),
            &[("wearable", Value::Bool(true))],
        )
    }

    fn potion() -> BTreeMap<String, Value> {
        with(
            item(7539.0, "Potion", "Item"),
            &[("link", text("potion")), ("meta", text("214"))],
        )
    }

    fn packet(data: serde_json::Value) -> Event {
        Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": data }).to_string(),
        }
    }

    struct Bag {
        items: Vec<BTreeMap<String, Value>>,
        replies: Vec<(&'static str, Vec<serde_json::Value>)>,
        sent: Vec<&'static str>,
    }

    fn bag(
        items: Vec<BTreeMap<String, Value>>,
        replies: Vec<(&'static str, Vec<serde_json::Value>)>,
    ) -> (FakeLoader, Rc<Events>, Rc<RefCell<Bag>>) {
        let events = Rc::new(Events::default());
        let bag = Rc::new(RefCell::new(Bag {
            items,
            replies,
            sent: Vec::new(),
        }));
        let (publisher, state) = (events.clone(), bag.clone());
        let loader = FakeLoader::new(move |callback, args| {
            let mut bag = state.borrow_mut();
            let selected = |bag: &Bag| {
                let Value::Object(selector) = &args[0] else {
                    panic!("{callback} needs a selector: {args:?}");
                };
                bag.items
                    .iter()
                    .position(|row| match selector.get("itemId") {
                        Some(id) => row.get("itemId") == Some(id),
                        None => {
                            let name = |fields: &BTreeMap<String, Value>| match fields.get("name") {
                                Some(Value::String(name)) => name.to_lowercase(),
                                _ => String::new(),
                            };
                            name(row) == name(selector)
                        }
                    })
            };
            Ok(match callback {
                "auth.isLoggedIn"
                | "world.isLoaded"
                | "player.isLoaded"
                | "world.isActionAvailable" => Value::Bool(true),
                "player.getUserId" => Value::Number(UID),
                "inventory.getItem" => selected(&bag)
                    .map_or(Value::Null, |index| Value::Object(bag.items[index].clone())),
                "inventory.equip" | "inventory.unequip" | "inventory.wear" | "inventory.use" => {
                    bag.sent.push(callback);
                    let index = selected(&bag).expect("the item is in the bag");
                    if callback == "inventory.equip" {
                        bag.items[index].insert("equipped".into(), Value::Bool(true));
                    }
                    let replies = bag
                        .replies
                        .iter()
                        .position(|(trigger, _)| *trigger == callback)
                        .map(|index| bag.replies.remove(index).1)
                        .unwrap_or_default();
                    for reply in replies {
                        publisher.publish(&packet(reply));
                    }
                    Value::Bool(true)
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        (loader, events, bag)
    }

    fn by_name(name: &str) -> ItemQuery {
        ItemQuery::Name(name.into())
    }

    #[test]
    fn enhancement_and_class_rank_appear_only_on_items_that_have_them() {
        let potion = with(
            potion(),
            &[
                ("description", text("Restores health.")),
                ("quantity", Value::Number(3.0)),
                ("pool", text("misc")),
                ("enhId", Value::Number(0.0)),
            ],
        );
        assert_eq!(
            serde_json::to_value(item_from(Value::Object(potion)).unwrap()).unwrap(),
            json!({ "itemId": 7539.0, "charItemId": 75390.0, "name": "Potion",
                    "description": "Restores health.", "quantity": 3.0, "maxStack": 1.0,
                    "category": "Item", "slot": "co", "link": "potion", "meta": "214", "file": "",
                    "equipped": false, "wearable": false, "worn": false, "member": false,
                    "coins": false, "pool": "misc", "classRank": null, "enhancement": null })
        );

        let class = with(
            item(2.0, "Class", "Class"),
            &[
                ("slot", text("ar")),
                ("pool", text("class")),
                ("classRank", Value::Number(10.0)),
                ("enhId", Value::Number(55.0)),
                ("enhLevel", Value::Number(100.0)),
                ("enhPatternId", Value::Number(0.0)),
                ("enhProcId", Value::Number(3.0)),
                ("enhDps", Value::Number(100.0)),
                ("enhRange", Value::Number(10.0)),
                ("enhRarity", Value::Number(9.0)),
            ],
        );
        let class = serde_json::to_value(item_from(Value::Object(class)).unwrap()).unwrap();
        assert_eq!(class["classRank"], json!(10.0));
        assert_eq!(
            class["enhancement"],
            json!({ "id": 55.0, "level": 100.0, "patternId": null, "procId": 3.0,
                    "dps": 100.0, "range": 10.0, "rarity": 9.0 })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn equipping_gear_finishes_on_the_servers_reply_for_this_avatar() {
        let other = json!({ "cmd": "equipItem", "ItemID": 5518, "uid": 99, "strES": "co" });
        let ours = json!({ "cmd": "equipItem", "ItemID": 5518, "uid": UID, "strES": "co" });
        let (loader, events, _) = bag(
            vec![armor()],
            vec![("inventory.equip", vec![other.clone(), ours])],
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            equip(&loader, subscribe, &by_name("armor"), false).await,
            Ok(true)
        );

        let (loader, events, _) = bag(vec![armor()], vec![("inventory.equip", vec![other])]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            equip(&loader, subscribe, &by_name("Armor"), false).await,
            Ok(false)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn equipping_a_consumable_waits_until_its_action_loads() {
        let loaded =
            |id: u32, accepted: u8| json!({ "cmd": "seia", "iRes": accepted, "o": { "id": id } });
        let (loader, events, _) = bag(
            vec![potion()],
            vec![("inventory.equip", vec![loaded(400, 0), loaded(214, 1)])],
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            equip(&loader, subscribe, &by_name("Potion"), false).await,
            Ok(true)
        );

        let (loader, events, bag) = bag(
            vec![potion()],
            vec![
                ("inventory.equip", vec![loaded(214, 0)]),
                ("inventory.equip", vec![loaded(214, 1)]),
            ],
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            equip(&loader, &subscribe, &by_name("Potion"), false).await,
            Ok(false)
        );
        assert_eq!(
            equip(&loader, &subscribe, &by_name("Potion"), false).await,
            Ok(true)
        );
        assert_eq!(bag.borrow().sent, ["inventory.equip", "inventory.equip"]);
    }

    #[tokio::test(start_paused = true)]
    async fn wearing_is_opt_in_and_reports_the_servers_answer() {
        let equipped = json!({ "cmd": "equipItem", "ItemID": 5518, "uid": UID });
        let worn = |success: bool| json!({ "cmd": "wearItem", "ItemID": 5518, "uid": UID, "success": success, "msg": "" });
        for (wear, success, expected, sent) in [
            (false, true, true, vec!["inventory.equip"]),
            (true, true, true, vec!["inventory.equip", "inventory.wear"]),
            (
                true,
                false,
                false,
                vec!["inventory.equip", "inventory.wear"],
            ),
        ] {
            let (loader, events, bag) = bag(
                vec![armor()],
                vec![
                    ("inventory.equip", vec![equipped.clone()]),
                    ("inventory.wear", vec![worn(success)]),
                ],
            );
            let subscribe = |wants| events.subscribe(wants);
            assert_eq!(
                equip(&loader, subscribe, &by_name("Armor"), wear).await,
                Ok(expected),
                "wear {wear}, success {success}"
            );
            assert_eq!(bag.borrow().sent, sent);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn unequipping_gear_waits_for_the_server_but_a_consumable_does_not() {
        let helm = with(
            item(73297.0, "Helm", "Helm"),
            &[("slot", text("he")), ("equipped", Value::Bool(true))],
        );
        let removed = json!({ "cmd": "unequipItem", "ItemID": 73297, "bUnload": false, "uid": UID, "strES": "he" });
        let (loader, events, _) = bag(
            vec![helm.clone()],
            vec![("inventory.unequip", vec![removed])],
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            unequip(&loader, subscribe, &by_name("Helm")).await,
            Ok(true)
        );

        let (loader, events, _) = bag(vec![helm], Vec::new());
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            unequip(&loader, subscribe, &by_name("Helm")).await,
            Ok(false)
        );

        let equipped_potion = with(potion(), &[("equipped", Value::Bool(true))]);
        let (loader, events, bag) = bag(vec![equipped_potion, armor()], Vec::new());
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            unequip(&loader, &subscribe, &by_name("Potion")).await,
            Ok(true)
        );
        assert_eq!(
            unequip(&loader, &subscribe, &by_name("Armor")).await,
            Ok(true)
        );
        assert_eq!(bag.borrow().sent, ["inventory.unequip"]);
    }

    #[tokio::test(start_paused = true)]
    async fn using_an_item_finishes_when_the_server_takes_it() {
        let tonic = with(item(11683.0, "Tonic", "Item"), &[("link", text("tonic"))]);
        let took = |items: &str| json!({ "cmd": "turnIn", "sItems": items });
        let (loader, events, _) = bag(
            vec![tonic.clone()],
            vec![("inventory.use", vec![took("7539:1"), took("4:2,11683:1")])],
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            use_item(&loader, subscribe, &by_name("Tonic")).await,
            Ok(true)
        );

        let (loader, events, _) = bag(vec![tonic], vec![("inventory.use", vec![took("7539:1")])]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            use_item(&loader, subscribe, &by_name("Tonic")).await,
            Ok(false)
        );
    }
}
