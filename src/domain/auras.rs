use serde::{Deserialize, Serialize};

use super::inventory::{row, rows};
use crate::bridge::{self, CallError, Format, Loader, Packet, Value, packets::wire};

#[derive(Deserialize)]
pub(crate) struct FightAurasPacket {
    #[serde(rename = "a", default)]
    pub(crate) changes: Vec<Option<AuraChange>>,
}

impl Packet for FightAurasPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "ct";
}

#[derive(Deserialize)]
pub(crate) struct AuraChange {
    #[serde(default, deserialize_with = "wire::text")]
    pub(crate) cmd: String,
    #[serde(rename = "tInf", default, deserialize_with = "wire::text")]
    pub(crate) targets: String,
    #[serde(default)]
    pub(crate) auras: Vec<AuraEntry>,
    pub(crate) aura: Option<AuraEntry>,
}

#[derive(Deserialize)]
pub(crate) struct AuraEntry {
    #[serde(rename = "nam", default, deserialize_with = "wire::text")]
    pub(crate) name: String,
    #[serde(rename = "dur", default, deserialize_with = "wire::number")]
    pub(crate) duration: Option<f64>,
    #[serde(rename = "stk", default, deserialize_with = "wire::number")]
    pub(crate) stack: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum AuraValue {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Aura {
    name: String,
    value: Option<AuraValue>,
    stack: i32,
    pub(super) category: Option<String>,
    icon: Option<String>,
    pub(super) persistent: bool,
    pub(super) duration: f64,
    expires_at: Option<f64>,
}

impl Aura {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn stack(&self) -> i32 {
        self.stack
    }

    pub(crate) fn number(&self) -> Option<f64> {
        match &self.value {
            Some(AuraValue::Number(number)) => Some(*number),
            Some(AuraValue::Text(text)) => text
                .trim()
                .parse()
                .ok()
                .filter(|number: &f64| number.is_finite()),
            None => None,
        }
    }

    pub(crate) fn is_named(&self, name: &str) -> bool {
        self.name.to_lowercase() == name.to_lowercase()
    }
}

fn aura_from(value: Value) -> Option<Aura> {
    let row = row(value)?;
    let name = row.text("name");
    if name.is_empty() {
        return None;
    }
    let present = |key: &str| Some(row.text(key)).filter(|text| !text.is_empty());
    Some(Aura {
        name,
        value: match row.0.get("value") {
            Some(Value::Number(number)) if number.is_finite() => Some(AuraValue::Number(*number)),
            Some(Value::String(text)) => Some(AuraValue::Text(text.clone())),
            _ => None,
        },
        stack: row.positive("stack").unwrap_or(1),
        category: present("category"),
        icon: present("icon"),
        persistent: row.flag("persistent"),
        duration: row
            .number("duration")
            .filter(|seconds| *seconds > 0.0)
            .unwrap_or(0.0),
        expires_at: row.number("expiresAt"),
    })
}

pub(crate) fn player(loader: &impl Loader) -> Result<Vec<Aura>, CallError> {
    Ok(from_rows(bridge::player::get_auras(loader)?))
}

pub(super) fn from_rows(value: Value) -> Vec<Aura> {
    rows(value, aura_from)
}

pub(crate) fn find(auras: Vec<Aura>, name: &str) -> Option<Aura> {
    auras.into_iter().find(|aura| aura.is_named(name))
}

pub(crate) fn has_any(auras: &[Aura], names: &[&str]) -> bool {
    auras
        .iter()
        .any(|aura| names.iter().any(|name| aura.is_named(name)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::testing::FakeLoader;

    fn aura(fields: &[(&str, Value)]) -> Value {
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
    fn auras_report_their_value_stack_and_timer() {
        let loader = FakeLoader::new(|callback, _| {
            assert_eq!(callback, "player.getAuras");
            Ok(Value::List(vec![
                aura(&[
                    ("name", text("Spirit Power")),
                    ("value", Value::Number(40.0)),
                    ("stack", Value::Number(3.0)),
                    ("category", text("buff")),
                    ("persistent", Value::Bool(false)),
                    ("duration", Value::Number(5.0)),
                    ("expiresAt", Value::Number(6_000.0)),
                ]),
                aura(&[
                    ("name", text("Skill Locked")),
                    ("value", text("Aura of Swords")),
                    ("persistent", Value::Bool(true)),
                    ("duration", Value::Number(0.0)),
                ]),
                aura(&[("icon", text("isp1"))]),
            ]))
        });
        assert_eq!(
            serde_json::to_value(player(&loader).unwrap()).unwrap(),
            json!([
                {
                    "name": "Spirit Power",
                    "value": 40.0,
                    "stack": 3,
                    "category": "buff",
                    "icon": null,
                    "persistent": false,
                    "duration": 5.0,
                    "expiresAt": 6_000.0,
                },
                {
                    "name": "Skill Locked",
                    "value": "Aura of Swords",
                    "stack": 1,
                    "category": null,
                    "icon": null,
                    "persistent": true,
                    "duration": 0.0,
                    "expiresAt": null,
                },
            ])
        );
    }

    #[test]
    fn names_match_regardless_of_case() {
        let loader = FakeLoader::new(|_, _| {
            Ok(Value::List(vec![aura(&[(
                "name",
                text("Positive Charge"),
            )])]))
        });
        let auras = player(&loader).unwrap();
        assert!(has_any(&auras, &["Negative Charge?", "positive charge"]));
        assert!(!has_any(&auras, &["Positive Charge?"]));
        assert_eq!(
            find(auras, "POSITIVE CHARGE").map(|aura| aura.name),
            Some("Positive Charge".into())
        );
    }
}
