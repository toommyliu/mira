use std::{collections::BTreeMap, fmt, time::Duration};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::combat::Skill;

pub(crate) const GENERIC: &str = "Generic";
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Profile {
    pub(crate) on_cooldown: OnCooldown,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) triggers: Vec<Trigger>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) priority: Vec<PriorityStep>,
    pub(crate) steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Trigger {
    pub(crate) includes: Text,
    pub(crate) skill: Skill,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum OnCooldown {
    #[default]
    Skip,
    Wait,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PriorityStep {
    pub(crate) skill: Skill,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) when: Vec<Condition>,
    #[serde(default, with = "millis", skip_serializing_if = "Duration::is_zero")]
    pub(crate) pause_after: Duration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Step {
    pub(crate) skill: Skill,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) when: Vec<Condition>,
    #[serde(default, with = "millis", skip_serializing_if = "Duration::is_zero")]
    pub(crate) pause_after: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) on_cooldown: Option<OnCooldown>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Condition {
    Hp {
        of: Who,
        #[serde(flatten)]
        bound: Bound<Amount>,
    },
    Mp {
        #[serde(flatten)]
        bound: Bound<Amount>,
    },
    Aura {
        on: Holder,
        name: Text,
        #[serde(default, skip_serializing_if = "Measure::is_stacks")]
        measure: Measure,
        #[serde(flatten)]
        bound: Bound<Count>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Who {
    Me,
    AnyAlly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Holder {
    Me,
    Target,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Measure {
    #[default]
    Stacks,
    Value,
}

impl Measure {
    fn is_stacks(&self) -> bool {
        *self == Measure::Stacks
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Bound<T> {
    AtMost(T),
    AtLeast(T),
}

impl<T: Copy> Bound<T> {
    pub(crate) fn limit(self) -> T {
        match self {
            Bound::AtMost(limit) | Bound::AtLeast(limit) => limit,
        }
    }

    pub(crate) fn admits(self, actual: f64, limit: impl Fn(T) -> f64) -> bool {
        match self {
            Bound::AtMost(most) => actual <= limit(most),
            Bound::AtLeast(least) => actual >= limit(least),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Amount {
    Points(u32),
    Percent(u8),
}

impl Amount {
    pub(crate) fn of(self, current: f64, max: f64) -> (f64, f64) {
        match self {
            Amount::Points(points) => (current, f64::from(points)),
            Amount::Percent(percent) => {
                let ratio = if max > 0.0 {
                    current / max * 100.0
                } else {
                    0.0
                };
                (ratio, f64::from(percent))
            }
        }
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Amount::Points(points) => write!(f, "{points}"),
            Amount::Percent(percent) => write!(f, "{percent}%"),
        }
    }
}

impl Serialize for Amount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Amount::Points(points) => serializer.serialize_u32(*points),
            Amount::Percent(_) => serializer.collect_str(self),
        }
    }
}

impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(f64),
            Text(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Number(number) => points(number).map_err(de::Error::custom),
            Raw::Text(text) => Amount::parse(&text).map_err(de::Error::custom),
        }
    }
}

impl Amount {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        match text.strip_suffix('%') {
            Some(percent) => {
                let percent: f64 = percent
                    .trim()
                    .parse()
                    .map_err(|_| format!("\"{text}\" isn't a percent"))?;
                if percent.fract() != 0.0 || !(0.0..=100.0).contains(&percent) {
                    return Err(format!("{text} isn't a whole percent from 0% to 100%"));
                }
                Ok(Amount::Percent(percent as u8))
            }
            None => points(
                text.parse()
                    .map_err(|_| format!("\"{text}\" isn't a number or a percent"))?,
            ),
        }
    }
}

fn points(number: f64) -> Result<Amount, String> {
    if number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&number) {
        Ok(Amount::Points(number as u32))
    } else {
        Err(format!("{number} isn't a whole number of points"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(try_from = "f64")]
pub(crate) struct Count(f64);

impl Serialize for Count {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.fract() == 0.0 && self.0 <= u64::MAX as f64 {
            serializer.serialize_u64(self.0 as u64)
        } else {
            serializer.serialize_f64(self.0)
        }
    }
}

impl Count {
    pub(crate) fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Count {
    type Error = String;

    fn try_from(count: f64) -> Result<Self, Self::Error> {
        if count.is_finite() && count >= 0.0 {
            Ok(Count(count))
        } else {
            Err(format!("{count} isn't a count of zero or more"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct Text(String);

impl Text {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Text {
    type Error = &'static str;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            Err("this text can't be empty")
        } else {
            Ok(Text(trimmed.to_owned()))
        }
    }
}

impl From<Text> for String {
    fn from(text: Text) -> Self {
        text.0
    }
}

mod millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer, de};

    pub(super) fn serialize<S: Serializer>(
        duration: &Duration,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(duration.as_millis().try_into().unwrap_or(u64::MAX))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Duration, D::Error> {
        let millis = f64::deserialize(deserializer)?;
        if millis.is_finite() && millis >= 0.0 {
            Ok(Duration::from_millis(millis.ceil() as u64))
        } else {
            Err(de::Error::custom(format!(
                "{millis} isn't a duration in milliseconds"
            )))
        }
    }
}

impl Profile {
    pub(crate) fn generic() -> Self {
        Self::from_skills(&super::combat::DEFAULT_ROTATION)
    }

    pub(crate) fn from_skills(skills: &[Skill]) -> Self {
        Self {
            on_cooldown: OnCooldown::Skip,
            triggers: Vec::new(),
            priority: Vec::new(),
            steps: skills
                .iter()
                .map(|&skill| Step {
                    skill,
                    when: Vec::new(),
                    pause_after: Duration::ZERO,
                    on_cooldown: None,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum ProfileChoice {
    Named(String),
    Inline(Profile),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ProfileError {
    #[error("no combat profile is named \"{0}\"")]
    Unknown(String),
    #[error("pass either skills or a profile, not both")]
    Conflict,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Library {
    profiles: Vec<(String, Profile)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Issue {
    pub(crate) name: String,
    pub(crate) problem: String,
}

#[derive(Serialize, Deserialize)]
struct File<P> {
    version: u32,
    profiles: BTreeMap<String, P>,
}

pub(crate) fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

pub(crate) fn check_name(
    name: &str,
    taken: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A profile needs a name.".into());
    }
    if same_name(name, GENERIC) {
        return Err(format!("\"{GENERIC}\" is built in. Pick another name."));
    }
    if taken
        .into_iter()
        .any(|other| same_name(other.as_ref(), name))
    {
        return Err(format!("Another profile is already named \"{name}\"."));
    }
    Ok(name.to_owned())
}

impl Library {
    pub(crate) fn parse(bytes: &[u8]) -> Result<(Self, Vec<Issue>), String> {
        let file: File<serde_json::Value> =
            serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        if file.version != VERSION {
            return Err(format!("version {} isn't one Mira reads", file.version));
        }
        let mut library = Library::default();
        let mut issues = Vec::new();
        for (name, value) in file.profiles {
            let checked = check_name(&name, library.names()).and_then(|name| {
                serde_json::from_value::<Profile>(value)
                    .map(|profile| (name, profile))
                    .map_err(|error| error.to_string())
            });
            match checked {
                Ok((name, profile)) => library.profiles.push((name, profile)),
                Err(problem) => issues.push(Issue { name, problem }),
            }
        }
        library.sort();
        Ok((library, issues))
    }

    pub(crate) fn to_json(&self) -> Vec<u8> {
        let file = File {
            version: VERSION,
            profiles: self
                .profiles
                .iter()
                .map(|(name, profile)| (name.clone(), profile))
                .collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&file).expect("profiles always serialize");
        bytes.push(b'\n');
        bytes
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.profiles.iter().map(|(name, _)| name.as_str())
    }

    pub(crate) fn get(&self, name: &str) -> Option<Profile> {
        if same_name(name, GENERIC) {
            return Some(Profile::generic());
        }
        self.profiles
            .iter()
            .find(|(saved, _)| same_name(saved, name))
            .map(|(_, profile)| profile.clone())
    }

    pub(crate) fn put(&mut self, previous: Option<&str>, name: String, profile: Profile) {
        self.profiles.retain(|(saved, _)| {
            !same_name(saved, &name) && previous.is_none_or(|previous| !same_name(saved, previous))
        });
        self.profiles.push((name, profile));
        self.sort();
    }

    pub(crate) fn remove(&mut self, name: &str) {
        self.profiles.retain(|(saved, _)| !same_name(saved, name));
    }

    fn sort(&mut self) {
        self.profiles.sort_by_key(|(name, _)| name.to_lowercase());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn skill(slot: u8) -> Skill {
        Skill::try_from(slot).unwrap()
    }

    fn boss_healer() -> serde_json::Value {
        json!({
            "onCooldown": "skip",
            "triggers": [{ "includes": "prepares", "skill": 3 }],
            "priority": [
                { "skill": 5, "when": [{ "type": "hp", "of": "me", "atMost": "30%" }] },
                { "skill": 2, "when": [{ "type": "hp", "of": "anyAlly", "atMost": 6000 }] }
            ],
            "steps": [
                { "skill": 1 },
                {
                    "skill": 4,
                    "onCooldown": "wait",
                    "pauseAfter": 500,
                    "when": [
                        { "type": "aura", "on": "target", "name": "Burn", "atMost": 0 },
                        { "type": "aura", "on": "me", "name": "Spirit Power", "measure": "value", "atLeast": 40 },
                        { "type": "mp", "atLeast": "40%" }
                    ]
                }
            ]
        })
    }

    #[test]
    fn a_profile_reads_and_writes_the_documented_shape() {
        let profile: Profile = serde_json::from_value(boss_healer()).unwrap();
        assert_eq!(profile.priority[1].skill, skill(2));
        assert_eq!(
            profile.priority[1].when,
            [Condition::Hp {
                of: Who::AnyAlly,
                bound: Bound::AtMost(Amount::Points(6000)),
            }]
        );
        assert_eq!(profile.steps[1].on_cooldown, Some(OnCooldown::Wait));
        assert_eq!(profile.steps[1].pause_after, Duration::from_millis(500));
        assert_eq!(serde_json::to_value(&profile).unwrap(), boss_healer());
    }

    #[test]
    fn values_outside_their_range_are_refused() {
        for (condition, refused) in [
            (json!({ "type": "hp", "of": "me", "atMost": "101%" }), true),
            (json!({ "type": "hp", "of": "me", "atMost": "12.5%" }), true),
            (json!({ "type": "mp", "atLeast": -1 }), true),
            (json!({ "type": "mp", "atLeast": "0%" }), false),
            (
                json!({ "type": "aura", "on": "me", "name": "  ", "atMost": 0 }),
                true,
            ),
            (
                json!({ "type": "aura", "on": "me", "name": "Burn", "atMost": -1 }),
                true,
            ),
            (
                json!({ "type": "aura", "on": "target", "name": "Burn", "atLeast": 2.5 }),
                false,
            ),
            (
                json!({ "type": "message", "includes": "slam", "within": 1000 }),
                true,
            ),
            (json!({ "type": "hp", "of": "target", "atMost": 10 }), true),
        ] {
            let parsed = serde_json::from_value::<Condition>(condition.clone());
            assert_eq!(parsed.is_err(), refused, "{condition}");
        }
        assert!(
            serde_json::from_value::<Profile>(
                json!({ "onCooldown": "skip", "steps": [{ "skill": 6 }] })
            )
            .is_err()
        );
        assert!(serde_json::from_value::<Profile>(json!({ "onCooldown": "skip", "priority": [{ "skill": 1, "onCooldown": "wait" }], "steps": [] })).is_err());
        for trigger in [
            json!({ "includes": " ", "skill": 3 }),
            json!({ "includes": "slam", "skill": 9 }),
            json!({ "skill": 3 }),
        ] {
            assert!(
                serde_json::from_value::<Profile>(
                    json!({ "onCooldown": "skip", "triggers": [trigger], "steps": [] })
                )
                .is_err(),
                "{trigger}"
            );
        }
    }

    #[test]
    fn a_library_keeps_good_profiles_and_reports_the_rest() {
        let bytes = serde_json::to_vec(&json!({
            "version": 1,
            "profiles": {
                "Boss healer": boss_healer(),
                "boss HEALER": boss_healer(),
                "generic": { "onCooldown": "skip", "steps": [] },
                "Broken": { "onCooldown": "sometimes", "steps": [] },
                "ArchMage farm": { "onCooldown": "wait", "steps": [{ "skill": 1 }] }
            }
        }))
        .unwrap();
        let (library, issues) = Library::parse(&bytes).unwrap();
        assert_eq!(
            library.names().collect::<Vec<_>>(),
            ["ArchMage farm", "Boss healer"]
        );
        assert_eq!(
            issues
                .iter()
                .map(|issue| issue.name.as_str())
                .collect::<Vec<_>>(),
            ["Broken", "boss HEALER", "generic"]
        );
        assert!(library.get("archmage FARM").is_some());
        assert_eq!(library.get("Generic"), Some(Profile::generic()));
        assert_eq!(library.get("Nope"), None);
        let (again, issues) = Library::parse(&library.to_json()).unwrap();
        assert!(issues.is_empty());
        assert_eq!(again, library);
        assert!(Library::parse(br#"{"version":2,"profiles":{}}"#).is_err());
    }

    #[test]
    fn saving_under_a_new_name_replaces_the_old_entry() {
        let mut library = Library::default();
        library.put(None, "Solo".into(), Profile::generic());
        library.put(
            Some("solo"),
            "Solo DoT".into(),
            Profile::from_skills(&[skill(3)]),
        );
        assert_eq!(library.names().collect::<Vec<_>>(), ["Solo DoT"]);
        assert_eq!(
            library.get("solo dot"),
            Some(Profile::from_skills(&[skill(3)]))
        );
        library.remove("SOLO DOT");
        assert_eq!(library.names().count(), 0);
    }

    #[test]
    fn names_must_be_present_unique_and_not_the_built_in_one() {
        assert_eq!(check_name("  Boss  ", ["Solo"]), Ok("Boss".into()));
        assert!(check_name(" ", ["Solo"]).is_err());
        assert!(check_name("generic", ["Solo"]).is_err());
        assert!(check_name("SOLO", ["Solo"]).is_err());
    }
}
