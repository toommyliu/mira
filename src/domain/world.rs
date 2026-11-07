use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::{Instant, timeout_at};

use super::{State, arrive, ready, wait_for_action, wait_until_ready};
use crate::bridge::{self, CallError, Event, GameAction, Loader, Subscription, Value};

const LOAD_TIMEOUT: Duration = Duration::from_secs(20);
const DROPS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum MonsterQuery {
    Name(String),
    MapId {
        #[serde(rename = "mapId")]
        map_id: f64,
    },
}

impl MonsterQuery {
    fn matches(&self, monster: &Monster) -> bool {
        match self {
            MonsterQuery::Name(name) => {
                let name = name.trim();
                name == "*" || monster.name.trim().to_lowercase() == name.to_lowercase()
            }
            MonsterQuery::MapId { map_id } => monster.map_id == *map_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Monster {
    map_id: f64,
    monster_id: f64,
    name: String,
    race: String,
    cell: String,
    hp: f64,
    max_hp: f64,
    level: f64,
    state: Option<State>,
    #[serde(skip)]
    available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Drop {
    item_id: f64,
    name: String,
    quantity: f64,
    rate: Option<f64>,
    boosted_rate: Option<f64>,
    event_drop: bool,
    quest_gated: bool,
    required_quest_ids: Vec<i64>,
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
    let cell = bridge::player::get_cell(loader)?.unwrap_or_default();
    Ok(monsters(loader, Some(&cell))?
        .iter()
        .any(|monster| monster.available && query.matches(monster)))
}

pub(crate) fn dropped(event: &Event) -> bool {
    matches!(event, Event::ExtensionResponse { packet } if packet.contains("\"cmd\":\"monsterDrops\""))
}

pub(crate) async fn monster_drops(
    loader: &impl Loader,
    replies: Subscription,
    query: &MonsterQuery,
) -> Result<Option<Vec<Drop>>, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(None);
    }
    let Some(monster) = monster(loader, query)? else {
        return Ok(None);
    };
    replies.clear();
    if !bridge::world::request_monster_drops(loader, monster.map_id)? {
        return Ok(None);
    }
    let deadline = Instant::now() + DROPS_TIMEOUT;
    while let Ok(reply) = timeout_at(deadline, replies.next()).await {
        if let Ok(Event::ExtensionResponse { packet }) = reply
            && let Some(drops) = drops_for(&packet, monster.map_id)
        {
            return Ok(Some(drops));
        }
    }
    Ok(None)
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

fn monster_from(value: Value) -> Option<Monster> {
    let Value::Object(fields) = value else {
        return None;
    };
    let number = |key: &str| match fields.get(key) {
        Some(Value::Number(number)) if number.is_finite() => *number,
        _ => 0.0,
    };
    let text = |key: &str| match fields.get(key) {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    };
    let map_id = number("mapId");
    (map_id > 0.0).then(|| Monster {
        map_id,
        monster_id: number("monsterId"),
        name: text("name"),
        race: text("race"),
        cell: text("cell"),
        hp: number("hp"),
        max_hp: number("maxHp"),
        level: number("level"),
        state: State::from_game(number("state") as i32),
        available: matches!(fields.get("available"), Some(Value::Bool(true))),
    })
}

fn drops_for(packet: &str, map_id: f64) -> Option<Vec<Drop>> {
    let reply: serde_json::Value = serde_json::from_str(packet).ok()?;
    let data = reply.get("dataObj")?;
    if data.get("cmd")?.as_str()? != "monsterDrops" || wire_number(data.get("MonMapID")?)? != map_id
    {
        return None;
    }
    Some(
        data.get("items")
            .and_then(serde_json::Value::as_object)
            .map(|items| items.values().filter_map(drop_from).collect())
            .unwrap_or_default(),
    )
}

fn drop_from(item: &serde_json::Value) -> Option<Drop> {
    let item_id = wire_number(item.get("ItemID")?)?;
    if item_id <= 0.0 {
        return None;
    }
    let number = |key: &str| item.get(key).and_then(wire_number);
    let flag = |key: &str| item.get(key).is_some_and(wire_flag);
    let quantity = number("iQty").unwrap_or(1.0);
    let divisor = if flag("bVaryQty") {
        quantity.max(1.0)
    } else {
        1.0
    };
    let quest_names = item
        .get("questGated")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|names| !names.is_empty());
    let mut required_quest_ids: Vec<i64> = Vec::new();
    for part in item
        .get("sReqQuests")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .split(',')
    {
        if let Ok(id) = part.trim().parse::<i64>()
            && id > 0
            && !required_quest_ids.contains(&id)
        {
            required_quest_ids.push(id);
        }
    }
    Some(Drop {
        item_id,
        name: item
            .get("sName")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned(),
        quantity,
        rate: number("iRate").map(|rate| (rate / divisor).min(100.0)),
        boosted_rate: number("iRateBoosted").map(|rate| (rate / divisor).min(100.0)),
        event_drop: flag("bEventDrop"),
        quest_gated: flag("bQuestGated") || quest_names || !required_quest_ids.is_empty(),
        required_quest_ids,
    })
}

fn wire_number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
    .filter(|number: &f64| number.is_finite())
}

fn wire_flag(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        serde_json::Value::String(text) => matches!(text.trim(), "1" | "true"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;
    use tokio::task::LocalSet;
    use tracing_subscriber::util::SubscriberInitExt as _;

    use super::*;
    use crate::{bridge::Events, domain::testing::FakeLoader};

    fn monster_row(map_id: f64, name: &str, cell: &str, state: f64, available: bool) -> Value {
        Value::Object(
            [
                ("mapId", Value::Number(map_id)),
                ("monsterId", Value::Number(map_id + 100.0)),
                ("name", Value::String(name.into())),
                ("race", Value::String("Beast".into())),
                ("cell", Value::String(cell.into())),
                ("hp", Value::Number(if state == 0.0 { 0.0 } else { 900.0 })),
                ("maxHp", Value::Number(1000.0)),
                ("level", Value::Number(12.0)),
                ("state", Value::Number(state)),
                ("available", Value::Bool(available)),
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

    fn drops_reply(map_id: f64) -> serde_json::Value {
        json!({ "type": "json", "dataObj": {
            "cmd": "monsterDrops",
            "MonMapID": map_id,
            "items": {
                "11": { "ItemID": 11, "sName": "Frogzard Scale", "iQty": 4, "bVaryQty": true,
                        "iRate": 200, "iRateBoosted": "300", "sReqQuests": "7, 3,7,x,0" },
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
            json!({ "mapId": 1.0, "monsterId": 101.0, "name": "Frogzard", "race": "Beast",
                    "cell": "r2", "hp": 900.0, "maxHp": 1000.0, "level": 12.0, "state": "idle" })
        );
        assert_eq!(
            serde_json::to_value(&all[1]).unwrap()["state"],
            json!("dead")
        );
    }

    #[test]
    fn queries_match_within_the_current_cell() {
        let (loader, _) = scene(quiet("r2"), Rc::default(), Vec::new());
        let id = |query: MonsterQuery| {
            monster(&loader, &query)
                .unwrap()
                .map(|monster| monster.map_id)
        };
        assert_eq!(id(MonsterQuery::Name("  FROGZARD ".into())), Some(1.0));
        assert_eq!(id(MonsterQuery::Name("*".into())), Some(1.0));
        assert_eq!(id(MonsterQuery::MapId { map_id: 2.0 }), Some(2.0));
        assert_eq!(id(MonsterQuery::Name("Slime".into())), None);
        let available = |query: MonsterQuery| monster_available(&loader, &query).unwrap();
        assert!(available(MonsterQuery::Name("frogzard".into())));
        assert!(!available(MonsterQuery::MapId { map_id: 2.0 }));
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
    async fn monster_drops_wait_for_that_monsters_reply() {
        LocalSet::new()
            .run_until(async {
                let events = Rc::new(Events::default());
                let (loader, _) = scene(
                    quiet("r2"),
                    events.clone(),
                    vec![drops_reply(9.0), drops_reply(1.0)],
                );
                let replies = events.subscribe(dropped);
                events.publish(&Event::ExtensionResponse {
                    packet:
                        json!({ "dataObj": { "cmd": "monsterDrops", "MonMapID": 1, "items": {} } })
                            .to_string(),
                });
                let drops = monster_drops(&loader, replies, &MonsterQuery::Name("Frogzard".into()))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(&drops).unwrap(),
                    json!([
                        { "itemId": 11.0, "name": "Frogzard Scale", "quantity": 4.0, "rate": 50.0,
                          "boostedRate": 75.0, "eventDrop": false, "questGated": true,
                          "requiredQuestIds": [7, 3] },
                        { "itemId": 12.0, "name": "Frog Hat", "quantity": 1.0, "rate": 2.5,
                          "boostedRate": null, "eventDrop": true, "questGated": false,
                          "requiredQuestIds": [] },
                    ])
                );

                let (loader, scene) = scene(quiet("r2"), events.clone(), Vec::new());
                let slime = MonsterQuery::Name("Slime".into());
                let frogzard = MonsterQuery::Name("Frogzard".into());
                let missing = monster_drops(&loader, events.subscribe(dropped), &slime).await;
                assert_eq!(missing.unwrap(), None);
                let unanswered = monster_drops(&loader, events.subscribe(dropped), &frogzard).await;
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
        let _debug = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .set_default();
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
