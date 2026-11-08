use serde::Deserialize;

use super::{
    inventory::{Item, ItemQuery, item_from, reply, rows},
    wait_until_ready,
};
use crate::bridge::{
    self, CallError, Event, Format, Loader, Packet, Packets, Subscription, packets::wire,
};

pub(crate) fn items(loader: &impl Loader) -> Result<Vec<Item>, CallError> {
    Ok(rows(bridge::drops::get_items(loader)?, item_from))
}

pub(crate) fn item(loader: &impl Loader, query: &ItemQuery) -> Result<Option<Item>, CallError> {
    Ok(item_from(bridge::drops::get_item(
        loader,
        query.selector(),
    )?))
}

pub(crate) fn reject(loader: &impl Loader, query: &ItemQuery) -> Result<bool, CallError> {
    bridge::drops::reject(loader, query.selector())
}

#[derive(Deserialize)]
pub(crate) struct GetDropPacket {
    #[serde(rename = "ItemID", default, deserialize_with = "wire::number")]
    item_id: Option<f64>,
    #[serde(rename = "bSuccess", default, deserialize_with = "wire::flag")]
    success: bool,
}

impl Packet for GetDropPacket {
    const FORMAT: Format = Format::Json;
    const CMD: &'static str = "getDrop";
}

pub(crate) async fn accept(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    query: &ItemQuery,
) -> Result<bool, CallError> {
    if !wait_until_ready(loader).await? {
        return Ok(false);
    }
    let replies = Packets::<GetDropPacket>::subscribe(subscribe);
    let requested = bridge::drops::accept(loader, query.selector())?;
    if requested <= 0 {
        return Ok(false);
    }
    Ok(reply(&replies, |packet| {
        (packet.item_id == Some(f64::from(requested))).then_some(packet.success)
    })
    .await)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;

    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    fn packet(data: serde_json::Value) -> Event {
        Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": data }).to_string(),
        }
    }

    fn drop_list(
        requestable: Vec<(f64, &'static str)>,
        replies: Vec<serde_json::Value>,
    ) -> (FakeLoader, Rc<Events>) {
        let events = Rc::new(Events::default());
        let replies = Rc::new(RefCell::new(replies));
        let publisher = events.clone();
        let loader = FakeLoader::new(move |callback, args| {
            let find = || {
                let Value::Object(selector) = &args[0] else {
                    panic!("{callback} needs a selector: {args:?}");
                };
                requestable.iter().find(|(id, name)| match selector.get("itemId") {
                    Some(Value::Number(wanted)) => id == wanted,
                    _ => matches!(selector.get("name"), Some(Value::String(wanted)) if wanted.eq_ignore_ascii_case(name)),
                })
            };
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "drops.accept" => match find() {
                    Some((id, _)) => {
                        for reply in replies.borrow_mut().drain(..) {
                            publisher.publish(&packet(reply));
                        }
                        Value::Number(*id)
                    }
                    None => Value::Number(0.0),
                },
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        (loader, events)
    }

    fn by_name(name: &str) -> ItemQuery {
        ItemQuery::Name(name.into())
    }

    #[tokio::test(start_paused = true)]
    async fn accepting_finishes_on_the_servers_answer_for_that_drop() {
        let other = json!({ "cmd": "getDrop", "ItemID": 9, "bSuccess": 1 });
        for (answer, expected) in [(1, true), (0, false)] {
            let ours = json!({ "cmd": "getDrop", "ItemID": "45230", "bSuccess": answer });
            let (loader, events) = drop_list(vec![(45230.0, "Templar")], vec![other.clone(), ours]);
            let subscribe = |wants| events.subscribe(wants);
            assert_eq!(
                accept(&loader, subscribe, &by_name("templar")).await,
                Ok(expected),
                "bSuccess {answer}"
            );
        }

        let (loader, events) = drop_list(vec![(45230.0, "Templar")], vec![other]);
        let subscribe = |wants| events.subscribe(wants);
        assert_eq!(
            accept(&loader, subscribe, &by_name("Templar")).await,
            Ok(false)
        );

        let (loader, events) = drop_list(Vec::new(), Vec::new());
        let subscribe = |wants| events.subscribe(wants);
        let started = tokio::time::Instant::now();
        assert_eq!(
            accept(&loader, subscribe, &by_name("Templar")).await,
            Ok(false)
        );
        assert_eq!(started.elapsed(), std::time::Duration::ZERO);
    }
}
