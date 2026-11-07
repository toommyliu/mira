use std::marker::PhantomData;

use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, IgnoredAny, MapAccess, SeqAccess},
};

use super::{Event, Lagged, Subscription};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Format {
    Json,
    Str,
}

pub(crate) trait Packet: DeserializeOwned {
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
            return false;
        };
        let opening = match T::FORMAT {
            Format::Json => "\"cmd\":\"",
            Format::Str => "\"dataObj\":[\"",
        };
        packet.match_indices(opening).any(|(at, key)| {
            packet[at + key.len()..]
                .strip_prefix(T::CMD)
                .is_some_and(|rest| rest.starts_with('"'))
        })
    }

    pub(crate) async fn next(&self) -> Result<T, Lagged> {
        loop {
            let Event::ExtensionResponse { packet } = self.events.next().await? else {
                continue;
            };
            match decode(&packet) {
                Ok(Some(decoded)) => return Ok(decoded),
                Ok(None) => {}
                Err(error) => tracing::debug!(cmd = T::CMD, %error, "skipped an unreadable packet"),
            }
        }
    }

    pub(crate) fn clear(&self) {
        self.events.clear();
    }
}

fn decode<T: Packet>(packet: &str) -> Result<Option<T>, serde_json::Error> {
    #[derive(Deserialize)]
    struct Head<'a> {
        #[serde(rename = "type")]
        format: Format,
        #[serde(rename = "dataObj", borrow)]
        data: Cmd<'a>,
    }

    #[derive(Deserialize)]
    struct Body<T> {
        #[serde(rename = "dataObj")]
        data: T,
    }

    let head = serde_json::from_str::<Head>(packet)?;
    if head.format != T::FORMAT || head.data.0 != T::CMD {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str::<Body<T>>(packet)?.data))
}

struct Cmd<'a>(&'a str);

impl<'de: 'a, 'a> Deserialize<'de> for Cmd<'a> {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Cmd<'de>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an object with a cmd or an array starting with one")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Cmd<'de>, A::Error> {
                let mut cmd = None;
                while let Some(key) = map.next_key::<Key>()? {
                    match key {
                        Key::Cmd => cmd = Some(map.next_value()?),
                        Key::Other => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                cmd.map(Cmd)
                    .ok_or_else(|| serde::de::Error::missing_field("cmd"))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Cmd<'de>, A::Error> {
                let cmd = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Cmd(cmd))
            }
        }

        input.deserialize_any(Visitor)
    }
}

enum Key {
    Cmd,
    Other,
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = Key;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a key")
            }

            fn visit_str<E: serde::de::Error>(self, key: &str) -> Result<Key, E> {
                Ok(if key == "cmd" { Key::Cmd } else { Key::Other })
            }
        }

        input.deserialize_identifier(Visitor)
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
        assert_eq!(waves.next().await, Ok(WavePacket { size: 5 }));
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
            respawns.next().await,
            Ok(RespawnMonPacket {
                cmd: "respawnMon".into(),
                room: "-1".into(),
                ids: "2,3".into(),
            })
        );
    }
}
