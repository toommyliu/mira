use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    calls::Reply,
    feeds::{Feed, FeedId, Feeds},
};
use crate::{
    bridge::{
        Event, Format, Loader, Subscription,
        packets::{disconnected, names},
    },
    domain::packets,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Sender {
    Client,
    Server,
}

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
        from: Option<Sender>,
        cmds: Option<Vec<String>>,
    },
    SendToServer {
        packet: String,
    },
    SendToClient {
        data: serde_json::Map<String, Value>,
    },
}

pub(super) fn serve(
    loader: &impl Loader,
    subscribe: impl FnOnce(Box<dyn Fn(&Event) -> bool>) -> Subscription,
    feeds: &Feeds,
    request: Request,
) -> Reply {
    Ok(match request {
        Request::Subscribe { id, from, cmds } => {
            let events = subscribe(Box::new(wants(from, cmds)));
            feeds.open(
                id,
                Feed::new(events, |event, items| items.extend(read(event))),
            );
            Value::Null
        }
        Request::SendToServer { packet } => json!(packets::send_to_server(loader, &packet)?),
        Request::SendToClient { data } => json!(packets::send_to_client(loader, &data)?),
    })
}

fn wants(from: Option<Sender>, cmds: Option<Vec<String>>) -> impl Fn(&Event) -> bool {
    move |event| match event {
        Event::PacketFromClient { packet } => {
            from != Some(Sender::Server)
                && cmds.as_ref().is_none_or(|cmds| {
                    let cmd = packet.split('%').nth(3);
                    cmds.iter().any(|wanted| Some(wanted.as_str()) == cmd)
                })
        }
        Event::ExtensionResponse { packet } => {
            from != Some(Sender::Client)
                && cmds.as_ref().is_none_or(|cmds| {
                    cmds.iter().any(|cmd| {
                        names(packet, Format::Json, cmd) || names(packet, Format::Str, cmd)
                    })
                })
        }
        event => disconnected(event),
    }
}

fn read(event: Event) -> Option<Value> {
    match event {
        Event::PacketFromClient { packet } => Some(json!({ "from": "client", "packet": packet })),
        Event::ExtensionResponse { packet } => Some(json!({ "from": "server", "packet": packet })),
        event if disconnected(&event) => Some(json!({ "lost": true })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bridge::Events, domain::testing::FakeLoader, scripting::feeds};

    fn subscribe(events: &Events, feeds: &Feeds, id: u64, request: Value) {
        let mut request = request;
        request["op"] = "subscribe".into();
        request["id"] = id.into();
        let request = serde_json::from_value(request).unwrap();
        let loader = FakeLoader::new(|callback, _| panic!("{callback} was called"));
        assert_eq!(
            serve(&loader, |wants| events.subscribe(wants), feeds, request),
            Ok(Value::Null)
        );
    }

    async fn next(feeds: &Feeds, id: u64) -> Value {
        let request = serde_json::from_value(json!({ "op": "next", "id": id })).unwrap();
        feeds::serve(feeds, request).await.unwrap()
    }

    #[tokio::test]
    async fn a_feed_delivers_the_packets_its_filter_names_in_order() {
        let events = Events::default();
        let feeds = Feeds::default();
        subscribe(
            &events,
            &feeds,
            1,
            json!({ "from": "server", "cmds": ["ct", "respawnMon"] }),
        );
        subscribe(
            &events,
            &feeds,
            2,
            json!({ "from": "client", "cmds": ["afk"] }),
        );
        subscribe(&events, &feeds, 3, json!({}));
        let ct = r#"{"type":"json","dataObj":{"cmd":"ct","m":{}}}"#;
        let respawn = r#"{"type":"str","dataObj":["respawnMon","-1","2"]}"#;
        for event in [
            Event::ExtensionResponse {
                packet: r#"{"type":"json","dataObj":{"cmd":"cts"}}"#.into(),
            },
            Event::ExtensionResponse { packet: ct.into() },
            Event::PacketFromClient {
                packet: "[Sending - STR]: %xt%zm%afk%1%true%, (len: 24)".into(),
            },
            Event::PacketFromClient {
                packet: "%xt%zm%afkx%1%".into(),
            },
            Event::ExtensionResponse {
                packet: respawn.into(),
            },
            Event::Connection {
                status: "OnConnectionLost".into(),
            },
        ] {
            events.publish(&event);
        }
        assert_eq!(
            next(&feeds, 1).await,
            json!({ "lagged": false, "items": [
                { "from": "server", "packet": ct },
                { "from": "server", "packet": respawn },
                { "lost": true },
            ] })
        );
        assert_eq!(
            next(&feeds, 2).await,
            json!({ "lagged": false, "items": [
                { "from": "client", "packet": "[Sending - STR]: %xt%zm%afk%1%true%, (len: 24)" },
                { "lost": true },
            ] })
        );
        let everything = next(&feeds, 3).await;
        assert_eq!(everything["items"].as_array().unwrap().len(), 6);
    }

    #[tokio::test]
    async fn a_closed_feed_reports_that_it_is_closed() {
        let events = Events::default();
        let feeds = Feeds::default();
        subscribe(&events, &feeds, 1, json!({}));
        let close = serde_json::from_value(json!({ "op": "close", "id": 1 })).unwrap();
        assert_eq!(feeds::serve(&feeds, close).await, Ok(Value::Null));
        let request = serde_json::from_value(json!({ "op": "next", "id": 1 })).unwrap();
        assert_eq!(
            feeds::serve(&feeds, request).await,
            Err(crate::bridge::CallError::Closed)
        );
    }
}
