use std::{cell::RefCell, collections::HashMap, rc::Rc};

use serde::Deserialize;
use serde_json::{Value, json};

use super::calls::Reply;
use crate::bridge::{CallError, Drained, Event, Subscription};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(transparent)]
pub(crate) struct FeedId(pub(super) u64);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Next { id: FeedId },
    Close { id: FeedId },
}

type Read = Box<dyn Fn(Event, &mut Vec<Value>)>;

pub(crate) struct Feed {
    events: Subscription,
    read: Read,
}

impl Feed {
    pub(super) fn new(
        events: Subscription,
        read: impl Fn(Event, &mut Vec<Value>) + 'static,
    ) -> Self {
        Self {
            events,
            read: Box::new(read),
        }
    }

    async fn next(&self) -> Value {
        let Drained { events, lagged } = self.events.drain().await;
        let mut items = Vec::new();
        for event in events {
            (self.read)(event, &mut items);
        }
        json!({ "items": items, "lagged": lagged })
    }
}

#[derive(Default)]
pub(crate) struct Feeds(RefCell<HashMap<FeedId, Rc<Feed>>>);

impl Feeds {
    pub(super) fn open(&self, id: FeedId, feed: Feed) {
        self.0.borrow_mut().insert(id, Rc::new(feed));
    }
}

pub(super) async fn serve(feeds: &Feeds, request: Request) -> Reply {
    match request {
        Request::Next { id } => {
            let feed = feeds.0.borrow().get(&id).cloned();
            Ok(feed.ok_or(CallError::Closed)?.next().await)
        }
        Request::Close { id } => {
            feeds.0.borrow_mut().remove(&id);
            Ok(Value::Null)
        }
    }
}
