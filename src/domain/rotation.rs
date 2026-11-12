use std::{sync::Arc, time::Duration};

use tokio::{sync::watch, time::Instant};

use super::{
    auras::{self, Aura},
    combat::Skill,
    events::{GameEvent, Kind, Myself},
    inventory::{Row, row},
    profiles::{Amount, Condition, Holder, Library, Measure, OnCooldown, Profile, Who},
};
use crate::bridge::{self, CallError, Event, Loader, Subscription, Value};

const SLOTS: usize = 6;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Vitals {
    pub(crate) hp: f64,
    pub(crate) max_hp: f64,
    pub(crate) mp: f64,
    pub(crate) max_mp: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SkillState {
    pub(crate) cooldown: Duration,
    pub(crate) castable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Foe {
    pub(crate) auras: Vec<Aura>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) me: Vitals,
    pub(crate) auras: Vec<Aura>,
    pub(crate) target: Option<Foe>,
    pub(crate) allies: Vec<Vitals>,
    pub(crate) skills: [Option<SkillState>; SLOTS],
}

impl Snapshot {
    pub(crate) fn read(loader: &impl Loader) -> Result<Option<Self>, CallError> {
        Ok(Self::from_value(bridge::combat::get_snapshot(loader)?))
    }

    fn from_value(value: Value) -> Option<Self> {
        let mut fields = row(value)?;
        let auras = auras::from_rows(fields.0.remove("auras").unwrap_or(Value::Null));
        let target = fields
            .0
            .remove("target")
            .and_then(row)
            .map(|mut target| Foe {
                auras: auras::from_rows(target.0.remove("auras").unwrap_or(Value::Null)),
            });
        let allies = match fields.0.remove("allies") {
            Some(Value::List(allies)) => allies
                .into_iter()
                .filter_map(row)
                .map(|ally| vitals(&ally))
                .collect(),
            _ => Vec::new(),
        };
        let mut skills = [None; SLOTS];
        if let Some(Value::List(states)) = fields.0.remove("skills") {
            for (slot, state) in skills.iter_mut().zip(states) {
                *slot = row(state).map(|state| SkillState {
                    cooldown: Duration::from_millis(
                        state.number("cooldown").unwrap_or(0.0).max(0.0) as u64,
                    ),
                    castable: state.flag("castable"),
                });
            }
        }
        Some(Snapshot {
            me: vitals(&fields),
            auras,
            target,
            allies,
            skills,
        })
    }

    fn skill(&self, skill: Skill) -> Option<SkillState> {
        self.skills
            .get(usize::from(u8::from(skill)))
            .copied()
            .flatten()
    }

    fn ready(&self, skill: Skill) -> bool {
        self.skill(skill)
            .is_some_and(|state| state.castable && state.cooldown.is_zero())
    }
}

fn vitals(row: &Row) -> Vitals {
    let number = |key: &str| row.number(key).unwrap_or(0.0);
    Vitals {
        hp: number("hp"),
        max_hp: number("maxHp"),
        mp: number("mp"),
        max_mp: number("maxMp"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lane {
    Priority,
    Rotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Trigger(usize),
    Step(Lane, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cast {
    pub(crate) skill: Skill,
    source: Source,
}

pub(crate) struct Rotation {
    profile: Profile,
    cursor: usize,
    paused_until: Option<Instant>,
    pending: Vec<Option<Instant>>,
}

impl Rotation {
    pub(crate) fn new(profile: Profile) -> Self {
        Self {
            pending: vec![None; profile.triggers.len()],
            profile,
            cursor: 0,
            paused_until: None,
        }
    }

    pub(crate) fn replace(&mut self, profile: Profile) {
        *self = Self::new(profile);
    }

    pub(crate) fn restart(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn hear(&mut self, text: &str, now: Instant) {
        let text = normalized(text);
        for (trigger, pending) in self.profile.triggers.iter().zip(&mut self.pending) {
            if text.contains(&normalized(trigger.includes.as_str())) {
                *pending = Some(now);
            }
        }
    }

    pub(crate) fn decide(&mut self, snapshot: &Snapshot, now: Instant) -> Option<Cast> {
        for (index, (trigger, pending)) in self
            .profile
            .triggers
            .iter()
            .zip(&mut self.pending)
            .enumerate()
        {
            let Some(heard) = *pending else {
                continue;
            };
            if now.saturating_duration_since(heard) > trigger.within {
                *pending = None;
            } else if snapshot.ready(trigger.skill) {
                return Some(Cast {
                    skill: trigger.skill,
                    source: Source::Trigger(index),
                });
            }
        }
        if self.paused_until.is_some_and(|until| now < until) {
            return None;
        }
        for (step, priority) in self.profile.priority.iter().enumerate() {
            if snapshot.ready(priority.skill) && holds(&priority.when, snapshot) {
                return Some(Cast {
                    skill: priority.skill,
                    source: Source::Step(Lane::Priority, step),
                });
            }
        }
        let count = self.profile.steps.len();
        for offset in 0..count {
            let index = (self.cursor + offset) % count;
            let step = &self.profile.steps[index];
            let Some(state) = snapshot.skill(step.skill) else {
                continue;
            };
            if !state.castable || !holds(&step.when, snapshot) {
                continue;
            }
            if state.cooldown.is_zero() {
                return Some(Cast {
                    skill: step.skill,
                    source: Source::Step(Lane::Rotation, index),
                });
            }
            if step.on_cooldown.unwrap_or(self.profile.on_cooldown) == OnCooldown::Wait {
                self.cursor = index;
                return None;
            }
        }
        None
    }

    pub(crate) fn cast(&mut self, cast: Cast, now: Instant) {
        let pause = match cast.source {
            Source::Trigger(index) => {
                self.pending[index] = None;
                return;
            }
            Source::Step(Lane::Priority, index) => self.profile.priority[index].pause_after,
            Source::Step(Lane::Rotation, index) => {
                self.cursor = (index + 1) % self.profile.steps.len();
                self.profile.steps[index].pause_after
            }
        };
        self.paused_until = (!pause.is_zero()).then(|| now + pause);
    }
}

fn holds(when: &[Condition], snapshot: &Snapshot) -> bool {
    when.iter().all(|condition| match condition {
        Condition::Hp { of: Who::Me, bound } => vital(snapshot.me.hp, snapshot.me.max_hp, *bound),
        Condition::Hp {
            of: Who::AnyAlly,
            bound,
        } => snapshot
            .allies
            .iter()
            .any(|ally| vital(ally.hp, ally.max_hp, *bound)),
        Condition::Mp { bound } => vital(snapshot.me.mp, snapshot.me.max_mp, *bound),
        Condition::Aura {
            on,
            name,
            measure,
            bound,
        } => {
            let auras = match on {
                Holder::Me => snapshot.auras.as_slice(),
                Holder::Target => snapshot
                    .target
                    .as_ref()
                    .map_or(&[][..], |target| &target.auras),
            };
            let measured = match auras.iter().find(|aura| aura.is_named(name.as_str())) {
                None => Some(0.0),
                Some(aura) => match measure {
                    Measure::Stacks => Some(f64::from(aura.stack())),
                    Measure::Value => aura.number(),
                },
            };
            measured.is_some_and(|measured| bound.admits(measured, |count| count.get()))
        }
    })
}

fn vital(current: f64, max: f64, bound: super::profiles::Bound<Amount>) -> bool {
    let (actual, limit) = bound.limit().of(current, max);
    bound.admits(actual, |_| limit)
}

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(crate) struct Fighter {
    rotation: Rotation,
    follows: Option<(String, watch::Receiver<Arc<Library>>)>,
    messages: Option<Subscription>,
}

impl Fighter {
    pub(crate) fn fixed(profile: Profile) -> Self {
        Self {
            rotation: Rotation::new(profile),
            follows: None,
            messages: None,
        }
    }

    pub(crate) fn named(name: String, mut library: watch::Receiver<Arc<Library>>) -> Option<Self> {
        let profile = library.borrow_and_update().get(&name)?;
        Some(Self {
            rotation: Rotation::new(profile),
            follows: Some((name, library)),
            messages: None,
        })
    }

    pub(crate) fn listening(mut self, messages: Subscription) -> Self {
        self.messages = Some(messages);
        self
    }

    pub(crate) fn switch(&mut self, other: Fighter) {
        self.rotation.replace(other.rotation.profile);
        self.follows = other.follows;
    }

    pub(crate) async fn listen(&mut self, loader: &impl Loader) {
        let Self {
            rotation,
            messages: Some(messages),
            ..
        } = self
        else {
            return std::future::pending().await;
        };
        loop {
            if let Ok(event) = messages.next().await {
                let now = Instant::now();
                for text in heard(&event, loader) {
                    rotation.hear(&text, now);
                }
            }
        }
    }

    pub(crate) fn restart(&mut self) {
        self.rotation.restart();
    }

    pub(crate) fn act(&mut self, loader: &impl Loader) -> Result<bool, CallError> {
        self.follow();
        let Some(snapshot) = Snapshot::read(loader)? else {
            return Ok(false);
        };
        let now = Instant::now();
        let Some(cast) = self.rotation.decide(&snapshot, now) else {
            return Ok(false);
        };
        let used =
            bridge::combat::use_skill(loader, &u8::from(cast.skill).to_string(), None, None)?;
        if used {
            self.rotation.cast(cast, now);
        }
        Ok(used)
    }

    fn follow(&mut self) {
        let Some((name, library)) = &mut self.follows else {
            return;
        };
        if !library.has_changed().unwrap_or(false) {
            return;
        }
        match library.borrow_and_update().get(name) {
            Some(profile) if profile != self.rotation.profile => self.rotation.replace(profile),
            Some(_) => {}
            None => tracing::warn!(
                profile = %name,
                "the combat profile in use was renamed or deleted, so this fight keeps the version it started with"
            ),
        }
    }
}

fn heard(event: &Event, loader: &impl Loader) -> Vec<String> {
    let mut read = Vec::new();
    Kind::Message.read(event, &Myself(loader), &mut read);
    read.into_iter()
        .filter_map(|event| match event {
            GameEvent::Message { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

pub(crate) fn wants(event: &Event) -> bool {
    Kind::Message.wants(event)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn value(json: serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(flag) => Value::Bool(flag),
            serde_json::Value::Number(number) => Value::Number(number.as_f64().unwrap()),
            serde_json::Value::String(text) => Value::String(text),
            serde_json::Value::Array(items) => Value::List(items.into_iter().map(value).collect()),
            serde_json::Value::Object(fields) => Value::Object(
                fields
                    .into_iter()
                    .map(|(key, field)| (key, value(field)))
                    .collect(),
            ),
        }
    }

    fn profile(json: serde_json::Value) -> Profile {
        serde_json::from_value(json).unwrap()
    }

    struct Fight {
        me: serde_json::Value,
        target: serde_json::Value,
        allies: serde_json::Value,
        cooling: [u64; SLOTS],
        blocked: Vec<usize>,
    }

    impl Default for Fight {
        fn default() -> Self {
            Self {
                me: json!({ "hp": 1000, "maxHp": 1000, "mp": 100, "maxMp": 100, "auras": [] }),
                target: json!({ "auras": [] }),
                allies: json!([]),
                cooling: [0; SLOTS],
                blocked: Vec::new(),
            }
        }
    }

    impl Fight {
        fn snapshot(&self) -> Snapshot {
            let mut fields = self.me.clone();
            fields["target"] = self.target.clone();
            fields["allies"] = self.allies.clone();
            fields["skills"] = (0..SLOTS)
                .map(|slot| json!({ "cooldown": self.cooling[slot], "castable": !self.blocked.contains(&slot) }))
                .collect();
            Snapshot::from_value(value(fields)).unwrap()
        }
    }

    fn casts(rotation: &mut Rotation, fight: &mut Fight, ticks: usize, cooldown: u64) -> Vec<u8> {
        let mut cast = Vec::new();
        let start = Instant::now();
        for tick in 0..ticks {
            let now = start + Duration::from_millis(200 * tick as u64);
            for slot in &mut fight.cooling {
                *slot = slot.saturating_sub(200);
            }
            if let Some(chosen) = rotation.decide(&fight.snapshot(), now) {
                rotation.cast(chosen, now);
                fight.cooling[usize::from(u8::from(chosen.skill))] = cooldown;
                cast.push(u8::from(chosen.skill));
            }
        }
        cast
    }

    #[test]
    fn the_snapshot_reads_vitals_target_allies_and_each_slot() {
        let snapshot = Snapshot::from_value(value(json!({
            "hp": 300, "maxHp": 1000, "mp": 20, "maxMp": 100,
            "auras": [{ "name": "Spirit Power", "value": 40, "stack": 2 }],
            "target": { "auras": [{ "name": "Burn", "stack": 3 }] },
            "allies": [{ "hp": 50, "maxHp": 200 }],
            "skills": [null, { "cooldown": 1500, "castable": true }, { "cooldown": -5, "castable": false }]
        })))
        .unwrap();
        assert_eq!(
            snapshot.me,
            Vitals {
                hp: 300.0,
                max_hp: 1000.0,
                mp: 20.0,
                max_mp: 100.0
            }
        );
        assert_eq!(snapshot.auras[0].number(), Some(40.0));
        assert_eq!(
            snapshot
                .target
                .as_ref()
                .map(|target| target.auras[0].stack()),
            Some(3)
        );
        assert_eq!(
            snapshot.allies,
            [Vitals {
                hp: 50.0,
                max_hp: 200.0,
                ..Vitals::default()
            }]
        );
        assert_eq!(snapshot.skills[0], None);
        assert_eq!(
            snapshot.skills[1],
            Some(SkillState {
                cooldown: Duration::from_millis(1500),
                castable: true
            })
        );
        assert_eq!(
            snapshot.skills[2],
            Some(SkillState {
                cooldown: Duration::ZERO,
                castable: false
            })
        );
        assert_eq!(Snapshot::from_value(Value::Null), None);
    }

    #[test]
    fn the_rotation_skips_skills_that_are_cooling_or_unusable() {
        let mut fight = Fight {
            blocked: vec![3],
            ..Fight::default()
        };
        let mut rotation = Rotation::new(Profile::generic());
        assert_eq!(casts(&mut rotation, &mut fight, 6, 600), [1, 2, 4, 1, 2, 4]);
    }

    #[test]
    fn a_waiting_step_holds_the_rotation_until_its_cooldown_ends() {
        let mut fight = Fight {
            cooling: [0, 0, 0, 0, 400, 0],
            ..Fight::default()
        };
        let mut rotation = Rotation::new(profile(json!({
            "onCooldown": "skip",
            "steps": [{ "skill": 4, "onCooldown": "wait" }, { "skill": 1 }]
        })));
        assert_eq!(casts(&mut rotation, &mut fight, 4, 0), [4, 1, 4]);

        fight.cooling = [0, 400, 0, 0, 600, 0];
        let mut rotation = Rotation::new(profile(json!({
            "onCooldown": "skip",
            "steps": [{ "skill": 1 }, { "skill": 4, "onCooldown": "wait" }]
        })));
        assert_eq!(casts(&mut rotation, &mut fight, 4, 0), [4, 1]);
    }

    #[test]
    fn a_waiting_step_that_cannot_cast_is_skipped_instead() {
        let mut fight = Fight {
            blocked: vec![4],
            ..Fight::default()
        };
        let mut rotation = Rotation::new(profile(json!({
            "onCooldown": "wait",
            "steps": [{ "skill": 4 }, { "skill": 1 }]
        })));
        assert_eq!(casts(&mut rotation, &mut fight, 3, 0), [1, 1, 1]);
    }

    #[test]
    fn priority_steps_cast_first_without_moving_the_rotation() {
        let mut fight = Fight {
            allies: json!([{ "hp": 500, "maxHp": 1000 }]),
            ..Fight::default()
        };
        let mut rotation = Rotation::new(profile(json!({
            "onCooldown": "skip",
            "priority": [{ "skill": 2, "when": [{ "type": "hp", "of": "anyAlly", "atMost": "60%" }] }],
            "steps": [{ "skill": 1 }, { "skill": 3 }]
        })));
        assert_eq!(casts(&mut rotation, &mut fight, 4, 400), [2, 1, 2, 3]);
        fight.allies = json!([{ "hp": 900, "maxHp": 1000 }]);
        fight.cooling = [0; SLOTS];
        assert_eq!(casts(&mut rotation, &mut fight, 2, 400), [1, 3]);
    }

    #[test]
    fn a_priority_step_on_cooldown_never_stalls_the_rotation() {
        let mut fight = Fight {
            cooling: [0, 0, 0, 0, 0, 5000],
            ..Fight::default()
        };
        let mut rotation = Rotation::new(profile(json!({
            "onCooldown": "wait",
            "priority": [{ "skill": 5 }],
            "steps": [{ "skill": 1 }]
        })));
        assert_eq!(casts(&mut rotation, &mut fight, 3, 0), [1, 1, 1]);
    }

    #[test]
    fn conditions_read_points_percents_auras_and_their_values() {
        let mut fight = Fight {
            me: json!({
                "hp": 5000, "maxHp": 10000, "mp": 30, "maxMp": 100,
                "auras": [{ "name": "Spirit Power", "value": "40", "stack": 1 }, { "name": "Focus", "value": "on", "stack": 2 }]
            }),
            target: json!({ "auras": [{ "name": "Burn", "stack": 3 }] }),
            ..Fight::default()
        };
        for (condition, holds) in [
            (json!({ "type": "hp", "of": "me", "atMost": 6000 }), true),
            (json!({ "type": "hp", "of": "me", "atMost": "49%" }), false),
            (
                json!({ "type": "hp", "of": "anyAlly", "atLeast": 0 }),
                false,
            ),
            (json!({ "type": "mp", "atLeast": "30%" }), true),
            (
                json!({ "type": "aura", "on": "target", "name": "burn", "atLeast": 3 }),
                true,
            ),
            (
                json!({ "type": "aura", "on": "target", "name": "Poison", "atMost": 0 }),
                true,
            ),
            (
                json!({ "type": "aura", "on": "me", "name": "Spirit Power", "measure": "value", "atLeast": 40 }),
                true,
            ),
            (
                json!({ "type": "aura", "on": "me", "name": "Spirit Power", "atLeast": 2 }),
                false,
            ),
            (
                json!({ "type": "aura", "on": "me", "name": "Focus", "measure": "value", "atMost": 100 }),
                false,
            ),
            (
                json!({ "type": "aura", "on": "me", "name": "Gone", "measure": "value", "atMost": 0 }),
                true,
            ),
        ] {
            let mut rotation = Rotation::new(profile(json!({
                "onCooldown": "skip",
                "steps": [{ "skill": 1, "when": [condition.clone()] }]
            })));
            assert_eq!(
                !casts(&mut rotation, &mut fight, 1, 0).is_empty(),
                holds,
                "{condition}"
            );
        }
        fight.target = json!(null);
        let mut rotation = Rotation::new(profile(json!({
            "onCooldown": "skip",
            "steps": [{ "skill": 1, "when": [{ "type": "aura", "on": "target", "name": "Burn", "atMost": 0 }] }]
        })));
        assert_eq!(casts(&mut rotation, &mut fight, 1, 0), [1]);
    }

    #[test]
    fn a_pause_after_a_cast_holds_every_lane() {
        let mut fight = Fight::default();
        let mut rotation = Rotation::new(profile(json!({
            "onCooldown": "skip",
            "priority": [{ "skill": 5, "when": [{ "type": "hp", "of": "me", "atMost": "50%" }] }],
            "steps": [{ "skill": 1, "pauseAfter": 500 }, { "skill": 2 }]
        })));
        let start = Instant::now();
        let first = rotation.decide(&fight.snapshot(), start).unwrap();
        rotation.cast(first, start);
        fight.me["hp"] = json!(100);
        assert_eq!(
            rotation.decide(&fight.snapshot(), start + Duration::from_millis(400)),
            None
        );
        assert_eq!(
            rotation
                .decide(&fight.snapshot(), start + Duration::from_millis(500))
                .map(|cast| u8::from(cast.skill)),
            Some(5)
        );
    }

    #[test]
    fn a_trigger_answers_each_message_once_ahead_of_everything_else() {
        let mut fight = Fight::default();
        let mut rotation = Rotation::new(profile(json!({
            "onCooldown": "skip",
            "triggers": [{ "includes": "Prepares  a", "skill": 3, "within": 1000 }],
            "priority": [{ "skill": 5 }],
            "steps": [{ "skill": 1, "pauseAfter": 5000 }]
        })));
        let start = Instant::now();
        let at = |millis| start + Duration::from_millis(millis);
        let skill = |cast: Option<Cast>| cast.map(|cast| u8::from(cast.skill));
        fight.blocked = vec![5];
        let first = rotation.decide(&fight.snapshot(), start).unwrap();
        rotation.cast(first, start);
        rotation.hear("The boss PREPARES a slam!", at(100));
        let answer = rotation.decide(&fight.snapshot(), at(200));
        assert_eq!(skill(answer), Some(3));
        rotation.cast(answer.unwrap(), at(200));
        assert_eq!(skill(rotation.decide(&fight.snapshot(), at(300))), None);
        fight.blocked = vec![3, 5];
        rotation.hear("prepares a slam", at(400));
        rotation.restart();
        assert_eq!(skill(rotation.decide(&fight.snapshot(), at(900))), None);
        fight.blocked = vec![5];
        assert_eq!(skill(rotation.decide(&fight.snapshot(), at(1300))), Some(3));
        rotation.hear("prepares a slam", at(2000));
        fight.blocked = vec![3, 5];
        assert_eq!(skill(rotation.decide(&fight.snapshot(), at(3100))), None);
        fight.blocked = vec![5];
        assert_eq!(skill(rotation.decide(&fight.snapshot(), at(5000))), Some(1));
    }
}
