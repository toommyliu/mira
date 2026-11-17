use std::num::NonZeroU32;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Beep { times: NonZeroU32 },
}

pub(super) async fn serve(request: Request) -> Value {
    match request {
        Request::Beep { times } => crate::beep::beep(times).await,
    }
    Value::Null
}
