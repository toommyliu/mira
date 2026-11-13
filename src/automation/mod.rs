pub(crate) mod anti_counter;
pub(crate) mod auto_attack;
pub(crate) mod auto_relogin;
pub(crate) mod auto_zone;
pub(crate) mod boosts;
pub(crate) mod drops;
pub(crate) mod follower;
pub(crate) mod game_settings;
pub(crate) mod packets;
pub(crate) mod quests;
pub(crate) mod settings;
pub(crate) mod sign_in;
pub(crate) mod spammer;

use crate::domain::inventory::ItemQuery;

pub(crate) fn register_items(registered: &mut Vec<ItemQuery>, items: Vec<ItemQuery>) {
    for item in items.into_iter().filter_map(ItemQuery::normalized) {
        if !registered.iter().any(|known| known.same(&item)) {
            registered.push(item);
        }
    }
}

pub(crate) fn unregister_items(registered: &mut Vec<ItemQuery>, items: &[ItemQuery]) {
    registered.retain(|known| !items.iter().any(|item| item.same(known)));
}
