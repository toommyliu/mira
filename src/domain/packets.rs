use serde_json::json;

use super::auth;
use crate::bridge::{self, CallError, Loader, Value};

pub(crate) fn send_to_server(loader: &impl Loader, packet: &str) -> Result<bool, CallError> {
    if !bridge::auth::is_logged_in(loader)? {
        return Ok(false);
    }
    bridge::flash::call_game_function(
        loader,
        "sfc.sendString",
        vec![Value::String(packet.to_owned())],
    )?;
    Ok(true)
}

pub(crate) fn send_to_client(
    loader: &impl Loader,
    data: &serde_json::Map<String, serde_json::Value>,
) -> Result<bool, CallError> {
    if !auth::in_game(loader)? {
        return Ok(false);
    }
    let packet = json!({ "t": "xt", "b": { "r": -1, "o": data } });
    bridge::flash::send_client_packet(loader, &packet.to_string(), "json")?;
    Ok(true)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Target {
    #[default]
    Server,
    Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Problem {
    Empty,
    NotForClient,
}

impl Problem {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Problem::Empty => "Type a packet",
            Problem::NotForClient => "The client reads %xt% text, JSON or XML",
        }
    }
}

pub(crate) fn check(target: Target, packet: &str) -> Result<(), Problem> {
    let packet = packet.trim();
    if packet.is_empty() {
        return Err(Problem::Empty);
    }
    if target == Target::Client && client_format(packet).is_none() {
        return Err(Problem::NotForClient);
    }
    Ok(())
}

fn client_format(packet: &str) -> Option<&'static str> {
    match packet.as_bytes().first()? {
        b'%' => Some("str"),
        b'{' => Some("json"),
        b'<' => Some("xml"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    Sent,
    NotInGame,
    Refused(Problem),
}

pub(crate) fn send(
    loader: &impl Loader,
    target: Target,
    packet: &str,
) -> Result<Delivery, CallError> {
    if let Err(problem) = check(target, packet) {
        return Ok(Delivery::Refused(problem));
    }
    let packet = packet.trim();
    let sent = match (target, client_format(packet)) {
        (Target::Client, Some(format)) => {
            let ready = auth::in_game(loader)?;
            if ready {
                bridge::flash::send_client_packet(loader, &fill(loader, packet)?, format)?;
            }
            ready
        }
        _ => send_to_server(loader, &fill(loader, packet)?)?,
    };
    Ok(if sent {
        Delivery::Sent
    } else {
        Delivery::NotInGame
    })
}

const ROOM: &str = "{room}";

fn fill(loader: &impl Loader, packet: &str) -> Result<String, CallError> {
    let mut packet = packet.to_owned();
    let game_object = |path| -> Result<String, CallError> {
        Ok(
            match bridge::flash::get_game_object(loader, path)?
                .and_then(|json| serde_json::from_str(&json).ok())
            {
                Some(serde_json::Value::String(text)) => text,
                Some(serde_json::Value::Number(number)) => number.to_string(),
                _ => String::new(),
            },
        )
    };
    if packet.contains(ROOM) {
        packet = packet.replace(ROOM, &game_object("world.curRoom")?);
    }
    if packet.contains("{map}") {
        packet = packet.replace("{map}", &game_object("world.strAreaName")?);
    }
    if packet.contains("{player}") {
        let name = super::player::identity(loader)?.map(|identity| identity.name);
        packet = packet.replace("{player}", name.as_deref().unwrap_or_default());
    }
    Ok(packet)
}

pub(crate) fn with_room_placeholder(packet: &str) -> String {
    let mut fields: Vec<&str> = packet.split('%').collect();
    match fields.as_slice() {
        ["", "xt", _, _, room, ..]
            if !room.is_empty() && room.bytes().all(|b| b.is_ascii_digit()) =>
        {
            fields[4] = ROOM;
            fields.join("%")
        }
        _ => packet.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::domain::testing::FakeLoader;

    type Sent = Rc<RefCell<Vec<(String, Vec<Value>)>>>;

    fn sending(logged_in: bool) -> (FakeLoader, Sent) {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let seen = sent.clone();
        let loader = FakeLoader::new(move |callback, args| match callback {
            "auth.isLoggedIn" | "world.isLoaded" => Ok(Value::Bool(logged_in)),
            "flash.callGameFunction" | "flash.sendClientPacket" => {
                seen.borrow_mut().push((callback.to_owned(), args.to_vec()));
                Ok(Value::Null)
            }
            other => panic!("unexpected {other}"),
        });
        (loader, sent)
    }

    #[test]
    fn packets_go_out_only_while_logged_in() {
        let (loader, sent) = sending(true);
        assert_eq!(send_to_server(&loader, "%xt%zm%afk%1%false%"), Ok(true));
        let data = json!({ "cmd": "levelUp", "intLevel": 100 });
        assert_eq!(send_to_client(&loader, data.as_object().unwrap()), Ok(true));
        let sent = sent.borrow();
        assert_eq!(
            sent[0],
            (
                "flash.callGameFunction".to_owned(),
                vec!["sfc.sendString".into(), "%xt%zm%afk%1%false%".into()]
            )
        );
        let [Value::String(packet), Value::String(format)] = sent[1].1.as_slice() else {
            panic!("the client packet was sent with its format: {:?}", sent[1]);
        };
        assert_eq!(sent[1].0, "flash.sendClientPacket");
        assert_eq!(format, "json");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(packet).unwrap(),
            json!({ "t": "xt", "b": { "r": -1, "o": { "cmd": "levelUp", "intLevel": 100 } } })
        );

        let (loader, sent) = sending(false);
        assert_eq!(send_to_server(&loader, "%xt%zm%afk%1%false%"), Ok(false));
        assert_eq!(
            send_to_client(&loader, data.as_object().unwrap()),
            Ok(false)
        );
        assert!(sent.borrow().is_empty());
    }

    #[test]
    fn placeholders_take_the_tabs_room_map_and_player_when_sent() {
        let (loader, sent) = sending(true);
        let loader = FakeLoader::new(move |callback, args| match (callback, args) {
            ("flash.getGameObject", [Value::String(path)]) => Ok(Value::String(
                match path.as_str() {
                    "world.curRoom" => "16842",
                    "world.strAreaName" => "\"battleon-1234\"",
                    other => panic!("unexpected {other}"),
                }
                .into(),
            )),
            ("player.getData", _) => Ok(Value::Object(
                [("strUsername".to_owned(), Value::String("Artix".into()))].into(),
            )),
            _ => loader.call(callback, args.to_vec()),
        });
        assert_eq!(
            send(
                &loader,
                Target::Server,
                " %xt%zm%cmd%{room}%tfer%{player}%{map}%{other}% "
            ),
            Ok(Delivery::Sent)
        );
        assert_eq!(
            sent.borrow()[0].1[1],
            Value::String("%xt%zm%cmd%16842%tfer%Artix%battleon-1234%{other}%".into())
        );
    }

    #[test]
    fn the_client_is_sent_text_json_or_xml_in_its_own_format() {
        let (loader, sent) = sending(true);
        for packet in [
            "%xt%server%-1%Hi%",
            r#"{"t":"xt","b":{"r":-1,"o":{"cmd":"levelUp"}}}"#,
            "<msg t='sys'><body action='apiOK' r='0'></body></msg>",
        ] {
            assert_eq!(send(&loader, Target::Client, packet), Ok(Delivery::Sent));
        }
        let formats: Vec<Value> = sent
            .borrow()
            .iter()
            .map(|(_, args)| args[1].clone())
            .collect();
        assert_eq!(formats, ["str".into(), "json".into(), "xml".into()]);
        assert_eq!(
            send(&loader, Target::Client, "hello"),
            Ok(Delivery::Refused(Problem::NotForClient))
        );
        assert_eq!(
            send(&loader, Target::Server, "  "),
            Ok(Delivery::Refused(Problem::Empty))
        );
        assert_eq!(formats.len(), 3);
    }

    #[test]
    fn a_captured_packets_room_becomes_the_placeholder() {
        assert_eq!(
            with_room_placeholder("%xt%zm%getDrop%16842%2%"),
            "%xt%zm%getDrop%{room}%2%"
        );
        assert_eq!(
            with_room_placeholder("%xt%zm%cmd%{room}%"),
            "%xt%zm%cmd%{room}%"
        );
        assert_eq!(
            with_room_placeholder("%xt%server%-1%Hi%"),
            "%xt%server%-1%Hi%"
        );
        assert_eq!(with_room_placeholder("<msg/>"), "<msg/>");
    }
}
