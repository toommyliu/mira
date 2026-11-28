use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use super::{
    calls::Reply,
    feeds::{Feed, FeedId, Feeds},
};
use crate::{
    bridge::{Event, Loader, Subscription, packets::disconnected},
    domain::events::{Kind, Myself},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Subscribe { id: FeedId, filter: Filter },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Filter {
    #[serde(rename = "type")]
    kind: Kind,
    fields: Vec<(String, Field)>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
enum Field {
    Text(String),
    Flag(bool),
    Number {
        #[serde(deserialize_with = "decimal")]
        number: f64,
    },
    Null,
}

fn decimal<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    String::deserialize(deserializer)?
        .parse()
        .map_err(serde::de::Error::custom)
}

impl Field {
    fn matches(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Text(wanted), Value::String(value)) => wanted == value,
            (Self::Flag(wanted), Value::Bool(value)) => wanted == value,
            (Self::Number { number }, Value::Number(value)) => value.as_f64() == Some(*number),
            (Self::Null, Value::Null) => true,
            _ => false,
        }
    }
}

pub(super) fn serve(
    loader: &(impl Loader + Clone + 'static),
    subscribe: impl FnOnce(Box<dyn Fn(&Event) -> bool>) -> Subscription,
    feeds: &Feeds,
    request: Request,
) -> Reply {
    let Request::Subscribe { id, filter } = request;
    let Filter { kind, fields } = filter;
    let events = subscribe(Box::new(move |event| kind.wants(event)));
    let loader = loader.clone();
    feeds.open(
        id,
        Feed::new(events, move |event, items| {
            if kind.needs_connection() && disconnected(&event) {
                items.push(json!({ "lost": true }));
                return;
            }
            let mut read = Vec::new();
            kind.read(&event, &Myself(&loader), &mut read);
            items.extend(read.into_iter().map(|event| json!(event)).filter(|event| {
                fields
                    .iter()
                    .all(|(key, field)| event.get(key).is_some_and(|value| field.matches(value)))
            }));
        }),
    );
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bridge::{CallError, Events, Value as Flash},
        scripting::feeds,
    };

    #[derive(Clone)]
    struct Offline;

    impl Loader for Offline {
        fn call(&self, callback: &'static str, _: Vec<Flash>) -> Result<Flash, CallError> {
            panic!("{callback} was called");
        }
    }

    #[tokio::test]
    async fn a_filtered_aura_feed_batches_exact_matches_in_packet_order() {
        let events = Events::default();
        let feeds = Feeds::default();
        let request = serde_json::from_value(json!({
            "op": "subscribe", "id": 1, "filter": {
                "type": "auraAdded",
                "fields": [["targetType", "monster"], ["self", false],
                    ["duration", { "number": "3.9999999999999996" }], ["icon", null]],
            }
        }))
        .unwrap();
        assert_eq!(
            serve(&Offline, |wants| events.subscribe(wants), &feeds, request),
            Ok(Value::Null)
        );
        events.publish(&Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": { "cmd": "ct", "a": [{
                "cmd": "aura+", "tInf": "m:4", "auras": [
                    { "nam": "First", "dur": "3.9999999999999996" },
                    { "nam": "Wrong duration", "dur": 4 },
                    { "nam": "Wrong icon", "dur": "3.9999999999999996", "icon": "buff" },
                    { "nam": "Last", "dur": "3.9999999999999996" },
                ]
            }] } })
            .to_string()
            .into(),
        });
        let request = feeds::Request::Next { id: FeedId(1) };
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            feeds::serve(&feeds, request),
        )
        .await
        .unwrap()
        .unwrap();
        let aura = |name| {
            json!({
                "type": "auraAdded", "name": name, "targetType": "monster", "targetId": 4,
                "self": false, "duration": 3.9999999999999996, "icon": null
            })
        };
        assert_eq!(
            reply,
            json!({ "lagged": false, "items": [aura("First"), aura("Last")] })
        );
    }

    #[tokio::test]
    async fn losing_the_connection_is_the_logout_event_and_ends_every_other_kind() {
        let events = Events::default();
        let feeds = Feeds::default();
        for (id, filter) in [
            (1, json!({ "type": "logout", "fields": [] })),
            (2, json!({ "type": "zone", "fields": [["zone", "left"]] })),
        ] {
            let request =
                serde_json::from_value(json!({ "op": "subscribe", "id": id, "filter": filter }))
                    .unwrap();
            assert_eq!(
                serve(&Offline, |wants| events.subscribe(wants), &feeds, request),
                Ok(Value::Null)
            );
        }
        events.publish(&Event::Connection {
            status: "OnConnectionLost".into(),
        });
        let next = |id: u64| {
            let request = serde_json::from_value(json!({ "op": "next", "id": id })).unwrap();
            feeds::serve(&feeds, request)
        };
        assert_eq!(
            next(1).await,
            Ok(json!({ "lagged": false, "items": [{ "type": "logout" }] }))
        );
        assert_eq!(
            next(2).await,
            Ok(json!({ "lagged": false, "items": [{ "lost": true }] }))
        );
    }
}
