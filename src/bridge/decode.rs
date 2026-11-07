use super::Value;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{name} sent {value:?} where {expected} was expected")]
pub(crate) struct DecodeError {
    name: &'static str,
    expected: &'static str,
    value: Value,
}

pub(super) trait Decode: Sized {
    const EXPECTED: &'static str;

    fn decode(value: Value) -> Result<Self, Value>;
}

impl Decode for () {
    const EXPECTED: &'static str = "nothing";

    fn decode(_: Value) -> Result<Self, Value> {
        Ok(())
    }
}

impl Decode for bool {
    const EXPECTED: &'static str = "a boolean";

    fn decode(value: Value) -> Result<Self, Value> {
        match value {
            Value::Bool(value) => Ok(value),
            value => Err(value),
        }
    }
}

impl Decode for f64 {
    const EXPECTED: &'static str = "a number";

    fn decode(value: Value) -> Result<Self, Value> {
        match value {
            Value::Number(value) => Ok(value),
            value => Err(value),
        }
    }
}

impl Decode for i32 {
    const EXPECTED: &'static str = "an int";

    fn decode(value: Value) -> Result<Self, Value> {
        match value {
            Value::Number(number)
                if number.fract() == 0.0 && i32::try_from(number as i64).is_ok() =>
            {
                Ok(number as i32)
            }
            value => Err(value),
        }
    }
}

impl Decode for u32 {
    const EXPECTED: &'static str = "a uint";

    fn decode(value: Value) -> Result<Self, Value> {
        match value {
            Value::Number(number)
                if number.fract() == 0.0 && u32::try_from(number as i64).is_ok() =>
            {
                Ok(number as u32)
            }
            value => Err(value),
        }
    }
}

impl Decode for String {
    const EXPECTED: &'static str = "a string";

    fn decode(value: Value) -> Result<Self, Value> {
        match value {
            Value::String(value) => Ok(value),
            value => Err(value),
        }
    }
}

impl Decode for Option<String> {
    const EXPECTED: &'static str = "a string or null";

    fn decode(value: Value) -> Result<Self, Value> {
        match value {
            Value::String(value) => Ok(Some(value)),
            Value::Null | Value::Undefined => Ok(None),
            value => Err(value),
        }
    }
}

impl Decode for Value {
    const EXPECTED: &'static str = "a value";

    fn decode(value: Value) -> Result<Self, Value> {
        Ok(value)
    }
}

pub(super) fn decode<T: Decode>(name: &'static str, value: Value) -> Result<T, DecodeError> {
    T::decode(value).map_err(|value| DecodeError {
        name,
        expected: T::EXPECTED,
        value,
    })
}

pub(super) fn arg(args: &[Value], index: usize) -> Value {
    args.get(index).cloned().unwrap_or(Value::Undefined)
}
