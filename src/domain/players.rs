use serde::{Deserialize, Deserializer, Serialize};

use super::{
    State,
    auras::{self, Aura},
    inventory::{row, rows},
};
use crate::bridge::{self, CallError, Format, Loader, Packet, Value, packets::wire};

#[derive(Deserialize)]
pub(crate) struct PlayerUpdatePacket {
    #[serde(rename = "unm", deserialize_with = "wire::text")]
    pub(crate) username: String,
    #[serde(rename = "o", default)]
    update: PlayerUpdate,
}

#[derive(Default, Deserialize)]
struct PlayerUpdate {
    #[serde(default, deserialize_with = "flagged")]
    afk: Option<bool>,
}

fn flagged<'de, D: Deserializer<'de>>(input: D) -> Result<Option<bool>, D::Error> {
    wire::flag(input).map(Some)
}

impl Packet for PlayerUpdatePacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "uotls";
}

impl PlayerUpdatePacket {
    pub(crate) fn afk(&self) -> Option<bool> {
        self.update.afk
    }
}

#[derive(Deserialize)]
pub(crate) struct PlayerUpdateLine {
    _cmd: String,
    _room: String,
    pub(crate) username: String,
    update: String,
}

impl Packet for PlayerUpdateLine {
    const FORMAT: Format = Format::Str;
    const CMD: &'static str = "uotls";
}

impl PlayerUpdateLine {
    pub(crate) fn afk(&self) -> Option<bool> {
        self.update
            .split(',')
            .find_map(|field| match field.split_once(':') {
                Some(("afk", value)) => Some(value.trim() == "true"),
                _ => None,
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct Position {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoomPlayer {
    name: String,
    username: String,
    user_id: i32,
    position: Option<Position>,
    cell: String,
    pad: String,
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
