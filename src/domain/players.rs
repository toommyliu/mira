use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize, de::IgnoredAny};

use super::{
    State,
    auras::{self, Aura},
    inventory::{row, rows},
};
use crate::bridge::{self, CallError, Format, Loader, Packet, Value, packets::wire};

pub(crate) struct PlayerUpdate {
    pub(crate) username: String,
    pub(crate) afk: Option<bool>,
    pub(crate) cell: Option<String>,
    pub(crate) pad: Option<String>,
    pub(crate) state: Option<State>,
    pub(crate) walk: Option<(f64, f64)>,
    pub(crate) joined: bool,
}

fn destination(x: Option<f64>, y: Option<f64>) -> Option<(f64, f64)> {
    match (x, y) {
        (Some(x), Some(y)) if (x, y) != (0.0, 0.0) => Some((x, y)),
        _ => None,
    }
}

#[derive(Deserialize)]
pub(crate) struct PlayerUpdatePacket {
    #[serde(rename = "unm", deserialize_with = "wire::text")]
    username: String,
    #[serde(rename = "o", default)]
    changes: Changes,
}

#[derive(Default, Deserialize)]
struct Changes {
    #[serde(default, deserialize_with = "flagged")]
    afk: Option<bool>,
    #[serde(rename = "strFrame")]
    cell: Option<String>,
    #[serde(rename = "strPad")]
    pad: Option<String>,
    #[serde(rename = "intState", default, deserialize_with = "wire::integer")]
    state: Option<i32>,
    #[serde(default, deserialize_with = "wire::number")]
    tx: Option<f64>,
    #[serde(default, deserialize_with = "wire::number")]
    ty: Option<f64>,
    #[serde(rename = "strUsername")]
    everything: Option<IgnoredAny>,
}

fn flagged<'de, D: Deserializer<'de>>(input: D) -> Result<Option<bool>, D::Error> {
    wire::flag(input).map(Some)
}

impl Packet for PlayerUpdatePacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "uotls";
}

impl From<PlayerUpdatePacket> for PlayerUpdate {
    fn from(packet: PlayerUpdatePacket) -> Self {
        PlayerUpdate {
            username: packet.username,
            afk: packet.changes.afk,
            cell: packet.changes.cell,
            pad: packet.changes.pad,
            state: packet.changes.state.and_then(State::from_game),
            walk: destination(packet.changes.tx, packet.changes.ty),
            joined: packet.changes.everything.is_some(),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct PlayerUpdateLine {
    _cmd: String,
    _room: String,
    username: String,
    changes: String,
}

impl Packet for PlayerUpdateLine {
    const FORMAT: Format = Format::Str;
    const CMD: &'static str = "uotls";
}

impl From<PlayerUpdateLine> for PlayerUpdate {
    fn from(line: PlayerUpdateLine) -> Self {
        let field = |key: &str| {
            line.changes
                .split(',')
                .find_map(|field| field.split_once(':').filter(|(name, _)| *name == key))
                .map(|(_, value)| value.trim().to_owned())
        };
        let afk = field("afk").map(|value| value == "true");
        let cell = field("strFrame");
        let pad = field("strPad");
        let state = field("intState")
            .and_then(|state| state.parse().ok())
            .and_then(State::from_game);
        let number = |key: &str| field(key).and_then(|value| value.parse().ok());
        PlayerUpdate {
            username: line.username,
            afk,
            cell,
            pad,
            state,
            walk: destination(number("tx"), number("ty")),
            joined: false,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct FightPlayersPacket {
    #[serde(rename = "p", default)]
    pub(crate) players: HashMap<String, Option<FightPlayer>>,
}

impl Packet for FightPlayersPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "ct";
}

#[derive(Deserialize)]
pub(crate) struct FightPlayer {
    #[serde(rename = "intState", default, deserialize_with = "wire::integer")]
    pub(crate) state: Option<i32>,
}

#[derive(Deserialize)]
pub(crate) struct ExitAreaLine {
    _cmd: String,
    _room: IgnoredAny,
    _id: IgnoredAny,
    #[serde(deserialize_with = "wire::text")]
    pub(crate) username: String,
}

impl Packet for ExitAreaLine {
    const FORMAT: Format = Format::Str;
    const CMD: &'static str = "exitArea";
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct Position {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoomPlayer {
    name: String,
    username: String,
    user_id: i32,
    pub(crate) position: Option<Position>,
    pub(crate) cell: String,
    pub(crate) pad: String,
    hp: f64,
    max_hp: f64,
    mp: f64,
    max_mp: f64,
    level: f64,
    state: Option<State>,
    afk: bool,
    auras: Vec<Aura>,
}

pub(crate) fn player_from(value: Value) -> Option<RoomPlayer> {
    let mut row = row(value)?;
    let auras = row
        .0
        .remove("auras")
        .map_or_else(Vec::new, auras::from_rows);
    let username = row.text("username");
    let user_id = row.positive("userId")?;
    if username.is_empty() {
        return None;
    }
    let number = |key: &str| row.number(key).unwrap_or(0.0);
    Some(RoomPlayer {
        name: row.text("name"),
        username,
        user_id,
        position: row
            .number("x")
            .zip(row.number("y"))
            .map(|(x, y)| Position { x, y }),
        cell: row.text("cell"),
        pad: row.text("pad"),
        hp: number("hp"),
        max_hp: number("maxHp"),
        mp: number("mp"),
        max_mp: number("maxMp"),
        level: number("level"),
        state: State::from_game(number("state") as i32),
        afk: row.flag("afk"),
        auras,
    })
}

pub(crate) fn all(loader: &impl Loader) -> Result<Vec<RoomPlayer>, CallError> {
    Ok(rows(bridge::players::get_all(loader)?, player_from))
}

pub(crate) fn get(loader: &impl Loader, username: &str) -> Result<Option<RoomPlayer>, CallError> {
    Ok(player_from(bridge::players::get(loader, username.trim())?))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::testing::FakeLoader;

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

    #[test]
    fn players_in_the_room_report_their_id_position_and_auras() {
        let loader = FakeLoader::new(|callback, args| {
            Ok(match callback {
                "players.getAll" => Value::List(vec![
                    object(&[
                        ("name", text("Player1")),
                        ("username", text("player1")),
                        ("userId", Value::Number(26_909.0)),
                        ("x", Value::Number(139.0)),
                        ("y", Value::Number(332.0)),
                        ("cell", text("Enter")),
                        ("pad", text("Spawn")),
                        ("hp", Value::Number(900.0)),
                        ("maxHp", Value::Number(1000.0)),
                        ("mp", Value::Number(50.0)),
                        ("maxMp", Value::Number(100.0)),
                        ("level", Value::Number(100.0)),
                        ("state", Value::Number(2.0)),
                        ("afk", Value::Bool(true)),
                        (
                            "auras",
                            Value::List(vec![object(&[("name", text("Burn"))])]),
                        ),
                    ]),
                    object(&[
                        ("name", text("No username")),
                        ("userId", Value::Number(1.0)),
                    ]),
                    object(&[("username", text("player3"))]),
                ]),
                "players.get" => {
                    assert_eq!(args, [text("player2")]);
                    Value::Null
                }
                other => panic!("unexpected call {other}"),
            })
        });
        assert_eq!(
            serde_json::to_value(all(&loader).unwrap()).unwrap(),
            json!([{
                "name": "Player1", "username": "player1", "userId": 26_909,
                "position": { "x": 139.0, "y": 332.0 }, "cell": "Enter", "pad": "Spawn",
                "hp": 900.0, "maxHp": 1000.0, "mp": 50.0, "maxMp": 100.0, "level": 100.0,
                "state": "combat", "afk": true,
                "auras": [{ "name": "Burn", "value": null, "stack": 1,
                            "category": null, "icon": null, "persistent": false,
                            "duration": 0.0, "expiresAt": null }],
            }])
        );
        assert_eq!(get(&loader, " player2 ").unwrap(), None);
    }
}
