use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
    sync::LazyLock,
    time::Duration,
};

use regex::Regex;
use serde::Deserialize;
use tokio::time::Instant;

use super::{
    auras,
    events::{GameEvent, Kind, Me, TargetType, targets},
};
use crate::bridge::{
    CallError, Event, Format, Loader, Packet,
    packets::{self, wire},
};

const GRACE: Duration = Duration::from_secs(7);
const LONGEST: Duration = Duration::from_secs(30);

const KINDS: [Kind; 6] = [
    Kind::Message,
    Kind::AuraAdded,
    Kind::AuraRemoved,
    Kind::MonsterDeath,
    Kind::MonsterRespawn,
    Kind::MapJoined,
];

struct Triggers {
    warnings: Vec<Regex>,
    monster_auras: Vec<Regex>,
    self_auras: Vec<Regex>,
}

static TRIGGERS: LazyLock<Triggers> = LazyLock::new(|| {
    let patterns = |sources: &[&str]| {
        sources
            .iter()
            .map(|source| Regex::new(&format!("(?i){source}")).expect("a valid trigger pattern"))
            .collect()
    };
    Triggers {
        warnings: patterns(&[r"prepares\s+a\s+counter\s+attack"]),
        monster_auras: patterns(&[r"\bcounter\b.*\battack\b"]),
        self_auras: patterns(&[r"^\s*sun's\s+heat\s*$"]),
    }
});

fn matches(patterns: &[Regex], text: &str) -> bool {
    patterns.iter().any(|pattern| pattern.is_match(text))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Signal {
    Begin { mon_map_id: i32, lasts: Duration },
    End { mon_map_id: i32 },
    SelfAuraFaded,
    Reset,
}

pub(crate) fn wants(event: &Event) -> bool {
    matches!(event, Event::Connection { .. }) || KINDS.iter().any(|kind| kind.wants(event))
}

pub(crate) fn signals(event: &Event, me: &impl Me) -> Vec<Signal> {
    let Event::ExtensionResponse { packet } = event else {
        return match event {
            Event::Connection { .. } => vec![Signal::Reset],
            _ => Vec::new(),
        };
    };
    let mut read = Vec::new();
    for kind in KINDS {
        kind.read(event, me, &mut read);
    }
    let mut casts = None;
    read.into_iter()
        .filter_map(|event| match event {
            GameEvent::Message {
                text,
                mon_map_id: Some(mon_map_id),
            } if matches(&TRIGGERS.warnings, &text) => {
                let cast = casts
                    .get_or_insert_with(|| Casts::read(packet))
                    .of(mon_map_id);
                Some(Signal::Begin {
                    mon_map_id,
                    lasts: cast + GRACE,
                })
            }
            GameEvent::AuraAdded {
                name,
                target_type: TargetType::Monster,
                target_id,
                duration,
                ..
            } if matches(&TRIGGERS.monster_auras, &name) => Some(Signal::Begin {
                mon_map_id: target_id,
                lasts: seconds(duration) + GRACE,
            }),
            GameEvent::AuraRemoved {
                name,
                target_type: TargetType::Monster,
                target_id,
                ..
            } if matches(&TRIGGERS.monster_auras, &name) => Some(Signal::End {
                mon_map_id: target_id,
            }),
            GameEvent::AuraRemoved {
                name,
                is_self: true,
                ..
            } if matches(&TRIGGERS.self_auras, &name) => Some(Signal::SelfAuraFaded),
            GameEvent::MonsterDeath { mon_map_id } | GameEvent::MonsterRespawn { mon_map_id } => {
                Some(Signal::End { mon_map_id })
            }
            GameEvent::MapJoined { .. } => Some(Signal::Reset),
            _ => None,
        })
        .collect()
}

fn seconds(value: Option<f64>) -> Duration {
    value
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map_or(Duration::ZERO, |seconds| {
            Duration::from_secs_f64(seconds.min(LONGEST.as_secs_f64()))
        })
}

#[derive(Deserialize)]
struct Casts {
    #[serde(default)]
    sara: Vec<Option<Cast>>,
}

#[derive(Deserialize)]
struct Cast {
    #[serde(rename = "actionResult")]
    result: Option<CastResult>,
}

#[derive(Deserialize)]
struct CastResult {
    #[serde(rename = "cInf", default, deserialize_with = "wire::text")]
    caster: String,
    #[serde(rename = "ct", default, deserialize_with = "wire::number")]
    milliseconds: Option<f64>,
}

impl Packet for Casts {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "ct";
}

impl Casts {
    fn read(packet: &str) -> Self {
        packets::decode(packet)
            .ok()
            .flatten()
            .unwrap_or(Casts { sara: Vec::new() })
    }

    fn of(&self, mon_map_id: i32) -> Duration {
        let longest = self
            .sara
            .iter()
            .flatten()
            .filter_map(|cast| cast.result.as_ref())
            .filter(|result| {
                targets(&result.caster).any(|target| target == (TargetType::Monster, mon_map_id))
            })
            .filter_map(|result| result.milliseconds)
            .fold(0.0, f64::max);
        seconds(Some(longest / 1000.0))
    }
}

#[derive(Clone)]
pub(crate) struct CounterAttacks(Rc<RefCell<State>>);

struct State {
    enabled: bool,
    countering: Vec<Hold>,
    skills_held_since: Option<Instant>,
}

struct Hold {
    mon_map_id: i32,
    since: Instant,
    until: Instant,
}

impl Default for CounterAttacks {
    fn default() -> Self {
        Self(Rc::new(RefCell::new(State {
            enabled: true,
            countering: Vec::new(),
            skills_held_since: None,
        })))
    }
}

impl CounterAttacks {
    pub(crate) fn set_enabled(&self, enabled: bool) {
        let mut state = self.0.borrow_mut();
        state.enabled = enabled;
        if !enabled {
            state.countering.clear();
            state.skills_held_since = None;
        }
    }

    pub(crate) fn apply(&self, signal: Signal) -> Option<i32> {
        let mut state = self.0.borrow_mut();
        if !state.enabled {
            return None;
        }
        match signal {
            Signal::Begin { mon_map_id, lasts } => {
                let now = Instant::now();
                state.countering.retain(|hold| hold.until > now);
                let until = now + lasts.min(LONGEST);
                match state
                    .countering
                    .iter_mut()
                    .find(|hold| hold.mon_map_id == mon_map_id)
                {
                    Some(hold) => hold.until = hold.until.max(until).min(hold.since + LONGEST),
                    None => state.countering.push(Hold {
                        mon_map_id,
                        since: now,
                        until,
                    }),
                }
                Some(mon_map_id)
            }
            Signal::End { mon_map_id } => {
                state
                    .countering
                    .retain(|hold| hold.mon_map_id != mon_map_id);
                None
            }
            Signal::SelfAuraFaded => {
                state.skills_held_since = None;
                None
            }
            Signal::Reset => {
                state.countering.clear();
                state.skills_held_since = None;
                None
            }
        }
    }

    pub(crate) fn countering(&self, mon_map_id: f64) -> bool {
        self.live()
            .iter()
            .any(|hold| f64::from(hold.mon_map_id) == mon_map_id)
    }

    pub(crate) fn any(&self) -> bool {
        !self.live().is_empty()
    }

    fn live(&self) -> RefMut<'_, Vec<Hold>> {
        let now = Instant::now();
        let mut countering = RefMut::map(self.0.borrow_mut(), |state| &mut state.countering);
        countering.retain(|hold| hold.until > now);
        countering
    }

    pub(crate) fn holds_skills(&self, loader: &impl Loader) -> Result<bool, CallError> {
        if !self.0.borrow().enabled {
            return Ok(false);
        }
        let on = auras::player(loader)?
            .iter()
            .any(|aura| matches(&TRIGGERS.self_auras, aura.name()));
        let now = Instant::now();
        let mut state = self.0.borrow_mut();
        if !on {
            state.skills_held_since = None;
            return Ok(false);
        }
        Ok(now < *state.skills_held_since.get_or_insert(now) + LONGEST)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::time::advance;

    use super::*;
    use crate::{bridge::Value, domain::testing::FakeLoader};

    struct Player1;

    impl Me for Player1 {
        fn username(&self) -> Option<String> {
            Some("Player1".into())
        }

        fn user_id(&self) -> Option<i32> {
            Some(1)
        }
    }

    fn read(event: &Event) -> Vec<Signal> {
        signals(event, &Player1)
    }

    fn server(packet: serde_json::Value) -> Event {
        Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": packet }).to_string(),
        }
    }

    fn warning(mon_map_id: i32, text: &str) -> serde_json::Value {
        json!({ "cmd": "ct", "anims": [
            { "msg": text, "cInf": format!("m:{mon_map_id}"), "tInf": "p:1" },
        ] })
    }

    fn aura(cmd: &str, target: &str, name: &str) -> Event {
        server(json!({ "cmd": "ct", "a": [
            { "cmd": cmd, "tInf": target, "auras": [{ "nam": name, "dur": 5 }] },
        ] }))
    }

    fn begin(mon_map_id: i32, seconds: u64) -> Signal {
        Signal::Begin {
            mon_map_id,
            lasts: Duration::from_secs(seconds),
        }
    }

    #[test]
    fn warnings_and_counter_auras_on_monsters_begin_a_counter() {
        assert_eq!(
            read(&server(warning(1, "<mon> prepares   a Counter Attack!"))),
            [begin(1, 7)]
        );
        assert_eq!(
            read(&aura("aura++", "m:2", "Empowered Counter Attack")),
            [begin(2, 12)]
        );
        assert!(read(&server(warning(1, "<mon> prepares an attack"))).is_empty());
        assert!(read(&aura("aura+", "m:2", "Focus")).is_empty());
        assert!(read(&aura("aura+", "p:1", "Counter Attack")).is_empty());
    }

    #[test]
    fn a_warning_lasts_through_the_monster_s_own_cast() {
        let mut packet = warning(3, "<mon> prepares a counter attack!");
        packet["sara"] = json!([
            { "actionResult": { "cInf": "m:3", "ct": 4500 } },
            { "actionResult": { "cInf": "m:4", "ct": 9000 } },
            { "actionResult": null },
        ]);
        assert_eq!(
            read(&server(packet)),
            [Signal::Begin {
                mon_map_id: 3,
                lasts: Duration::from_millis(11_500),
            }]
        );
    }

    #[test]
    fn a_fade_death_or_respawn_ends_it_and_a_new_map_or_connection_resets_all() {
        assert_eq!(
            read(&aura("aura--", "m:2", "Counter Attack")),
            [Signal::End { mon_map_id: 2 }]
        );
        assert_eq!(
            read(&server(
                json!({ "cmd": "ct", "m": { "2": { "intState": 0 } } })
            )),
            [Signal::End { mon_map_id: 2 }]
        );
        assert_eq!(
            read(&Event::ExtensionResponse {
                packet: json!({ "type": "str", "dataObj": ["respawnMon", "-1", "2"] }).to_string()
            }),
            [Signal::End { mon_map_id: 2 }]
        );
        assert_eq!(
            read(&server(
                json!({ "cmd": "moveToArea", "strMapName": "ultraezrajal" })
            )),
            [Signal::Reset]
        );
        for status in ["OnConnection", "OnConnectionFailed", "OnConnectionLost"] {
            let event = Event::Connection {
                status: status.into(),
            };
            assert!(wants(&event));
            assert_eq!(read(&event), [Signal::Reset]);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_counter_holds_until_it_ends_or_its_time_runs_out() {
        let attacks = CounterAttacks::default();
        assert_eq!(attacks.apply(begin(1, 7)), Some(1));
        assert_eq!(attacks.apply(begin(2, 7)), Some(2));
        assert!(attacks.countering(1.0) && attacks.countering(2.0) && !attacks.countering(3.0));
        attacks.apply(Signal::End { mon_map_id: 2 });
        assert!(!attacks.countering(2.0));
        advance(Duration::from_secs(6)).await;
        attacks.apply(begin(1, 3));
        advance(Duration::from_secs(2)).await;
        assert!(attacks.countering(1.0));
        advance(Duration::from_secs(1)).await;
        assert!(!attacks.any());
        attacks.apply(begin(1, 7));
        attacks.apply(Signal::Reset);
        assert!(!attacks.any());
    }

    #[tokio::test(start_paused = true)]
    async fn no_counter_holds_a_monster_past_thirty_seconds() {
        let attacks = CounterAttacks::default();
        let endless = server(json!({ "cmd": "ct", "a": [
            { "cmd": "aura+", "tInf": "m:1", "auras": [{ "nam": "Counter Attack", "dur": 1e300 }] },
        ] }));
        for signal in read(&endless) {
            attacks.apply(signal);
        }
        for _ in 0..5 {
            advance(Duration::from_secs(5)).await;
            attacks.apply(begin(1, 7));
        }
        assert!(attacks.countering(1.0));
        advance(Duration::from_secs(5)).await;
        assert!(!attacks.countering(1.0));
        attacks.apply(begin(1, 7));
        assert!(attacks.countering(1.0));
    }

    #[test]
    fn turning_it_off_forgets_counters_and_ignores_new_ones() {
        let attacks = CounterAttacks::default();
        attacks.apply(begin(1, 7));
        attacks.set_enabled(false);
        assert!(!attacks.any());
        assert_eq!(attacks.apply(begin(1, 7)), None);
        assert!(!attacks.any());
    }

    #[tokio::test(start_paused = true)]
    async fn sun_s_heat_on_you_holds_skills_while_it_is_on_for_at_most_thirty_seconds() {
        let auras = |names: &[&str]| {
            let names: Vec<String> = names.iter().map(|name| (*name).into()).collect();
            FakeLoader::new(move |callback, _| {
                assert_eq!(callback, "player.getAuras");
                Ok(Value::List(
                    names
                        .iter()
                        .map(|name| {
                            Value::Object([("name".to_owned(), Value::String(name.clone()))].into())
                        })
                        .collect(),
                ))
            })
        };
        let attacks = CounterAttacks::default();
        assert_eq!(attacks.holds_skills(&auras(&["Focus"])), Ok(false));
        assert_eq!(
            attacks.holds_skills(&auras(&["Focus", "Sun's Heat"])),
            Ok(true)
        );
        advance(LONGEST).await;
        assert_eq!(attacks.holds_skills(&auras(&["Sun's Heat"])), Ok(false));
        assert_eq!(attacks.holds_skills(&auras(&[])), Ok(false));
        assert_eq!(attacks.holds_skills(&auras(&["Sun's Heat"])), Ok(true));
        advance(Duration::from_secs(25)).await;
        assert_eq!(read(&aura("aura++", "p:1", "Sun's Heat")), []);
        assert_eq!(attacks.holds_skills(&auras(&["Sun's Heat"])), Ok(true));
        advance(Duration::from_secs(5)).await;
        assert_eq!(attacks.holds_skills(&auras(&["Sun's Heat"])), Ok(false));
        assert_eq!(read(&aura("aura-", "p:2", "Sun's Heat")), []);
        for signal in read(&aura("aura-", "p:1", "Sun's Heat")) {
            attacks.apply(signal);
        }
        assert_eq!(attacks.holds_skills(&auras(&["Sun's Heat"])), Ok(true));
        advance(LONGEST).await;
        attacks.apply(Signal::Reset);
        assert_eq!(attacks.holds_skills(&auras(&["Sun's Heat"])), Ok(true));
        attacks.set_enabled(false);
        assert_eq!(attacks.holds_skills(&auras(&["Sun's Heat"])), Ok(false));
    }
}
