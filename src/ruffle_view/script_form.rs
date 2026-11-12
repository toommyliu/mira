use std::path::{Path, PathBuf};

use gpui::SharedString;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Text { required: bool },
    Number,
    Toggle,
    Choice(&'static [&'static str]),
}

pub(super) struct FieldSpec {
    pub(super) label: &'static str,
    pub(super) description: Option<&'static str>,
    pub(super) kind: Kind,
    default: &'static str,
}

const HUNT: &[FieldSpec] = &[
    FieldSpec {
        label: "Monster",
        description: Some("Name as it appears in game."),
        kind: Kind::Text { required: true },
        default: "Frogzard",
    },
    FieldSpec {
        label: "Kills",
        description: None,
        kind: Kind::Number,
        default: "5",
    },
    FieldSpec {
        label: "Use potions",
        description: Some("Drink a health potion below 40% HP."),
        kind: Kind::Toggle,
        default: "",
    },
];

const DAILY: &[FieldSpec] = &[
    FieldSpec {
        label: "Dailies",
        description: None,
        kind: Kind::Choice(&["All", "Class", "Reputation", "Gold"]),
        default: "All",
    },
    FieldSpec {
        label: "Bank rewards",
        description: Some("Move finished rewards to the bank."),
        kind: Kind::Toggle,
        default: "on",
    },
];

const COMBAT: &[FieldSpec] = &[
    FieldSpec {
        label: "Target",
        description: Some("Leave empty to attack anything nearby."),
        kind: Kind::Text { required: false },
        default: "",
    },
    FieldSpec {
        label: "Skill set",
        description: None,
        kind: Kind::Choice(&["Default", "Burst", "Support"]),
        default: "Default",
    },
];

pub(super) fn sample_fields(script: &Path) -> &'static [FieldSpec] {
    match script.file_name().and_then(|name| name.to_str()) {
        Some("hunt.ts") => HUNT,
        Some("daily.ts") => DAILY,
        Some("combat.ts") => COMBAT,
        _ => &[],
    }
}

#[derive(Clone, PartialEq)]
pub(super) enum Value {
    Text(SharedString),
    Toggle(bool),
    Choice(usize),
}

impl FieldSpec {
    pub(super) fn initial(&self) -> Value {
        match self.kind {
            Kind::Text { .. } | Kind::Number => Value::Text(self.default.into()),
            Kind::Toggle => Value::Toggle(self.default == "on"),
            Kind::Choice(options) => Value::Choice(
                options
                    .iter()
                    .position(|option| *option == self.default)
                    .unwrap_or(0),
            ),
        }
    }

    pub(super) fn error(&self, value: &Value) -> Option<SharedString> {
        let Value::Text(text) = value else {
            return None;
        };
        match self.kind {
            Kind::Text { required: true } if text.trim().is_empty() => Some("Required".into()),
            Kind::Number if text.trim().parse::<u32>().is_err() => {
                Some("Enter a whole number".into())
            }
            _ => None,
        }
    }
}

pub(super) fn summary(fields: &[FieldSpec], values: &[Value]) -> SharedString {
    fields
        .iter()
        .zip(values)
        .filter_map(|(field, value)| match (field.kind, value) {
            (Kind::Choice(options), Value::Choice(index)) => Some(options[*index].to_owned()),
            (_, Value::Toggle(true)) => Some(field.label.to_owned()),
            (_, Value::Text(text)) if !text.trim().is_empty() => Some(text.trim().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" · ")
        .into()
}

pub(super) struct Queued {
    pub(super) id: u64,
    pub(super) path: PathBuf,
    pub(super) name: SharedString,
}

#[derive(Default)]
pub(super) struct TabState {
    pub(super) queue: Vec<Queued>,
    pub(super) queue_running: bool,
    pub(super) safe_start: bool,
    pub(super) restart: bool,
    pub(super) room: usize,
    pub(super) room_number: SharedString,
    pub(super) room_error: bool,
}

impl TabState {
    pub(super) fn room_ready(&self) -> bool {
        self.room != SPECIFIC_ROOM || room_valid(&self.room_number)
    }
}

pub(super) fn room_valid(input: &str) -> bool {
    !input.trim().is_empty() && room_note(input).is_ok()
}

pub(super) const ROOMS: [&str; 3] = ["Public rooms", "Random private room", "Specific room"];
pub(super) const SPECIFIC_ROOM: usize = 2;

pub(super) fn room_note(input: &str) -> Result<&'static str, &'static str> {
    const RANGE: &str = "Enter a room number from 1 to 99,999.";
    let input = input.trim();
    if input.is_empty() {
        return Ok(RANGE);
    }
    if !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(RANGE);
    }
    match input.parse::<u32>() {
        Ok(1..=1_000) => Ok("This number selects a public room."),
        Ok(1_001..=99_999) => Ok("This number selects a private room."),
        _ => Err(RANGE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_specific_room_needs_a_number_from_1_to_99999() {
        let room = |room, number: &str| TabState {
            room,
            room_number: number.to_owned().into(),
            ..TabState::default()
        };
        assert!(room(0, "").room_ready());
        for number in ["1", "1000", "1001", "99999", " 42 "] {
            assert!(room(SPECIFIC_ROOM, number).room_ready(), "{number}");
        }
        for number in ["", "0", "+5", "12a", "100000"] {
            assert!(!room(SPECIFIC_ROOM, number).room_ready(), "{number}");
        }
    }
}
