use serde::Deserialize;
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
    Subscribe {
        id: FeedId,
        #[serde(rename = "type")]
        kind: Kind,
    },
}

pub(super) fn serve(
    loader: &(impl Loader + Clone + 'static),
    subscribe: impl FnOnce(Box<dyn Fn(&Event) -> bool>) -> Subscription,
    feeds: &Feeds,
    request: Request,
) -> Reply {
    let Request::Subscribe { id, kind } = request;
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
            items.extend(read.into_iter().map(|event| json!(event)));
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
    async fn losing_the_connection_is_the_logout_event_and_ends_every_other_kind() {
        let events = Events::default();
        let feeds = Feeds::default();
        for (id, kind) in [(1, "logout"), (2, "zone")] {
            let request =
                serde_json::from_value(json!({ "op": "subscribe", "id": id, "type": kind }))
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
