use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::{Event, Loader, Subscription},
    domain::{self, drops, inventory::ItemQuery},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Items,
    Item {
        query: ItemQuery,
    },
    Contains {
        query: ItemQuery,
        quantity: Option<f64>,
    },
    Accept {
        query: ItemQuery,
    },
    Reject {
        query: ItemQuery,
    },
}

impl Request {
    fn without_avatar(&self) -> serde_json::Value {
        match self {
            Request::Items => json!([]),
            Request::Item { .. } => serde_json::Value::Null,
            _ => json!(false),
        }
    }
}

pub(super) async fn serve(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    request: Request,
) -> Reply {
    Ok(match request {
        Request::Accept { query } => json!(drops::accept(loader, subscribe, &query).await?),
        request if !domain::has_avatar(loader)? => request.without_avatar(),
        Request::Items => json!(drops::items(loader)?),
        Request::Item { query } => json!(drops::item(loader, &query)?),
        Request::Contains { query, quantity } => json!(
            drops::item(loader, &query)?
                .is_some_and(|item| item.quantity >= quantity.unwrap_or(1.0).max(1.0))
        ),
        Request::Reject { query } => json!(drops::reject(loader, &query)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    #[tokio::test]
    async fn out_of_game_reads_are_empty_and_actions_do_nothing() {
        let loader = FakeLoader::new(|callback, _| match callback {
            "auth.isLoggedIn" => Ok(Value::Bool(false)),
            other => panic!("{other} was called while out of game"),
        });
        let events = Events::default();
        let gem = || ItemQuery::Name("Gem".into());
        for (request, expected) in [
            (Request::Items, json!([])),
            (Request::Item { query: gem() }, json!(null)),
            (
                Request::Contains {
                    query: gem(),
                    quantity: None,
                },
                json!(false),
            ),
            (Request::Accept { query: gem() }, json!(false)),
            (Request::Reject { query: gem() }, json!(false)),
        ] {
            assert_eq!(
                serve(&loader, |wants| events.subscribe(wants), request.clone()).await,
                Ok(expected),
                "{request:?}"
            );
        }
    }
}
