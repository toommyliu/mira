use std::collections::HashMap;

use serde::Serialize;

use super::{
    CombatCooldown, bank,
    inventory::{self, Item, ItemCount, ItemQuery, TempItem},
    player,
    quests::{self, Cadence, Quest, QuestStatus, RewardKind},
    ready,
    shops::{self, Shop, ShopItem, ShopItemQuery},
    world::{self, Monster, MonsterDropsPacket, MonsterQuery},
};
use crate::bridge::{self, CallError, Event, Loader, Packets, Subscription};

const MAX_QUESTS: usize = 100;

pub(crate) type Reply<T> = async_channel::Sender<Result<T, CallError>>;

#[derive(Clone)]
pub(crate) enum Job {
    Read {
        source: Source,
        reply: Reply<Option<Snapshot>>,
    },
    Drops {
        mon_map_id: f64,
        reply: Reply<Option<Group>>,
    },
    Load {
        load: Load,
        reply: Reply<bool>,
    },
    Act {
        action: Action,
        reply: Reply<bool>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Shop,
    Quests,
    Inventory,
    TempInventory,
    Bank,
    Monsters,
}

impl Source {
    pub(crate) const ALL: [Source; 6] = [
        Source::Shop,
        Source::Quests,
        Source::Inventory,
        Source::TempInventory,
        Source::Bank,
        Source::Monsters,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Source::Shop => "Shop",
            Source::Quests => "Quests",
            Source::Inventory => "Inventory",
            Source::TempInventory => "Temp inventory",
            Source::Bank => "Bank",
            Source::Monsters => "Monsters",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Load {
    Shop(i32),
    Quests(Vec<i32>),
    HairShop(i32),
    ArmorCustomizer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadKind {
    Shop,
    Quests,
    HairShop,
    ArmorCustomizer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadProblem {
    Empty,
    NotAnId,
    TooMany,
}

impl LoadProblem {
    pub(crate) fn message(self) -> &'static str {
        match self {
            LoadProblem::Empty => "Type an ID",
            LoadProblem::NotAnId => "IDs are whole numbers above 0",
            LoadProblem::TooMany => "Load up to 100 quests at a time",
        }
    }
}

impl LoadKind {
    pub(crate) const ALL: [LoadKind; 4] = [
        LoadKind::Shop,
        LoadKind::Quests,
        LoadKind::HairShop,
        LoadKind::ArmorCustomizer,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            LoadKind::Shop => "Shop",
            LoadKind::Quests => "Quests",
            LoadKind::HairShop => "Hair shop",
            LoadKind::ArmorCustomizer => "Armor customizer",
        }
    }

    pub(crate) fn placeholder(self) -> &'static str {
        match self {
            LoadKind::Shop | LoadKind::HairShop => "Shop ID",
            LoadKind::Quests => "4567, 4570-4575",
            LoadKind::ArmorCustomizer => "",
        }
    }

    pub(crate) fn takes_ids(self) -> bool {
        self != LoadKind::ArmorCustomizer
    }

    pub(crate) fn parse(self, text: &str) -> Result<Load, LoadProblem> {
        let single = || match text.trim() {
            "" => Err(LoadProblem::Empty),
            id => positive(id).ok_or(LoadProblem::NotAnId),
        };
        Ok(match self {
            LoadKind::Shop => Load::Shop(single()?),
            LoadKind::HairShop => Load::HairShop(single()?),
            LoadKind::Quests => Load::Quests(quest_ids(text)?),
            LoadKind::ArmorCustomizer => Load::ArmorCustomizer,
        })
    }
}

fn positive(text: &str) -> Option<i32> {
    text.trim().parse().ok().filter(|id| *id > 0)
}

fn quest_ids(text: &str) -> Result<Vec<i32>, LoadProblem> {
    let mut ids = Vec::new();
    for part in text
        .split([',', ' '])
        .filter(|part| !part.trim().is_empty())
    {
        let (first, last) = match part.split_once('-') {
            Some((first, last)) => (first, last),
            None => (part, part),
        };
        let (Some(first), Some(last)) = (positive(first), positive(last)) else {
            return Err(LoadProblem::NotAnId);
        };
        let (first, last) = (first.min(last), first.max(last));
        if last - first >= MAX_QUESTS as i32 {
            return Err(LoadProblem::TooMany);
        }
        for id in first..=last {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.len() > MAX_QUESTS {
            return Err(LoadProblem::TooMany);
        }
    }
    if ids.is_empty() {
        return Err(LoadProblem::Empty);
    }
    Ok(ids)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    Buy { shop_item_id: i32 },
    Accept { quest_id: i32 },
    FakeComplete { quest_id: i32 },
    Equip { item_id: i32 },
    Deposit { item_id: i32 },
    Withdraw { item_id: i32 },
    Jump { cell: String },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Act {
    Game(Action),
    Register { quest_id: i32 },
    AddDrop { name: String },
}

impl Act {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Act::Game(Action::Buy { .. }) => "Buy",
            Act::Game(Action::Accept { .. }) => "Accept",
            Act::Game(Action::FakeComplete { .. }) => "Fake complete",
            Act::Game(Action::Equip { .. }) => "Equip",
            Act::Game(Action::Deposit { .. }) => "To bank",
            Act::Game(Action::Withdraw { .. }) => "To inventory",
            Act::Game(Action::Jump { .. }) => "Jump to cell",
            Act::Register { .. } => "Register quest",
            Act::AddDrop { .. } => "Add to drops",
        }
    }
}

impl Action {
    pub(crate) fn done(&self) -> &'static str {
        match self {
            Action::Buy { .. } => "Bought",
            Action::Accept { .. } => "Accepted",
            Action::FakeComplete { .. } => "Marked done on this client",
            Action::Equip { .. } => "Equipped",
            Action::Deposit { .. } => "Moved to the bank",
            Action::Withdraw { .. } => "Moved to the inventory",
            Action::Jump { .. } => "Jumped",
        }
    }

    pub(crate) fn failed(&self) -> &'static str {
        match self {
            Action::Buy { .. } => "Couldn't buy it",
            Action::Accept { .. } => "Couldn't accept it",
            Action::FakeComplete { .. } => "Couldn't mark it done",
            Action::Equip { .. } => "Couldn't equip it",
            Action::Deposit { .. } => "Couldn't move it to the bank",
            Action::Withdraw { .. } => "Couldn't move it to the inventory",
            Action::Jump { .. } => "Couldn't jump there",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Holding {
    pub(crate) inventory: i32,
    pub(crate) temp: i32,
    pub(crate) bank: i32,
}

impl Holding {
    pub(crate) fn total(self) -> i32 {
        self.inventory + self.temp + self.bank
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub(crate) name: String,
    pub(crate) quantity: i32,
    pub(crate) chance: Option<f64>,
    pub(crate) needed: bool,
    pub(crate) held: Holding,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Group {
    pub(crate) title: &'static str,
    pub(crate) lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub(crate) key: i64,
    pub(crate) name: String,
    pub(crate) id_label: &'static str,
    pub(crate) id: String,
    pub(crate) meta: String,
    pub(crate) badges: Vec<&'static str>,
    pub(crate) facts: Vec<(&'static str, String)>,
    pub(crate) groups: Vec<Group>,
    pub(crate) acts: Vec<Act>,
    pub(crate) drops_from: Option<f64>,
    pub(crate) json: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) heading: Option<String>,
    pub(crate) entries: Vec<Entry>,
}

pub(crate) async fn run(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
    job: Job,
) {
    match job {
        Job::Read { source, reply } => {
            let _ = reply.try_send(read(loader, source).await);
        }
        Job::Drops { mon_map_id, reply } => {
            let _ = reply.try_send(drops(loader, &subscribe, mon_map_id).await);
        }
        Job::Load { load, reply } => {
            let _ = reply.try_send(run_load(loader, load).await);
        }
        Job::Act { action, reply } => {
            let _ = reply.try_send(act(loader, subscribe, cooldown, action).await);
        }
    }
}

async fn run_load(loader: &impl Loader, load: Load) -> Result<bool, CallError> {
    match load {
        Load::Shop(shop_id) => shops::load(loader, shop_id).await,
        Load::Quests(quest_ids) => Ok(quests::load_batch(loader, &quest_ids, false)
            .await?
            .contains(&true)),
        Load::HairShop(shop_id) => shops::load_hair_shop(loader, shop_id).await,
        Load::ArmorCustomizer => shops::open_armor_customizer(loader).await,
    }
}

async fn act(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
    action: Action,
) -> Result<bool, CallError> {
    match action {
        Action::Buy { shop_item_id } => {
            shops::buy(
                loader,
                subscribe,
                &ShopItemQuery::ShopItemId { shop_item_id },
                1,
            )
            .await
        }
        Action::Accept { quest_id } => quests::accept(loader, subscribe, quest_id, true).await,
        Action::FakeComplete { quest_id } => quests::fake_complete(loader, quest_id).await,
        Action::Equip { item_id } => {
            inventory::equip(
                loader,
                subscribe,
                cooldown,
                &ItemQuery::ItemId { item_id },
                false,
            )
            .await
        }
        Action::Deposit { item_id } => {
            bank::deposit(loader, subscribe, &ItemQuery::ItemId { item_id }).await
        }
        Action::Withdraw { item_id } => {
            bank::withdraw(loader, subscribe, &ItemQuery::ItemId { item_id }).await
        }
        Action::Jump { cell } => player::jump(loader, &cell, None).await,
    }
}

async fn read(loader: &impl Loader, source: Source) -> Result<Option<Snapshot>, CallError> {
    if !ready(loader)? {
        return Ok(None);
    }
    Ok(Some(match source {
        Source::Shop => {
            let shop = shops::current(loader)?;
            let held = holdings(loader).await?;
            Snapshot {
                heading: shop.as_ref().map(shop_heading),
                entries: shop.map_or_else(Vec::new, |shop| {
                    shop.items
                        .iter()
                        .map(|item| shop_entry(item, &held))
                        .collect()
                }),
            }
        }
        Source::Quests => {
            let mut loaded = quests::loaded_quests(loader)?;
            loaded.sort_by_key(|(quest, _)| quest.id);
            let held = holdings(loader).await?;
            listing(
                loaded
                    .iter()
                    .map(|(quest, status)| quest_entry(quest, *status, &held)),
            )
        }
        Source::Inventory => listing(inventory::items(loader)?.iter().map(inventory_entry)),
        Source::TempInventory => listing(inventory::temp_items(loader)?.iter().map(temp_entry)),
        Source::Bank => listing(bank::items(loader).await?.iter().map(bank_entry)),
        Source::Monsters => {
            let here = bridge::player::get_cell(loader)?.unwrap_or_default();
            let mut monsters = world::monsters(loader, None)?;
            monsters.sort_by_key(|monster| !monster.cell.eq_ignore_ascii_case(&here));
            Snapshot {
                heading: (!here.is_empty()).then(|| format!("You're in {here}")),
                entries: monsters
                    .iter()
                    .map(|monster| monster_entry(monster, &here))
                    .collect(),
            }
        }
    }))
}

fn listing(entries: impl Iterator<Item = Entry>) -> Snapshot {
    Snapshot {
        heading: None,
        entries: entries.collect(),
    }
}

async fn drops(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    mon_map_id: f64,
) -> Result<Option<Group>, CallError> {
    let Some(drops) = world::monster_drops(
        loader,
        Packets::<MonsterDropsPacket>::subscribe(subscribe),
        &MonsterQuery::MonMapId { mon_map_id },
    )
    .await?
    else {
        return Ok(None);
    };
    let held = holdings(loader).await?;
    Ok(Some(Group {
        title: "Drops",
        lines: drops
            .iter()
            .map(|drop| Line {
                name: drop.name.clone(),
                quantity: drop.quantity,
                chance: drop.rate,
                needed: false,
                held: held.of(drop.item_id),
            })
            .collect(),
    }))
}

#[derive(Default)]
struct Holdings(HashMap<i32, Holding>);

impl Holdings {
    fn of(&self, item_id: i32) -> Holding {
        self.0.get(&item_id).copied().unwrap_or_default()
    }

    fn add(&mut self, item_id: i32, quantity: i32, slot: fn(&mut Holding) -> &mut i32) {
        *slot(self.0.entry(item_id).or_default()) += quantity;
    }
}

async fn holdings(loader: &impl Loader) -> Result<Holdings, CallError> {
    let mut held = Holdings::default();
    for item in inventory::items(loader)? {
        held.add(item.item_id, item.quantity, |holding| {
            &mut holding.inventory
        });
    }
    for item in inventory::temp_items(loader)? {
        held.add(item.item_id, item.quantity, |holding| &mut holding.temp);
    }
    if bridge::bank::is_loaded(loader)? {
        for item in bank::items(loader).await? {
            held.add(item.item_id, item.quantity, |holding| &mut holding.bank);
        }
    }
    Ok(held)
}

fn json(value: &impl Serialize) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn grouped(number: i32) -> String {
    let digits = number.unsigned_abs().to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    if number < 0 {
        text.insert(0, '-');
    }
    text
}

fn needs(requirements: &[ItemCount], held: &Holdings) -> Vec<Line> {
    requirements
        .iter()
        .map(|need| Line {
            name: need.name.clone(),
            quantity: need.quantity,
            chance: None,
            needed: true,
            held: held.of(need.item_id),
        })
        .collect()
}

fn shop_heading(shop: &Shop) -> String {
    let kind = [
        (shop.merge, "Merge"),
        (shop.limited, "Limited"),
        (shop.house, "House"),
    ]
    .into_iter()
    .filter_map(|(on, kind)| on.then_some(kind))
    .collect::<Vec<_>>();
    let mut heading = format!("{} · {}", shop.name, shop.id);
    for kind in kind {
        heading.push_str(" · ");
        heading.push_str(kind);
    }
    heading
}

fn price(item: &ShopItem) -> String {
    match (item.cost, item.coins) {
        (0, _) if !item.requirements.is_empty() => "Merge".into(),
        (0, _) => "Free".into(),
        (cost, true) => format!("{} AC", grouped(cost)),
        (cost, false) => format!("{} gold", grouped(cost)),
    }
}

fn shop_entry(item: &ShopItem, held: &Holdings) -> Entry {
    let mut facts = vec![
        ("Shop item ID", item.shop_item_id.to_string()),
        ("Category", item.category.clone()),
        ("Price", price(item)),
    ];
    if item.quantity > 1 {
        facts.push(("Per purchase", item.quantity.to_string()));
    }
    if item.level > 0 {
        facts.push(("Level", item.level.to_string()));
    }
    if let Some(faction) = &item.faction {
        facts.push((
            "Reputation",
            format!("{} · {}", faction.name, grouped(faction.rep)),
        ));
    }
    let owned = held.of(item.item_id).total();
    if owned > 0 {
        facts.push(("You have", grouped(owned)));
    }
    let groups = if item.requirements.is_empty() {
        Vec::new()
    } else {
        vec![Group {
            title: "Costs",
            lines: needs(&item.requirements, held),
        }]
    };
    Entry {
        key: i64::from(item.shop_item_id),
        name: item.name.clone(),
        id_label: "Item ID",
        id: item.item_id.to_string(),
        meta: price(item),
        badges: flags(item.coins, item.member),
        facts,
        groups,
        acts: vec![
            Act::Game(Action::Buy {
                shop_item_id: item.shop_item_id,
            }),
            Act::AddDrop {
                name: item.name.clone(),
            },
        ],
        drops_from: None,
        json: json(item),
    }
}

fn flags(coins: bool, member: bool) -> Vec<&'static str> {
    [(coins, "AC"), (member, "Member")]
        .into_iter()
        .filter_map(|(on, badge)| on.then_some(badge))
        .collect()
}

fn status_name(status: QuestStatus) -> &'static str {
    match status {
        QuestStatus::Available => "Available",
        QuestStatus::InProgress => "In progress",
        QuestStatus::Done => "Done",
        QuestStatus::Locked => "Locked",
    }
}

fn quest_entry(quest: &Quest, status: QuestStatus, held: &Holdings) -> Entry {
    let mut badges = Vec::new();
    match quest.cadence {
        Cadence::None => {}
        Cadence::Daily => badges.push("Daily"),
        Cadence::Weekly => badges.push("Weekly"),
        Cadence::Monthly => badges.push("Monthly"),
    }
    if quest.once {
        badges.push("Once");
    }
    let mut groups = Vec::new();
    if !quest.requirements.is_empty() {
        groups.push(Group {
            title: "Requirements",
            lines: needs(&quest.requirements, held),
        });
    }
    if !quest.rewards.is_empty() {
        groups.push(Group {
            title: "Rewards",
            lines: quest
                .rewards
                .iter()
                .map(|reward| Line {
                    name: reward.item.name.clone(),
                    quantity: reward.item.quantity,
                    chance: (reward.kind != RewardKind::Choice && reward.drop_chance < 100.0)
                        .then_some(reward.drop_chance),
                    needed: false,
                    held: held.of(reward.item.item_id),
                })
                .collect(),
        });
    }
    let mut acts = Vec::new();
    if status == QuestStatus::Available {
        acts.push(Act::Game(Action::Accept { quest_id: quest.id }));
    }
    acts.push(Act::Register { quest_id: quest.id });
    if status != QuestStatus::Done {
        acts.push(Act::Game(Action::FakeComplete { quest_id: quest.id }));
    }
    Entry {
        key: i64::from(quest.id),
        name: quest.name.clone(),
        id_label: "Quest ID",
        id: quest.id.to_string(),
        meta: match status {
            QuestStatus::Available => String::new(),
            status => status_name(status).into(),
        },
        badges,
        facts: vec![
            ("Quest ID", quest.id.to_string()),
            ("Status", status_name(status).into()),
        ],
        groups,
        acts,
        drops_from: None,
        json: json(quest),
    }
}

fn item_facts(item: &Item) -> Vec<(&'static str, String)> {
    let mut facts = vec![
        ("Item ID", item.item_id.to_string()),
        ("Category", item.category.clone()),
        (
            "Quantity",
            format!("{} of {}", grouped(item.quantity), grouped(item.max_stack)),
        ),
    ];
    if let Some(enhancement) = &item.enhancement {
        facts.push(("Enhancement", format!("Level {}", enhancement.level)));
    }
    facts
}

fn count(quantity: i32) -> String {
    if quantity > 1 {
        format!("×{}", grouped(quantity))
    } else {
        String::new()
    }
}

fn inventory_entry(item: &Item) -> Entry {
    let mut badges = Vec::new();
    if item.equipped {
        badges.push("Equipped");
    }
    badges.extend(flags(item.coins(), item.member()));
    let mut acts = Vec::new();
    if !item.equipped {
        acts.push(Act::Game(Action::Equip {
            item_id: item.item_id,
        }));
        acts.push(Act::Game(Action::Deposit {
            item_id: item.item_id,
        }));
    }
    acts.push(Act::AddDrop {
        name: item.name().to_owned(),
    });
    Entry {
        key: i64::from(item.item_id),
        name: item.name().to_owned(),
        id_label: "Item ID",
        id: item.item_id.to_string(),
        meta: count(item.quantity),
        badges,
        facts: item_facts(item),
        groups: Vec::new(),
        acts,
        drops_from: None,
        json: json(item),
    }
}

fn bank_entry(item: &Item) -> Entry {
    Entry {
        key: i64::from(item.item_id),
        name: item.name().to_owned(),
        id_label: "Item ID",
        id: item.item_id.to_string(),
        meta: count(item.quantity),
        badges: flags(item.coins(), item.member()),
        facts: item_facts(item),
        groups: Vec::new(),
        acts: vec![
            Act::Game(Action::Withdraw {
                item_id: item.item_id,
            }),
            Act::AddDrop {
                name: item.name().to_owned(),
            },
        ],
        drops_from: None,
        json: json(item),
    }
}

fn temp_entry(item: &TempItem) -> Entry {
    Entry {
        key: i64::from(item.item_id),
        name: item.name.clone(),
        id_label: "Item ID",
        id: item.item_id.to_string(),
        meta: format!("{}/{}", grouped(item.quantity), grouped(item.max_stack)),
        badges: Vec::new(),
        facts: vec![
            ("Item ID", item.item_id.to_string()),
            (
                "Quantity",
                format!("{} of {}", grouped(item.quantity), grouped(item.max_stack)),
            ),
        ],
        groups: Vec::new(),
        acts: vec![Act::AddDrop {
            name: item.name.clone(),
        }],
        drops_from: None,
        json: json(item),
    }
}

fn monster_entry(monster: &Monster, here: &str) -> Entry {
    let in_cell = monster.cell.eq_ignore_ascii_case(here);
    let mon_map_id = monster.mon_map_id;
    let mut facts = vec![
        ("MonMapID", mon_map_id.to_string()),
        ("Monster ID", monster.monster_id.to_string()),
        ("Cell", monster.cell.clone()),
        ("Level", monster.level.to_string()),
        (
            "Health",
            format!("{} of {}", monster.hp.max(0.0), monster.max_hp),
        ),
    ];
    if !monster.race.is_empty() {
        facts.push(("Race", monster.race.clone()));
    }
    Entry {
        key: mon_map_id as i64,
        name: monster.name.clone(),
        id_label: "MonMapID",
        id: mon_map_id.to_string(),
        meta: monster.cell.clone(),
        badges: if in_cell { vec!["Here"] } else { Vec::new() },
        facts,
        groups: Vec::new(),
        acts: if in_cell {
            Vec::new()
        } else {
            vec![Act::Game(Action::Jump {
                cell: monster.cell.clone(),
            })]
        },
        drops_from: in_cell.then_some(mon_map_id),
        json: json(monster),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        bridge::Value,
        domain::testing::{FakeLoader, value},
    };

    fn game(quests: serde_json::Value) -> FakeLoader {
        FakeLoader::new(move |callback, _| {
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "bank.isLoaded" => Value::Bool(true),
                "quests.getLoaded" => value(quests.clone()),
                "inventory.getItems" => value(json!([
                    { "itemId": 1, "name": "Bone Dust", "quantity": 3, "maxStack": 99 }
                ])),
                "tempInventory.getItems" => value(json!([
                    { "itemId": 2, "name": "Slime Sample", "quantity": 5, "maxStack": 5 }
                ])),
                "bank.getItems" => value(json!([
                    { "itemId": 1, "name": "Bone Dust", "quantity": 4, "maxStack": 99 }
                ])),
                callback => panic!("unexpected call {callback}"),
            })
        })
    }

    fn quest(id: i32, status: &str) -> serde_json::Value {
        json!({
            "id": id,
            "name": format!("Quest {id}"),
            "once": true,
            "status": status,
            "requirements": [
                { "itemId": 1, "name": "Bone Dust", "quantity": 10 },
                { "itemId": 2, "name": "Slime Sample", "quantity": 5 }
            ],
            "rewards": []
        })
    }

    #[tokio::test]
    async fn loaded_quests_list_in_id_order_with_what_you_hold() {
        let loader = game(json!([quest(30, "done"), quest(12, "available")]));
        let snapshot = read(&loader, Source::Quests).await.unwrap().unwrap();
        let ids: Vec<_> = snapshot.entries.iter().map(|entry| entry.key).collect();
        assert_eq!(ids, [12, 30]);
        let needs = &snapshot.entries[0].groups[0].lines;
        assert_eq!(
            needs
                .iter()
                .map(|line| (line.name.as_str(), line.held.total(), line.quantity))
                .collect::<Vec<_>>(),
            [("Bone Dust", 7, 10), ("Slime Sample", 5, 5)]
        );
        assert_eq!(needs[0].held.bank, 4);
    }

    #[tokio::test]
    async fn only_available_quests_offer_accept_and_done_ones_skip_fake_complete() {
        let loader = game(json!([
            quest(1, "available"),
            quest(2, "inProgress"),
            quest(3, "done")
        ]));
        let snapshot = read(&loader, Source::Quests).await.unwrap().unwrap();
        let labels = |index: usize| {
            snapshot.entries[index]
                .acts
                .iter()
                .map(Act::label)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(0), ["Accept", "Register quest", "Fake complete"]);
        assert_eq!(labels(1), ["Register quest", "Fake complete"]);
        assert_eq!(labels(2), ["Register quest"]);
    }

    #[test]
    fn quests_load_from_lists_and_ranges() {
        assert_eq!(
            LoadKind::Quests.parse("4567, 4570-4572 4569"),
            Ok(Load::Quests(vec![4567, 4570, 4571, 4572, 4569]))
        );
        assert_eq!(
            LoadKind::Quests.parse("12-10,11"),
            Ok(Load::Quests(vec![10, 11, 12]))
        );
    }

    #[test]
    fn a_quest_load_refuses_bad_ids_and_more_than_a_hundred() {
        assert_eq!(LoadKind::Quests.parse(" , "), Err(LoadProblem::Empty));
        assert_eq!(LoadKind::Quests.parse("12, abc"), Err(LoadProblem::NotAnId));
        assert_eq!(LoadKind::Quests.parse("0-5"), Err(LoadProblem::NotAnId));
        assert_eq!(LoadKind::Quests.parse("1-100").map(|_| ()), Ok(()));
        assert_eq!(LoadKind::Quests.parse("1-101"), Err(LoadProblem::TooMany));
        assert_eq!(
            LoadKind::Quests.parse("1-60, 100-160"),
            Err(LoadProblem::TooMany)
        );
    }

    #[test]
    fn shops_take_one_id_and_the_armor_customizer_takes_none() {
        assert_eq!(LoadKind::Shop.parse(" 1234 "), Ok(Load::Shop(1234)));
        assert_eq!(LoadKind::HairShop.parse("12"), Ok(Load::HairShop(12)));
        assert_eq!(LoadKind::Shop.parse("1, 2"), Err(LoadProblem::NotAnId));
        assert_eq!(LoadKind::Shop.parse(""), Err(LoadProblem::Empty));
        assert_eq!(
            LoadKind::ArmorCustomizer.parse("anything"),
            Ok(Load::ArmorCustomizer)
        );
    }

    #[test]
    fn large_numbers_get_thousands_separators() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(-1234567), "-1,234,567");
    }
}
