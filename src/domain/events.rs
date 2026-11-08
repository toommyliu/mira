use serde::{Deserialize, Serialize};

use super::{
    combat::CtPacket,
    drops::DropItemPacket,
    players::{PlayerUpdateLine, PlayerUpdatePacket},
    world::EventPacket,
};
use crate::bridge::{
    Event, Packet,
    packets::{self, disconnected, names},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    MonsterDeath,
    ItemDrop,
    Afk,
    Zone,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum GameEvent {
    MonsterDeath {
        mon_map_id: i32,
    },
    ItemDrop {
        item_id: i32,
        name: String,
        quantity: i32,
    },
    Afk {
        afk: bool,
    },
    Zone {
        zone: String,
    },
}

impl Kind {
    pub(crate) fn wants(self, event: &Event) -> bool {
        let Event::ExtensionResponse { packet } = event else {
            return disconnected(event);
        };
        match self {
            Kind::MonsterDeath => names(packet, CtPacket::FORMAT, CtPacket::CMD),
            Kind::ItemDrop => names(packet, DropItemPacket::FORMAT, DropItemPacket::CMD),
            Kind::Afk => {
                names(packet, PlayerUpdatePacket::FORMAT, PlayerUpdatePacket::CMD)
                    || names(packet, PlayerUpdateLine::FORMAT, PlayerUpdateLine::CMD)
            }
            Kind::Zone => names(packet, EventPacket::FORMAT, EventPacket::CMD),
        }
    }

    pub(crate) fn read(
        self,
        packet: &str,
        username: impl FnOnce() -> Option<String>,
        events: &mut Vec<GameEvent>,
    ) {
        match self {
            Kind::MonsterDeath => {
                if let Some(ct) = decode::<CtPacket>(packet) {
                    events.extend(
                        ct.deaths()
                            .into_iter()
                            .map(|mon_map_id| GameEvent::MonsterDeath { mon_map_id }),
                    );
                }
            }
            Kind::ItemDrop => {
                if let Some(dropped) = decode::<DropItemPacket>(packet) {
                    events.extend(dropped.items.into_iter().filter_map(|item| {
                        Some(GameEvent::ItemDrop {
                            item_id: item.item_id.filter(|id| *id > 0)?,
                            name: item.name,
                            quantity: item.quantity.unwrap_or(1),
                        })
                    }));
                }
            }
            Kind::Afk => {
                let update = match decode::<PlayerUpdatePacket>(packet) {
                    Some(update) => update.afk().map(|afk| (update.username, afk)),
                    None => decode::<PlayerUpdateLine>(packet)
                        .and_then(|update| update.afk().map(|afk| (update.username, afk))),
                };
                if let Some((who, afk)) = update
                    && username().is_some_and(|me| me.eq_ignore_ascii_case(&who))
                {
                    events.push(GameEvent::Afk { afk });
                }
            }
            Kind::Zone => {
                if let Some(event) = decode::<EventPacket>(packet) {
                    events.push(GameEvent::Zone {
                        zone: event.args.zone_set,
                    });
                }
            }
        }
    }
}

fn decode<T: Packet>(packet: &str) -> Option<T> {
    packets::decode(packet).unwrap_or_else(|error| {
        tracing::debug!(cmd = T::CMD, %error, "skipped an unreadable packet");
        None
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(kind: Kind, packet: serde_json::Value) -> Vec<GameEvent> {
        let mut events = Vec::new();
        kind.read(&packet.to_string(), || Some("Player1".into()), &mut events);
        events
    }

    fn wanted(kind: Kind, packet: serde_json::Value) -> bool {
        kind.wants(&Event::ExtensionResponse {
            packet: packet.to_string(),
        })
    }

    #[test]
    fn each_event_type_reads_only_its_own_packet() {
        let ct = json!({ "type": "json", "dataObj": { "cmd": "ct" } });
        let drop = json!({ "type": "json", "dataObj": { "cmd": "dropItem" } });
        let line = json!({ "type": "str", "dataObj": ["uotls", "-1", "player1", "afk:true"] });
        let zone = json!({ "type": "json", "dataObj": { "cmd": "event", "args": {} } });
        assert!(
            wanted(Kind::MonsterDeath, ct.clone()) && !wanted(Kind::MonsterDeath, drop.clone())
        );
        assert!(wanted(Kind::ItemDrop, drop) && !wanted(Kind::ItemDrop, ct.clone()));
        assert!(wanted(Kind::Afk, line) && !wanted(Kind::Afk, ct.clone()));
        assert!(wanted(Kind::Zone, zone) && !wanted(Kind::Zone, ct));
        assert!(Kind::Zone.wants(&Event::Connection {
            status: "OnConnectionLost".into()
        }));
    }

    #[test]
    fn a_fight_update_reports_each_monster_that_died() {
        let packet = json!({ "type": "json", "dataObj": { "cmd": "ct", "m": {
            "7": { "intState": 0, "intHP": 0 },
            "2": { "intState": "0" },
            "3": { "intState": 2, "intHP": 50 },
        } } });
        assert_eq!(
            read(Kind::MonsterDeath, packet),
            [
                GameEvent::MonsterDeath { mon_map_id: 2 },
                GameEvent::MonsterDeath { mon_map_id: 7 },
            ]
        );
    }

    #[test]
    fn a_drop_reports_each_item_with_one_when_the_quantity_is_missing() {
        let packet = json!({ "type": "json", "dataObj": { "cmd": "dropItem", "items": {
            "70": { "ItemID": 70, "sName": "Gem", "iQty": 3 },
            "5": { "ItemID": "5", "sName": "Frog Leg" },
            "0": { "ItemID": 0, "sName": "Nothing" },
        } } });
        assert_eq!(
            read(Kind::ItemDrop, packet),
            [
                GameEvent::ItemDrop {
                    item_id: 70,
                    name: "Gem".into(),
                    quantity: 3
                },
                GameEvent::ItemDrop {
                    item_id: 5,
                    name: "Frog Leg".into(),
                    quantity: 1
                },
            ]
        );
    }

    #[test]
    fn only_your_own_afk_changes_count() {
        let json = |name: &str, afk: bool| json!({ "type": "json", "dataObj": { "cmd": "uotls", "unm": name, "o": { "afk": afk } } });
        let line = |name: &str, update: &str| json!({ "type": "str", "dataObj": ["uotls", "-1", name, update] });
        for (packet, afk) in [
            (json("player1", true), true),
            (json("player1", false), false),
            (line("player1", "strFrame:r2,afk:true"), true),
            (line("player1", "afk:false"), false),
        ] {
            assert_eq!(
                read(Kind::Afk, packet.clone()),
                [GameEvent::Afk { afk }],
                "{packet}"
            );
        }
        for packet in [
            json("player2", true),
            line("player1", "strFrame:r2"),
            json!({ "type": "json", "dataObj": { "cmd": "uotls", "unm": "player1", "o": { "tx": 5 } } }),
        ] {
            assert_eq!(read(Kind::Afk, packet.clone()), [], "{packet}");
        }
        let mut events = Vec::new();
        Kind::Afk.read(&json("player1", true).to_string(), || None, &mut events);
        assert_eq!(events, []);
    }

    #[test]
    fn a_zone_change_reports_the_zone_it_sets() {
        let packet =
            json!({ "type": "json", "dataObj": { "cmd": "event", "args": { "zoneSet": "A" } } });
        assert_eq!(
            read(Kind::Zone, packet),
            [GameEvent::Zone { zone: "A".into() }]
        );
    }
}
