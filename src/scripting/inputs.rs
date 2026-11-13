use std::{collections::HashSet, path::Path};

use oxc::{
    allocator::Allocator,
    ast::ast::{
        ArrayExpressionElement, Declaration, Expression, ObjectExpression, ObjectPropertyKind,
        PropertyKey, Statement, UnaryOperator, VariableDeclarationKind,
    },
    parser::Parser,
    span::{GetSpan as _, SourceType},
};
use serde_json::{Map, Value as Json};

pub(crate) type Values = Map<String, Json>;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Supplied {
    Values(Values),
    StoreUnreadable,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Definition(Vec<Field>);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Field {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) description: Option<String>,
    pub(crate) kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Text {
        required: bool,
        default: String,
    },
    Number {
        integer: bool,
        min: Option<f64>,
        max: Option<f64>,
        default: Option<f64>,
    },
    Boolean {
        default: bool,
    },
    Select {
        options: Vec<String>,
        default: Option<String>,
    },
    MultiSelect {
        options: Vec<String>,
        default: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Problem {
    Missing { label: String },
    Invalid { label: String, reason: String },
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

impl Definition {
    pub(crate) fn fields(&self) -> &[Field] {
        &self.0
    }

    pub(crate) fn resolve(&self, supplied: &Values) -> Result<Values, Vec<Problem>> {
        let mut values = Values::new();
        let mut problems = Vec::new();
        for field in &self.0 {
            match field.resolve(supplied.get(&field.key)) {
                Ok(value) => {
                    values.insert(field.key.clone(), value);
                }
                Err(problem) => problems.push(problem),
            }
        }
        if problems.is_empty() {
            Ok(values)
        } else {
            Err(problems)
        }
    }
}

impl Field {
    fn resolve(&self, supplied: Option<&Json>) -> Result<Json, Problem> {
        match supplied {
            Some(value) => self.check(value).map_err(|reason| Problem::Invalid {
                label: self.label.clone(),
                reason: format!("{reason} (got {value})"),
            }),
            None => self
                .initial()
                .filter(|value| self.check(value).is_ok())
                .ok_or_else(|| Problem::Missing {
                    label: self.label.clone(),
                }),
        }
    }

    pub(crate) fn initial(&self) -> Option<Json> {
        match &self.kind {
            Kind::Text { default, .. } => Some(Json::from(default.as_str())),
            Kind::Number { default, .. } => default.map(number),
            Kind::Boolean { default } => Some(Json::from(*default)),
            Kind::Select { default, .. } => default.as_deref().map(Json::from),
            Kind::MultiSelect { default, .. } => Some(Json::from(default.clone())),
        }
    }

    pub(crate) fn check(&self, value: &Json) -> Result<Json, String> {
        match &self.kind {
            Kind::Text { required, .. } => {
                let text = value.as_str().ok_or("must be text")?;
                if *required && text.trim().is_empty() {
                    return Err("is required".into());
                }
                Ok(value.clone())
            }
            Kind::Number {
                integer, min, max, ..
            } => {
                let value = value.as_f64().ok_or("must be a number")?;
                check_number(value, *integer, *min, *max)?;
                Ok(number(value))
            }
            Kind::Boolean { .. } => value
                .is_boolean()
                .then(|| value.clone())
                .ok_or_else(|| "must be on or off".into()),
            Kind::Select { options, .. } => {
                let choice = value.as_str().ok_or("must be one of the options")?;
                if options.iter().any(|option| option == choice) {
                    Ok(value.clone())
                } else {
                    Err("isn't one of the options".into())
                }
            }
            Kind::MultiSelect { options, .. } => {
                let chosen: Vec<&str> = value
                    .as_array()
                    .and_then(|items| items.iter().map(Json::as_str).collect())
                    .ok_or("must be a list of options")?;
                choices(options, &chosen).map(Json::from)
            }
        }
    }
}

fn choices(options: &[String], chosen: &[&str]) -> Result<Vec<String>, String> {
    if let Some(unknown) = chosen
        .iter()
        .find(|choice| !options.iter().any(|option| option == *choice))
    {
        return Err(format!("has {unknown:?}, which isn't one of the options"));
    }
    if chosen.iter().collect::<HashSet<_>>().len() != chosen.len() {
        return Err("lists an option twice".into());
    }
    Ok(options
        .iter()
        .filter(|option| chosen.contains(&option.as_str()))
        .cloned()
        .collect())
}

pub(crate) fn check_number(
    value: f64,
    integer: bool,
    min: Option<f64>,
    max: Option<f64>,
) -> Result<(), String> {
    if !value.is_finite() {
        return Err("must be a number".into());
    }
    if integer && (value.fract() != 0.0 || value.abs() > MAX_SAFE_INTEGER) {
        return Err("must be a whole number".into());
    }
    if let Some(min) = min
        && value < min
    {
        return Err(format!("must be at least {min}"));
    }
    if let Some(max) = max
        && value > max
    {
        return Err(format!("must be at most {max}"));
    }
    Ok(())
}

pub(crate) fn number(value: f64) -> Json {
    if value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER {
        Json::from(value as i64)
    } else {
        Json::from(value)
    }
}

pub(crate) fn describe(script: &str, problems: &[Problem]) -> String {
    let missing: Vec<&str> = problems
        .iter()
        .filter_map(|problem| match problem {
            Problem::Missing { label } => Some(label.as_str()),
            Problem::Invalid { .. } => None,
        })
        .collect();
    let mut invalid: Vec<String> = problems
        .iter()
        .filter_map(|problem| match problem {
            Problem::Invalid { label, reason } => Some(format!("{label} {reason}")),
            Problem::Missing { .. } => None,
        })
        .collect();
    if invalid.is_empty() {
        return format!(
            "{script} needs inputs: {}. Open Scripts to fill them in.",
            missing.join(", ")
        );
    }
    invalid.extend(missing.iter().map(|label| format!("{label} needs a value")));
    format!(
        "{script}: {}. Open Scripts to review its inputs.",
        invalid.join("; ")
    )
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExtractError {
    Syntax(String),
    Inputs(String),
}

impl ExtractError {
    pub(crate) fn into_message(self) -> String {
        match self {
            Self::Syntax(message) | Self::Inputs(message) => message,
        }
    }
}

pub(super) fn prepare(
    display: &str,
    path: &Path,
    text: &str,
    supplied: &Supplied,
) -> Result<Values, String> {
    let Some(definition) = extract(display, path, text).map_err(ExtractError::into_message)? else {
        return Ok(Values::new());
    };
    match supplied {
        Supplied::Values(values) => definition
            .resolve(values)
            .map_err(|problems| describe(display, &problems)),
        Supplied::StoreUnreadable => Err(format!(
            "{display} didn't start: saved script inputs couldn't be read. Open Scripts to recover them."
        )),
    }
}

pub(crate) fn extract(
    display: &str,
    path: &Path,
    text: &str,
) -> Result<Option<Definition>, ExtractError> {
    let typescript = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "ts" | "mts"));
    let source_type = if typescript {
        SourceType::ts().with_module(true)
    } else {
        SourceType::mjs()
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, text, source_type).parse();
    let located = |offset: u32, message: &str| {
        let (row, column) = position(text, offset as usize);
        format!("{display}:{row}:{column}: {message}")
    };
    if let Some(diagnostic) = parsed.diagnostics.first() {
        let offset = diagnostic.labels.first().map_or(0, |label| label.offset());
        return Err(ExtractError::Syntax(located(offset, &diagnostic.message)));
    }
    let mut found: Option<&Expression> = None;
    for statement in &parsed.program.body {
        let misdeclared = match statement {
            Statement::ExportNamedDeclaration(export) => export
                .specifiers
                .iter()
                .find(|specifier| specifier.exported.name().as_str() == "inputs")
                .map(|specifier| specifier.span.start),
            Statement::ExportFromDeclaration(export) => export
                .specifiers
                .iter()
                .find(|specifier| specifier.exported.name().as_str() == "inputs")
                .map(|specifier| specifier.span.start),
            Statement::ExportAllDeclaration(export) => export
                .exported
                .as_ref()
                .filter(|exported| exported.name().as_str() == "inputs")
                .map(|_| export.span.start),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(variables) => {
                    let mut misdeclared = None;
                    for declarator in &variables.declarations {
                        let names = declarator.id.get_binding_identifiers();
                        if !names.iter().any(|name| name.name.as_str() == "inputs") {
                            continue;
                        }
                        let plain = declarator.id.get_identifier_name().is_some()
                            && declarator.id.get_binding_identifier().is_some()
                            && names.len() == 1;
                        match &declarator.init {
                            Some(init)
                                if plain
                                    && variables.kind == VariableDeclarationKind::Const
                                    && variables.declarations.len() == 1
                                    && found.is_none() =>
                            {
                                found = Some(init);
                            }
                            _ => misdeclared = Some(declarator.span.start),
                        }
                    }
                    misdeclared
                }
                Declaration::FunctionDeclaration(function) => function
                    .id
                    .as_ref()
                    .filter(|id| id.name.as_str() == "inputs")
                    .map(|_| function.span.start),
                Declaration::ClassDeclaration(class) => class
                    .id
                    .as_ref()
                    .filter(|id| id.name.as_str() == "inputs")
                    .map(|_| class.span.start),
                _ => None,
            },
            _ => None,
        };
        if let Some(offset) = misdeclared {
            return Err(ExtractError::Inputs(located(
                offset,
                "declare inputs as `export const inputs = { ... }`",
            )));
        }
    }
    let Some(init) = found else {
        return Ok(None);
    };
    let Expression::ObjectExpression(object) = init.get_inner_expression() else {
        return Err(ExtractError::Inputs(located(
            init.span().start,
            "inputs must be an object literal like `{ kills: { type: \"number\", label: \"Kills\" } }`",
        )));
    };
    definition(object)
        .map(|definition| (!definition.0.is_empty()).then_some(definition))
        .map_err(|error| ExtractError::Inputs(located(error.offset, &error.message)))
}

pub(super) fn position(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let row = before.matches('\n').count() + 1;
    let column = before
        .rfind('\n')
        .map_or(before.len(), |newline| before.len() - newline - 1)
        + 1;
    (row, column)
}

struct Error {
    offset: u32,
    message: String,
}

fn error(offset: u32, message: impl Into<String>) -> Error {
    Error {
        offset,
        message: message.into(),
    }
}

enum Literal {
    Text(String),
    Number(f64),
    Boolean(bool),
    Texts(Vec<String>),
}

impl Literal {
    fn describe(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Number(_) => "a number",
            Self::Boolean(_) => "true or false",
            Self::Texts(_) => "a list of text",
        }
    }
}

struct Property {
    name: String,
    offset: u32,
    value: Literal,
    value_offset: u32,
}

fn properties<'a>(
    object: &'a ObjectExpression<'a>,
    path: &str,
) -> Result<Vec<(String, u32, &'a Expression<'a>)>, Error> {
    let mut seen: Vec<(String, u32, &Expression)> = Vec::new();
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return Err(error(
                property.span().start,
                format!("{path} can't use `...` spreads"),
            ));
        };
        if property.computed || property.method || property.shorthand {
            return Err(error(
                property.span.start,
                format!("{path} can only have plain `key: value` properties"),
            ));
        }
        let name = match &property.key {
            PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str().to_owned(),
            PropertyKey::StringLiteral(literal) => literal
                .value
                .as_str()
                .ok_or_else(|| error(literal.span.start, format!("{path} has an invalid key")))?
                .to_owned(),
            key => {
                return Err(error(
                    key.span().start,
                    format!("{path} keys must be plain names"),
                ));
            }
        };
        if seen.iter().any(|(seen, ..)| *seen == name) {
            return Err(error(
                property.key.span().start,
                format!("{path}.{name} is declared twice"),
            ));
        }
        seen.push((name, property.key.span().start, &property.value));
    }
    Ok(seen)
}

fn definition(object: &ObjectExpression) -> Result<Definition, Error> {
    let entries = properties(object, "inputs")?;
    let mut fields = Vec::with_capacity(entries.len());
    for (key, offset, value) in entries {
        if !is_identifier(&key) || key == "__proto__" {
            return Err(error(
                offset,
                format!("inputs.{key} must be named like a JavaScript variable"),
            ));
        }
        let Expression::ObjectExpression(object) = value.get_inner_expression() else {
            return Err(error(
                value.span().start,
                format!(
                    "inputs.{key} must be an object like `{{ type: \"string\", label: \"...\" }}`"
                ),
            ));
        };
        let path = format!("inputs.{key}");
        let properties = properties(object, &path)?
            .into_iter()
            .map(|(name, offset, value)| {
                Ok(Property {
                    value: literal(value, &format!("{path}.{name}"))?,
                    value_offset: value.span().start,
                    name,
                    offset,
                })
            })
            .collect::<Result<_, Error>>()?;
        fields.push(field(key, object.span.start, properties)?);
    }
    Ok(Definition(fields))
}

fn literal(value: &Expression, path: &str) -> Result<Literal, Error> {
    let value = value.get_inner_expression();
    let unsupported = || {
        error(
            value.span().start,
            format!(
                "{path} must be written out as text, a number, true or false, or a list of text"
            ),
        )
    };
    match value {
        Expression::StringLiteral(literal) => literal
            .value
            .as_str()
            .map(|text| Literal::Text(text.to_owned()))
            .ok_or_else(unsupported),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked)
            .and_then(|cooked| cooked.as_str())
            .map(|text| Literal::Text(text.to_owned()))
            .ok_or_else(unsupported),
        Expression::NumericLiteral(literal) => Ok(Literal::Number(literal.value)),
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::UnaryNegation => {
            match unary.argument.get_inner_expression() {
                Expression::NumericLiteral(literal) => Ok(Literal::Number(-literal.value)),
                _ => Err(unsupported()),
            }
        }
        Expression::BooleanLiteral(literal) => Ok(Literal::Boolean(literal.value)),
        Expression::ArrayExpression(array) => array
            .elements
            .iter()
            .map(|element| match element {
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {
                    Err(unsupported())
                }
                element => match literal(element.to_expression(), path)? {
                    Literal::Text(text) => Ok(text),
                    _ => Err(error(
                        element.span().start,
                        format!("{path} can only list text"),
                    )),
                },
            })
            .collect::<Result<_, _>>()
            .map(Literal::Texts),
        _ => Err(unsupported()),
    }
}

fn field(key: String, offset: u32, properties: Vec<Property>) -> Result<Field, Error> {
    let path = format!("inputs.{key}");
    let find = |name: &str| properties.iter().find(|property| property.name == name);
    let Some(kind) = find("type") else {
        return Err(error(
            offset,
            format!("{path} needs a type: \"string\", \"number\", \"boolean\" or \"select\""),
        ));
    };
    let Literal::Text(kind_name) = &kind.value else {
        return Err(error(
            kind.value_offset,
            format!("{path}.type must be text"),
        ));
    };
    let allowed: &[&str] = match kind_name.as_str() {
        "string" => &["required", "default"],
        "number" => &["integer", "min", "max", "default"],
        "boolean" => &["default"],
        "select" => &["options", "multiple", "default"],
        _ => {
            return Err(error(
                kind.value_offset,
                format!(
                    "{path}.type must be \"string\", \"number\", \"boolean\" or \"select\", not {kind_name:?}"
                ),
            ));
        }
    };
    for property in &properties {
        let name = property.name.as_str();
        if !["type", "label", "description"].contains(&name) && !allowed.contains(&name) {
            return Err(error(
                property.offset,
                format!("{path}.{name} isn't a setting of a {kind_name} input"),
            ));
        }
    }
    let wrong = |property: &Property, expected: &str| {
        error(
            property.value_offset,
            format!(
                "{path}.{} must be {expected}, not {}",
                property.name,
                property.value.describe()
            ),
        )
    };
    let text = |name: &str| -> Result<Option<(&String, u32)>, Error> {
        match find(name) {
            None => Ok(None),
            Some(Property {
                value: Literal::Text(text),
                value_offset,
                ..
            }) => Ok(Some((text, *value_offset))),
            Some(property) => Err(wrong(property, "text")),
        }
    };
    let number = |name: &str| -> Result<Option<(f64, u32)>, Error> {
        match find(name) {
            None => Ok(None),
            Some(Property {
                value: Literal::Number(value),
                value_offset,
                ..
            }) if value.is_finite() => Ok(Some((*value, *value_offset))),
            Some(property) => Err(wrong(property, "a finite number")),
        }
    };
    let boolean = |name: &str| -> Result<Option<bool>, Error> {
        match find(name) {
            None => Ok(None),
            Some(Property {
                value: Literal::Boolean(value),
                ..
            }) => Ok(Some(*value)),
            Some(property) => Err(wrong(property, "true or false")),
        }
    };
    let texts = |name: &str| -> Result<Option<(&Vec<String>, u32)>, Error> {
        match find(name) {
            None => Ok(None),
            Some(Property {
                value: Literal::Texts(texts),
                value_offset,
                ..
            }) => Ok(Some((texts, *value_offset))),
            Some(property) => Err(wrong(property, "a list of text")),
        }
    };
    let label = match text("label")? {
        Some((label, _)) if !label.trim().is_empty() => label.clone(),
        Some((_, at)) => return Err(error(at, format!("{path}.label can't be empty"))),
        None => return Err(error(offset, format!("{path} needs a label"))),
    };
    let description = text("description")?.map(|(text, _)| text.clone());
    let kind = match kind_name.as_str() {
        "string" => {
            let required = boolean("required")?.unwrap_or(false);
            let default = text("default")?;
            if required
                && let Some((default, at)) = default
                && default.trim().is_empty()
            {
                return Err(error(
                    at,
                    format!("{path}.default can't be empty when the input is required"),
                ));
            }
            Kind::Text {
                required,
                default: default.map(|(text, _)| text.clone()).unwrap_or_default(),
            }
        }
        "number" => {
            let integer = boolean("integer")?.unwrap_or(false);
            let min = number("min")?;
            let max = number("max")?;
            for (bound, name) in [(min, "min"), (max, "max")] {
                if let Some((value, at)) = bound {
                    check_number(value, integer, None, None)
                        .map_err(|reason| error(at, format!("{path}.{name} {reason}")))?;
                }
            }
            if let (Some((min, _)), Some((max, at))) = (min, max)
                && min > max
            {
                return Err(error(
                    at,
                    format!("{path}.max must be at least min ({min})"),
                ));
            }
            let (min, max) = (min.map(|(min, _)| min), max.map(|(max, _)| max));
            let default = number("default")?;
            if let Some((default, at)) = default {
                check_number(default, integer, min, max)
                    .map_err(|reason| error(at, format!("{path}.default {reason}")))?;
            }
            Kind::Number {
                integer,
                min,
                max,
                default: default.map(|(default, _)| default),
            }
        }
        "boolean" => Kind::Boolean {
            default: boolean("default")?.unwrap_or(false),
        },
        _ => {
            let Some((options, at)) = texts("options")? else {
                return Err(error(offset, format!("{path} needs options")));
            };
            if options.is_empty() {
                return Err(error(at, format!("{path}.options can't be empty")));
            }
            if options.iter().any(|option| option.trim().is_empty()) {
                return Err(error(at, format!("{path}.options can't have empty text")));
            }
            if let Some(duplicate) = options
                .iter()
                .enumerate()
                .find(|(index, option)| options[..*index].contains(option))
                .map(|(_, option)| option)
            {
                return Err(error(
                    at,
                    format!("{path}.options lists {duplicate:?} twice"),
                ));
            }
            let options = options.clone();
            if boolean("multiple")?.unwrap_or(false) {
                let default = match texts("default")? {
                    Some((default, at)) => {
                        let chosen: Vec<&str> = default.iter().map(String::as_str).collect();
                        choices(&options, &chosen)
                            .map_err(|reason| error(at, format!("{path}.default {reason}")))?
                    }
                    None => Vec::new(),
                };
                Kind::MultiSelect { options, default }
            } else {
                let default = text("default")?;
                if let Some((default, at)) = default
                    && !options.contains(default)
                {
                    return Err(error(
                        at,
                        format!("{path}.default must be one of the options, not {default:?}"),
                    ));
                }
                Kind::Select {
                    default: default.map(|(default, _)| default.clone()),
                    options,
                }
            }
        }
    };
    Ok(Field {
        key,
        label,
        description,
        kind,
    })
}

fn is_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_' || first == '$')
        && characters.all(|rest| rest.is_ascii_alphanumeric() || rest == '_' || rest == '$')
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(source: &str) -> Result<Option<Definition>, String> {
        extract("hunt.ts", Path::new("hunt.ts"), source).map_err(ExtractError::into_message)
    }

    fn fields(source: &str) -> Vec<Field> {
        read(source).unwrap().unwrap().0
    }

    fn values(value: Json) -> Values {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn a_script_without_an_inputs_export_has_no_inputs() {
        for source in [
            "export default function* () {}",
            "const inputs = { kills: { type: \"number\", label: \"Kills\" } };\nexport default function* () {}",
            "export const inputs = {};",
        ] {
            assert_eq!(read(source), Ok(None), "{source}");
        }
    }

    #[test]
    fn every_input_type_is_read_without_running_the_script() {
        let source = r#"
            import { Script, type Inputs } from "mira";
            console.log("top level");
            export const inputs = {
              monster: { type: "string", label: "Monster", description: "As shown in game.", required: true, default: "Frogzard" },
              note: { type: `string`, label: "Note" },
              kills: { type: "number", label: "Kills", integer: true, min: 1, max: 1_000, default: 5 },
              interval: { type: "number", label: "Interval", min: -0.5, default: 0.5 },
              potions: { type: "boolean", label: "Use potions", default: true },
              quiet: { type: "boolean", label: "Quiet" },
              daily: { type: "select", label: "Daily", options: ["All", "Class"], default: "Class" },
              room: { "type": "select", label: "Room", options: ["Public", "Private"] },
              buy: { type: "select", label: "Buy", options: ["A", "B", "C"], multiple: true, default: ["C", "A"] },
            } as const satisfies Inputs;
            export default function* () {}
        "#;
        let kinds: Vec<(String, Kind)> = fields(source)
            .into_iter()
            .map(|field| (field.key, field.kind))
            .collect();
        let options = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();
        assert_eq!(
            kinds,
            vec![
                (
                    "monster".into(),
                    Kind::Text {
                        required: true,
                        default: "Frogzard".into()
                    }
                ),
                (
                    "note".into(),
                    Kind::Text {
                        required: false,
                        default: String::new()
                    }
                ),
                (
                    "kills".into(),
                    Kind::Number {
                        integer: true,
                        min: Some(1.0),
                        max: Some(1000.0),
                        default: Some(5.0)
                    }
                ),
                (
                    "interval".into(),
                    Kind::Number {
                        integer: false,
                        min: Some(-0.5),
                        max: None,
                        default: Some(0.5)
                    }
                ),
                ("potions".into(), Kind::Boolean { default: true }),
                ("quiet".into(), Kind::Boolean { default: false }),
                (
                    "daily".into(),
                    Kind::Select {
                        options: options(&["All", "Class"]),
                        default: Some("Class".into())
                    }
                ),
                (
                    "room".into(),
                    Kind::Select {
                        options: options(&["Public", "Private"]),
                        default: None
                    }
                ),
                (
                    "buy".into(),
                    Kind::MultiSelect {
                        options: options(&["A", "B", "C"]),
                        default: options(&["A", "C"])
                    }
                ),
            ]
        );
        let monster = &fields(source)[0];
        assert_eq!(monster.label, "Monster");
        assert_eq!(monster.description.as_deref(), Some("As shown in game."));
    }

    #[test]
    fn a_syntax_error_is_not_blamed_on_the_inputs() {
        let source = "import { Army } from \"mira\";\n\nArmy.\n\nexport default function* () {}\n";
        let error = extract("army.ts", Path::new("army.ts"), source).unwrap_err();
        let ExtractError::Syntax(message) = error else {
            panic!("expected a syntax error, got {error:?}");
        };
        assert!(message.starts_with("army.ts:5:"), "{message}");
        assert!(matches!(
            extract("hunt.ts", Path::new("hunt.ts"), "export let inputs = {};"),
            Err(ExtractError::Inputs(_))
        ));
    }

    #[test]
    fn plain_javascript_entries_declare_inputs_the_same_way() {
        let source = "export const inputs = { kills: { type: \"number\", label: \"Kills\" } };";
        for name in ["hunt.js", "hunt.mjs", "hunt.mts"] {
            let definition = extract(name, Path::new(name), source).unwrap().unwrap();
            assert_eq!(definition.0[0].key, "kills", "{name}");
        }
    }

    #[test]
    fn invalid_definitions_name_the_line_and_the_setting() {
        let cases = [
            (
                "export const inputs = makeInputs();",
                "hunt.ts:1:23: inputs must be an object literal",
            ),
            (
                "export let inputs = {};",
                "hunt.ts:1:12: declare inputs as `export const inputs = { ... }`",
            ),
            (
                "const fields = {};\nexport { fields as inputs };",
                "hunt.ts:2:10: declare inputs",
            ),
            (
                "export { inputs } from \"./other.ts\";",
                "hunt.ts:1:10: declare inputs",
            ),
            (
                "export const inputs = { ...more };",
                "hunt.ts:1:25: inputs can't use `...` spreads",
            ),
            (
                "export const inputs = { [key]: {} };",
                "hunt.ts:1:25: inputs can only have plain",
            ),
            (
                "export const inputs = { kills: base };",
                "hunt.ts:1:32: inputs.kills must be an object",
            ),
            (
                "export const inputs = { \"bad key\": { type: \"boolean\", label: \"A\" } };",
                "inputs.bad key must be named like a JavaScript variable",
            ),
            (
                "export const inputs = { kills: { label: \"Kills\" } };",
                "inputs.kills needs a type",
            ),
            (
                "export const inputs = { kills: { type: \"integer\", label: \"Kills\" } };",
                "inputs.kills.type must be \"string\", \"number\", \"boolean\" or \"select\", not \"integer\"",
            ),
            (
                "export const inputs = { kills: { type: \"number\" } };",
                "inputs.kills needs a label",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \" \" } };",
                "inputs.kills.label can't be empty",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", defualt: 5 } };",
                "inputs.kills.defualt isn't a setting of a number input",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", options: [] } };",
                "inputs.kills.options isn't a setting of a number input",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", default: \"5\" } };",
                "inputs.kills.default must be a finite number, not text",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", default: KILLS } };",
                "inputs.kills.default must be written out",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: `Kills ${n}` } };",
                "inputs.kills.label must be written out",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", min: 1, default: 0 } };",
                "hunt.ts:1:83: inputs.kills.default must be at least 1",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", integer: true, default: 1.5 } };",
                "inputs.kills.default must be a whole number",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", integer: true, min: 0.5 } };",
                "inputs.kills.min must be a whole number",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", min: 5, max: 1 } };",
                "inputs.kills.max must be at least min (5)",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", max: 1e999 } };",
                "inputs.kills.max must be a finite number",
            ),
            (
                "export const inputs = { kills: { type: \"number\", label: \"Kills\", label: \"Again\" } };",
                "inputs.kills.label is declared twice",
            ),
            (
                "export const inputs = { name: { type: \"string\", label: \"Name\", required: true, default: \"\" } };",
                "inputs.name.default can't be empty when the input is required",
            ),
            (
                "export const inputs = { day: { type: \"select\", label: \"Day\" } };",
                "inputs.day needs options",
            ),
            (
                "export const inputs = { day: { type: \"select\", label: \"Day\", options: [] } };",
                "inputs.day.options can't be empty",
            ),
            (
                "export const inputs = { day: { type: \"select\", label: \"Day\", options: [\"A\", \"A\"] } };",
                "inputs.day.options lists \"A\" twice",
            ),
            (
                "export const inputs = { day: { type: \"select\", label: \"Day\", options: [\"A\", 1] } };",
                "inputs.day.options can only list text",
            ),
            (
                "export const inputs = { day: { type: \"select\", label: \"Day\", options: [\"A\"], default: \"B\" } };",
                "inputs.day.default must be one of the options, not \"B\"",
            ),
            (
                "export const inputs = { day: { type: \"select\", label: \"Day\", options: [\"A\"], multiple: \"yes\" } };",
                "inputs.day.multiple must be true or false, not text",
            ),
            (
                "export const inputs = { day: { type: \"select\", label: \"Day\", options: [\"A\"], multiple: true, default: \"A\" } };",
                "inputs.day.default must be a list of text, not text",
            ),
            (
                "export const inputs = { day: { type: \"select\", label: \"Day\", options: [\"A\"], multiple: true, default: [\"B\"] } };",
                "inputs.day.default has \"B\", which isn't one of the options",
            ),
            (
                "export const inputs = { day: { type: \"select\", label: \"Day\", options: [\"A\"], multiple: true, default: [\"A\", \"A\"] } };",
                "inputs.day.default lists an option twice",
            ),
            ("export const inputs = {", "hunt.ts:1:"),
        ];
        for (source, expected) in cases {
            let error = read(source).expect_err(source);
            assert!(
                error.contains(expected),
                "{source}\n  got: {error}\n  want: {expected}"
            );
        }
    }

    #[test]
    fn resolving_keeps_supplied_values_and_fills_the_rest_from_defaults() {
        let definition = read(
            r#"export const inputs = {
              monster: { type: "string", label: "Monster", default: "Frogzard" },
              kills: { type: "number", label: "Kills", integer: true, min: 1, default: 5 },
              rate: { type: "number", label: "Rate" },
              potions: { type: "boolean", label: "Potions" },
              day: { type: "select", label: "Day", options: ["A", "B"] },
              buy: { type: "select", label: "Buy", options: ["A", "B", "C"], multiple: true },
            };"#,
        )
        .unwrap()
        .unwrap();
        let resolved = definition.resolve(&values(json!({
            "kills": 12,
            "rate": 0.25,
            "day": "B",
            "buy": ["C", "A"],
            "removed": "ignored",
        })));
        assert_eq!(
            resolved,
            Ok(values(json!({
                "monster": "Frogzard",
                "kills": 12,
                "rate": 0.25,
                "potions": false,
                "day": "B",
                "buy": ["A", "C"],
            })))
        );
        assert_eq!(
            definition.resolve(&values(
                json!({ "kills": 0, "day": "C", "buy": ["A", "A"] })
            )),
            Err(vec![
                Problem::Invalid {
                    label: "Kills".into(),
                    reason: "must be at least 1 (got 0)".into()
                },
                Problem::Missing {
                    label: "Rate".into()
                },
                Problem::Invalid {
                    label: "Day".into(),
                    reason: "isn't one of the options (got \"C\")".into()
                },
                Problem::Invalid {
                    label: "Buy".into(),
                    reason: "lists an option twice (got [\"A\",\"A\"])".into()
                },
            ])
        );
    }

    #[test]
    fn supplied_values_of_the_wrong_kind_are_never_replaced_by_defaults() {
        let definition = read(
            r#"export const inputs = {
              name: { type: "string", label: "Name", required: true, default: "Frog" },
              kills: { type: "number", label: "Kills", integer: true, default: 5 },
              potions: { type: "boolean", label: "Potions", default: true },
            };"#,
        )
        .unwrap()
        .unwrap();
        let problems = definition
            .resolve(&values(
                json!({ "name": "  ", "kills": 2.5, "potions": "yes" }),
            ))
            .unwrap_err();
        assert_eq!(
            describe("hunt.ts", &problems),
            "hunt.ts: Name is required (got \"  \"); Kills must be a whole number (got 2.5); Potions must be on or off (got \"yes\"). Open Scripts to review its inputs."
        );
        let missing = read(
            r#"export const inputs = {
              kills: { type: "number", label: "Kills" },
              day: { type: "select", label: "Day", options: ["A"] },
              monster: { type: "string", label: "Monster", required: true },
            };"#,
        )
        .unwrap()
        .unwrap()
        .resolve(&Values::new())
        .unwrap_err();
        assert_eq!(
            describe("hunt.ts", &missing),
            "hunt.ts needs inputs: Kills, Day, Monster. Open Scripts to fill them in."
        );
    }

    #[test]
    fn whole_numbers_resolve_without_a_fraction() {
        assert_eq!(number(5.0).to_string(), "5");
        assert_eq!(number(0.5).to_string(), "0.5");
        assert_eq!(number(-3.0).to_string(), "-3");
    }
}
