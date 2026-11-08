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
}
