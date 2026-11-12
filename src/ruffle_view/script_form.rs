use std::path::{Path, PathBuf};

use gpui::SharedString;

use crate::account_settings::{RoomNumber, Rooms};

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
}

pub(super) const ROOMS: [&str; 3] = ["Public rooms", "Random private room", "Specific room"];
pub(super) const SPECIFIC_ROOM: usize = 2;

pub(super) fn room_choice(rooms: Rooms) -> usize {
    match rooms {
        Rooms::Public => 0,
        Rooms::RandomPrivate => 1,
        Rooms::Specific { .. } => SPECIFIC_ROOM,
    }
}

pub(super) fn room_note(input: &str) -> Result<&'static str, &'static str> {
    const RANGE: &str = "Enter a room number from 1 to 99,999.";
    if input.trim().is_empty() {
        return Ok(RANGE);
    }
    match RoomNumber::parse(input) {
        Some(room) if room.is_private() => Ok("This number selects a private room."),
        Some(_) => Ok("This number selects a public room."),
        None => Err(RANGE),
    }
}
