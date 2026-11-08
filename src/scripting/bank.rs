use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::{self, Event, Loader, Subscription},
    domain::{self, bank, inventory::ItemQuery},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Load,
    Items,
    Item {
        query: ItemQuery,
    },
    Quantity {
        query: ItemQuery,
    },
    Contains {
        query: ItemQuery,
        quantity: Option<f64>,
    },
    Slots,
    UsedSlots,
    AvailableSlots,
    IsOpen,
    Open,
    Deposit {
        query: ItemQuery,
    },
    Withdraw {
        query: ItemQuery,
    },
    Swap {
        inventory_query: ItemQuery,
        bank_query: ItemQuery,
    },
}

pub(super) async fn serve(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    request: Request,
) -> Reply {
    Ok(match request {
        Request::Load => json!(bank::load(loader).await?),
        Request::Items => json!(bank::items(loader).await?),
        Request::Item { query } => json!(bank::item(loader, &query).await?),
        Request::Quantity { query } => {
            json!(
                bank::item(loader, &query)
                    .await?
                    .map_or(0.0, |item| item.quantity)
            )
        }
        Request::Contains { query, quantity } => json!(
            bank::item(loader, &query)
                .await?
                .is_some_and(|item| item.quantity >= quantity.unwrap_or(1.0).max(1.0))
        ),
        Request::Open => json!(bank::open(loader).await?),
        Request::Deposit { query } => json!(bank::deposit(loader, subscribe, &query).await?),
        Request::Withdraw { query } => json!(bank::withdraw(loader, subscribe, &query).await?),
        Request::Swap {
            inventory_query,
            bank_query,
        } => json!(bank::swap(loader, subscribe, &inventory_query, &bank_query).await?),
        Request::IsOpen => json!(domain::has_avatar(loader)? && bridge::bank::is_open(loader)?),
        _ if !domain::has_avatar(loader)? => json!(0),
        Request::Slots => json!(bridge::bank::get_slots(loader)?),
        Request::UsedSlots => json!(bridge::bank::get_used_slots(loader)?),
        Request::AvailableSlots => json!(bank::available_slots(loader)?),
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
            (Request::Load, json!(false)),
            (Request::Items, json!([])),
            (Request::Item { query: gem() }, json!(null)),
            (Request::Quantity { query: gem() }, json!(0.0)),
            (
                Request::Contains {
                    query: gem(),
                    quantity: None,
                },
                json!(false),
            ),
            (Request::Slots, json!(0)),
            (Request::UsedSlots, json!(0)),
            (Request::AvailableSlots, json!(0)),
            (Request::IsOpen, json!(false)),
            (Request::Open, json!(false)),
            (Request::Deposit { query: gem() }, json!(false)),
            (Request::Withdraw { query: gem() }, json!(false)),
            (
                Request::Swap {
                    inventory_query: gem(),
                    bank_query: gem(),
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
