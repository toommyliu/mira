use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};

use super::{
    CombatCooldown,
    combat::selector,
    enhancements::{self, Kind, Slot},
    player::Boost,
    retry_after_combat, wait_for_action, wait_until_ready,
};
use crate::bridge::{
    self, CallError, Event, Format, GameAction, Loader, Packet, Packets, Subscription, Value,
    packets::wire,
};

const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ItemQuery {
    Name(String),
    ItemId {
        #[serde(rename = "itemId")]
        item_id: i32,
    },
}

impl ItemQuery {
    pub(super) fn selector(&self) -> Value {
        match self {
            ItemQuery::Name(name) => selector("name", Value::String(name.trim().to_owned())),
            ItemQuery::ItemId { item_id } => by_id(*item_id),
        }
    }

    pub(crate) fn normalized(self) -> Option<Self> {
        match self {
            ItemQuery::Name(name) => {
                let name = name.trim();
                (!name.is_empty()).then(|| ItemQuery::Name(name.to_owned()))
            }
            ItemQuery::ItemId { item_id } => (item_id > 0).then_some(self),
        }
    }

    pub(crate) fn same(&self, other: &ItemQuery) -> bool {
        match (self, other) {
            (ItemQuery::Name(a), ItemQuery::Name(b)) => same_name(a, b),
            (ItemQuery::ItemId { item_id: a }, ItemQuery::ItemId { item_id: b }) => a == b,
            _ => false,
        }
    }
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

pub(super) fn by_id(item_id: i32) -> Value {
    selector("itemId", Value::Number(f64::from(item_id)))
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
    pub(super) item_id: i32,
    pub(super) char_item_id: Option<i64>,
    name: String,
    description: String,
    pub(crate) quantity: i32,
    max_stack: i32,
    category: String,
    slot: String,
    link: String,
    meta: String,
    boosts: Vec<GearBoost>,
    file: String,
    pub(crate) equipped: bool,
    wearable: bool,
    worn: bool,
    member: bool,
    coins: bool,
    pool: ItemPool,
    class_rank: Option<i32>,
    enhancement: Option<Enhancement>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Enhancement {
    id: i32,
    level: i32,
    pattern_id: Option<i32>,
    proc_id: Option<i32>,
    dps: Option<i32>,
    range: Option<i32>,
    rarity: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct GearBoost {
    #[serde(flatten)]
    effect: GearEffect,
    percent: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum GearEffect {
    Damage { against: DamageTarget },
    Gold,
    Xp,
    Rep,
    ClassPoints,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum DamageTarget {
    All,
    Undead,
    Human,
    Chaos,
    Dragonkin,
    Orc,
    Drakath,
    Elemental,
}

impl DamageTarget {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "all" => DamageTarget::All,
            "undead" => DamageTarget::Undead,
            "human" => DamageTarget::Human,
            "chaos" => DamageTarget::Chaos,
            "dragonkin" => DamageTarget::Dragonkin,
            "orc" => DamageTarget::Orc,
            "drakath" => DamageTarget::Drakath,
            "elemental" => DamageTarget::Elemental,
            _ => return None,
        })
    }
}

fn gear_boost(value: Value) -> Option<GearBoost> {
    let row = row(value)?;
    let effect = match row.text("kind").as_str() {
        "damage" => GearEffect::Damage {
            against: DamageTarget::named(&row.text("against"))?,
        },
        "gold" => GearEffect::Gold,
        "xp" => GearEffect::Xp,
        "rep" => GearEffect::Rep,
        "classPoints" => GearEffect::ClassPoints,
        _ => return None,
    };
    Some(GearBoost {
        effect,
        percent: row.positive("percent")?,
    })
}

impl Item {
    pub(crate) fn query(&self) -> ItemQuery {
        ItemQuery::ItemId {
            item_id: self.item_id,
        }
    }

    pub(crate) fn matches(&self, query: &ItemQuery) -> bool {
        match query {
            ItemQuery::Name(name) => same_name(&self.name, name),
            ItemQuery::ItemId { item_id } => self.item_id == *item_id,
        }
    }

    pub(crate) fn coins(&self) -> bool {
        self.coins
    }

    pub(crate) fn member(&self) -> bool {
        self.member
    }

    pub(crate) fn boost(&self) -> Option<Boost> {
        if !self.category.eq_ignore_ascii_case("ServerUse") {
            return None;
        }
        let link = self.link.to_lowercase();
        match link.split("::").next()?.trim() {
            "xpboost" => Some(Boost::Xp),
            "gboost" => Some(Boost::Gold),
            "repboost" => Some(Boost::Rep),
            "cpboost" => Some(Boost::ClassPoints),
            _ => None,
        }
    }

    fn usable(&self) -> bool {
        self.category == "Item"
    }

    fn consumable_action(&self) -> Option<i32> {
        let link = self.link.trim().to_lowercase();
        if !self.usable() || matches!(link.as_str(), "" | "none" | "elixir" | "tonic") {
            return None;
        }
        self.meta.trim().parse().ok()
    }

    pub(super) fn enhancement_slot(&self) -> Option<Slot> {
        Slot::of(&self.slot)
    }

    pub(super) fn enhancement_level(&self, kind: Kind) -> Option<i32> {
        self.enhancement
            .as_ref()
            .filter(|enhancement| {
                kind == Kind {
                    pattern_id: enhancement.pattern_id.unwrap_or(0),
                    proc_id: enhancement.proc_id.unwrap_or(0),
                }
            })
            .map(|enhancement| enhancement.level)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TempItem {
    item_id: i32,
    name: String,
    pub(crate) quantity: i32,
    max_stack: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ItemCount {
    item_id: i32,
    name: String,
    quantity: i32,
}

impl ItemCount {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

pub(super) struct Row(pub(super) BTreeMap<String, Value>);

impl Row {
    pub(super) fn number(&self, key: &str) -> Option<f64> {
        match self.0.get(key) {
            Some(Value::Number(number)) if number.is_finite() => Some(*number),
            _ => None,
        }
    }

    pub(super) fn text(&self, key: &str) -> String {
        match self.0.get(key) {
            Some(Value::String(text)) => text.clone(),
            _ => String::new(),
        }
    }

    pub(super) fn flag(&self, key: &str) -> bool {
        matches!(self.0.get(key), Some(Value::Bool(true)))
    }

    pub(super) fn integer<T: TryFrom<i64>>(&self, key: &str) -> Option<T> {
        wire::whole(self.number(key)?)
    }

    pub(super) fn positive<T: TryFrom<i64> + PartialOrd + Default>(&self, key: &str) -> Option<T> {
        self.integer(key).filter(|number| *number > T::default())
    }

    fn item_id(&self) -> Option<i32> {
        self.positive("itemId")
    }
}

pub(super) fn row(value: Value) -> Option<Row> {
    match value {
        Value::Object(fields) => Some(Row(fields)),
        _ => None,
    }
}

pub(super) fn list(fields: &mut BTreeMap<String, Value>, key: &str) -> Vec<Value> {
    match fields.remove(key) {
        Some(Value::List(values)) => values,
        _ => Vec::new(),
    }
}

pub(super) fn item_count(row: &Row) -> Option<ItemCount> {
    Some(ItemCount {
        item_id: row.item_id()?,
        name: row.text("name"),
        quantity: row.integer("quantity").unwrap_or(1),
    })
}

pub(super) fn item_from(value: Value) -> Option<Item> {
    let mut row = row(value)?;
    let boosts = list(&mut row.0, "boosts")
        .into_iter()
        .filter_map(gear_boost)
        .collect();
    Some(Item {
        item_id: row.item_id()?,
        char_item_id: row.positive("charItemId"),
        name: row.text("name"),
        description: row.text("description"),
        quantity: row.integer("quantity").unwrap_or(0),
        max_stack: row.integer("maxStack").unwrap_or(1),
        category: row.text("category"),
        slot: row.text("slot"),
        link: row.text("link"),
        meta: row.text("meta"),
        boosts,
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
        class_rank: row.integer("classRank"),
        enhancement: row.positive("enhId").map(|id| Enhancement {
            id,
            level: row.integer("enhLevel").unwrap_or(0),
            pattern_id: row.positive("enhPatternId"),
            proc_id: row.positive("enhProcId"),
            dps: row.integer("enhDps"),
            range: row.integer("enhRange"),
            rarity: row.integer("enhRarity"),
        }),
    })
}

fn temp_item_from(value: Value) -> Option<TempItem> {
    let row = row(value)?;
    Some(TempItem {
        item_id: row.item_id()?,
        name: row.text("name"),
        quantity: row.integer("quantity").unwrap_or(0),
        max_stack: row.integer("maxStack").unwrap_or(1),
    })
}

pub(super) fn rows<T>(value: Value, parse: fn(Value) -> Option<T>) -> Vec<T> {
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
pub(crate) struct AddItemsPacket {
    #[serde(default, deserialize_with = "wire::values")]
    pub(crate) items: Vec<AddedItem>,
}

impl Packet for AddItemsPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "addItems";
}

#[derive(Deserialize)]
pub(crate) struct AddedItem {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::integer")]
    pub(crate) item_id: Option<i32>,
    #[serde(rename = "sName", default, deserialize_with = "wire::text")]
    pub(crate) name: String,
    #[serde(rename = "iQty", default, deserialize_with = "wire::integer")]
    pub(crate) quantity: Option<i32>,
    #[serde(rename = "bTemp", default, deserialize_with = "wire::flag")]
    pub(crate) temp: bool,
}

#[derive(Deserialize)]
pub(crate) struct EquipItemPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::integer")]
    item_id: Option<i32>,
    #[serde(default, deserialize_with = "wire::integer")]
    uid: Option<i32>,
}

impl Packet for EquipItemPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "equipItem";
}

#[derive(Deserialize)]
pub(crate) struct UnequipItemPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::integer")]
    item_id: Option<i32>,
    #[serde(default, deserialize_with = "wire::integer")]
    uid: Option<i32>,
}

impl Packet for UnequipItemPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "unequipItem";
}

#[derive(Deserialize)]
pub(crate) struct WearItemPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::integer")]
    item_id: Option<i32>,
    #[serde(default, deserialize_with = "wire::integer")]
    uid: Option<i32>,
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
    #[serde(default, deserialize_with = "wire::integer")]
    id: Option<i32>,
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
    pub(crate) fn took(&self, item_id: i32) -> bool {
        self.items.split(',').any(|entry| {
            entry
                .split(':')
                .next()
                .and_then(|id| id.trim().parse::<i32>().ok())
                == Some(item_id)
        })
    }
}

pub(super) async fn reply<T: Packet>(
    replies: &Packets<T>,
    answer: impl FnMut(T) -> Option<bool>,
) -> bool {
    replies.answer(REPLY_TIMEOUT, answer).await.unwrap_or(false)
}

pub(super) async fn ready_item(
    loader: &impl Loader,
    query: &ItemQuery,
) -> Result<Option<Item>, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(None);
    }
    item(loader, query)
}

pub(crate) async fn equip(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
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
            equip_consumable(loader, &subscribe, cooldown, &item).await?
        } else {
            equip_gear(loader, &subscribe, cooldown, &item).await?
        };
        if !equipped {
            return Ok(false);
        }
    }
    if wear && item.wearable && !item.worn {
        return wear_item(loader, &subscribe, cooldown, &item).await;
    }
    Ok(true)
}

pub(crate) async fn equip_by_enhancement(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
    enhancement: &str,
    special: Option<&str>,
    slot: Option<Slot>,
    wear: bool,
) -> Result<bool, CallError> {
    if (slot.is_none() && enhancements::is_plain_forge(enhancement, special))
        || !wait_until_ready(loader).await?
    {
        return Ok(false);
    }
    let best = items(loader)?
        .into_iter()
        .filter_map(|item| {
            let item_slot = item.enhancement_slot()?;
            if slot.is_some_and(|slot| slot != item_slot) {
                return None;
            }
            let strategy = enhancements::resolve(item_slot, enhancement, special)?;
            let level = item.enhancement_level(strategy.kind)?;
            Some((item.item_id, level))
        })
        .min_by_key(|(_, level)| std::cmp::Reverse(*level));
    let Some((item_id, _)) = best else {
        return Ok(false);
    };
    equip(
        loader,
        subscribe,
        cooldown,
        &ItemQuery::ItemId { item_id },
        wear,
    )
    .await
}

async fn equip_gear(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
    item: &Item,
) -> Result<bool, CallError> {
    retry_after_combat(subscribe, cooldown, async || {
        if !wait_for_action(loader, GameAction::EquipItem).await? {
            return Ok(false);
        }
        let replies = Packets::<EquipItemPacket>::subscribe(subscribe);
        if !bridge::inventory::equip(loader, by_id(item.item_id))? {
            return Ok(false);
        }
        let uid = bridge::player::get_user_id(loader)?;
        Ok(reply(&replies, |packet| {
            (packet.item_id == Some(item.item_id) && packet.uid == Some(uid)).then_some(true)
        })
        .await)
    })
    .await
}

async fn equip_consumable(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
    item: &Item,
) -> Result<bool, CallError> {
    let Some(action) = item.consumable_action() else {
        return Ok(false);
    };
    retry_after_combat(subscribe, cooldown, async || {
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
    })
    .await
}

async fn wear_item(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
    item: &Item,
) -> Result<bool, CallError> {
    retry_after_combat(subscribe, cooldown, async || {
        if !wait_for_action(loader, GameAction::WearItem).await? {
            return Ok(false);
        }
        let replies = Packets::<WearItemPacket>::subscribe(subscribe);
        if !bridge::inventory::wear(loader, by_id(item.item_id))? {
            return Ok(false);
        }
        let uid = bridge::player::get_user_id(loader)?;
        Ok(reply(&replies, |packet| {
            (packet.item_id == Some(item.item_id) && packet.uid == Some(uid))
                .then_some(packet.success)
        })
        .await)
    })
    .await
}

pub(crate) async fn wear(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
    query: &ItemQuery,
) -> Result<bool, CallError> {
    match ready_item(loader, query).await? {
        Some(item) if item.worn => Ok(true),
        Some(item) => wear_item(loader, &subscribe, cooldown, &item).await,
        None => Ok(false),
    }
}

pub(crate) async fn unequip(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
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
    retry_after_combat(&subscribe, cooldown, async || {
        if !wait_for_action(loader, GameAction::UnequipItem).await? {
            return Ok(false);
        }
        let replies = Packets::<UnequipItemPacket>::subscribe(&subscribe);
        if !bridge::inventory::unequip(loader, by_id(item.item_id))? {
            return Ok(false);
        }
        let uid = bridge::player::get_user_id(loader)?;
        Ok(reply(&replies, |packet| {
            (packet.item_id == Some(item.item_id) && packet.uid == Some(uid)).then_some(true)
        })
        .await)
    })
    .await
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
    use crate::{
        bridge::Events,
        domain::{AFTER_COMBAT, State, testing::FakeLoader},
    };

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
        let format = if data.is_array() { "str" } else { "json" };
        Event::ExtensionResponse {
            packet: json!({ "type": format, "dataObj": data }).to_string(),
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
                "inventory.getItems" => {
                    Value::List(bag.items.iter().cloned().map(Value::Object).collect())
                }
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
            json!({ "itemId": 7539, "charItemId": 75390, "name": "Potion",
                    "description": "Restores health.", "quantity": 3, "maxStack": 1,
                    "category": "Item", "slot": "co", "link": "potion", "meta": "214", "boosts": [],
                    "file": "", "equipped": false, "wearable": false, "worn": false, "member": false,
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
        assert_eq!(class["classRank"], json!(10));
        assert_eq!(
            class["enhancement"],
            json!({ "id": 55, "level": 100, "patternId": null, "procId": 3,
                    "dps": 100, "range": 10, "rarity": 9 })
        );
    }

    #[test]
    fn gear_boosts_keep_known_kinds_and_drop_the_rest() {
        let boost = |fields: &[(&str, Value)]| {
            Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), value.clone()))
                    .collect(),
            )
        };
        let sword = with(
            item(4.0, "Sword", "Sword"),
            &[(
                "boosts",
                Value::List(vec![
                    boost(&[
                        ("kind", text("damage")),
                        ("against", text("undead")),
                        ("percent", Value::Number(75.0)),
                    ]),
                    boost(&[
                        ("kind", text("classPoints")),
                        ("percent", Value::Number(25.0)),
                    ]),
                    boost(&[
                        ("kind", text("damage")),
                        ("against", text("goblin")),
                        ("percent", Value::Number(10.0)),
                    ]),
                    boost(&[("kind", text("luck")), ("percent", Value::Number(10.0))]),
                    boost(&[("kind", text("gold")), ("percent", Value::Number(0.0))]),
                ]),
            )],
        );
        assert_eq!(
            serde_json::to_value(item_from(Value::Object(sword)).unwrap()).unwrap()["boosts"],
            json!([
                { "kind": "damage", "against": "undead", "percent": 75 },
                { "kind": "classPoints", "percent": 25 }
            ])
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
            equip(
                &loader,
                subscribe,
                &CombatCooldown::default(),
                &by_name("armor"),
                false
            )
            .await,
            Ok(true)
        );

        let (loader, events, _) = bag(
            vec![armor()],
            vec![("inventory.equip", vec![other.clone()])],
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            equip(
                &loader,
                subscribe,
                &CombatCooldown::default(),
                &by_name("Armor"),
                false
            )
            .await,
            Ok(false)
        );

        let too_soon = json!(["warning", "-1", "You cannot do that too soon after combat."]);
        let (loader, events, retried) = bag(
            vec![armor()],
            vec![
                ("inventory.equip", vec![too_soon.clone()]),
                ("inventory.equip", vec![too_soon.clone()]),
                (
                    "inventory.equip",
                    vec![json!({ "cmd": "equipItem", "ItemID": 5518, "uid": UID })],
                ),
            ],
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            equip(
                &loader,
                subscribe,
                &CombatCooldown::default(),
                &by_name("Armor"),
                false
            )
            .await,
            Ok(true)
        );
        assert_eq!(retried.borrow().sent.len(), 3);

        let (loader, events, _) = bag(
            vec![armor()],
            (0..60)
                .map(|_| ("inventory.equip", vec![too_soon.clone()]))
                .collect(),
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            equip(
                &loader,
                subscribe,
                &CombatCooldown::default(),
                &by_name("Armor"),
                false
            )
            .await,
            Ok(false)
        );

        for (states, waits) in [
            (&[State::Combat, State::Idle][..], true),
            (&[State::Idle][..], false),
        ] {
            let cooldown = CombatCooldown::default();
            for state in states {
                cooldown.observe(*state);
            }
            let (loader, events, sent) = bag(
                vec![armor()],
                vec![(
                    "inventory.equip",
                    vec![json!({ "cmd": "equipItem", "ItemID": 5518, "uid": UID })],
                )],
            );
            let subscribe = |wants| events.subscribe(wants);
            let start = tokio::time::Instant::now();
            assert_eq!(
                equip(&loader, subscribe, &cooldown, &by_name("Armor"), false).await,
                Ok(true)
            );
            assert_eq!(start.elapsed() >= AFTER_COMBAT, waits, "{states:?}");
            assert_eq!(sent.borrow().sent.len(), 1);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn equipping_by_enhancement_picks_the_highest_level_match() {
        let gear = |id: f64, slot: &str, level: f64, pattern_id: f64, proc_id: f64| {
            with(
                item(id, "Gear", "Gear"),
                &[
                    ("slot", text(slot)),
                    ("enhId", Value::Number(1.0)),
                    ("enhLevel", Value::Number(level)),
                    ("enhPatternId", Value::Number(pattern_id)),
                    ("enhProcId", Value::Number(proc_id)),
                ],
            )
        };
        let items = || {
            vec![
                gear(1.0, "Weapon", 80.0, 9.0, 0.0),
                gear(2.0, "Weapon", 100.0, 9.0, 0.0),
                gear(3.0, "he", 100.0, 10.0, 0.0),
                gear(4.0, "ba", 90.0, 10.0, 0.0),
                gear(5.0, "Weapon", 100.0, 9.0, 2.0),
                gear(6.0, "he", 100.0, 9.0, 0.0),
            ]
        };
        for (enhancement, special, slot, expected) in [
            ("lucky", None, None, Some(2)),
            ("lucky", None, Some(Slot::Helm), Some(6)),
            ("forge", None, Some(Slot::Cape), Some(4)),
            ("forge", Some("vim"), None, None),
            ("lucky", Some("spiral carve"), None, Some(5)),
            ("forge", None, None, None),
        ] {
            let replies = expected.map_or_else(Vec::new, |id: u32| {
                vec![(
                    "inventory.equip",
                    vec![json!({ "cmd": "equipItem", "ItemID": id, "uid": UID })],
                )]
            });
            let (loader, events, bag) = bag(items(), replies);
            let subscribe = |wants| events.subscribe(wants);
            assert_eq!(
                equip_by_enhancement(
                    &loader,
                    subscribe,
                    &CombatCooldown::default(),
                    enhancement,
                    special,
                    slot,
                    false
                )
                .await,
                Ok(expected.is_some()),
                "{enhancement} {special:?} {slot:?}"
            );
            let equipped: Vec<u32> = bag
                .borrow()
                .items
                .iter()
                .filter(|row| row.get("equipped") == Some(&Value::Bool(true)))
                .filter_map(|row| match row.get("itemId") {
                    Some(Value::Number(id)) => Some(*id as u32),
                    _ => None,
                })
                .collect();
            assert_eq!(equipped, expected.into_iter().collect::<Vec<_>>());
        }
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
            equip(
                &loader,
                subscribe,
                &CombatCooldown::default(),
                &by_name("Potion"),
                false
            )
            .await,
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
            equip(
                &loader,
                &subscribe,
                &CombatCooldown::default(),
                &by_name("Potion"),
                false
            )
            .await,
            Ok(false)
        );
        assert_eq!(
            equip(
                &loader,
                &subscribe,
                &CombatCooldown::default(),
                &by_name("Potion"),
                false
            )
            .await,
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
                equip(
                    &loader,
                    subscribe,
                    &CombatCooldown::default(),
                    &by_name("Armor"),
                    wear
                )
                .await,
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
            unequip(
                &loader,
                subscribe,
                &CombatCooldown::default(),
                &by_name("Helm")
            )
            .await,
            Ok(true)
        );

        let (loader, events, _) = bag(vec![helm], Vec::new());
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            unequip(
                &loader,
                subscribe,
                &CombatCooldown::default(),
                &by_name("Helm")
            )
            .await,
            Ok(false)
        );

        let equipped_potion = with(potion(), &[("equipped", Value::Bool(true))]);
        let (loader, events, bag) = bag(vec![equipped_potion, armor()], Vec::new());
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            unequip(
                &loader,
                &subscribe,
                &CombatCooldown::default(),
                &by_name("Potion")
            )
            .await,
            Ok(true)
        );
        assert_eq!(
            unequip(
                &loader,
                &subscribe,
                &CombatCooldown::default(),
                &by_name("Armor")
            )
            .await,
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
