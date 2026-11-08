use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{
    arrive,
    combat::selector,
    inventory::{ItemCount, ItemQuery, Row, by_id, item_count, list, ready_item, reply, row},
    wait_for_action, wait_until_ready,
};
use crate::bridge::{
    self, CallError, Event, Format, GameAction, Loader, Packet, Packets, Subscription, Value,
    packets::wire,
};

const LOAD_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum ShopItemQuery {
    ShopItemId {
        #[serde(rename = "shopItemId")]
        shop_item_id: i32,
    },
    Item(ItemQuery),
}

impl ShopItemQuery {
    fn selector(&self) -> Value {
        match self {
            ShopItemQuery::ShopItemId { shop_item_id } => {
                selector("shopItemId", Value::Number(f64::from(*shop_item_id)))
            }
            ShopItemQuery::Item(query) => query.selector(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShopItem {
    shop_item_id: i32,
    item_id: i32,
    name: String,
    description: String,
    category: String,
    quantity: i32,
    max_stack: i32,
    cost: i32,
    coins: bool,
    member: bool,
    level: i32,
    faction: Option<FactionRequirement>,
    requirements: Vec<ItemCount>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct FactionRequirement {
    id: i32,
    name: String,
    rep: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Shop {
    id: i32,
    name: String,
    merge: bool,
    limited: bool,
    house: bool,
    pub(crate) items: Vec<ShopItem>,
}

fn item_from(value: Value) -> Option<ShopItem> {
    let Value::Object(mut fields) = value else {
        return None;
    };
    let requirements = list(&mut fields, "requirements")
        .into_iter()
        .filter_map(|value| item_count(&row(value)?))
        .collect();
    let faction = fields.remove("faction").and_then(row).and_then(|faction| {
        Some(FactionRequirement {
            id: faction.positive("id")?,
            name: faction.text("name"),
            rep: faction.integer("rep").unwrap_or(0),
        })
    });
    let item = Row(fields);
    Some(ShopItem {
        shop_item_id: item.positive("shopItemId")?,
        item_id: item.positive("itemId")?,
        name: item.text("name"),
        description: item.text("description"),
        category: item.text("category"),
        quantity: item.integer("quantity").unwrap_or(1),
        max_stack: item.integer("maxStack").unwrap_or(1),
        cost: item.integer("cost").unwrap_or(0),
        coins: item.flag("coins"),
        member: item.flag("member"),
        level: item.integer("level").unwrap_or(0),
        faction,
        requirements,
    })
}

fn shop_from(value: Value) -> Option<Shop> {
    let Value::Object(mut fields) = value else {
        return None;
    };
    let items = list(&mut fields, "items")
        .into_iter()
        .filter_map(item_from)
        .collect();
    let shop = Row(fields);
    Some(Shop {
        id: shop.positive("id")?,
        name: shop.text("name"),
        merge: shop.flag("merge"),
        limited: shop.flag("limited"),
        house: shop.flag("house"),
        items,
    })
}

pub(crate) fn current(loader: &impl Loader) -> Result<Option<Shop>, CallError> {
    Ok(shop_from(bridge::shops::get_shop(loader)?))
}

pub(crate) fn item(
    loader: &impl Loader,
    query: &ShopItemQuery,
) -> Result<Option<ShopItem>, CallError> {
    Ok(item_from(bridge::shops::get_item(
        loader,
        query.selector(),
    )?))
}

pub(crate) fn can_buy(
    loader: &impl Loader,
    query: &ShopItemQuery,
    quantity: i32,
) -> Result<bool, CallError> {
    bridge::shops::can_buy_item(loader, query.selector(), Some(quantity))
}

pub(crate) fn max_buy_quantity(
    loader: &impl Loader,
    query: &ShopItemQuery,
) -> Result<i32, CallError> {
    bridge::shops::get_max_buy_quantity(loader, query.selector())
}

pub(crate) async fn load(loader: &impl Loader, shop_id: i32) -> Result<bool, CallError> {
    if shop_id <= 0 || !wait_until_ready(loader).await? {
        return Ok(false);
    }
    if bridge::shops::is_open(loader, Some(shop_id))? {
        return Ok(true);
    }
    if !wait_for_action(loader, GameAction::LoadShop).await? {
        return Ok(false);
    }
    // The client's popup ignores fOpen("Shop") while it already shows a shop, so another open
    // shop would stay on screen after this one loads.
    bridge::shops::close(loader, None)?;
    bridge::shops::load(loader, shop_id)?;
    arrive(loader, LOAD_TIMEOUT, || {
        Ok(bridge::shops::is_open(loader, Some(shop_id))?.then_some(true))
    })
    .await
}

#[derive(Deserialize)]
pub(crate) struct BuyItemPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::integer")]
    item_id: Option<i32>,
    #[serde(rename = "bitSuccess", default, deserialize_with = "wire::flag")]
    success: bool,
}

impl Packet for BuyItemPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "buyItem";
}

#[derive(Deserialize)]
pub(crate) struct SellItemPacket {
    #[serde(rename = "CharItemID", default, deserialize_with = "wire::integer")]
    char_item_id: Option<i64>,
}

impl Packet for SellItemPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "sellItem";
}

pub(crate) async fn buy(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    query: &ShopItemQuery,
    quantity: i32,
) -> Result<bool, CallError> {
    if quantity < 1 || !wait_until_ready(loader).await? {
        return Ok(false);
    }
    let Some(listing) = item(loader, query)? else {
        return Ok(false);
    };
    if !wait_for_action(loader, GameAction::BuyItem).await? {
        return Ok(false);
    }
    let replies = Packets::<BuyItemPacket>::subscribe(subscribe);
    let selector = ShopItemQuery::ShopItemId {
        shop_item_id: listing.shop_item_id,
    }
    .selector();
    if !bridge::shops::buy(loader, selector, Some(quantity))? {
        return Ok(false);
    }
    Ok(reply(&replies, |packet| {
        packet
            .item_id
            .is_none_or(|item_id| item_id == listing.item_id)
            .then_some(packet.success)
    })
    .await)
}

pub(crate) async fn sell(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    query: &ItemQuery,
    quantity: Option<i32>,
) -> Result<bool, CallError> {
    if quantity.is_some_and(|quantity| quantity < 1) {
        return Ok(false);
    }
    let Some(item) = ready_item(loader, query).await? else {
        return Ok(false);
    };
    let Some(char_item_id) = item.char_item_id else {
        return Ok(false);
    };
    if !wait_for_action(loader, GameAction::SellItem).await? {
        return Ok(false);
    }
    let replies = Packets::<SellItemPacket>::subscribe(subscribe);
    if !bridge::shops::sell(loader, by_id(item.item_id), quantity)? {
        return Ok(false);
    }
    Ok(reply(&replies, |packet| {
        (packet.char_item_id == Some(char_item_id)).then_some(true)
    })
    .await)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

    use serde_json::json;

    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    fn object(fields: &[(&str, Value)]) -> Value {
        Value::Object(
            fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn packet(data: serde_json::Value) -> Event {
        Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": data }).to_string(),
        }
    }

    struct Game {
        loaded: Option<i32>,
        shown: Option<i32>,
        replies: Vec<serde_json::Value>,
    }

    fn shop_id(args: &[Value]) -> i32 {
        let Value::Number(shop_id) = args[0] else {
            panic!("needs a shop id: {args:?}");
        };
        shop_id as i32
    }

    fn game(
        open: Option<i32>,
        replies: Vec<serde_json::Value>,
    ) -> (FakeLoader, Rc<Events>, Rc<RefCell<Game>>) {
        let events = Rc::new(Events::default());
        let state = Rc::new(RefCell::new(Game {
            loaded: open,
            shown: open,
            replies,
        }));
        let (publisher, game_state) = (events.clone(), state.clone());
        let loader = FakeLoader::new(move |callback, args| {
            let mut game = game_state.borrow_mut();
            let open = |game: &Game, wanted: i32| {
                game.shown.is_some() && (wanted <= 0 || game.loaded == Some(wanted))
            };
            Ok(match callback {
                "auth.isLoggedIn"
                | "world.isLoaded"
                | "player.isLoaded"
                | "world.isActionAvailable" => Value::Bool(true),
                "shops.isOpen" => Value::Bool(open(&game, shop_id(args))),
                "shops.close" => {
                    let closed = open(&game, shop_id(args));
                    if closed {
                        game.shown = None;
                    }
                    Value::Bool(closed)
                }
                "shops.load" => {
                    game.loaded = Some(shop_id(args));
                    game.shown = game.shown.or(game.loaded);
                    Value::Null
                }
                "shops.getItem" => {
                    let Value::Object(selector) = &args[0] else {
                        panic!("getItem needs a selector: {args:?}");
                    };
                    match selector.get("shopItemId") {
                        Some(Value::Number(id)) if *id == 9740.0 => object(&[
                            ("shopItemId", Value::Number(9740.0)),
                            ("itemId", Value::Number(11482.0)),
                        ]),
                        _ => Value::Null,
                    }
                }
                "inventory.getItem" => object(&[
                    ("itemId", Value::Number(5.0)),
                    ("charItemId", Value::Number(50.0)),
                    ("name", Value::String("Gem".into())),
                    ("quantity", Value::Number(3.0)),
                ]),
                "shops.buy" | "shops.sell" => {
                    for reply in std::mem::take(&mut game.replies) {
                        publisher.publish(&packet(reply));
                    }
                    Value::Bool(true)
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        (loader, events, state)
    }

    #[tokio::test(start_paused = true)]
    async fn loading_finishes_once_that_shop_is_open() {
        let (loader, _, _) = game(None, Vec::new());
        assert_eq!(load(&loader, 2114).await, Ok(true));
        assert_eq!(loader.count("shops.load"), 1);

        let (loader, _, _) = game(Some(2114), Vec::new());
        assert_eq!(load(&loader, 2114).await, Ok(true));
        assert_eq!(loader.count("shops.load"), 0);

        let (loader, _, _) = game(Some(2114), Vec::new());
        assert_eq!(load(&loader, 0).await, Ok(false));
        assert_eq!(loader.count("shops.isOpen"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn loading_another_shop_replaces_the_one_on_screen() {
        let (loader, _, game) = game(Some(2114), Vec::new());
        assert_eq!(load(&loader, 211).await, Ok(true));
        assert_eq!(game.borrow().shown, Some(211));
    }

    #[tokio::test(start_paused = true)]
    async fn loading_a_shop_that_never_opens_gives_up() {
        let loader = FakeLoader::new(|callback, _| {
            Ok(match callback {
                "auth.isLoggedIn"
                | "world.isLoaded"
                | "player.isLoaded"
                | "world.isActionAvailable" => Value::Bool(true),
                "shops.isOpen" => Value::Bool(false),
                "shops.close" => Value::Bool(false),
                "shops.load" => Value::Null,
                other => panic!("unexpected call {other}"),
            })
        });
        let started = tokio::time::Instant::now();
        assert_eq!(load(&loader, 2114).await, Ok(false));
        assert_eq!(started.elapsed(), LOAD_TIMEOUT);
    }

    fn by_shop_item(shop_item_id: i32) -> ShopItemQuery {
        ShopItemQuery::ShopItemId { shop_item_id }
    }

    #[tokio::test(start_paused = true)]
    async fn buying_reports_the_servers_answer_for_that_item() {
        let other = json!({ "cmd": "buyItem", "bitSuccess": 1, "ItemID": 9, "iQty": 1 });
        for (bought, expected) in [
            (
                json!({ "cmd": "buyItem", "bitSuccess": 1, "ItemID": 11482, "iQty": 1 }),
                true,
            ),
            (
                json!({ "cmd": "buyItem", "bitSuccess": 0, "ItemID": "11482" }),
                false,
            ),
            (
                json!({ "cmd": "buyItem", "bitSuccess": 0, "strMessage": "" }),
                false,
            ),
        ] {
            let (loader, events, _) = game(Some(2114), vec![other.clone(), bought.clone()]);
            let subscribe = |wants| events.subscribe(wants);
            assert_eq!(
                buy(&loader, subscribe, &by_shop_item(9740), 1).await,
                Ok(expected),
                "{bought}"
            );
        }

        let (loader, events, _) = game(Some(2114), vec![other]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            buy(&loader, subscribe, &by_shop_item(9740), 1).await,
            Ok(false)
        );

        let (loader, events, _) = game(Some(2114), Vec::new());
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            buy(&loader, subscribe, &by_shop_item(9740), 0).await,
            Ok(false)
        );
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            buy(&loader, subscribe, &by_shop_item(1), 1).await,
            Ok(false)
        );
        assert_eq!(loader.count("shops.buy"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn selling_finishes_when_the_server_takes_that_stack() {
        let sold = |char_item_id: u32| json!({ "cmd": "sellItem", "CharItemID": char_item_id, "iQty": 1, "intAmount": 25 });
        let gem = ItemQuery::Name("Gem".into());
        let (loader, events, _) = game(None, vec![sold(51), sold(50)]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(sell(&loader, subscribe, &gem, Some(1)).await, Ok(true));

        let (loader, events, _) = game(None, vec![sold(51)]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(sell(&loader, subscribe, &gem, None).await, Ok(false));

        let (loader, events, _) = game(None, vec![sold(50)]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(sell(&loader, subscribe, &gem, Some(0)).await, Ok(false));
        assert_eq!(loader.count("shops.sell"), 0);
    }

    #[test]
    fn merge_requirements_and_listing_ids_reach_scripts() {
        let shop = object(&[
            ("id", Value::Number(2114.0)),
            ("name", Value::String("Alchemy Merge".into())),
            ("merge", Value::Bool(true)),
            ("limited", Value::Bool(false)),
            ("house", Value::Bool(false)),
            (
                "items",
                Value::List(vec![
                    object(&[
                        ("shopItemId", Value::Number(9740.0)),
                        ("itemId", Value::Number(11482.0)),
                        ("name", Value::String("Dragon Scale".into())),
                        ("quantity", Value::Number(2.0)),
                        ("maxStack", Value::Number(1000.0)),
                        ("cost", Value::Number(0.0)),
                        ("coins", Value::Bool(false)),
                        ("level", Value::Number(0.0)),
                        ("faction", Value::Null),
                        (
                            "requirements",
                            Value::List(vec![object(&[
                                ("itemId", Value::Number(11475.0)),
                                ("name", Value::String("Dragon Scape".into())),
                                ("quantity", Value::Number(1.0)),
                            ])]),
                        ),
                    ]),
                    object(&[
                        ("shopItemId", Value::Number(4721.0)),
                        ("itemId", Value::Number(5588.0)),
                        (
                            "faction",
                            object(&[
                                ("id", Value::Number(20.0)),
                                ("name", Value::String("Arcangrove".into())),
                                ("rep", Value::Number(302500.0)),
                            ]),
                        ),
                    ]),
                    object(&[("itemId", Value::Number(1.0))]),
                ]),
            ),
        ]);
        assert_eq!(
            serde_json::to_value(shop_from(shop).unwrap()).unwrap(),
            json!({ "id": 2114, "name": "Alchemy Merge", "merge": true, "limited": false,
                    "house": false, "items": [{
                        "shopItemId": 9740, "itemId": 11482, "name": "Dragon Scale",
                        "description": "", "category": "", "quantity": 2, "maxStack": 1000,
                        "cost": 0, "coins": false, "member": false, "level": 0, "faction": null,
                        "requirements": [{ "itemId": 11475, "name": "Dragon Scape", "quantity": 1 }]
                    }, {
                        "shopItemId": 4721, "itemId": 5588, "name": "", "description": "",
                        "category": "", "quantity": 1, "maxStack": 1, "cost": 0, "coins": false,
                        "member": false, "level": 0,
                        "faction": { "id": 20, "name": "Arcangrove", "rep": 302500 },
                        "requirements": []
                    }] })
        );
    }
}
