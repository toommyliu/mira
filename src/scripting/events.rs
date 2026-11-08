use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    calls::Reply,
    feeds::{Feed, FeedId, Feeds},
};
use crate::{
    bridge::{Event, Loader, Subscription, packets::disconnected},
    domain::{
        auth,
        events::{Kind, Me},
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
            if disconnected(&event) {
                items.push(json!({ "lost": true }));
                return;
            }
            let mut read = Vec::new();
            kind.read(&event, &Player(&loader), &mut read);
            items.extend(read.into_iter().map(|event| json!(event)));
        }),
    );
    Ok(Value::Null)
}

struct Player<'a, L>(&'a L);

impl<L: Loader> Me for Player<'_, L> {
    fn username(&self) -> Option<String> {
        auth::username(self.0).unwrap_or_else(|error| {
            tracing::debug!(%error, "couldn't read the username for an event");
            None
        })
    }
}
