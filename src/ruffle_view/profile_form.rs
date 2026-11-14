use std::time::Duration;

use crate::domain::{
    combat::Skill,
    profiles::{
        Amount, Bound, Condition, Count, Holder, Measure, OnCooldown, PriorityStep, Profile, Step,
        Text, Trigger, Who,
    },
    rotation::Lane,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Item {
    Trigger(usize),
    Step(Lane, usize),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Form {
    pub(super) on_cooldown: OnCooldown,
    pub(super) triggers: Vec<FormTrigger>,
    pub(super) priority: Vec<FormStep>,
    pub(super) steps: Vec<FormStep>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct FormTrigger {
    pub(super) includes: String,
    pub(super) skill: Skill,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct FormStep {
    pub(super) skill: Skill,
    pub(super) on_cooldown: Option<OnCooldown>,
    pub(super) pause: String,
    pub(super) when: Vec<Rule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Subject {
    MyHp,
    AllyHp,
    MyMp,
    AuraOnMe,
    AuraOnTarget,
}

impl Subject {
    pub(super) const ALL: [Subject; 5] = [
        Subject::MyHp,
        Subject::AllyHp,
        Subject::MyMp,
        Subject::AuraOnMe,
        Subject::AuraOnTarget,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Subject::MyHp => "Self HP",
            Subject::AllyHp => "Ally HP",
            Subject::MyMp => "Self MP",
            Subject::AuraOnMe => "Self aura",
            Subject::AuraOnTarget => "Target aura",
        }
    }

    pub(super) fn aura(self) -> bool {
        matches!(self, Subject::AuraOnMe | Subject::AuraOnTarget)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Rule {
    pub(super) subject: Subject,
    pub(super) at_most: bool,
    pub(super) value: String,
    pub(super) percent: bool,
    pub(super) name: String,
    pub(super) measure: Measure,
}

impl Default for Rule {
    fn default() -> Self {
        Self::new(Subject::MyHp)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Problem {
    pub(super) at: Item,
    pub(super) message: String,
}

impl FormTrigger {
    pub(super) fn new(skill: Skill) -> Self {
        Self {
            includes: String::new(),
            skill,
        }
    }

    pub(super) fn summary(&self) -> String {
        match self.includes.trim() {
            "" => "When a message appears".into(),
            text => format!("When \u{201c}{text}\u{201d} appears"),
        }
    }

    fn parse(&self) -> Result<Trigger, String> {
        Ok(Trigger {
            includes: text(
                &self.includes,
                "A trigger needs the message text to look for.",
            )?,
            skill: self.skill,
        })
    }
}

impl FormStep {
    pub(super) fn new(skill: Skill) -> Self {
        Self {
            skill,
            on_cooldown: None,
            pause: String::new(),
            when: Vec::new(),
        }
    }

    pub(super) fn summary(&self) -> String {
        if self.when.is_empty() {
            return "Always".into();
        }
        capitalized(
            &self
                .when
                .iter()
                .map(Rule::summary)
                .collect::<Vec<_>>()
                .join(", "),
        )
    }

    fn parse(&self) -> Result<(Vec<Condition>, Duration), String> {
        let when = self
            .when
            .iter()
            .map(Rule::parse)
            .collect::<Result<_, _>>()?;
        Ok((when, millis(&self.pause, "The pause")?))
    }
}

impl Rule {
    pub(super) fn new(subject: Subject) -> Self {
        let aura = subject.aura();
        Self {
            subject,
            at_most: !aura,
            value: if aura { "1" } else { "50" }.into(),
            percent: true,
            name: String::new(),
            measure: Measure::Stacks,
        }
    }

    fn sign(&self) -> &'static str {
        if self.at_most { "≤" } else { "≥" }
    }

    fn bound<T>(&self, limit: T) -> Bound<T> {
        if self.at_most {
            Bound::AtMost(limit)
        } else {
            Bound::AtLeast(limit)
        }
    }

    pub(super) fn summary(&self) -> String {
        let value = self.value.trim();
        let sign = self.sign();
        let aura = |on: &str| {
            let name = match self.name.trim() {
                "" => "an aura",
                name => name,
            };
            match self.measure {
                Measure::Stacks if value == "1" => format!("{name} on {on} {sign} 1 stack"),
                Measure::Stacks => format!("{name} on {on} {sign} {value} stacks"),
                Measure::Value => format!("{name} on {on} value {sign} {value}"),
            }
        };
        let unit = if self.percent { "%" } else { "" };
        match self.subject {
            Subject::MyHp | Subject::AllyHp | Subject::MyMp => {
                format!("{} {sign} {value}{unit}", self.subject.label())
            }
            Subject::AuraOnMe => aura("self"),
            Subject::AuraOnTarget => aura("target"),
        }
    }

    fn parse(&self) -> Result<Condition, String> {
        let hp = |of| -> Result<Condition, String> {
            Ok(Condition::Hp {
                of,
                bound: self.bound(self.amount("HP")?),
            })
        };
        let aura = |on| -> Result<Condition, String> {
            Ok(Condition::Aura {
                on,
                name: text(&self.name, "An aura condition needs the aura's name.")?,
                measure: self.measure,
                bound: self.bound(count(&self.value)?),
            })
        };
        match self.subject {
            Subject::MyHp => hp(Who::Me),
            Subject::AllyHp => hp(Who::AnyAlly),
            Subject::MyMp => Ok(Condition::Mp {
                bound: self.bound(self.amount("MP")?),
            }),
            Subject::AuraOnMe => aura(Holder::Me),
            Subject::AuraOnTarget => aura(Holder::Target),
        }
    }

    fn amount(&self, what: &str) -> Result<Amount, String> {
        let value = self.value.trim();
        let suffix = if self.percent { "%" } else { "" };
        match (self.percent, Amount::parse(&format!("{value}{suffix}"))) {
            (true, Ok(amount @ Amount::Percent(_))) | (false, Ok(amount @ Amount::Points(_))) => {
                Ok(amount)
            }
            (true, _) => Err(format!("{what} needs a whole percent from 0 to 100.")),
            (false, _) => Err(format!("{what} needs a whole number of points.")),
        }
    }

    fn of(condition: &Condition) -> Self {
        let defaults = Rule::default();
        let vital = |subject, bound: &Bound<Amount>| {
            let (value, percent) = match bound.limit() {
                Amount::Points(points) => (points.to_string(), false),
                Amount::Percent(percent) => (percent.to_string(), true),
            };
            Rule {
                subject,
                at_most: at_most(bound),
                value,
                percent,
                ..Rule::default()
            }
        };
        match condition {
            Condition::Hp { of, bound } => vital(
                match of {
                    Who::Me => Subject::MyHp,
                    Who::AnyAlly => Subject::AllyHp,
                },
                bound,
            ),
            Condition::Mp { bound } => vital(Subject::MyMp, bound),
            Condition::Aura {
                on,
                name,
                measure,
                bound,
            } => Rule {
                subject: match on {
                    Holder::Me => Subject::AuraOnMe,
                    Holder::Target => Subject::AuraOnTarget,
                },
                at_most: at_most(bound),
                value: bound.limit().get().to_string(),
                name: name.as_str().to_owned(),
                measure: *measure,
                ..defaults
            },
        }
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn at_most<T>(bound: &Bound<T>) -> bool {
    matches!(bound, Bound::AtMost(_))
}

fn count(value: &str) -> Result<Count, String> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .and_then(|count| Count::try_from(count).ok())
        .ok_or_else(|| "An aura needs a number of zero or more.".to_owned())
}

fn text(value: &str, missing: &str) -> Result<Text, String> {
    Text::try_from(value.to_owned()).map_err(|_| missing.to_owned())
}

fn millis(value: &str, what: &str) -> Result<Duration, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(Duration::ZERO);
    }
    value
        .parse::<u64>()
        .map(Duration::from_millis)
        .map_err(|_| format!("{what} needs a whole number of milliseconds."))
}

fn pause_text(pause: Duration) -> String {
    if pause.is_zero() {
        String::new()
    } else {
        pause.as_millis().to_string()
    }
}

impl Form {
    pub(super) fn of(profile: &Profile) -> Self {
        let step = |skill, on_cooldown, pause: Duration, when: &[Condition]| FormStep {
            skill,
            on_cooldown,
            pause: pause_text(pause),
            when: when.iter().map(Rule::of).collect(),
        };
        Self {
            on_cooldown: profile.on_cooldown,
            triggers: profile
                .triggers
                .iter()
                .map(|trigger| FormTrigger {
                    includes: trigger.includes.as_str().to_owned(),
                    skill: trigger.skill,
                })
                .collect(),
            priority: profile
                .priority
                .iter()
                .map(|priority| step(priority.skill, None, priority.pause_after, &priority.when))
                .collect(),
            steps: profile
                .steps
                .iter()
                .map(|rotation| {
                    step(
                        rotation.skill,
                        rotation.on_cooldown,
                        rotation.pause_after,
                        &rotation.when,
                    )
                })
                .collect(),
        }
    }

    pub(super) fn lane(&self, lane: Lane) -> &[FormStep] {
        match lane {
            Lane::Priority => &self.priority,
            Lane::Rotation => &self.steps,
        }
    }

    pub(super) fn lane_mut(&mut self, lane: Lane) -> &mut Vec<FormStep> {
        match lane {
            Lane::Priority => &mut self.priority,
            Lane::Rotation => &mut self.steps,
        }
    }

    pub(super) fn parse(&self) -> Result<Profile, Problem> {
        let problem = |at| move |message| Problem { at, message };
        let triggers = self
            .triggers
            .iter()
            .enumerate()
            .map(|(index, trigger)| trigger.parse().map_err(problem(Item::Trigger(index))))
            .collect::<Result<_, _>>()?;
        let priority = self
            .priority
            .iter()
            .enumerate()
            .map(|(index, step)| {
                let (when, pause_after) = step
                    .parse()
                    .map_err(problem(Item::Step(Lane::Priority, index)))?;
                Ok(PriorityStep {
                    skill: step.skill,
                    when,
                    pause_after,
                })
            })
            .collect::<Result<_, Problem>>()?;
        let steps = self
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| {
                let (when, pause_after) = step
                    .parse()
                    .map_err(problem(Item::Step(Lane::Rotation, index)))?;
                Ok(Step {
                    skill: step.skill,
                    when,
                    pause_after,
                    on_cooldown: step.on_cooldown,
                })
            })
            .collect::<Result<_, Problem>>()?;
        Ok(Profile {
            on_cooldown: self.on_cooldown,
            triggers,
            priority,
            steps,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn profile(json: serde_json::Value) -> Profile {
        serde_json::from_value(json).unwrap()
    }

    fn skill(slot: u8) -> Skill {
        Skill::try_from(slot).unwrap()
    }

    #[test]
    fn a_saved_profile_comes_back_unchanged_through_the_form() {
        let saved = profile(json!({
            "onCooldown": "wait",
            "triggers": [{ "includes": "prepares", "skill": 3 }],
            "priority": [
                { "skill": 5, "when": [{ "type": "hp", "of": "me", "atMost": "30%" }] },
                { "skill": 2, "pauseAfter": 400, "when": [{ "type": "hp", "of": "anyAlly", "atMost": 6000 }] }
            ],
            "steps": [
                { "skill": 1, "onCooldown": "skip" },
                { "skill": 4, "when": [
                    { "type": "aura", "on": "target", "name": "Burn", "atMost": 0 },
                    { "type": "aura", "on": "me", "name": "Spirit Power", "measure": "value", "atLeast": 12.5 },
                    { "type": "mp", "atLeast": "40%" }
                ] }
            ]
        }));
        let form = Form::of(&saved);
        assert_eq!(form.parse(), Ok(saved));
        assert_eq!(
            form.triggers[0].summary(),
            "When \u{201c}prepares\u{201d} appears"
        );
        assert_eq!(form.priority[1].summary(), "Ally HP ≤ 6000");
        assert_eq!(
            form.steps[1].summary(),
            "Burn on target ≤ 0 stacks, Spirit Power on self value ≥ 12.5, Self MP ≥ 40%"
        );
        assert_eq!(form.steps[0].summary(), "Always");
    }

    #[test]
    fn a_bad_field_names_its_item_and_what_it_needs() {
        let mut form = Form::of(&Profile::generic());
        form.steps[2].when.push(Rule {
            subject: Subject::AuraOnTarget,
            value: "2".into(),
            ..Rule::default()
        });
        assert_eq!(
            form.parse(),
            Err(Problem {
                at: Item::Step(Lane::Rotation, 2),
                message: "An aura condition needs the aura's name.".into(),
            })
        );
        form.steps[2].when[0].name = "Burn".into();
        form.steps[2].when[0].value = "two".into();
        assert!(form.parse().is_err());
        form.steps[2].when[0].value = "2".into();
        form.priority.push(FormStep {
            pause: "soon".into(),
            ..FormStep::new(skill(5))
        });
        assert_eq!(
            form.parse().map_err(|problem| problem.at),
            Err(Item::Step(Lane::Priority, 0))
        );
        form.priority[0].pause = " ".into();
        form.priority[0].when.push(Rule {
            value: "120".into(),
            ..Rule::default()
        });
        assert!(form.parse().is_err());
        form.priority[0].when[0].percent = false;
        form.priority[0].when[0].value = "60%".into();
        assert!(form.parse().is_err());
        form.priority[0].when[0].percent = true;
        form.priority[0].when[0].value = "45".into();
        form.triggers.push(FormTrigger::new(skill(3)));
        assert_eq!(
            form.parse().map_err(|problem| problem.at),
            Err(Item::Trigger(0))
        );
        form.triggers[0].includes = "slam".into();
        assert!(form.parse().is_ok());
    }
}
