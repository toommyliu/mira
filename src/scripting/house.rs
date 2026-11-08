use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::{self, Loader},
    domain::{self, house, inventory::ItemQuery},
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
        quantity: Option<f64>,
    },
    Slots,
    UsedSlots,
    AvailableSlots,
}

pub(super) fn serve(loader: &impl Loader, request: Request) -> Reply {
    if !domain::has_avatar(loader)? {
        return Ok(match request {
            Request::Items => json!([]),
            Request::Item { .. } => serde_json::Value::Null,
            Request::Contains { .. } => json!(false),
            Request::Quantity { .. }
            | Request::Slots
            | Request::UsedSlots
            | Request::AvailableSlots => json!(0),
        });
    }
    Ok(match request {
        Request::Items => json!(house::items(loader)?),
        Request::Item { query } => json!(house::item(loader, &query)?),
        Request::Quantity { query } => {
            json!(house::item(loader, &query)?.map_or(0.0, |item| item.quantity))
        }
        Request::Contains { query, quantity } => json!(
            house::item(loader, &query)?
                .is_some_and(|item| item.quantity >= quantity.unwrap_or(1.0).max(1.0))
        ),
        Request::Slots => json!(bridge::house::get_slots(loader)?),
        Request::UsedSlots => json!(bridge::house::get_used_slots(loader)?),
        Request::AvailableSlots => json!(house::available_slots(loader)?),
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
        let chair = || ItemQuery::Name("Chair".into());
        for (request, expected) in [
            (Request::Items, json!([])),
            (Request::Item { query: chair() }, json!(null)),
            (Request::Quantity { query: chair() }, json!(0)),
            (
                Request::Contains {
                    query: chair(),
                    quantity: None,
                },
                json!(false),
            ),
            (Request::Slots, json!(0)),
            (Request::UsedSlots, json!(0)),
            (Request::AvailableSlots, json!(0)),
        ] {
            assert_eq!(serve(&loader, request.clone()), Ok(expected), "{request:?}");
        }
    }
}
