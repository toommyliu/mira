use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::Loader,
    domain::{self, players},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    All,
    Get { username: String },
}

pub(super) fn serve(loader: &impl Loader, request: Request) -> Reply {
    if !domain::has_avatar(loader)? {
        return Ok(match request {
            Request::All => json!([]),
            Request::Get { .. } => serde_json::Value::Null,
        });
    }
    Ok(match request {
        Request::All => json!(players::all(loader)?),
        Request::Get { username } => json!(players::get(loader, &username)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bridge::Value, domain::testing::FakeLoader};

    #[test]
    fn out_of_game_reads_are_empty() {
        let loader = FakeLoader::new(|callback, _| match callback {
            "auth.isLoggedIn" => Ok(Value::Bool(false)),
            other => panic!("{other} was called while out of game"),
        });
        for (request, expected) in [
            (Request::All, json!([])),
            (
                Request::Get {
                    username: "player1".into(),
                },
                json!(null),
            ),
        ] {
            assert_eq!(serve(&loader, request.clone()), Ok(expected), "{request:?}");
        }
    }
}
