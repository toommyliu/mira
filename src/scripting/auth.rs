use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::{self, Loader},
    domain::auth::{self, ConnectError, Logins},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    LoggedIn,
    Username,
    Server,
    Servers,
    Login,
    Connect { server: String },
    Logout,
}

pub(super) async fn serve(loader: &impl Loader, logins: &Logins, request: Request) -> Reply {
    Ok(match request {
        Request::LoggedIn => json!(bridge::auth::is_logged_in(loader)?),
        Request::Username => json!(auth::username(loader)?),
        Request::Server if !bridge::auth::is_logged_in(loader)? => serde_json::Value::Null,
        Request::Server => json!(auth::current_server(loader)?),
        Request::Servers => json!(auth::servers(loader)?),
        Request::Login => json!(logins.log_in(loader).await?),
        Request::Connect { server } => return connected(logins.connect(loader, &server).await),
        Request::Logout => json!(logins.log_out(loader).await?),
    })
}

fn connected(result: Result<(), ConnectError>) -> Reply {
    let (reason, message) = match result {
        Ok(()) => return Ok(json!({ "ok": true })),
        Err(ConnectError::Call(error)) => return Err(error),
        Err(ConnectError::Refused { refusal, message }) => (json!(refusal), message),
        Err(error @ ConnectError::SelectionNotReady) => (json!("notReady"), error.to_string()),
        Err(error @ ConnectError::PlayerUnready) => (json!("timedOut"), error.to_string()),
    };
    Ok(json!({ "ok": false, "reason": reason, "message": message }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{bridge::Value, domain::testing::FakeLoader};

    fn refusing(reason: Option<&str>, status: &str) -> FakeLoader {
        let mut fields = BTreeMap::from([
            ("status".to_owned(), Value::from(status.to_owned())),
            (
                "message".to_owned(),
                Value::from("server is full".to_owned()),
            ),
        ]);
        if let Some(reason) = reason {
            fields.insert("reason".into(), Value::from(reason.to_owned()));
        }
        FakeLoader::new(move |callback, _| {
            assert_eq!(callback, "auth.connectTo");
            Ok(Value::Object(fields.clone()))
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_connect_names_the_reason() {
        let connect = Request::Connect {
            server: "Twilly".into(),
        };
        let logins = Logins::default();
        assert_eq!(
            serve(&refusing(Some("full"), "blocked"), &logins, connect.clone()).await,
            Ok(json!({ "ok": false, "reason": "full", "message": "server is full" }))
        );
        assert_eq!(
            serve(&refusing(None, "not-found"), &logins, connect.clone()).await,
            Ok(json!({ "ok": false, "reason": "notFound", "message": "server is full" }))
        );
        assert_eq!(
            serve(&refusing(None, "not-ready"), &logins, connect).await,
            Ok(json!({
                "ok": false,
                "reason": "notReady",
                "message": "server selection wasn't ready"
            }))
        );
    }

    #[tokio::test]
    async fn the_current_server_is_null_while_logged_out() {
        let loader = FakeLoader::new(|callback, _| match callback {
            "auth.isLoggedIn" => Ok(Value::Bool(false)),
            other => panic!("{other} was called while logged out"),
        });
        assert_eq!(
            serve(&loader, &Logins::default(), Request::Server).await,
            Ok(serde_json::Value::Null)
        );
    }
}
