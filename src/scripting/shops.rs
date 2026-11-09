use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::{self, Event, Loader, Subscription},
    domain::{
        self,
        inventory::ItemQuery,
        shops::{self, ShopItemQuery},
    },
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Load {
        shop_id: i32,
    },
    Current,
    Items,
    Item {
        query: ShopItemQuery,
    },
    IsOpen {
        shop_id: Option<i32>,
    },
    Close {
        shop_id: Option<i32>,
    },
    CanBuy {
        query: ShopItemQuery,
        quantity: Option<i32>,
    },
    MaxBuyQuantity {
        query: ShopItemQuery,
    },
    Buy {
        query: ShopItemQuery,
        quantity: Option<i32>,
    },
    Sell {
        query: ItemQuery,
        quantity: Option<i32>,
    },
    EnhanceItem {
        query: ItemQuery,
        enhancement: String,
        special: Option<String>,
    },
}

impl Request {
    fn without_avatar(&self) -> serde_json::Value {
        match self {
            Request::Items => json!([]),
            Request::Current | Request::Item { .. } => serde_json::Value::Null,
            Request::MaxBuyQuantity { .. } => json!(0),
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
        Request::Load { shop_id } => json!(shops::load(loader, shop_id).await?),
        Request::Buy { query, quantity } => {
            json!(shops::buy(loader, subscribe, &query, quantity.unwrap_or(1)).await?)
        }
        Request::Sell { query, quantity } => {
            json!(shops::sell(loader, subscribe, &query, quantity).await?)
        }
        Request::EnhanceItem {
            query,
            enhancement,
            special,
        } => json!(
            shops::enhance_item(loader, subscribe, &query, &enhancement, special.as_deref())
                .await?
        ),
        request if !domain::has_avatar(loader)? => request.without_avatar(),
        Request::Current => json!(shops::current(loader)?),
        Request::Items => json!(shops::current(loader)?.map_or_else(Vec::new, |shop| shop.items)),
        Request::Item { query } => json!(shops::item(loader, &query)?),
        Request::IsOpen { shop_id } => json!(bridge::shops::is_open(loader, shop_id)?),
        Request::Close { shop_id } => json!(bridge::shops::close(loader, shop_id)?),
        Request::CanBuy { query, quantity } => {
            json!(shops::can_buy(loader, &query, quantity.unwrap_or(1))?)
        }
        Request::MaxBuyQuantity { query } => json!(shops::max_buy_quantity(loader, &query)?),
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
        let gem = || ShopItemQuery::Item(ItemQuery::Name("Gem".into()));
        for (request, expected) in [
            (Request::Load { shop_id: 1 }, json!(false)),
            (Request::Current, json!(null)),
            (Request::Items, json!([])),
            (Request::Item { query: gem() }, json!(null)),
            (Request::IsOpen { shop_id: None }, json!(false)),
            (Request::Close { shop_id: None }, json!(false)),
            (
                Request::CanBuy {
                    query: gem(),
                    quantity: None,
                },
                json!(false),
            ),
            (Request::MaxBuyQuantity { query: gem() }, json!(0)),
            (
                Request::Buy {
                    query: gem(),
                    quantity: None,
                },
                json!(false),
            ),
            (
                Request::Sell {
                    query: ItemQuery::Name("Gem".into()),
                    quantity: None,
                },
                json!(false),
            ),
            (
                Request::EnhanceItem {
                    query: ItemQuery::Name("Sword".into()),
                    enhancement: "lucky".into(),
                    special: None,
                },
                json!(false),
            ),
        ] {
            assert_eq!(
                serve(&loader, |wants| events.subscribe(wants), request.clone()).await,
                Ok(expected),
                "{request:?}"
            );
        }
    }
}
