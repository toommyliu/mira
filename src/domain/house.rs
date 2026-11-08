use super::inventory::{Item, ItemQuery, item_from, rows};
use crate::bridge::{self, CallError, Loader};

pub(crate) fn items(loader: &impl Loader) -> Result<Vec<Item>, CallError> {
    Ok(rows(bridge::house::get_items(loader)?, item_from))
}

pub(crate) fn item(loader: &impl Loader, query: &ItemQuery) -> Result<Option<Item>, CallError> {
    Ok(item_from(bridge::house::get_item(
        loader,
        query.selector(),
    )?))
}

pub(crate) fn available_slots(loader: &impl Loader) -> Result<i32, CallError> {
    Ok((bridge::house::get_slots(loader)? - bridge::house::get_used_slots(loader)?).max(0))
}
