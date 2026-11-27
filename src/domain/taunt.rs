use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{
    State,
    combat::{self, Target},
    events::{self, TargetType},
    has_avatar, inventory, player,
    world::{self, MonsterQuery},
};
use crate::bridge::{self, CallError, Format, Loader, Packet, Packets, Value, packets::wire};

const SCROLL_OF_ENRAGE: i32 = 12_917;
const SCROLL_SLOT: i32 = 5;
const FOCUS: &str = "Focus";
const FOCUS_ICON: &str = "iwd1,ied1";
const ACKNOWLEDGED_WITHIN: Duration = Duration::from_secs(5);

pub(crate) fn is_focus(name: &str, icon: Option<&str>) -> bool {
    name.trim().eq_ignore_ascii_case(FOCUS) && icon.is_none_or(|icon| icon == FOCUS_ICON)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Sighted {
    pub(crate) mon_map_id: i32,
    pub(crate) alive: bool,
    pub(crate) focus: bool,
}

pub(crate) fn sight(
    loader: &impl Loader,
    query: &MonsterQuery,
) -> Result<Option<Sighted>, CallError> {
    Ok(world::monster_here(loader, query)?.and_then(|monster| {
        Some(Sighted {
            mon_map_id: wire::whole(monster.mon_map_id)?,
            alive: monster.state != Some(State::Dead),
            focus: monster
                .auras
                .iter()
                .any(|aura| is_focus(aura.name(), aura.icon())),
        })
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Readiness {
    Ready,
    Cooling(Duration),
    Unable,
}

pub(crate) fn readiness(loader: &impl Loader) -> Result<Readiness, CallError> {
    if !has_avatar(loader)? || !player::alive(loader)? {
        return Ok(Readiness::Unable);
    }
    let Some(slot) = inventory::row(bridge::combat::get_consumable_skill_item(loader)?) else {
        return Ok(Readiness::Unable);
    };
    if slot.positive("itemId") != Some(SCROLL_OF_ENRAGE) {
        return Ok(Readiness::Unable);
    }
    let left = bridge::combat::get_skill_cooldown_remaining(loader, SCROLL_SLOT)?;
    Ok(if left > 0 {
        Readiness::Cooling(Duration::from_millis(u64::from(left.unsigned_abs())))
    } else if slot.flag("ready") {
        Readiness::Ready
    } else {
        Readiness::Unable
    })
}

pub(crate) async fn cast(
    loader: &impl Loader,
    actions: Packets<ActionsPacket>,
    mon_map_id: i32,
) -> Result<bool, CallError> {
    let me = bridge::player::get_user_id(loader)?;
    let before = combat::target(loader)?;
    let target = f64::from(mon_map_id);
    let dispatched = bridge::combat::cast_consumable_on_monster(
        loader,
        combat::selector("monsterMapId", Value::Number(target)),
        SCROLL_OF_ENRAGE,
    )?;
    let Some(action) = inventory::row(dispatched)
        .and_then(|row| row.number("actionId"))
        .and_then(wire::whole::<i64>)
    else {
        return Ok(false);
    };
    let landed = actions
        .answer(ACKNOWLEDGED_WITHIN, |packet| packet.landed(action, me))
        .await
        .unwrap_or(false);
    if let Some(Target::Monster(previous)) = before
        && previous.mon_map_id != target
        && combat::targeting(loader, target)?
    {
        combat::attack_id(loader, previous.mon_map_id)?;
    }
    Ok(landed)
}

#[derive(Deserialize)]
pub(crate) struct ActionsPacket {
    #[serde(default)]
    sara: Vec<Option<Acknowledged>>,
    #[serde(default)]
    sarsa: Vec<Option<Applied>>,
}

impl Packet for ActionsPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "ct";
}

#[derive(Deserialize)]
struct Acknowledged {
    #[serde(rename = "actionResult", default)]
    result: Option<ActionResult>,
    #[serde(rename = "actID", default, deserialize_with = "wire::integer")]
    action: Option<i64>,
    #[serde(rename = "iRes", default, deserialize_with = "wire::integer")]
    outcome: Option<i64>,
}

#[derive(Deserialize)]
struct ActionResult {
    #[serde(rename = "actID", default, deserialize_with = "wire::integer")]
    action: Option<i64>,
    #[serde(rename = "cInf", default, deserialize_with = "wire::text")]
    caster: String,
}

#[derive(Deserialize)]
struct Applied {
    #[serde(rename = "actID", default, deserialize_with = "wire::integer")]
    action: Option<i64>,
    #[serde(rename = "cInf", default, deserialize_with = "wire::text")]
    caster: String,
    #[serde(rename = "iRes", default, deserialize_with = "wire::integer")]
    outcome: Option<i64>,
}

impl ActionsPacket {
    fn landed(&self, action: i64, me: i32) -> Option<bool> {
        let single = self.sara.iter().flatten().find_map(|entry| {
            let result = entry.result.as_ref()?;
            let landed = outcome(entry.outcome)?;
            let id = if landed { result.action } else { entry.action };
            (id == Some(action) && caster(&result.caster) == Some(me)).then_some(landed)
        });
        single.or_else(|| {
            self.sarsa.iter().flatten().find_map(|entry| {
                (entry.action == Some(action) && caster(&entry.caster) == Some(me))
                    .then(|| outcome(entry.outcome))
                    .flatten()
            })
        })
    }
}

fn outcome(code: Option<i64>) -> Option<bool> {
    match code? {
        1 => Some(true),
        0 => Some(false),
        _ => None,
    }
}

fn caster(reference: &str) -> Option<i32> {
    let reference = reference.rsplit('>').next()?;
    match events::targets(reference).next()? {
        (TargetType::Player, id) => Some(id),
        (TargetType::Monster, _) => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::bridge::packets;

    fn landed(packet: serde_json::Value, action: i64) -> Option<bool> {
        packets::decode::<ActionsPacket>(&json!({ "type": "json", "dataObj": packet }).to_string())
            .unwrap()
            .unwrap()
            .landed(action, 28552)
    }

    #[test]
    fn a_cast_lands_only_when_the_server_answers_this_players_action() {
        let accepted = json!({ "cmd": "ct", "sara": [
            { "actionResult": { "actID": 9, "cInf": "p:28554", "tInf": "m:7" }, "iRes": 1 },
            { "actionResult": { "actID": 9, "cInf": "p:28552", "tInf": "m:7" }, "iRes": 1 },
        ] });
        assert_eq!(landed(accepted.clone(), 9), Some(true));
        assert_eq!(landed(accepted, 8), None);
        let refused = json!({ "cmd": "ct", "sara": [
            { "actionResult": { "cInf": "p:28552" }, "actID": 9, "iRes": 0 },
        ] });
        assert_eq!(landed(refused, 9), Some(false));
        let applied = json!({ "cmd": "ct", "sarsa": [
            { "actID": "9", "cInf": "p:28552", "iRes": 1, "a": [{ "tInf": "m:7" }] },
        ] });
        assert_eq!(landed(applied, 9), Some(true));
    }

    #[test]
    fn focus_is_the_scrolls_taunt_unless_its_icon_says_otherwise() {
        assert!(is_focus(" focus ", Some("iwd1,ied1")));
        assert!(is_focus("Focus", None));
        assert!(!is_focus("Focus", Some("isp1")));
        assert!(!is_focus("Focused", None));
    }
}
