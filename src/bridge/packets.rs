use std::{any::Any, cell::OnceCell, marker::PhantomData, rc::Rc, time::Duration};

use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, IgnoredAny},
};
use tokio::time::{Instant, timeout_at};

use super::{Event, Lagged, Subscription, Value, decode::Decode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Interruption {
    Lagged,
    Disconnected,
}

pub(crate) fn disconnected(event: &Event) -> bool {
    matches!(event, Event::Connection { status } if status == "OnConnectionLost")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Sender {
    Client,
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Raw<'a> {
    pub(crate) from: Sender,
    pub(crate) text: &'a str,
}

pub(crate) fn raw(event: &Event) -> Option<Raw<'_>> {
    let Event::SmartFoxDebug { line } = event else {
        return None;
    };
    read_line(line)
}

fn read_line(line: &str) -> Option<Raw<'_>> {
    const SENT: [&str; 3] = ["[Sending - STR]: ", "[Sending - JSON]: ", "[Sending]: "];
    const RECEIVED: &str = "[ RECEIVED ]: ";
    let line = line.trim();
    let (from, text) = match line.strip_prefix(RECEIVED) {
        Some(text) => (Sender::Server, text),
        None => (
            Sender::Client,
            SENT.iter().find_map(|prefix| line.strip_prefix(prefix))?,
        ),
    };
    let text = without_length(text).trim();
    (!text.is_empty()).then_some(Raw { from, text })
}

fn without_length(text: &str) -> &str {
    text.rsplit_once(", (len: ")
        .filter(|(_, length)| {
            length.strip_suffix(')').is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
            })
        })
        .map_or(text, |(text, _)| text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Json,
    Str,
}

impl Format {
    fn read(format: &str) -> Option<Self> {
        match format {
            "json" => Some(Self::Json),
            "str" => Some(Self::Str),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Response(Rc<ResponseData>);

#[derive(Debug)]
struct ResponseData {
    format: Option<Format>,
    command: Option<String>,
    text: String,
    decoded: OnceCell<Option<Rc<dyn Any>>>,
}

impl PartialEq for Response {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
            || (self.0.format, &self.0.command, &self.0.text)
                == (other.0.format, &other.0.command, &other.0.text)
    }
}

impl Response {
    fn new(format: Option<Format>, command: Option<String>, text: String) -> Self {
        Self(Rc::new(ResponseData {
            format,
            command,
            text,
            decoded: OnceCell::new(),
        }))
    }

    pub(crate) fn text(&self) -> &str {
        &self.0.text
    }

    pub(crate) fn command(&self) -> Option<&str> {
        self.0.format.and(self.0.command.as_deref())
    }

    pub(crate) fn matches(&self, format: Format, cmd: &str) -> bool {
        self.0.format == Some(format) && self.0.command.as_deref() == Some(cmd)
    }

    pub(crate) fn is<T: Packet>(&self) -> bool {
        self.matches(T::FORMAT, T::CMD)
    }

    pub(crate) fn read<T: Packet>(&self) -> Option<Rc<T>> {
        if !self.is::<T>() {
            return None;
        }

        #[derive(Deserialize)]
        struct Envelope<T> {
            #[serde(rename = "dataObj")]
            data: T,
        }

        let decoded =
            self.0
                .decoded
                .get_or_init(|| match serde_json::from_str::<Envelope<T>>(self.text()) {
                    Ok(envelope) => Some(Rc::new(envelope.data)),
                    Err(error) => {
                        tracing::debug!(cmd = T::CMD, %error, "skipped an unreadable packet");
                        None
                    }
                });
        decoded.as_ref().map(|packet| {
            packet.clone().downcast::<T>().unwrap_or_else(|_| {
                panic!(
                    "more than one packet type declared for {:?} {}",
                    T::FORMAT,
                    T::CMD
                )
            })
        })
    }
}

impl Decode for Response {
    const EXPECTED: &'static str = "an extension response format, command and payload";

    fn decode(value: Value) -> Result<Self, Value> {
        let Value::List(parts) = value else {
            return Err(value);
        };
        match <[Value; 3]>::try_from(parts) {
            Ok(
                [
                    Value::String(format),
                    Value::String(command),
                    Value::String(text),
                ],
            ) => Ok(Self::new(Format::read(&format), Some(command), text)),
            Ok([Value::String(format), Value::Null, Value::String(text)]) => {
                Ok(Self::new(Format::read(&format), None, text))
            }
            Ok(parts) => Err(Value::List(parts.into())),
            Err(parts) => Err(Value::List(parts)),
        }
    }
}

#[cfg(test)]
impl From<String> for Response {
    fn from(text: String) -> Self {
        let envelope: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let format = envelope["type"].as_str().and_then(Format::read);
        let body = &envelope["dataObj"];
        let command = match format {
            Some(Format::Json) => body["cmd"].as_str(),
            Some(Format::Str) => body[0].as_str(),
            None => None,
        };
        Self::new(format, command.map(str::to_owned), text)
    }
}

#[cfg(test)]
impl From<&str> for Response {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

pub(crate) trait Packet: DeserializeOwned + 'static {
    const FORMAT: Format;
    const CMD: &'static str;
}

pub(crate) struct Packets<T> {
    events: Subscription,
    packet: PhantomData<fn() -> T>,
}

impl<T: Packet> Packets<T> {
    pub(crate) fn subscribe(subscribe: impl FnOnce(fn(&Event) -> bool) -> Subscription) -> Self {
        Self {
            events: subscribe(Self::wants),
            packet: PhantomData,
        }
    }

    pub(crate) fn wants(event: &Event) -> bool {
        let Event::ExtensionResponse { packet } = event else {
            return disconnected(event);
        };
        packet.is::<T>()
    }

    pub(crate) async fn next(&self) -> Result<Rc<T>, Interruption> {
        loop {
            let event = self
                .events
                .next()
                .await
                .map_err(|Lagged| Interruption::Lagged)?;
            let Event::ExtensionResponse { packet } = event else {
                if disconnected(&event) {
                    return Err(Interruption::Disconnected);
                }
                continue;
            };
            if let Some(decoded) = packet.read() {
                return Ok(decoded);
            }
        }
    }

    pub(crate) async fn answer<R>(
        &self,
        within: Duration,
        mut answer: impl FnMut(&T) -> Option<R>,
    ) -> Option<R> {
        let deadline = Instant::now() + within;
        loop {
            match timeout_at(deadline, self.next()).await {
                Ok(Ok(packet)) => {
                    if let Some(result) = answer(&packet) {
                        return Some(result);
                    }
                }
                Ok(Err(Interruption::Lagged)) => {}
                Ok(Err(Interruption::Disconnected)) | Err(_) => return None,
            }
        }
    }

    pub(crate) fn clear(&self) {
        self.events.clear();
    }
}

pub(crate) mod wire {
    use super::*;

    pub(crate) fn number<'de, D: Deserializer<'de>>(input: D) -> Result<Option<f64>, D::Error> {
        Ok(match serde_json::Value::deserialize(input)? {
            serde_json::Value::Number(number) => number.as_f64(),
            serde_json::Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
        .filter(|number: &f64| number.is_finite()))
    }

    pub(crate) fn integer<'de, D: Deserializer<'de>, T: TryFrom<i64>>(
        input: D,
    ) -> Result<Option<T>, D::Error> {
        Ok(number(input)?.and_then(whole))
    }

    pub(crate) fn whole<T: TryFrom<i64>>(number: f64) -> Option<T> {
        if number.fract() != 0.0 {
            return None;
        }
        T::try_from(number as i64).ok()
    }

    pub(crate) fn flag<'de, D: Deserializer<'de>>(input: D) -> Result<bool, D::Error> {
        Ok(match serde_json::Value::deserialize(input)? {
            serde_json::Value::Bool(flag) => flag,
            serde_json::Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
            serde_json::Value::String(text) => matches!(text.trim(), "1" | "true"),
            _ => false,
        })
    }

    pub(crate) fn values<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        input: D,
    ) -> Result<Vec<T>, D::Error> {
        struct Values<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Values<T> {
            type Value = Vec<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Vec<T>, A::Error> {
                let mut values = Vec::new();
                while let Some((_, value)) = map.next_entry::<IgnoredAny, T>()? {
                    values.push(value);
                }
                Ok(values)
            }
        }

        input.deserialize_map(Values(PhantomData))
    }

    pub(crate) fn text<'de, D: Deserializer<'de>>(input: D) -> Result<String, D::Error> {
        Ok(match serde_json::Value::deserialize(input)? {
            serde_json::Value::String(text) => text,
            _ => String::new(),
        })
    }

    pub(crate) fn present<'de, D: Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
        Ok(Some(text(input)?).filter(|text| !text.is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::Events;

    #[derive(Debug, PartialEq, Deserialize)]
    struct WavePacket {
        size: u32,
    }

    impl Packet for WavePacket {
        const FORMAT: Format = Format::Json;
        const CMD: &'static str = "wave";
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct RespawnMonPacket {
        cmd: String,
        room: String,
        ids: String,
    }

    impl Packet for RespawnMonPacket {
        const FORMAT: Format = Format::Str;
        const CMD: &'static str = "respawnMon";
    }

    #[test]
    fn smartfox_debug_lines_read_as_the_packet_each_side_sent() {
        let read = |line: &str| {
            raw(&Event::SmartFoxDebug { line: line.into() })
                .map(|raw| (raw.from, raw.text.to_owned()))
        };
        assert_eq!(
            read("[Sending - STR]: %xt%zm%afk%1%true%, (len: 24)\n"),
            Some((Sender::Client, "%xt%zm%afk%1%true%".into()))
        );
        assert_eq!(
            read(
                "[Sending]: <msg t='sys'><body action='verChk' r='0'><ver v='157' /></body></msg>\n"
            ),
            Some((
                Sender::Client,
                "<msg t='sys'><body action='verChk' r='0'><ver v='157' /></body></msg>".into()
            ))
        );
        assert_eq!(
            read(r#"[Sending - JSON]: {"t":"sys","b":{"o":{"cmd":"joinOK"}}}"#),
            Some((
                Sender::Client,
                r#"{"t":"sys","b":{"o":{"cmd":"joinOK"}}}"#.into()
            ))
        );
        assert_eq!(
            read("[ RECEIVED ]: %xt%server%-1%Welcome, (len: 1)%, (len: 32)"),
            Some((Sender::Server, "%xt%server%-1%Welcome, (len: 1)%".into()))
        );
        assert_eq!(
            read("[ RECEIVED ]: %xt%chatm%-1%zone~hi, (len: x)%"),
            Some((Sender::Server, "%xt%chatm%-1%zone~hi, (len: x)%".into()))
        );
        assert_eq!(read("Connection lost"), None);
        assert_eq!(read("[ RECEIVED ]: , (len: 0)"), None);
    }

    fn publish(events: &Events, packets: &[&str]) {
        for packet in packets {
            events.publish(&Event::ExtensionResponse {
                packet: (*packet).into(),
            });
        }
    }

    #[tokio::test]
    async fn only_well_formed_json_packets_with_that_cmd_arrive() {
        let events = Events::default();
        let waves = Packets::<WavePacket>::subscribe(|wants| events.subscribe(wants));
        publish(
            &events,
            &[
                r#"{"type":"json","dataObj":{"cmd":"waves","size":1}}"#,
                r#"{"type":"json","dataObj":{"cmd":"ct","nested":{"cmd":"wave"},"size":2}}"#,
                r#"{"type":"str","dataObj":{"cmd":"wave","size":3}}"#,
                r#"{"type":"json","dataObj":{"cmd":"wave","size":"four"}}"#,
                r#"{"type":"json","dataObj":{"size":5,"cmd":"wave"}}"#,
            ],
        );
        assert_eq!(waves.next().await.as_deref(), Ok(&WavePacket { size: 5 }));
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_connection_ends_the_wait_at_once() {
        let events = Events::default();
        let waves = Packets::<WavePacket>::subscribe(|wants| events.subscribe(wants));
        let connection = |status: &str| Event::Connection {
            status: status.into(),
        };
        events.publish(&connection("OnConnection"));
        publish(
            &events,
            &[r#"{"type":"json","dataObj":{"cmd":"wave","size":1}}"#],
        );
        events.publish(&connection("OnConnectionLost"));
        publish(
            &events,
            &[r#"{"type":"json","dataObj":{"cmd":"wave","size":2}}"#],
        );
        let started = tokio::time::Instant::now();
        let answer = waves
            .answer(Duration::from_secs(5), |wave| {
                (wave.size == 2).then_some(())
            })
            .await;
        assert_eq!(answer, None);
        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[test]
    fn ids_read_whole_numbers_and_numeric_strings() {
        #[derive(Deserialize)]
        struct Reply {
            #[serde(rename = "ItemID", default, deserialize_with = "wire::integer")]
            item_id: Option<i32>,
        }
        let read = |id: serde_json::Value| {
            serde_json::from_value::<Reply>(serde_json::json!({ "ItemID": id }))
                .unwrap()
                .item_id
        };
        assert_eq!(read(serde_json::json!(5)), Some(5));
        assert_eq!(read(serde_json::json!(5.0)), Some(5));
        assert_eq!(read(serde_json::json!(" 5 ")), Some(5));
        assert_eq!(read(serde_json::json!(5.5)), None);
        assert_eq!(read(serde_json::json!(3_000_000_000_u64)), None);
        assert_eq!(read(serde_json::json!("five")), None);
    }

    #[tokio::test]
    async fn str_packets_read_their_fields_by_position() {
        let events = Events::default();
        let respawns = Packets::<RespawnMonPacket>::subscribe(|wants| events.subscribe(wants));
        publish(
            &events,
            &[
                r#"{"type":"json","dataObj":{"cmd":"respawnMon","room":-1,"ids":"1"}}"#,
                r#"{"type":"str","dataObj":["respawnMon","-1","2,3"]}"#,
            ],
        );
        assert_eq!(
            respawns.next().await.as_deref(),
            Ok(&RespawnMonPacket {
                cmd: "respawnMon".into(),
                room: "-1".into(),
                ids: "2,3".into(),
            })
        );
    }
}
