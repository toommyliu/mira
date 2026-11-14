use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{
    CombatCooldown, State,
    auras::Aura,
    bank, enhancements,
    inventory::{self, Item, ItemCount, ItemQuery, TempItem},
    player,
    quests::{self, Cadence, Quest, QuestStatus, RewardKind},
    ready,
    shops::{self, Shop, ShopItem, ShopItemQuery},
    world::{self, Drop, Monster, MonsterDropsPacket, MonsterQuery},
};
use crate::bridge::{self, CallError, Event, Loader, Packets, Subscription, packets::wire};

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
    Raw {
        source: Source,
        reply: Reply<Option<String>>,
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

    pub(crate) fn columns(self) -> [&'static str; 2] {
        match self {
            Source::Shop => ["Price", "Item ID"],
            Source::Quests => ["Status", "Quest ID"],
            Source::Inventory | Source::TempInventory | Source::Bank => ["Quantity", "Item ID"],
            Source::Monsters => ["Cell", "MonMapID"],
        }
    }

    fn game_path(self) -> Option<&'static str> {
        match self {
            Source::Shop => Some("world.shopinfo"),
            Source::Quests => Some("world.questTree"),
            Source::Inventory => Some("world.myAvatar.items"),
            Source::TempInventory => Some("world.myAvatar.tempitems"),
            Source::Bank => Some("world.bankinfo.BankArray"),
            Source::Monsters => None,
        }
    }

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
    RegisterDrop { name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Offer {
    pub(crate) act: Act,
    pub(crate) enabled: bool,
}

impl From<Act> for Offer {
    fn from(act: Act) -> Self {
        Self { act, enabled: true }
    }
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
            Act::Game(Action::Jump { .. }) => "Jump to",
            Act::Register { .. } => "Register quest",
            Act::RegisterDrop { .. } => "Register drop",
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
    pub(crate) item_id: Option<i32>,
    pub(crate) name: String,
    pub(crate) quantity: i32,
    pub(crate) chance: Option<f64>,
    pub(crate) needed: bool,
    pub(crate) held: Holding,
    pub(crate) varies: bool,
    pub(crate) facts: Vec<(&'static str, String)>,
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
    pub(crate) acts: Vec<Offer>,
    pub(crate) here: bool,
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
        Job::Raw { source, reply } => {
            let _ = reply.try_send(raw(loader, source));
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
        Source::Inventory => {
            let patterns = Patterns::read(loader)?;
            listing(
                inventory::items(loader)?
                    .iter()
                    .map(|item| inventory_entry(item, &patterns)),
            )
        }
        Source::TempInventory => listing(inventory::temp_items(loader)?.iter().map(temp_entry)),
        Source::Bank => {
            let patterns = Patterns::read(loader)?;
            listing(
                bank::items(loader)
                    .await?
                    .iter()
                    .map(|item| bank_entry(item, &patterns)),
            )
        }
        Source::Monsters => {
            let here = bridge::player::get_cell(loader)?.unwrap_or_default();
            let mut monsters = world::monsters(loader, None)?;
            monsters.sort_by_key(|monster| !monster.cell.eq_ignore_ascii_case(&here));
            Snapshot {
                heading: None,
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
            .map(|drop| drop_line(drop, held.of(drop.item_id)))
            .collect(),
    }))
}

fn aura_line(aura: &Aura) -> Line {
    let mut facts = Vec::new();
    if let Some(value) = aura.number() {
        facts.push(("Value", value.to_string()));
    }
    if let Some(category) = &aura.category {
        facts.push(("Category", category.clone()));
    }
    if aura.duration > 0.0 {
        facts.push(("Duration", format!("{}s", aura.duration)));
    }
    if aura.persistent {
        facts.push(("Tags", "Persistent".into()));
    }
    Line {
        item_id: None,
        name: aura.name().to_owned(),
        quantity: aura.stack(),
        chance: None,
        needed: false,
        held: Holding::default(),
        varies: false,
        facts,
    }
}

fn rarity(rarity: i32) -> Option<&'static str> {
    Some(match rarity {
        1 => "Enhancement +0",
        2 => "Enhancement +1",
        3 => "Enhancement +2",
        4 => "Enhancement +3",
        5 => "Enhancement +4",
        6 => "Enhancement +5",
        7 => "Enhancement +6",
        8 => "Enhancement +100",
        9 => "Enhancement +200",
        11 => "Common",
        12 => "Weird",
        13 => "Awesome",
        14 => "1% Drop",
        15 => "5% Drop",
        16 => "Boss Drop",
        17 => "Hidden Secret",
        18 => "Junk",
        19 => "Impossible",
        20 => "Artifact",
        21 => "Limited Time Drop",
        23 => "Crazy",
        24 => "Expensive",
        25 => "Placeholder",
        30 => "Rare",
        35 => "Epic",
        36 => "Champion",
        40 => "Verification Shop",
        50 => "Seasonal",
        55 => "Seasonal Rare",
        60 => "Event",
        65 => "Event Rare",
        67 => "Infinity",
        68 => "New Collection Chest",
        70 => "Limited Rare",
        75 => "Collector",
        80 => "Promo",
        81 => "Founder",
        88 => "Benevolent",
        90 => "Ultra Rare",
        91 => "Achievement Tracker",
        92 => "Upgrade Pack",
        93 => "Limited Quantity",
        94 => "Frostval Gifting",
        95 => "Super Ultra Rare",
        96 => "Kickstarter Backer",
        99 => "Custom Item",
        100 => "Legendary",
        _ => return None,
    })
}

fn drop_line(drop: &Drop, held: Holding) -> Line {
    let mut facts = Vec::new();
    if !drop.category.is_empty() {
        facts.push(("Category", drop.category.clone()));
    }
    if let Some(name) = drop.rarity.and_then(rarity) {
        facts.push(("Rarity", name.to_owned()));
    }
    if let Some(boosted) = drop
        .boosted_rate
        .filter(|boosted| Some(*boosted) != drop.rate)
    {
        facts.push(("With boost", percent(boosted)));
    }
    let have = match drop.max_stack {
        Some(stack) => format!("{}/{}", grouped(held.total()), grouped(stack)),
        None => grouped(held.total()),
    };
    facts.push((
        "Owned",
        match held.bank {
            0 => have,
            bank => format!("{have}, {} in bank", grouped(bank)),
        },
    ));
    let tags: Vec<&str> = [
        (drop.member, "Member"),
        (drop.coins, "AC"),
        (drop.event_drop, "Event"),
        (drop.quest_gated, "Quest gated"),
    ]
    .into_iter()
    .filter_map(|(on, tag)| on.then_some(tag))
    .collect();
    if !tags.is_empty() {
        facts.push(("Tags", tags.join(", ")));
    }
    if !drop.required_quest_ids.is_empty() {
        let ids: Vec<String> = drop
            .required_quest_ids
            .iter()
            .map(ToString::to_string)
            .collect();
        facts.push(("Needs quests", ids.join(", ")));
    }
    if !drop.gating_quests.is_empty() {
        facts.push(("Needs quest names", drop.gating_quests.join(", ")));
    }
    if !drop.objective_of.is_empty() {
        facts.push(("Objective for", drop.objective_of.join(", ")));
    }
    Line {
        item_id: Some(drop.item_id),
        name: drop.name.clone(),
        quantity: drop.quantity,
        chance: drop.rate,
        needed: false,
        held,
        varies: drop.varies && drop.quantity > 1,
        facts,
    }
}

pub(crate) fn percent(value: f64) -> String {
    format!("{}%", (value * 100.0).round() / 100.0)
}

fn raw(loader: &impl Loader, source: Source) -> Result<Option<String>, CallError> {
    if !ready(loader)? {
        return Ok(None);
    }
    let value = match source.game_path() {
        Some(path) => bridge::flash::get_game_object(loader, path)?
            .and_then(|json| serde_json::from_str(&json).ok()),
        None => serde_json::to_value(world::monsters(loader, None)?).ok(),
    };
    Ok(value
        .filter(|value: &serde_json::Value| !value.is_null())
        .and_then(|value| serde_json::to_string_pretty(&value).ok()))
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
            item_id: Some(need.item_id),
            name: need.name.clone(),
            quantity: need.quantity,
            chance: None,
            needed: true,
            held: held.of(need.item_id),
            varies: false,
            facts: Vec::new(),
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
        facts.push(("Owned", grouped(owned)));
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
            })
            .into(),
        ],
        here: false,
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
                    item_id: Some(reward.item.item_id),
                    name: reward.item.name.clone(),
                    quantity: reward.item.quantity,
                    chance: (reward.kind != RewardKind::Choice && reward.drop_chance < 100.0)
                        .then_some(reward.drop_chance),
                    needed: false,
                    held: held.of(reward.item.item_id),
                    varies: false,
                    facts: Vec::new(),
                })
                .collect(),
        });
    }
    let mut acts = Vec::new();
    if status == QuestStatus::Available {
        acts.push(Act::Game(Action::Accept { quest_id: quest.id }).into());
    }
    acts.push(Act::Register { quest_id: quest.id }.into());
    if status != QuestStatus::Done {
        acts.push(Act::Game(Action::FakeComplete { quest_id: quest.id }).into());
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
        here: false,
        drops_from: None,
        json: json(quest),
    }
}

#[derive(Default)]
struct Patterns(HashMap<i32, String>);

#[derive(Deserialize)]
struct Pattern {
    #[serde(rename = "ID", default, deserialize_with = "wire::integer")]
    id: Option<i32>,
    #[serde(rename = "sName", default, deserialize_with = "wire::text")]
    name: String,
}

impl Patterns {
    fn read(loader: &impl Loader) -> Result<Self, CallError> {
        let patterns: Vec<Pattern> = bridge::flash::get_game_object(loader, "world.enhp")?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Ok(Self(
            patterns
                .into_iter()
                .filter_map(|pattern| Some((pattern.id?, pattern.name)))
                .collect(),
        ))
    }

    fn name(&self, pattern_id: i32) -> String {
        self.0
            .get(&pattern_id)
            .cloned()
            .unwrap_or_else(|| format!("Pattern {pattern_id}"))
    }
}

// A class item's quantity is its class points.
fn quantity(item: &Item) -> i32 {
    if item.class_rank.is_some() {
        1
    } else {
        item.quantity
    }
}

fn item_facts(item: &Item, patterns: &Patterns) -> Vec<(&'static str, String)> {
    let mut facts = vec![
        ("Item ID", item.item_id.to_string()),
        ("Category", item.category.clone()),
        (
            "Quantity",
            format!("{}/{}", grouped(quantity(item)), grouped(item.max_stack)),
        ),
    ];
    if let Some(rank) = item.class_rank {
        facts.push(("Rank", rank.to_string()));
        facts.push(("Class points", grouped(item.quantity)));
    }
    if let Some(enhancement) = &item.enhancement {
        if let Some(pattern_id) = enhancement.pattern_id {
            facts.push(("Enhancement", patterns.name(pattern_id)));
        }
        if let Some(proc_id) = enhancement.proc_id {
            let special = enhancements::special_name(proc_id)
                .map_or_else(|| format!("Special {proc_id}"), str::to_owned);
            facts.push(("Special", special));
        }
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

fn inventory_entry(item: &Item, patterns: &Patterns) -> Entry {
    let mut badges = Vec::new();
    if item.equipped {
        badges.push("Equipped");
    }
    badges.extend(flags(item.coins(), item.member()));
    let mut acts = Vec::new();
    if !item.equipped {
        acts.push(
            Act::Game(Action::Equip {
                item_id: item.item_id,
            })
            .into(),
        );
        acts.push(
            Act::Game(Action::Deposit {
                item_id: item.item_id,
            })
            .into(),
        );
    }
    acts.push(
        Act::RegisterDrop {
            name: item.name().to_owned(),
        }
        .into(),
    );
    Entry {
        key: i64::from(item.item_id),
        name: item.name().to_owned(),
        id_label: "Item ID",
        id: item.item_id.to_string(),
        meta: count(quantity(item)),
        badges,
        facts: item_facts(item, patterns),
        groups: Vec::new(),
        acts,
        here: false,
        drops_from: None,
        json: json(item),
    }
}

fn bank_entry(item: &Item, patterns: &Patterns) -> Entry {
    Entry {
        key: i64::from(item.item_id),
        name: item.name().to_owned(),
        id_label: "Item ID",
        id: item.item_id.to_string(),
        meta: count(quantity(item)),
        badges: flags(item.coins(), item.member()),
        facts: item_facts(item, patterns),
        groups: Vec::new(),
        acts: vec![
            Act::Game(Action::Withdraw {
                item_id: item.item_id,
            })
            .into(),
            Act::RegisterDrop {
                name: item.name().to_owned(),
            }
            .into(),
        ],
        here: false,
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
                format!("{}/{}", grouped(item.quantity), grouped(item.max_stack)),
            ),
        ],
        groups: Vec::new(),
        acts: vec![
            Act::RegisterDrop {
                name: item.name.clone(),
            }
            .into(),
        ],
        here: false,
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
    ];
    if in_cell {
        facts.push((
            "Health",
            format!(
                "{}/{}",
                grouped(monster.hp.max(0.0) as i32),
                grouped(monster.max_hp as i32)
            ),
        ));
    }
    if !monster.race.is_empty() {
        facts.push(("Race", monster.race.clone()));
    }
    let mut badges = Vec::new();
    if in_cell && monster.state == Some(State::Combat) {
        badges.push("In combat");
    }
    if monster.aggressive {
        badges.push("Aggressive");
    }
    let groups = if monster.auras.is_empty() {
        Vec::new()
    } else {
        vec![Group {
            title: "Auras",
            lines: monster.auras.iter().map(aura_line).collect(),
        }]
    };
    Entry {
        key: mon_map_id as i64,
        name: monster.name.clone(),
        id_label: "MonMapID",
        id: mon_map_id.to_string(),
        meta: monster.cell.clone(),
        badges,
        facts,
        groups,
        acts: vec![Offer {
            act: Act::Game(Action::Jump {
                cell: monster.cell.clone(),
            }),
            enabled: !in_cell,
        }],
        here: in_cell,
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
        FakeLoader::new(move |callback, args| {
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "bank.isLoaded" => Value::Bool(true),
                "quests.getLoaded" => value(quests.clone()),
                "inventory.getItems" => value(json!([
                    { "itemId": 1, "name": "Bone Dust", "quantity": 3, "maxStack": 99 },
                    { "itemId": 10, "name": "Echo", "quantity": 302500, "maxStack": 1,
                      "category": "Class", "classRank": 10, "enhId": 5, "enhLevel": 100,
                      "enhPatternId": 9 },
                    { "itemId": 11, "name": "Blade of Awe", "quantity": 1, "maxStack": 1,
                      "category": "Sword", "enhId": 6, "enhLevel": 100, "enhPatternId": 10,
                      "enhProcId": 9 }
                ])),
                "flash.getGameObject" if args == [Value::String("world.enhp".into())] => {
                    Value::String(
                        json!([{ "ID": 9, "sName": "Lucky" }, { "ID": 10, "sName": "Forge" }])
                            .to_string(),
                    )
                }
                "tempInventory.getItems" => value(json!([
                    { "itemId": 2, "name": "Slime Sample", "quantity": 5, "maxStack": 5 }
                ])),
                "bank.getItems" => value(json!([
                    { "itemId": 1, "name": "Bone Dust", "quantity": 4, "maxStack": 99 }
                ])),
                "player.getCell" => Value::String("Enter".into()),
                "flash.getGameObject" if args == [Value::String("world.shopinfo".into())] => {
                    Value::String(
                        json!({ "ShopID": 1234, "sName": "Dragon Merge Shop",
                                "Location": "alchemy,alchemyacademy", "items": [] })
                        .to_string(),
                    )
                }
                "world.getMonsters" => value(json!([monster(4.0, "r3"), monster(3.0, "Enter")])),
                callback => panic!("unexpected call {callback}"),
            })
        })
    }

    fn monster(mon_map_id: f64, cell: &str) -> serde_json::Value {
        let fighting = cell == "Enter";
        json!({
            "monMapId": mon_map_id, "monsterId": 2045, "name": "Dragon", "race": "Dragonkin",
            "cell": cell, "hp": 800, "maxHp": 800, "level": 25,
            "state": if fighting { 2 } else { 1 }, "available": true, "aggressive": false,
            "auras": if fighting { json!([{ "name": "Burn", "stack": 2, "duration": 5 }]) } else { json!([]) }
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
                .map(|offer| offer.act.label())
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(0), ["Accept", "Register quest", "Fake complete"]);
        assert_eq!(labels(1), ["Register quest", "Fake complete"]);
        assert_eq!(labels(2), ["Register quest"]);
    }

    #[tokio::test]
    async fn monsters_in_your_cell_list_first_and_offer_jump_to_only_elsewhere() {
        let loader = game(json!([]));
        let snapshot = read(&loader, Source::Monsters).await.unwrap().unwrap();
        let offers: Vec<_> = snapshot
            .entries
            .iter()
            .map(|entry| (entry.key, entry.acts[0].act.label(), entry.acts[0].enabled))
            .collect();
        assert_eq!(offers, [(3, "Jump to", false), (4, "Jump to", true)]);
        assert_eq!(snapshot.entries[0].drops_from, Some(3.0));
        let here: Vec<_> = snapshot.entries.iter().map(|entry| entry.here).collect();
        assert_eq!((snapshot.heading, here), (None, vec![true, false]));
        let health = |index: usize| {
            snapshot.entries[index]
                .facts
                .iter()
                .any(|(label, _)| *label == "Health")
        };
        assert_eq!((health(0), health(1)), (true, false));
        assert_eq!(snapshot.entries[0].badges, ["In combat"]);
        let burn = &snapshot.entries[0].groups[0].lines[0];
        assert_eq!(
            (burn.name.as_str(), burn.quantity, burn.facts.clone()),
            ("Burn", 2, vec![("Duration", "5s".to_owned())])
        );
    }

    #[tokio::test]
    async fn classes_show_their_rank_and_gear_names_its_enhancement() {
        let loader = game(json!([]));
        let snapshot = read(&loader, Source::Inventory).await.unwrap().unwrap();
        let entry = |key: i64| {
            let entry = snapshot
                .entries
                .iter()
                .find(|entry| entry.key == key)
                .unwrap();
            let facts: Vec<_> = entry
                .facts
                .iter()
                .map(|(label, value)| (*label, value.as_str()))
                .collect();
            (entry.meta.clone(), facts)
        };
        assert_eq!(
            entry(10),
            (
                String::new(),
                vec![
                    ("Item ID", "10"),
                    ("Category", "Class"),
                    ("Quantity", "1/1"),
                    ("Rank", "10"),
                    ("Class points", "302,500"),
                    ("Enhancement", "Lucky"),
                ]
            )
        );
        assert_eq!(
            entry(11).1[3..],
            [("Enhancement", "Forge"), ("Special", "Valiance")]
        );
    }

    #[test]
    fn a_whole_shop_exports_as_the_game_holds_it() {
        let loader = game(json!([]));
        let text = raw(&loader, Source::Shop).unwrap().unwrap();
        let shop: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(shop["Location"], "alchemy,alchemyacademy");
        assert!(text.contains('\n'));
    }

    #[test]
    fn drop_details_hold_what_the_row_leaves_out() {
        let line = drop_line(
            &Drop {
                item_id: 11,
                name: "Frogzard Scale".into(),
                quantity: 4,
                rate: Some(25.0),
                boosted_rate: Some(37.5),
                event_drop: true,
                quest_gated: true,
                required_quest_ids: vec![4567],
                category: "Resource".into(),
                member: true,
                coins: false,
                rarity: Some(16),
                max_stack: Some(100),
                varies: true,
                gating_quests: vec!["Spin the Wheel".into()],
                objective_of: vec!["Bounty Hunter".into()],
            },
            Holding {
                inventory: 3,
                temp: 0,
                bank: 1,
            },
        );
        assert_eq!(
            (line.item_id, line.quantity, line.varies),
            (Some(11), 4, true)
        );
        assert_eq!(
            line.facts,
            [
                ("Category", "Resource".to_owned()),
                ("Rarity", "Boss Drop".to_owned()),
                ("With boost", "37.5%".to_owned()),
                ("Owned", "4/100, 1 in bank".to_owned()),
                ("Tags", "Member, Event, Quest gated".to_owned()),
                ("Needs quests", "4567".to_owned()),
                ("Needs quest names", "Spin the Wheel".to_owned()),
                ("Objective for", "Bounty Hunter".to_owned()),
            ]
        );
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
