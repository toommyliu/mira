use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::{self, Event, Loader, Subscription},
    domain::{
        self,
        inventory::{self, ItemQuery, Pool},
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
    Items,
    Item {
        query: ItemQuery,
    },
    Quantity {
        query: ItemQuery,
    },
    Contains {
        query: ItemQuery,
        quantity: Option<i32>,
    },
    Equipped {
        query: ItemQuery,
    },
    Slots,
    UsedSlots {
        pool: Option<Pool>,
    },
    AvailableSlots {
        pool: Option<Pool>,
    },
    Equip {
        query: ItemQuery,
        wear: Option<bool>,
    },
    Unequip {
        query: ItemQuery,
    },
    Wear {
        query: ItemQuery,
    },
    Use {
        query: ItemQuery,
    },
}

impl Request {
    fn without_avatar(&self) -> serde_json::Value {
        match self {
            Request::Items => json!([]),
            Request::Item { .. } => serde_json::Value::Null,
            Request::Quantity { .. }
            | Request::Slots
            | Request::UsedSlots { .. }
            | Request::AvailableSlots { .. } => json!(0),
            Request::Contains { .. }
            | Request::Equipped { .. }
            | Request::Equip { .. }
            | Request::Unequip { .. }
            | Request::Wear { .. }
            | Request::Use { .. } => json!(false),
        }
    }
}

pub(super) async fn serve(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    request: Request,
) -> Reply {
    Ok(match request {
        Request::Equip { query, wear } => {
            json!(inventory::equip(loader, subscribe, &query, wear.unwrap_or(false)).await?)
        }
        Request::Unequip { query } => json!(inventory::unequip(loader, subscribe, &query).await?),
        Request::Wear { query } => json!(inventory::wear(loader, subscribe, &query).await?),
        Request::Use { query } => json!(inventory::use_item(loader, subscribe, &query).await?),
        request if !domain::has_avatar(loader)? => request.without_avatar(),
        Request::Items => json!(inventory::items(loader)?),
        Request::Item { query } => json!(inventory::item(loader, &query)?),
        Request::Quantity { query } => {
            json!(inventory::item(loader, &query)?.map_or(0, |item| item.quantity))
        }
        Request::Contains { query, quantity } => json!(
            inventory::item(loader, &query)?
                .is_some_and(|item| item.quantity >= quantity.unwrap_or(1).max(1))
        ),
        Request::Equipped { query } => {
            json!(inventory::item(loader, &query)?.is_some_and(|item| item.equipped))
        }
        Request::Slots => json!(bridge::inventory::get_slots(loader)?),
        Request::UsedSlots { pool } => {
            json!(inventory::used_slots(loader, pool.unwrap_or_default())?)
        }
        Request::AvailableSlots { pool } => {
            json!(inventory::available_slots(
                loader,
                pool.unwrap_or_default()
            )?)
        }
    })
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum TempRequest {
    Items,
    Item {
        query: ItemQuery,
    },
    Quantity {
        query: ItemQuery,
    },
    Contains {
        query: ItemQuery,
        quantity: Option<i32>,
    },
}

pub(super) fn serve_temp(loader: &impl Loader, request: TempRequest) -> Reply {
    if !domain::has_avatar(loader)? {
        return Ok(match request {
            TempRequest::Items => json!([]),
            TempRequest::Item { .. } => serde_json::Value::Null,
            TempRequest::Quantity { .. } => json!(0),
            TempRequest::Contains { .. } => json!(false),
        });
    }
    Ok(match request {
        TempRequest::Items => json!(inventory::temp_items(loader)?),
        TempRequest::Item { query } => json!(inventory::temp_item(loader, &query)?),
        TempRequest::Quantity { query } => {
            json!(inventory::temp_item(loader, &query)?.map_or(0, |item| item.quantity))
        }
        TempRequest::Contains { query, quantity } => json!(
            inventory::temp_item(loader, &query)?
                .is_some_and(|item| item.quantity >= quantity.unwrap_or(1).max(1))
        ),
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
        let potion = || ItemQuery::Name("Potion".into());
        for (request, expected) in [
            (Request::Items, json!([])),
            (Request::Item { query: potion() }, json!(null)),
            (Request::Quantity { query: potion() }, json!(0)),
            (
                Request::Contains {
                    query: potion(),
                    quantity: None,
                },
                json!(false),
            ),
            (Request::Equipped { query: potion() }, json!(false)),
            (Request::Slots, json!(0)),
            (Request::AvailableSlots { pool: None }, json!(0)),
            (
                Request::Equip {
                    query: potion(),
                    wear: Some(true),
                },
                json!(false),
            ),
            (Request::Unequip { query: potion() }, json!(false)),
            (Request::Wear { query: potion() }, json!(false)),
            (Request::Use { query: potion() }, json!(false)),
        ] {
            assert_eq!(
                serve(&loader, |wants| events.subscribe(wants), request.clone()).await,
                Ok(expected),
                "{request:?}"
            );
        }
        for (request, expected) in [
            (TempRequest::Items, json!([])),
            (TempRequest::Item { query: potion() }, json!(null)),
            (TempRequest::Quantity { query: potion() }, json!(0)),
            (
                TempRequest::Contains {
                    query: potion(),
                    quantity: Some(2),
                },
                json!(false),
            ),
        ] {
            assert_eq!(
                serve_temp(&loader, request.clone()),
                Ok(expected),
                "{request:?}"
            );
        }
    }

    #[tokio::test]
    async fn counts_compare_against_the_stack_a_script_asks_for() {
        let loader = FakeLoader::new(|callback, _| {
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" => Value::Bool(true),
                "flash.isNull" => Value::Bool(false),
                "inventory.getItem" | "tempInventory.getItem" => Value::Object(
                    [
                        ("itemId".to_owned(), Value::Number(9.0)),
                        ("quantity".to_owned(), Value::Number(3.0)),
                    ]
                    .into(),
                ),
                "inventory.getSlots" => Value::Number(30.0),
                "inventory.getUsedSlots" => Value::Number(32.0),
                other => panic!("unexpected call {other}"),
            })
        });
        let events = Events::default();
        let ask = |request| serve(&loader, |wants| events.subscribe(wants), request);
        let item = || ItemQuery::ItemId { item_id: 9 };
        for (quantity, expected) in [
            (None, true),
            (Some(0), true),
            (Some(3), true),
            (Some(4), false),
        ] {
            assert_eq!(
                ask(Request::Contains {
                    query: item(),
                    quantity
                })
                .await,
                Ok(json!(expected)),
                "{quantity:?}"
            );
            assert_eq!(
                serve_temp(
                    &loader,
                    TempRequest::Contains {
                        query: item(),
                        quantity
                    }
                ),
                Ok(json!(expected)),
                "{quantity:?}"
            );
        }
        assert_eq!(ask(Request::Quantity { query: item() }).await, Ok(json!(3)));
        assert_eq!(
            ask(Request::AvailableSlots {
                pool: Some(Pool::Misc)
            })
            .await,
            Ok(json!(0))
        );
    }
}
