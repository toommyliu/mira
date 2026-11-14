use gpui::SharedString;
use serde_json::Value as Json;

use crate::{
    account_settings::{RoomNumber, Rooms},
    scripting::{Definition, Field, Kind, Launch, Values, check_number, number},
};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Draft {
    Text(SharedString),
    Toggle(bool),
    Choice(Option<usize>),
    Choices(Vec<bool>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FieldDraft {
    pub(super) draft: Draft,
    pub(super) stale: Option<SharedString>,
}

impl FieldDraft {
    fn fresh(draft: Draft) -> Self {
        Self { draft, stale: None }
    }
}

pub(super) fn drafts(definition: &Definition, saved: &Values) -> Vec<FieldDraft> {
    definition
        .fields()
        .iter()
        .map(|field| match saved.get(&field.key) {
            None => FieldDraft::fresh(starting(field)),
            Some(value) => match field.check(value) {
                Ok(value) => FieldDraft::fresh(draft_of(field, &value)),
                Err(reason) => FieldDraft {
                    draft: starting(field),
                    stale: Some(format!("Saved value {value} {reason}").into()),
                },
            },
        })
        .collect()
}

pub(super) fn starting(field: &Field) -> Draft {
    match field.initial() {
        Some(value) => draft_of(field, &value),
        None => match &field.kind {
            Kind::Select { .. } => Draft::Choice(None),
            _ => Draft::Text(SharedString::default()),
        },
    }
}

fn draft_of(field: &Field, value: &Json) -> Draft {
    match (&field.kind, value) {
        (Kind::Boolean { .. }, Json::Bool(on)) => Draft::Toggle(*on),
        (Kind::Select { options, .. }, Json::String(choice)) => {
            Draft::Choice(options.iter().position(|option| option == choice))
        }
        (Kind::MultiSelect { options, .. }, Json::Array(chosen)) => Draft::Choices(
            options
                .iter()
                .map(|option| chosen.iter().any(|item| item.as_str() == Some(option)))
                .collect(),
        ),
        (_, Json::String(text)) => Draft::Text(text.clone().into()),
        (_, Json::Number(value)) => Draft::Text(
            value
                .as_f64()
                .map_or_else(|| value.to_string(), |value| value.to_string())
                .into(),
        ),
        (_, value) => Draft::Text(value.to_string().into()),
    }
}

pub(super) fn value(field: &Field, draft: &Draft) -> Result<Json, SharedString> {
    match (&field.kind, draft) {
        (Kind::Text { required, .. }, Draft::Text(text)) => {
            if *required && text.trim().is_empty() {
                Err("Required".into())
            } else {
                Ok(Json::from(text.as_ref()))
            }
        }
        (
            Kind::Number {
                integer, min, max, ..
            },
            Draft::Text(text),
        ) => {
            let enter = if *integer {
                "Enter a whole number"
            } else {
                "Enter a number"
            };
            let parsed = parse_number(text).ok_or(enter)?;
            check_number(parsed, *integer, *min, *max).map_err(|reason| capitalize(&reason))?;
            Ok(number(parsed))
        }
        (Kind::Boolean { .. }, Draft::Toggle(on)) => Ok(Json::from(*on)),
        (Kind::Select { options, .. }, Draft::Choice(choice)) => choice
            .and_then(|index| options.get(index))
            .map(|option| Json::from(option.as_str()))
            .ok_or_else(|| "Choose one".into()),
        (Kind::MultiSelect { options, .. }, Draft::Choices(chosen)) => Ok(Json::from(
            options
                .iter()
                .zip(chosen)
                .filter(|(_, chosen)| **chosen)
                .map(|(option, _)| option.as_str())
                .collect::<Vec<_>>(),
        )),
        _ => Err("Enter a value".into()),
    }
}

pub(super) fn error(field: &Field, draft: &FieldDraft) -> Option<SharedString> {
    draft
        .stale
        .clone()
        .or_else(|| value(field, &draft.draft).err())
}

pub(super) fn values(definition: &Definition, drafts: &[FieldDraft]) -> Option<Values> {
    definition
        .fields()
        .iter()
        .zip(drafts)
        .map(|(field, draft)| {
            if draft.stale.is_some() {
                return None;
            }
            Some((field.key.clone(), value(field, &draft.draft).ok()?))
        })
        .collect()
}

fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim();
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let plain = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() && fraction.is_empty() || !plain(whole) || !plain(fraction) {
        return None;
    }
    text.parse().ok().filter(|value: &f64| value.is_finite())
}

fn capitalize(text: &str) -> SharedString {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), characters.as_str()).into(),
        None => SharedString::default(),
    }
}

pub(super) fn shown(field: &Field, value: &Json) -> SharedString {
    match value {
        Json::Bool(true) => "On".into(),
        Json::Bool(false) => "Off".into(),
        Json::String(text) if text.is_empty() && matches!(field.kind, Kind::Text { .. }) => {
            "Empty".into()
        }
        Json::String(text) => text.clone().into(),
        Json::Array(items) if items.is_empty() => "None".into(),
        Json::Array(items) => items
            .iter()
            .filter_map(Json::as_str)
            .collect::<Vec<_>>()
            .join(", ")
            .into(),
        value => value.to_string().into(),
    }
}

pub(super) struct Queued {
    pub(super) name: SharedString,
    #[expect(dead_code, reason = "the queue runner starts queued launches")]
    pub(super) launch: Launch,
    pub(super) summary: SharedString,
    pub(super) rows: Vec<(SharedString, SharedString)>,
}

impl Queued {
    pub(super) fn new(
        name: SharedString,
        launch: Launch,
        definition: Option<&Definition>,
        values: &Values,
    ) -> Self {
        let rows: Vec<(SharedString, SharedString)> = definition
            .map(Definition::fields)
            .unwrap_or_default()
            .iter()
            .filter_map(|field| {
                let value = values.get(&field.key)?;
                Some((field.label.clone().into(), shown(field, value)))
            })
            .collect();
        let summary = definition
            .map(Definition::fields)
            .unwrap_or_default()
            .iter()
            .filter_map(|field| match (&field.kind, values.get(&field.key)?) {
                (Kind::Boolean { .. }, Json::Bool(true)) => Some(field.label.clone()),
                (Kind::Boolean { .. }, _) => None,
                (_, Json::String(text)) if text.trim().is_empty() => None,
                (_, Json::Array(items)) if items.is_empty() => None,
                (_, value) => Some(shown(field, value).trim().to_owned()),
            })
            .collect::<Vec<_>>()
            .join(" · ")
            .into();
        Self {
            name,
            launch,
            summary,
            rows,
        }
    }
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::scripting::extract;

    fn definition() -> Definition {
        extract(
            "hunt.ts",
            Path::new("hunt.ts"),
            r#"export const inputs = {
              monster: { type: "string", label: "Monster", required: true, default: "Frogzard" },
              kills: { type: "number", label: "Kills", integer: true, min: 1 },
              rate: { type: "number", label: "Rate", default: 0.5 },
              potions: { type: "boolean", label: "Potions" },
              day: { type: "select", label: "Day", options: ["All", "Class"] },
              buy: { type: "select", label: "Buy", options: ["A", "B", "C"], multiple: true, default: ["C"] },
            };"#,
        )
        .unwrap()
        .unwrap()
    }

    fn saved(value: Json) -> Values {
        value.as_object().unwrap().clone()
    }

    fn errors(definition: &Definition, drafts: &[FieldDraft]) -> Vec<Option<SharedString>> {
        definition
            .fields()
            .iter()
            .zip(drafts)
            .map(|(field, draft)| error(field, draft))
            .collect()
    }

    #[test]
    fn a_new_form_starts_from_defaults_and_needs_the_fields_without_one() {
        let definition = definition();
        let drafts = drafts(&definition, &Values::new());
        assert_eq!(
            drafts
                .iter()
                .map(|draft| draft.draft.clone())
                .collect::<Vec<_>>(),
            vec![
                Draft::Text("Frogzard".into()),
                Draft::Text("".into()),
                Draft::Text("0.5".into()),
                Draft::Toggle(false),
                Draft::Choice(None),
                Draft::Choices(vec![false, false, true]),
            ]
        );
        assert_eq!(
            errors(&definition, &drafts),
            vec![
                None,
                Some("Enter a whole number".into()),
                None,
                None,
                Some("Choose one".into()),
                None
            ]
        );
        assert_eq!(values(&definition, &drafts), None);
    }

    #[test]
    fn a_filled_form_produces_the_values_a_run_receives() {
        let definition = definition();
        let mut drafts = drafts(&definition, &Values::new());
        drafts[1].draft = Draft::Text(" 12 ".into());
        drafts[2].draft = Draft::Text("-.25".into());
        drafts[4].draft = Draft::Choice(Some(1));
        drafts[5].draft = Draft::Choices(vec![true, false, true]);
        assert_eq!(
            values(&definition, &drafts),
            Some(saved(json!({
                "monster": "Frogzard",
                "kills": 12,
                "rate": -0.25,
                "potions": false,
                "day": "Class",
                "buy": ["A", "C"],
            })))
        );
        for (text, expected) in [
            ("1.5", "Must be a whole number"),
            ("0", "Must be at least 1"),
            ("1e3", "Enter a whole number"),
            ("NaN", "Enter a whole number"),
            ("five", "Enter a whole number"),
        ] {
            drafts[1].draft = Draft::Text(text.into());
            assert_eq!(
                error(&definition.fields()[1], &drafts[1]),
                Some(expected.into()),
                "{text}"
            );
        }
        drafts[0].draft = Draft::Text("  ".into());
        assert_eq!(
            error(&definition.fields()[0], &drafts[0]),
            Some("Required".into())
        );
    }

    #[test]
    fn saved_values_that_no_longer_fit_block_the_form_instead_of_using_defaults() {
        let definition = definition();
        let drafts = drafts(
            &definition,
            &saved(
                json!({ "kills": 0, "rate": 0.0000001, "day": "Gold", "buy": ["B"], "potions": true, "gone": 1 }),
            ),
        );
        assert_eq!(drafts[2].draft, Draft::Text("0.0000001".into()));
        assert_eq!(drafts[1].draft, Draft::Text("".into()));
        assert_eq!(drafts[3].draft, Draft::Toggle(true));
        assert_eq!(drafts[5].draft, Draft::Choices(vec![false, true, false]));
        assert_eq!(
            errors(&definition, &drafts),
            vec![
                None,
                Some("Saved value 0 must be at least 1".into()),
                None,
                None,
                Some("Saved value \"Gold\" isn't one of the options".into()),
                None
            ]
        );
        assert_eq!(values(&definition, &drafts), None);
    }

    #[test]
    fn a_queued_script_keeps_a_readable_copy_of_its_values() {
        let definition = definition();
        let values = saved(json!({
            "monster": "Frogzard",
            "kills": 12,
            "rate": 0.5,
            "potions": true,
            "day": "All",
            "buy": [],
        }));
        let queued = Queued::new(
            "hunt.ts".into(),
            Launch {
                entry: "hunt.ts".into(),
                inputs: crate::scripting::Supplied::Values(values.clone()),
            },
            Some(&definition),
            &values,
        );
        assert_eq!(queued.summary, "Frogzard · 12 · 0.5 · Potions · All");
        assert_eq!(
            queued.rows,
            [
                ("Monster", "Frogzard"),
                ("Kills", "12"),
                ("Rate", "0.5"),
                ("Potions", "On"),
                ("Day", "All"),
                ("Buy", "None"),
            ]
            .map(|(label, value)| (label.into(), value.into()))
        );
    }
}
