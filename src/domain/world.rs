use std::{cmp::Reverse, time::Duration};

use serde::{Deserialize, Deserializer, Serialize};

use super::{
    State, arrive,
    auras::{self, Aura},
    events::{GameEvent, Kind, Myself},
    ready, wait_for_action, wait_until_ready,
};
use crate::bridge::{
    self, CallError, Event, Format, GameAction, Loader, Packet, Packets, Subscription, Value,
    packets::{Sender, disconnected, raw, wire},
};

const LOAD_TIMEOUT: Duration = Duration::from_secs(20);
const DROPS_TIMEOUT: Duration = Duration::from_secs(5);
const MOVE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum MonsterQuery {
    Name(String),
    MonMapId {
        #[serde(rename = "monMapId")]
        mon_map_id: f64,
    },
}

impl MonsterQuery {
    fn matches(&self, monster: &Monster) -> bool {
        match self {
            MonsterQuery::Name(name) => {
                let name = name.trim();
                name == "*" || monster.name.trim().to_lowercase() == name.to_lowercase()
            }
            MonsterQuery::MonMapId { mon_map_id } => monster.mon_map_id == *mon_map_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Monster {
    pub(crate) mon_map_id: f64,
    pub(super) monster_id: f64,
    pub(super) name: String,
    pub(super) race: String,
    pub(crate) cell: String,
    pub(super) hp: f64,
    pub(super) max_hp: f64,
    pub(super) level: f64,
    pub(super) state: Option<State>,
    pub(super) auras: Vec<Aura>,
    #[serde(skip)]
    available: bool,
    pub(crate) aggressive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Drop {
    pub(super) item_id: i32,
    pub(super) name: String,
    pub(super) quantity: i32,
    pub(super) rate: Option<f64>,
    pub(super) boosted_rate: Option<f64>,
    pub(super) event_drop: bool,
    pub(super) quest_gated: bool,
    pub(super) required_quest_ids: Vec<i64>,
    #[serde(skip)]
    pub(super) category: String,
    #[serde(skip)]
    pub(super) member: bool,
    #[serde(skip)]
    pub(super) coins: bool,
    #[serde(skip)]
    pub(super) rarity: Option<i32>,
    #[serde(skip)]
    pub(super) max_stack: Option<i32>,
    #[serde(skip)]
    pub(super) varies: bool,
    #[serde(skip)]
    pub(super) gating_quests: Vec<String>,
    #[serde(skip)]
    pub(super) objective_of: Vec<String>,
}

pub(crate) fn cell_state(loader: &impl Loader) -> String {
    let mut state = Vec::new();
    for path in [
        "world.mapEntered",
        "world.mapLoadInProgress",
        "world.strFrame",
        "world.strPad",
        "world.bitWalk",
        "world.map.currentLabel",
        "world.map.isPlaying",
    ] {
        let value = match bridge::flash::get_game_object(loader, path) {
            Ok(value) => value.unwrap_or_else(|| "null".into()),
            Err(error) => format!("unreadable ({error})"),
        };
        state.push(format!("{path}={value}"));
    }
    for path in ["world.map.stage", "world.myAvatar.pMC.stage"] {
        let value = match bridge::flash::is_null(loader, path) {
            Ok(null) => (!null).to_string(),
            Err(error) => format!("unreadable ({error})"),
        };
        state.push(format!("{path}={value}"));
    }
    state.join(" ")
}

pub(crate) fn cells(loader: &impl Loader) -> Result<Vec<String>, CallError> {
    Ok(texts(bridge::world::get_cells(loader)?))
}

pub(crate) fn has_cell(loader: &impl Loader, cell: &str) -> Result<bool, CallError> {
    Ok(cells(loader)?.iter().any(|known| known == cell))
}

pub(crate) fn cell_pads(loader: &impl Loader) -> Result<Vec<String>, CallError> {
    Ok(texts(bridge::world::get_cell_pads(loader)?))
}

pub(crate) const PADS: [&str; 8] = [
    "Spawn", "Center", "Left", "Right", "Top", "Bottom", "Up", "Down",
];

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Location {
    pub(crate) map: String,
    pub(crate) room: Option<u32>,
    pub(crate) cell: String,
    pub(crate) pad: String,
    pub(crate) cells: Vec<String>,
    pub(crate) cell_pads: Vec<String>,
}

pub(crate) fn location(loader: &impl Loader) -> Result<Option<Location>, CallError> {
    if !ready(loader)? {
        return Ok(None);
    }
    Ok(Some(Location {
        map: bridge::player::get_map(loader)?.unwrap_or_default(),
        room: room_number(loader)?,
        cell: bridge::player::get_cell(loader)?.unwrap_or_default(),
        pad: bridge::player::get_pad(loader)?.unwrap_or_default(),
        cells: cells(loader)?,
        cell_pads: cell_pads(loader)?,
    }))
}

pub(crate) fn moves(event: &Event) -> bool {
    match event {
        Event::SmartFoxDebug { .. } => sent_move(event).is_some(),
        event => Kind::MapJoined.wants(event) || Kind::CellChanged.wants(event),
    }
}

enum Move {
    Map,
    Cell { cell: String, pad: String },
}

fn sent_move(event: &Event) -> Option<Move> {
    let packet = raw(event).filter(|packet| packet.from == Sender::Client)?;
    let mut fields = packet.text.split('%').skip(3);
    match (
        fields.next()?,
        fields.next()?,
        fields.next()?,
        fields.next()?,
    ) {
        ("moveToCell", _room, cell, pad) => Some(Move::Cell {
            cell: cell.to_owned(),
            pad: pad.to_owned(),
        }),
        _ => None,
    }
}

fn own_move(loader: &impl Loader, event: &Event) -> Option<Move> {
    if let Event::SmartFoxDebug { .. } = event {
        return sent_move(event);
    }
    let mut moved = Vec::new();
    Kind::MapJoined.read(event, &Myself(loader), &mut moved);
    Kind::CellChanged.read(event, &Myself(loader), &mut moved);
    moved.into_iter().find_map(|event| match event {
        GameEvent::MapJoined { .. } => Some(Move::Map),
        GameEvent::CellChanged {
            is_self: true,
            cell,
            pad,
            ..
        } => Some(Move::Cell { cell, pad }),
        _ => None,
    })
}

pub(crate) async fn track_location(
    loader: impl Loader,
    events: Subscription,
    report: impl Fn(Option<Location>),
) {
    loop {
        let read = match events.next().await {
            Ok(event) if disconnected(&event) => Ok(None),
            Ok(event) => match own_move(&loader, &event) {
                None => continue,
                Some(Move::Map) => map_loaded(&loader).await.and_then(|_| location(&loader)),
                Some(Move::Cell { cell, pad }) => arrive(&loader, MOVE_TIMEOUT, || {
                    Ok(bridge::world::is_cell_ready(&loader, &cell, Some(&pad))?.then_some(true))
                })
                .await
                .and_then(|_| location(&loader)),
            },
            Err(_) => location(&loader),
        };
        match read {
            Ok(location) => report(location),
            Err(error) => tracing::debug!(%error, "couldn't read the player's location"),
        }
    }
}

pub(crate) fn area_id(loader: &impl Loader) -> Result<f64, CallError> {
    Ok(bridge::flash::get_game_object(loader, "world.curRoom")?
        .and_then(|json| serde_json::from_str::<f64>(&json).ok())
        .unwrap_or(0.0))
}

pub(crate) fn room_number(loader: &impl Loader) -> Result<Option<u32>, CallError> {
    let area = bridge::flash::get_game_object(loader, "world.strAreaName")?
        .and_then(|json| serde_json::from_str::<String>(&json).ok());
    Ok(area.and_then(|area| {
        let (_, number) = area.rsplit_once('-')?;
        number.parse().ok()
    }))
}

pub(crate) fn monsters(
    loader: &impl Loader,
    cell: Option<&str>,
) -> Result<Vec<Monster>, CallError> {
    let Value::List(rows) = bridge::world::get_monsters(loader)? else {
        return Ok(Vec::new());
    };
    Ok(rows
        .into_iter()
        .filter_map(monster_from)
        .filter(|monster| cell.is_none_or(|cell| monster.cell.eq_ignore_ascii_case(cell)))
        .collect())
}

pub(crate) fn monster(
    loader: &impl Loader,
    query: &MonsterQuery,
) -> Result<Option<Monster>, CallError> {
    let cell = bridge::player::get_cell(loader)?.unwrap_or_default();
    Ok(monsters(loader, Some(&cell))?
        .into_iter()
        .find(|monster| query.matches(monster)))
}

pub(crate) fn monster_available(
    loader: &impl Loader,
    query: &MonsterQuery,
) -> Result<bool, CallError> {
    Ok(available_mon_map_id(loader, query)?.is_some())
}

pub(crate) fn available_mon_map_id(
    loader: &impl Loader,
    query: &MonsterQuery,
) -> Result<Option<f64>, CallError> {
    first_available_mon_map_id(loader, std::slice::from_ref(query), |_| true)
}

pub(crate) fn first_available_mon_map_id(
    loader: &impl Loader,
    queries: &[MonsterQuery],
    keep: impl Fn(f64) -> bool,
) -> Result<Option<f64>, CallError> {
    if queries.is_empty() {
        return Ok(None);
    }
    let cell = bridge::player::get_cell(loader)?.unwrap_or_default();
    let monsters = monsters(loader, Some(&cell))?;
    Ok(queries.iter().find_map(|query| {
        monsters
            .iter()
            .find(|monster| monster.available && query.matches(monster) && keep(monster.mon_map_id))
            .map(|monster| monster.mon_map_id)
    }))
}

pub(crate) fn hunt_target(
    loader: &impl Loader,
    here: &str,
    query: &MonsterQuery,
    most_matches: bool,
) -> Result<Option<Monster>, CallError> {
    let mut alive: Vec<Monster> = monsters(loader, None)?
        .into_iter()
        .filter(|monster| {
            !monster.cell.is_empty()
                && matches!(monster.state, Some(State::Idle | State::Combat))
                && query.matches(monster)
        })
        .collect();
    alive.sort_by_key(|monster| !monster.cell.eq_ignore_ascii_case(here));
    if !most_matches {
        return Ok(alive.into_iter().next());
    }
    let crowd = |cell: &str| {
        alive
            .iter()
            .filter(|monster| monster.cell.eq_ignore_ascii_case(cell))
            .count()
    };
    Ok(alive
        .iter()
        .min_by_key(|monster| Reverse(crowd(&monster.cell)))
        .cloned())
}

pub(crate) async fn monster_drops(
    loader: &impl Loader,
    replies: Packets<MonsterDropsPacket>,
    query: &MonsterQuery,
) -> Result<Option<Vec<Drop>>, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(None);
    }
    let Some(monster) = monster(loader, query)? else {
        return Ok(None);
    };
    replies.clear();
    if !bridge::world::request_monster_drops(loader, monster.mon_map_id)? {
        return Ok(None);
    }
    Ok(replies
        .answer(DROPS_TIMEOUT, |packet| {
            (packet.mon_map_id == Some(monster.mon_map_id)).then(|| {
                packet
                    .items
                    .into_iter()
                    .filter_map(DropItem::into_drop)
                    .collect()
            })
        })
        .await)
}

pub(crate) async fn set_spawn_point(
    loader: &impl Loader,
    cell: Option<&str>,
    pad: Option<&str>,
) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    bridge::world::set_spawn_point(loader, cell, pad)?;
    Ok(true)
}

pub(crate) async fn get_map_item(loader: &impl Loader, item_id: i32) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? || !wait_for_action(loader, GameAction::GetMapItem).await? {
        return Ok(false);
    }
    bridge::world::get_map_item(loader, item_id)?;
    Ok(true)
}

pub(crate) async fn load_swf(loader: &impl Loader, swf: &str) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    bridge::world::load_swf(loader, swf)?;
    let loaded = arrive(loader, LOAD_TIMEOUT, || Ok(ready(loader)?.then_some(true))).await?;
    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!(swf, loaded, state = %cell_state(loader), "loadSwf finished waiting");
    }
    Ok(loaded)
}

pub(crate) async fn reload(loader: &impl Loader) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    bridge::world::reload(loader)?;
    map_loaded(loader).await
}

async fn map_loaded(loader: &impl Loader) -> Result<bool, CallError> {
    arrive(loader, LOAD_TIMEOUT, || {
        if !bridge::world::is_loaded(loader)? {
            return Ok(None);
        }
        let cell = bridge::player::get_cell(loader)?.unwrap_or_default();
        Ok((bridge::world::is_cell_ready(loader, &cell, None)?
            && bridge::player::is_loaded(loader)?)
        .then_some(true))
    })
    .await
}

fn texts(value: Value) -> Vec<String> {
    let Value::List(items) = value else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| match item {
            Value::String(text) => Some(text),
            _ => None,
        })
        .collect()
}

pub(crate) fn monster_from(value: Value) -> Option<Monster> {
    let Value::Object(mut fields) = value else {
        return None;
    };
    let auras = fields
        .remove("auras")
        .map_or_else(Vec::new, auras::from_rows);
    let number = |key: &str| match fields.get(key) {
        Some(Value::Number(number)) if number.is_finite() => *number,
        _ => 0.0,
    };
    let text = |key: &str| match fields.get(key) {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    };
    let mon_map_id = number("monMapId");
    (mon_map_id > 0.0).then(|| Monster {
        mon_map_id,
        monster_id: number("monsterId"),
        name: text("name"),
        race: text("race"),
        cell: text("cell"),
        hp: number("hp"),
        max_hp: number("maxHp"),
        level: number("level"),
        state: State::from_game(number("state") as i32),
        auras,
        available: matches!(fields.get("available"), Some(Value::Bool(true))),
        aggressive: matches!(fields.get("aggressive"), Some(Value::Bool(true))),
    })
}

#[derive(Deserialize)]
pub(crate) struct EventPacket {
    pub(crate) args: EventArgs,
}

#[derive(Deserialize)]
pub(crate) struct EventArgs {
    #[serde(rename = "zoneSet", default)]
    pub(crate) zone_set: String,
}

impl Packet for EventPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "event";
}

#[derive(Deserialize)]
pub(crate) struct RespawnMonLine {
    _cmd: String,
    _room: serde::de::IgnoredAny,
    #[serde(deserialize_with = "id_list")]
    pub(crate) ids: Vec<i32>,
}

impl Packet for RespawnMonLine {
    const FORMAT: Format = Format::Str;
    const CMD: &'static str = "respawnMon";
}

fn id_list<'de, D: Deserializer<'de>>(input: D) -> Result<Vec<i32>, D::Error> {
    let ids = match serde_json::Value::deserialize(input)? {
        serde_json::Value::String(ids) => ids,
        serde_json::Value::Number(id) => id.to_string(),
        _ => String::new(),
    };
    Ok(ids
        .split(',')
        .filter_map(|id| id.trim().parse().ok())
        .collect())
}

#[derive(Deserialize)]
pub(crate) struct MonsterDropsPacket {
    #[serde(rename = "MonMapID", default, deserialize_with = "wire::number")]
    mon_map_id: Option<f64>,
    #[serde(default, deserialize_with = "wire::values")]
    items: Vec<DropItem>,
}

impl Packet for MonsterDropsPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "monsterDrops";
}

#[derive(Deserialize)]
struct DropItem {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::integer")]
    item_id: Option<i32>,
    #[serde(rename = "sName", default, deserialize_with = "wire::text")]
    name: String,
    #[serde(rename = "iQty", default, deserialize_with = "wire::integer")]
    quantity: Option<i32>,
    #[serde(rename = "bVaryQty", default, deserialize_with = "wire::flag")]
    varies: bool,
    #[serde(rename = "iRate", default, deserialize_with = "wire::number")]
    rate: Option<f64>,
    #[serde(rename = "iRateBoosted", default, deserialize_with = "wire::number")]
    boosted_rate: Option<f64>,
    #[serde(rename = "bEventDrop", default, deserialize_with = "wire::flag")]
    event_drop: bool,
    #[serde(rename = "bQuestGated", default, deserialize_with = "wire::flag")]
    quest_gated: bool,
    #[serde(rename = "questGated", default, deserialize_with = "names")]
    gating_quests: Vec<String>,
    #[serde(rename = "questObjective", default, deserialize_with = "names")]
    objective_of: Vec<String>,
    #[serde(rename = "iRty", default, deserialize_with = "wire::integer")]
    rarity: Option<i32>,
    #[serde(rename = "iStk", default, deserialize_with = "wire::integer")]
    max_stack: Option<i32>,
    #[serde(rename = "sReqQuests", default, deserialize_with = "wire::text")]
    required_quests: String,
    #[serde(rename = "sType", default, deserialize_with = "wire::text")]
    category: String,
    #[serde(rename = "bUpg", default, deserialize_with = "wire::flag")]
    member: bool,
    #[serde(rename = "bCoins", default, deserialize_with = "wire::flag")]
    coins: bool,
}

fn names<'de, D: Deserializer<'de>>(input: D) -> Result<Vec<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(input)? {
        serde_json::Value::Array(names) => names
            .into_iter()
            .filter_map(|name| match name {
                serde_json::Value::String(name) if !name.trim().is_empty() => Some(name),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

impl DropItem {
    fn into_drop(self) -> Option<Drop> {
        let item_id = self.item_id.filter(|id| *id > 0)?;
        let quantity = self.quantity.unwrap_or(1);
        let divisor = if self.varies {
            f64::from(quantity.max(1))
        } else {
            1.0
        };
        let mut required_quest_ids: Vec<i64> = Vec::new();
        for part in self.required_quests.split(',') {
            if let Ok(id) = part.trim().parse::<i64>()
                && id > 0
                && !required_quest_ids.contains(&id)
            {
                required_quest_ids.push(id);
            }
        }
        Some(Drop {
            item_id,
            name: self.name,
            quantity,
            rate: self.rate.map(|rate| (rate / divisor).min(100.0)),
            boosted_rate: self.boosted_rate.map(|rate| (rate / divisor).min(100.0)),
            event_drop: self.event_drop,
            quest_gated: self.quest_gated
                || !self.gating_quests.is_empty()
                || !required_quest_ids.is_empty(),
            required_quest_ids,
            category: self.category,
            member: self.member,
            coins: self.coins,
            rarity: self.rarity.filter(|rarity| *rarity > 0),
            max_stack: self.max_stack.filter(|stack| *stack > 0),
            varies: self.varies,
            gating_quests: self.gating_quests,
            objective_of: self.objective_of,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;
    use tokio::task::LocalSet;

    use super::*;
    use crate::{
        bridge::{Event, Events},
        domain::testing::{FakeLoader, debug_logs},
    };

    fn monster_row(mon_map_id: f64, name: &str, cell: &str, state: f64, available: bool) -> Value {
        Value::Object(
            [
                ("monMapId", Value::Number(mon_map_id)),
                ("monsterId", Value::Number(mon_map_id + 100.0)),
                ("name", Value::String(name.into())),
                ("race", Value::String("Beast".into())),
                ("cell", Value::String(cell.into())),
                ("hp", Value::Number(if state == 0.0 { 0.0 } else { 900.0 })),
                ("maxHp", Value::Number(1000.0)),
                ("level", Value::Number(12.0)),
                ("state", Value::Number(state)),
                ("available", Value::Bool(available)),
                ("aggressive", Value::Bool(mon_map_id == 1.0)),
                (
                    "auras",
                    Value::List(if mon_map_id == 1.0 {
                        vec![Value::Object(
                            [("name".to_owned(), Value::String("Burn".into()))].into(),
                        )]
                    } else {
                        Vec::new()
                    }),
                ),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
        )
    }

    struct Scene {
        cell: &'static str,
        area: Option<&'static str>,
        room: Option<f64>,
        loading: u32,
        loads: u32,
        old_cell_survives: bool,
        drop_requests: Vec<f64>,
    }

    fn scene(
        scene: Scene,
        events: Rc<Events>,
        replies: Vec<serde_json::Value>,
    ) -> (FakeLoader, Rc<RefCell<Scene>>) {
        let scene = Rc::new(RefCell::new(scene));
        let state = scene.clone();
        let loader = FakeLoader::new(move |callback, args| {
            let mut scene = state.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn" => Value::Bool(true),
                "player.isLoaded" => Value::Bool(scene.loading == 0),
                "world.isLoaded" => {
                    let loaded = scene.loading == 0;
                    scene.loading = scene.loading.saturating_sub(1);
                    Value::Bool(loaded)
                }
                "world.isCellReady" => Value::Bool(scene.loading == 0 && scene.old_cell_survives),
                "player.getCell" => Value::String(scene.cell.into()),
                "world.getMonsters" => Value::List(vec![
                    monster_row(1.0, "Frogzard", "r2", 1.0, true),
                    monster_row(2.0, "Frogzard", "r2", 0.0, false),
                    monster_row(3.0, "Slime", "r3", 1.0, true),
                    monster_row(f64::NAN, "Broken", "r2", 1.0, true),
                ]),
                "flash.getGameObject" => match &args[0] {
                    Value::String(path) if path == "world.strAreaName" => scene
                        .area
                        .map_or(Value::Null, |area| Value::String(json!(area).to_string())),
                    Value::String(path) if path == "world.curRoom" => scene
                        .room
                        .map_or(Value::Null, |room| Value::String(room.to_string())),
                    other => panic!("unexpected path {other:?}"),
                },
                "world.requestMonsterDrops" => {
                    let [Value::Number(id)] = args else {
                        panic!("requestMonsterDrops needs an id: {args:?}");
                    };
                    scene.drop_requests.push(*id);
                    let events = events.clone();
                    let replies = replies.clone();
                    tokio::task::spawn_local(async move {
                        for reply in replies {
                            tokio::time::sleep(Duration::from_millis(300)).await;
                            events.publish(&Event::ExtensionResponse {
                                packet: reply.to_string(),
                            });
                        }
                    });
                    Value::Bool(true)
                }
                "world.loadSwf" | "world.reload" => {
                    scene.loads += 1;
                    scene.loading = 3;
                    Value::Undefined
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        (loader, scene)
    }

    fn quiet(cell: &'static str) -> Scene {
        Scene {
            cell,
            area: None,
            room: None,
            loading: 0,
            loads: 0,
            old_cell_survives: true,
            drop_requests: Vec::new(),
        }
    }

    fn drops_reply(mon_map_id: f64) -> serde_json::Value {
        json!({ "type": "json", "dataObj": {
            "cmd": "monsterDrops",
            "MonMapID": mon_map_id,
            "items": {
                "11": { "ItemID": 11, "sName": "Frogzard Scale", "iQty": 4, "bVaryQty": true,
                        "iRate": 200, "iRateBoosted": "300", "sReqQuests": "7, 3,7,x,0",
                        "sType": "Resource", "bUpg": 1, "bCoins": "0", "iRty": 16, "iStk": 100,
                        "questGated": ["Spin the Wheel"], "questObjective": ["Bounty Hunter"] },
                "12": { "ItemID": 12, "sName": "Frog Hat", "iQty": 1, "iRate": 2.5,
                        "bEventDrop": 1, "questGated": [] },
                "13": { "ItemID": 0 }
            }
        }})
    }

    #[test]
    fn monsters_cover_the_whole_map_and_filter_by_cell() {
        let (loader, _) = scene(quiet("r2"), Rc::default(), Vec::new());
        let all = monsters(&loader, None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(monsters(&loader, Some("R2")).unwrap().len(), 2);
        assert_eq!(
            serde_json::to_value(&all[0]).unwrap(),
            json!({ "monMapId": 1.0, "monsterId": 101.0, "name": "Frogzard", "race": "Beast",
                    "cell": "r2", "hp": 900.0, "maxHp": 1000.0, "level": 12.0, "state": "idle",
                    "aggressive": true,
                    "auras": [{ "name": "Burn", "value": null, "stack": 1,
                                "category": null, "icon": null, "persistent": false,
                                "duration": 0.0, "expiresAt": null }] })
        );
        assert_eq!(
            serde_json::to_value(&all[1]).unwrap()["state"],
            json!("dead")
        );
        assert_eq!(serde_json::to_value(&all[1]).unwrap()["auras"], json!([]));
    }

    #[test]
    fn queries_match_within_the_current_cell() {
        let (loader, _) = scene(quiet("r2"), Rc::default(), Vec::new());
        let id = |query: MonsterQuery| {
            monster(&loader, &query)
                .unwrap()
                .map(|monster| monster.mon_map_id)
        };
        assert_eq!(id(MonsterQuery::Name("  FROGZARD ".into())), Some(1.0));
        assert_eq!(id(MonsterQuery::Name("*".into())), Some(1.0));
        assert_eq!(id(MonsterQuery::MonMapId { mon_map_id: 2.0 }), Some(2.0));
        assert_eq!(id(MonsterQuery::Name("Slime".into())), None);
        let available = |query: MonsterQuery| monster_available(&loader, &query).unwrap();
        assert!(available(MonsterQuery::Name("frogzard".into())));
        assert!(!available(MonsterQuery::MonMapId { mon_map_id: 2.0 }));
        assert!(!available(MonsterQuery::Name("Slime".into())));
    }

    #[test]
    fn the_room_number_and_area_id_come_from_the_current_area() {
        for (area, expected) in [
            (Some("battleon-1234"), Some(1234)),
            (Some("battleon"), None),
            (Some("ultradage:7"), None),
            (None, None),
        ] {
            let (loader, _) = scene(
                Scene {
                    area,
                    ..quiet("Enter")
                },
                Rc::default(),
                Vec::new(),
            );
            assert_eq!(room_number(&loader).unwrap(), expected, "{area:?}");
        }
        let (loader, _) = scene(
            Scene {
                room: Some(42.0),
                ..quiet("Enter")
            },
            Rc::default(),
            Vec::new(),
        );
        assert_eq!(area_id(&loader).unwrap(), 42.0);
        let (loader, _) = scene(quiet("Enter"), Rc::default(), Vec::new());
        assert_eq!(area_id(&loader).unwrap(), 0.0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_location_follows_your_own_moves_until_the_connection_drops() {
        let place = Rc::new(RefCell::new(("battleon", "Enter", "Spawn")));
        let current = place.clone();
        let loader = FakeLoader::new(move |callback, _| {
            let (map, cell, pad) = *current.borrow();
            let texts = |texts: &[&str]| {
                Value::List(
                    texts
                        .iter()
                        .map(|text| Value::String((*text).into()))
                        .collect(),
                )
            };
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" | "world.isCellReady" => {
                    Value::Bool(true)
                }
                "flash.getGameObjectS" => Value::String(json!("player1").to_string()),
                "flash.getGameObject" => Value::String(json!(format!("{map}-1234")).to_string()),
                "player.getMap" => Value::String(map.into()),
                "player.getCell" => Value::String(cell.into()),
                "player.getPad" => Value::String(pad.into()),
                "world.getCells" if map == "battleon" => texts(&["Enter", "r2"]),
                "world.getCells" => texts(&["Enter", "Boss"]),
                "world.getCellPads" => texts(&["Spawn", "Left"]),
                other => panic!("unexpected call {other}"),
            })
        });
        let events = Rc::new(Events::default());
        let reports = Rc::new(RefCell::new(Vec::new()));
        let seen = reports.clone();
        LocalSet::new()
            .run_until(async {
                tokio::task::spawn_local(track_location(
                    loader,
                    events.subscribe(moves),
                    move |location| {
                        seen.borrow_mut().push(location.map(|location| {
                            (
                                location.map,
                                location.room,
                                location.cell,
                                location.pad,
                                location.cells,
                            )
                        }))
                    },
                ));
                let publish = |packet: serde_json::Value| {
                    events.publish(&Event::ExtensionResponse {
                        packet: packet.to_string(),
                    });
                    tokio::time::sleep(Duration::from_millis(50))
                };
                let moved = |name: &str, cell: &str, pad: &str| {
                    json!({ "type": "str", "dataObj": ["uotls", "-1", name, format!("strFrame:{cell},strPad:{pad}")] })
                };
                publish(moved("player2", "r2", "Left")).await;
                assert_eq!(*reports.borrow(), []);

                *place.borrow_mut() = ("battleon", "r2", "Left");
                publish(moved("player1", "r2", "Left")).await;
                *place.borrow_mut() = ("battleon", "Enter", "Right");
                events.publish(&Event::SmartFoxDebug {
                    line: "[Sending - STR]: %xt%zm%moveToCell%1%Enter%Right%, (len: 32)".into(),
                });
                tokio::time::sleep(Duration::from_millis(50)).await;
                *place.borrow_mut() = ("dage", "Enter", "Spawn");
                publish(json!({ "type": "json", "dataObj": { "cmd": "moveToArea", "strMapName": "dage" } }))
                    .await;
                events.publish(&Event::Connection {
                    status: "OnConnectionLost".into(),
                });
                tokio::time::sleep(Duration::from_millis(50)).await;

                let texts = |texts: &[&str]| texts.iter().map(|text| text.to_string()).collect();
                assert_eq!(
                    *reports.borrow(),
                    [
                        Some((
                            "battleon".into(),
                            Some(1234),
                            "r2".into(),
                            "Left".into(),
                            texts(&["Enter", "r2"])
                        )),
                        Some((
                            "battleon".into(),
                            Some(1234),
                            "Enter".into(),
                            "Right".into(),
                            texts(&["Enter", "r2"])
                        )),
                        Some((
                            "dage".into(),
                            Some(1234),
                            "Enter".into(),
                            "Spawn".into(),
                            texts(&["Enter", "Boss"])
                        )),
                        None,
                    ]
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn monster_drops_wait_for_that_monsters_reply() {
        LocalSet::new()
            .run_until(async {
                let events = Rc::new(Events::default());
                let (loader, _) = scene(
                    quiet("r2"),
                    events.clone(),
                    vec![drops_reply(9.0), drops_reply(1.0)],
                );
                let replies = Packets::subscribe(|wants| events.subscribe(wants));
                events.publish(&Event::ExtensionResponse {
                    packet:
                        json!({ "type": "json", "dataObj": { "cmd": "monsterDrops", "MonMapID": 1, "items": {} } })
                            .to_string(),
                });
                let drops = monster_drops(&loader, replies, &MonsterQuery::Name("Frogzard".into()))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(&drops).unwrap(),
                    json!([
                        { "itemId": 11, "name": "Frogzard Scale", "quantity": 4, "rate": 50.0,
                          "boostedRate": 75.0, "eventDrop": false, "questGated": true,
                          "requiredQuestIds": [7, 3] },
                        { "itemId": 12, "name": "Frog Hat", "quantity": 1, "rate": 2.5,
                          "boostedRate": null, "eventDrop": true, "questGated": false,
                          "requiredQuestIds": [] },
                    ])
                );
                assert_eq!(
                    drops
                        .iter()
                        .map(|drop| (drop.category.as_str(), drop.member, drop.coins))
                        .collect::<Vec<_>>(),
                    [("Resource", true, false), ("", false, false)]
                );
                assert_eq!(
                    drops
                        .iter()
                        .map(|drop| (
                            drop.rarity,
                            drop.max_stack,
                            drop.varies,
                            drop.gating_quests.clone(),
                            drop.objective_of.clone()
                        ))
                        .collect::<Vec<_>>(),
                    [
                        (
                            Some(16),
                            Some(100),
                            true,
                            vec!["Spin the Wheel".to_owned()],
                            vec!["Bounty Hunter".to_owned()]
                        ),
                        (None, None, false, Vec::new(), Vec::new()),
                    ]
                );

                let (loader, scene) = scene(quiet("r2"), events.clone(), Vec::new());
                let slime = MonsterQuery::Name("Slime".into());
                let frogzard = MonsterQuery::Name("Frogzard".into());
                let missing = monster_drops(
                    &loader,
                    Packets::subscribe(|wants| events.subscribe(wants)),
                    &slime,
                )
                .await;
                assert_eq!(missing.unwrap(), None);
                let unanswered = monster_drops(
                    &loader,
                    Packets::subscribe(|wants| events.subscribe(wants)),
                    &frogzard,
                )
                .await;
                assert_eq!(unanswered.unwrap(), None);
                assert_eq!(scene.borrow().drop_requests, [1.0]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn reloading_waits_until_the_map_and_cell_are_ready_again() {
        let (loader, scene) = scene(quiet("Enter"), Rc::default(), Vec::new());
        assert_eq!(reload(&loader).await, Ok(true));
        let scene = scene.borrow();
        assert_eq!((scene.loads, scene.loading), (1, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn loading_a_swf_waits_for_the_new_map_not_the_old_cell() {
        let (loader, scene) = scene(
            Scene {
                old_cell_survives: false,
                ..quiet("Enter")
            },
            Rc::default(),
            Vec::new(),
        );
        assert_eq!(
            load_swf(&loader, "ChiralValley/town-Mobius-21Feb14.swf").await,
            Ok(true)
        );
        let scene = scene.borrow();
        assert_eq!((scene.loads, scene.loading), (1, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_diagnostic_never_changes_a_failed_loads_result() {
        let _debug = debug_logs();
        let loaded = Rc::new(std::cell::Cell::new(true));
        let state = loaded.clone();
        let loader = FakeLoader::new(move |callback, _| match callback {
            "auth.isLoggedIn" => Ok(Value::Bool(true)),
            "world.isLoaded" | "player.isLoaded" => Ok(Value::Bool(state.get())),
            "world.loadSwf" => {
                state.set(false);
                Ok(Value::Undefined)
            }
            "flash.getGameObject" | "flash.isNull" => Err(CallError::Threw {
                callback,
                message: "the map is null".into(),
            }),
            other => panic!("unexpected call {other}"),
        });
        assert_eq!(load_swf(&loader, "missing.swf").await, Ok(false));
        assert!(loader.count("flash.getGameObject") > 0);
    }
}
