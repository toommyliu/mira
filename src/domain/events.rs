use serde::{Deserialize, Serialize};

use super::{
    State,
    auras::FightAurasPacket,
    auth,
    combat::{CtPacket, FightMessagesPacket, UserMessagePacket},
    drops::DropItemPacket,
    inventory::AddItemsPacket,
    player::MoveToAreaPacket,
    players::{
        ExitAreaLine, FightPlayersPacket, PlayerUpdate, PlayerUpdateLine, PlayerUpdatePacket,
    },
    world::{EventPacket, RespawnMonLine},
};
use crate::bridge::{
    self, Event, Loader, Packet,
    packets::{self, disconnected, names},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    MonsterDeath,
    ItemDrop,
    Afk,
    Zone,
    MapJoined,
    CellChanged,
    PlayerJoined,
    PlayerLeft,
    Logout,
    PlayerState,
    MonsterRespawn,
    AuraAdded,
    AuraRemoved,
    Message,
    ItemAdded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TargetType {
    Monster,
    Player,
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
        username: String,
        #[serde(rename = "self")]
        is_self: bool,
        afk: bool,
    },
    Zone {
        zone: String,
    },
    MapJoined {
        map: String,
    },
    CellChanged {
        username: String,
        #[serde(rename = "self")]
        is_self: bool,
        cell: String,
        pad: String,
    },
    PlayerJoined {
        username: String,
        #[serde(rename = "self")]
        is_self: bool,
    },
    PlayerLeft {
        username: String,
        #[serde(rename = "self")]
        is_self: bool,
    },
    Logout,
    PlayerState {
        username: String,
        #[serde(rename = "self")]
        is_self: bool,
        state: State,
    },
    MonsterRespawn {
        mon_map_id: i32,
    },
    AuraAdded {
        name: String,
        target_type: TargetType,
        target_id: i32,
        #[serde(rename = "self")]
        is_self: bool,
        duration: Option<f64>,
    },
    AuraRemoved {
        name: String,
        target_type: TargetType,
        target_id: i32,
        #[serde(rename = "self")]
        is_self: bool,
    },
    Message {
        text: String,
        mon_map_id: Option<i32>,
    },
    ItemAdded {
        item_id: i32,
        name: String,
        quantity: i32,
        temp: bool,
    },
}

pub(crate) trait Me {
    fn username(&self) -> Option<String>;
    fn user_id(&self) -> Option<i32>;
}

pub(crate) struct Myself<'a, L>(pub(crate) &'a L);

impl<L: Loader> Me for Myself<'_, L> {
    fn username(&self) -> Option<String> {
        auth::username(self.0).unwrap_or_else(|error| {
            tracing::debug!(%error, "couldn't read the username for an event");
            None
        })
    }

    fn user_id(&self) -> Option<i32> {
        bridge::player::get_user_id(self.0)
            .inspect_err(|error| tracing::debug!(%error, "couldn't read the user id for an event"))
            .ok()
    }
}

impl Kind {
    pub(crate) fn wants(self, event: &Event) -> bool {
        let Event::ExtensionResponse { packet } = event else {
            return disconnected(event);
        };
        match self {
            Kind::MonsterDeath => names(packet, CtPacket::FORMAT, CtPacket::CMD),
            Kind::ItemDrop => names(packet, DropItemPacket::FORMAT, DropItemPacket::CMD),
            Kind::Afk | Kind::CellChanged => {
                names(packet, PlayerUpdatePacket::FORMAT, PlayerUpdatePacket::CMD)
                    || names(packet, PlayerUpdateLine::FORMAT, PlayerUpdateLine::CMD)
            }
            Kind::PlayerJoined => {
                names(packet, PlayerUpdatePacket::FORMAT, PlayerUpdatePacket::CMD)
            }
            Kind::Zone => names(packet, EventPacket::FORMAT, EventPacket::CMD),
            Kind::MapJoined => names(packet, MoveToAreaPacket::FORMAT, MoveToAreaPacket::CMD),
            Kind::PlayerLeft => names(packet, ExitAreaLine::FORMAT, ExitAreaLine::CMD),
            Kind::Logout => false,
            Kind::PlayerState => {
                names(packet, FightPlayersPacket::FORMAT, FightPlayersPacket::CMD)
                    || names(packet, PlayerUpdatePacket::FORMAT, PlayerUpdatePacket::CMD)
                    || names(packet, PlayerUpdateLine::FORMAT, PlayerUpdateLine::CMD)
            }
            Kind::MonsterRespawn => names(packet, RespawnMonLine::FORMAT, RespawnMonLine::CMD),
            Kind::AuraAdded | Kind::AuraRemoved => {
                names(packet, FightAurasPacket::FORMAT, FightAurasPacket::CMD)
            }
            Kind::Message => {
                names(
                    packet,
                    FightMessagesPacket::FORMAT,
                    FightMessagesPacket::CMD,
                ) || names(packet, UserMessagePacket::FORMAT, UserMessagePacket::CMD)
            }
            Kind::ItemAdded => names(packet, AddItemsPacket::FORMAT, AddItemsPacket::CMD),
        }
    }

    pub(crate) fn needs_connection(self) -> bool {
        self != Kind::Logout
    }

    pub(crate) fn read(self, event: &Event, me: &impl Me, events: &mut Vec<GameEvent>) {
        let Event::ExtensionResponse { packet } = event else {
            if self == Kind::Logout && disconnected(event) {
                events.push(GameEvent::Logout);
            }
            return;
        };
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
                if let Some(PlayerUpdate {
                    username,
                    afk: Some(afk),
                    ..
                }) = player_update(packet)
                {
                    events.push(GameEvent::Afk {
                        is_self: is_me(me, &username),
                        username,
                        afk,
                    });
                }
            }
            Kind::CellChanged => {
                if let Some(PlayerUpdate {
                    username,
                    cell: Some(cell),
                    pad: Some(pad),
                    ..
                }) = player_update(packet)
                {
                    events.push(GameEvent::CellChanged {
                        is_self: is_me(me, &username),
                        username,
                        cell,
                        pad,
                    });
                }
            }
            Kind::PlayerJoined => {
                if let Some(PlayerUpdate {
                    username,
                    joined: true,
                    ..
                }) = decode::<PlayerUpdatePacket>(packet).map(PlayerUpdate::from)
                {
                    events.push(GameEvent::PlayerJoined {
                        is_self: is_me(me, &username),
                        username,
                    });
                }
            }
            Kind::PlayerLeft => {
                if let Some(ExitAreaLine { username, .. }) = decode::<ExitAreaLine>(packet) {
                    events.push(GameEvent::PlayerLeft {
                        is_self: is_me(me, &username),
                        username,
                    });
                }
            }
            Kind::MapJoined => {
                if let Some(MoveToAreaPacket { map }) = decode::<MoveToAreaPacket>(packet)
                    && !map.is_empty()
                {
                    events.push(GameEvent::MapJoined { map });
                }
            }
            Kind::Logout => {}
            Kind::PlayerState => {
                if let Some(fight) = decode::<FightPlayersPacket>(packet) {
                    let mut players: Vec<_> = fight
                        .players
                        .into_iter()
                        .filter_map(|(username, player)| {
                            Some((username, State::from_game(player?.state?)?))
                        })
                        .collect();
                    players.sort_by(|(left, _), (right, _)| left.cmp(right));
                    for (username, state) in players {
                        events.push(GameEvent::PlayerState {
                            is_self: is_me(me, &username),
                            username,
                            state,
                        });
                    }
                } else if let Some(PlayerUpdate {
                    username,
                    state: Some(state),
                    ..
                }) = player_update(packet)
                {
                    events.push(GameEvent::PlayerState {
                        is_self: is_me(me, &username),
                        username,
                        state,
                    });
                }
            }
            Kind::MonsterRespawn => {
                if let Some(respawned) = decode::<RespawnMonLine>(packet) {
                    events.extend(
                        respawned
                            .ids
                            .into_iter()
                            .map(|mon_map_id| GameEvent::MonsterRespawn { mon_map_id }),
                    );
                }
            }
            Kind::AuraAdded | Kind::AuraRemoved => {
                let Some(fight) = decode::<FightAurasPacket>(packet) else {
                    return;
                };
                for change in fight.changes.into_iter().flatten() {
                    let added = match change.cmd.as_str() {
                        "aura+" => true,
                        "aura-" => false,
                        _ => continue,
                    };
                    if added != (self == Kind::AuraAdded) {
                        continue;
                    }
                    for (target_type, target_id) in targets(&change.targets) {
                        let is_self =
                            target_type == TargetType::Player && me.user_id() == Some(target_id);
                        for aura in change.auras.iter().chain(&change.aura) {
                            let name = aura.name.clone();
                            events.push(if added {
                                GameEvent::AuraAdded {
                                    name,
                                    target_type,
                                    target_id,
                                    is_self,
                                    duration: aura.duration,
                                }
                            } else {
                                GameEvent::AuraRemoved {
                                    name,
                                    target_type,
                                    target_id,
                                    is_self,
                                }
                            });
                        }
                    }
                }
            }
            Kind::Message => {
                if let Some(fight) = decode::<FightMessagesPacket>(packet) {
                    for animation in fight.anims.into_iter().flatten() {
                        if animation.msg.is_empty() {
                            continue;
                        }
                        let mon_map_id = targets(&animation.caster)
                            .chain(targets(&animation.target))
                            .find(|(target_type, _)| *target_type == TargetType::Monster)
                            .map(|(_, id)| id);
                        events.push(GameEvent::Message {
                            text: animation.msg,
                            mon_map_id,
                        });
                    }
                } else if let Some(message) = decode::<UserMessagePacket>(packet)
                    && !message.text.is_empty()
                {
                    events.push(GameEvent::Message {
                        text: message.text,
                        mon_map_id: None,
                    });
                }
            }
            Kind::ItemAdded => {
                if let Some(added) = decode::<AddItemsPacket>(packet) {
                    events.extend(added.items.into_iter().filter_map(|item| {
                        Some(GameEvent::ItemAdded {
                            item_id: item.item_id.filter(|id| *id > 0)?,
                            name: item.name,
                            quantity: item.quantity.unwrap_or(1),
                            temp: item.temp,
                        })
                    }));
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

fn player_update(packet: &str) -> Option<PlayerUpdate> {
    decode::<PlayerUpdatePacket>(packet)
        .map(PlayerUpdate::from)
        .or_else(|| decode::<PlayerUpdateLine>(packet).map(PlayerUpdate::from))
}

fn targets(references: &str) -> impl Iterator<Item = (TargetType, i32)> + '_ {
    references.split(',').filter_map(|reference| {
        let (kind, id) = reference.trim().split_once(':')?;
        let kind = match kind {
            "m" => TargetType::Monster,
            "p" => TargetType::Player,
            _ => return None,
        };
        Some((kind, id.parse().ok()?))
    })
}

fn is_me(me: &impl Me, username: &str) -> bool {
    me.username()
        .is_some_and(|me| me.eq_ignore_ascii_case(username))
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

    struct Player1;

    impl Me for Player1 {
        fn username(&self) -> Option<String> {
            Some("Player1".into())
        }

        fn user_id(&self) -> Option<i32> {
            Some(28552)
        }
    }

    fn server(packet: serde_json::Value) -> Event {
        Event::ExtensionResponse {
            packet: packet.to_string(),
        }
    }

    fn read(kind: Kind, packet: serde_json::Value) -> Vec<GameEvent> {
        let mut events = Vec::new();
        kind.read(&server(packet), &Player1, &mut events);
        events
    }

    fn wanted(kind: Kind, packet: serde_json::Value) -> bool {
        kind.wants(&server(packet))
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
    fn afk_changes_name_the_player_and_whether_it_is_you() {
        let json = |name: &str, afk: bool| json!({ "type": "json", "dataObj": { "cmd": "uotls", "unm": name, "o": { "afk": afk } } });
        let line = |name: &str, update: &str| json!({ "type": "str", "dataObj": ["uotls", "-1", name, update] });
        let afk = |username: &str, is_self: bool, afk: bool| GameEvent::Afk {
            username: username.into(),
            is_self,
            afk,
        };
        for (packet, expected) in [
            (json("player1", true), afk("player1", true, true)),
            (json("player1", false), afk("player1", true, false)),
            (
                line("player1", "strFrame:r2,afk:true"),
                afk("player1", true, true),
            ),
            (line("player2", "afk:false"), afk("player2", false, false)),
            (json("player2", true), afk("player2", false, true)),
        ] {
            assert_eq!(read(Kind::Afk, packet.clone()), [expected], "{packet}");
        }
        for packet in [
            line("player1", "strFrame:r2"),
            json!({ "type": "json", "dataObj": { "cmd": "uotls", "unm": "player1", "o": { "tx": 5 } } }),
        ] {
            assert_eq!(read(Kind::Afk, packet.clone()), [], "{packet}");
        }
    }

    #[test]
    fn players_moving_joining_and_leaving_name_the_player() {
        let jumped = json!({ "type": "str", "dataObj": ["uotls", "-1", "player2", "strFrame:r2,strPad:Right,px:500,py:375,mvts:-1,mvtd:0,tx:0,ty:0,bResting:false"] });
        let walked = json!({ "type": "str", "dataObj": ["uotls", "-1", "player2", "tx:793,ty:353,sp:8,strFrame:r2"] });
        let respawned = json!({ "type": "json", "dataObj": { "cmd": "uotls", "unm": "player1", "o": {
            "intHP": 4850, "intState": 1, "strFrame": "Enter", "strPad": "Spawn"
        } } });
        let entered = json!({ "type": "json", "dataObj": { "cmd": "uotls", "unm": "player2", "o": {
            "strUsername": "Player2", "entID": 28554, "strFrame": "r2", "strPad": "Right", "intState": 1, "afk": false
        } } });
        let left = json!({ "type": "str", "dataObj": ["exitArea", "-1", "28554", "player2"] });
        let cell = |username: &str, is_self: bool, cell: &str, pad: &str| GameEvent::CellChanged {
            username: username.into(),
            is_self,
            cell: cell.into(),
            pad: pad.into(),
        };
        assert_eq!(
            read(Kind::CellChanged, jumped),
            [cell("player2", false, "r2", "Right")]
        );
        assert_eq!(read(Kind::CellChanged, walked), []);
        assert_eq!(
            read(Kind::CellChanged, respawned.clone()),
            [cell("player1", true, "Enter", "Spawn")]
        );
        assert_eq!(read(Kind::PlayerJoined, respawned), []);
        assert_eq!(
            read(Kind::PlayerJoined, entered),
            [GameEvent::PlayerJoined {
                username: "player2".into(),
                is_self: false
            }]
        );
        assert!(wanted(Kind::PlayerLeft, left.clone()));
        assert_eq!(
            read(Kind::PlayerLeft, left),
            [GameEvent::PlayerLeft {
                username: "player2".into(),
                is_self: false
            }]
        );
    }

    #[test]
    fn joining_a_map_reports_its_name() {
        let packet = json!({ "type": "json", "dataObj": {
            "cmd": "moveToArea", "strMapName": "battleon", "areaName": "battleon-1", "areaId": 161642
        } });
        assert!(wanted(Kind::MapJoined, packet.clone()));
        assert_eq!(
            read(Kind::MapJoined, packet),
            [GameEvent::MapJoined {
                map: "battleon".into()
            }]
        );
    }

    #[test]
    fn logging_out_comes_from_the_lost_connection() {
        let lost = Event::Connection {
            status: "OnConnectionLost".into(),
        };
        let mut events = Vec::new();
        Kind::Logout.read(&lost, &Player1, &mut events);
        assert_eq!(events, [GameEvent::Logout]);
        assert!(Kind::Logout.wants(&lost) && !Kind::Logout.needs_connection());
        assert!(!wanted(
            Kind::Logout,
            json!({ "type": "json", "dataObj": { "cmd": "ct" } })
        ));
    }

    #[test]
    fn player_states_come_from_fight_updates_and_player_updates() {
        let fight = json!({ "type": "json", "dataObj": { "cmd": "ct", "p": {
            "player2": { "intState": 2 },
            "player1": { "intMP": 63, "intHP": 0, "intState": 0 },
            "player3": { "intHP": 10 },
        } } });
        let update = json!({ "type": "json", "dataObj": { "cmd": "uotls", "unm": "player1", "o": { "intState": 1 } } });
        let state = |username: &str, is_self: bool, state: State| GameEvent::PlayerState {
            username: username.into(),
            is_self,
            state,
        };
        assert!(
            wanted(Kind::PlayerState, fight.clone()) && wanted(Kind::PlayerState, update.clone())
        );
        assert_eq!(
            read(Kind::PlayerState, fight),
            [
                state("player1", true, State::Dead),
                state("player2", false, State::Combat),
            ]
        );
        assert_eq!(
            read(Kind::PlayerState, update),
            [state("player1", true, State::Idle)]
        );
    }

    #[test]
    fn a_respawn_reports_each_monster() {
        let respawned = |ids: &str| json!({ "type": "str", "dataObj": ["respawnMon", "-1", ids] });
        assert!(wanted(Kind::MonsterRespawn, respawned("8")));
        assert_eq!(
            read(Kind::MonsterRespawn, respawned("8")),
            [GameEvent::MonsterRespawn { mon_map_id: 8 }]
        );
        assert_eq!(
            read(Kind::MonsterRespawn, respawned("2,3")),
            [
                GameEvent::MonsterRespawn { mon_map_id: 2 },
                GameEvent::MonsterRespawn { mon_map_id: 3 },
            ]
        );
    }

    #[test]
    fn auras_report_their_target_and_whether_it_is_you() {
        let packet = json!({ "type": "json", "dataObj": { "cmd": "ct", "a": [
            { "cmd": "aura+", "cInf": "p:28552", "tInf": "m:1", "auras": [{ "nam": "Censure", "dur": 10, "isNew": true }] },
            { "cmd": "aura+", "cInf": "p:28552", "tInf": "p:28552", "auras": [{ "nam": "Royal Resolve", "dur": 6 }] },
            { "cmd": "aura+", "cInf": "p:28554", "tInf": "p:28554", "auras": [{ "nam": "Shield" }] },
            { "cmd": "aura-", "tInf": "p:28552", "aura": { "nam": "Stacks Consumed" } },
            null,
        ] } });
        let added = |name: &str, target_type, target_id, is_self, duration| GameEvent::AuraAdded {
            name: name.into(),
            target_type,
            target_id,
            is_self,
            duration,
        };
        assert!(wanted(Kind::AuraAdded, packet.clone()));
        assert_eq!(
            read(Kind::AuraAdded, packet.clone()),
            [
                added("Censure", TargetType::Monster, 1, false, Some(10.0)),
                added("Royal Resolve", TargetType::Player, 28552, true, Some(6.0)),
                added("Shield", TargetType::Player, 28554, false, None),
            ]
        );
        assert_eq!(
            read(Kind::AuraRemoved, packet),
            [GameEvent::AuraRemoved {
                name: "Stacks Consumed".into(),
                target_type: TargetType::Player,
                target_id: 28552,
                is_self: true,
            }]
        );
    }

    #[test]
    fn messages_come_from_monster_animations_and_server_lines() {
        let fight = json!({ "type": "json", "dataObj": { "cmd": "ct", "anims": [
            { "fx": "m", "animStr": "ChargeA", "msg": "You shall listen.", "cInf": "m:1", "tInf": "m:1", "strFrame": "Boss" },
            { "animStr": "Attack1", "cInf": "p:28552", "tInf": "m:1", "strFrame": "Boss" },
        ] } });
        let line = json!({ "type": "json", "dataObj": { "cmd": "umsg", "s": "We move closer to wiping the slate clean." } });
        assert!(wanted(Kind::Message, fight.clone()) && wanted(Kind::Message, line.clone()));
        assert_eq!(
            read(Kind::Message, fight),
            [GameEvent::Message {
                text: "You shall listen.".into(),
                mon_map_id: Some(1)
            }]
        );
        assert_eq!(
            read(Kind::Message, line),
            [GameEvent::Message {
                text: "We move closer to wiping the slate clean.".into(),
                mon_map_id: None
            }]
        );
    }

    #[test]
    fn added_items_report_whether_they_are_temporary() {
        let packet = json!({ "type": "json", "dataObj": { "cmd": "addItems", "items": {
            "87": { "ItemID": 87, "sName": "Treeant Branch", "iQty": 1, "bTemp": true, "iStk": 1 }
        } } });
        assert!(wanted(Kind::ItemAdded, packet.clone()));
        assert_eq!(
            read(Kind::ItemAdded, packet),
            [GameEvent::ItemAdded {
                item_id: 87,
                name: "Treeant Branch".into(),
                quantity: 1,
                temp: true
            }]
        );
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
