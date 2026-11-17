use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use super::{player, world};
use crate::bridge::{self, CallError, Loader};

const PRIVATE: RangeInclusive<u32> = 1_001..=99_999;
const RANDOM: RangeInclusive<u32> = 10_000..=99_999;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum Rooms {
    Public,
    Private { room: Option<PrivateRoom> },
}

impl Default for Rooms {
    fn default() -> Self {
        Self::Private { room: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct PrivateRoom(u32);

impl PrivateRoom {
    pub(crate) fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        input.parse::<u32>().ok()?.try_into().ok()
    }

    pub(crate) fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for PrivateRoom {
    type Error = &'static str;

    fn try_from(number: u32) -> Result<Self, Self::Error> {
        if PRIVATE.contains(&number) {
            Ok(Self(number))
        } else {
            Err("a private room is from 1,001 to 99,999")
        }
    }
}

impl From<PrivateRoom> for u32 {
    fn from(room: PrivateRoom) -> Self {
        room.0
    }
}

pub(crate) fn destination(
    loader: &impl Loader,
    map: &str,
    rooms: Rooms,
) -> Result<String, CallError> {
    let bare = map.trim();
    let here = match rooms {
        Rooms::Private { room: None } if player::split_room(bare).1.is_none() => {
            bridge::player::get_map(loader)?.filter(|name| name.eq_ignore_ascii_case(bare))
        }
        _ => None,
    };
    let room = match here {
        Some(_) => world::room_number(loader)?,
        None => None,
    };
    Ok(resolve(
        map,
        rooms,
        here.as_deref().map(|name| (name, room)),
        random,
    ))
}

fn resolve(
    map: &str,
    rooms: Rooms,
    here: Option<(&str, Option<u32>)>,
    random: impl FnOnce() -> u32,
) -> String {
    let map = map.trim();
    if map.is_empty() || player::split_room(map).1.is_some() {
        return map.to_owned();
    }
    let room = match rooms {
        Rooms::Public => return map.to_owned(),
        Rooms::Private { room: Some(room) } => room.get(),
        Rooms::Private { room: None } => {
            let current = here
                .filter(|(name, _)| name.eq_ignore_ascii_case(map))
                .and_then(|(_, room)| room);
            match current {
                Some(room) if PRIVATE.contains(&room) => return map.to_owned(),
                _ => random(),
            }
        }
    };
    format!("{map}-{room}")
}

fn random() -> u32 {
    let mut bytes = [0; 4];
    let random = match getrandom::getrandom(&mut bytes) {
        Ok(()) => u32::from_le_bytes(bytes),
        Err(_) => 0,
    };
    RANDOM.start() + random % (RANDOM.end() - RANDOM.start() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bridge::Value, domain::testing::FakeLoader};

    fn pinned(room: u32) -> Rooms {
        Rooms::Private {
            room: Some(room.try_into().unwrap()),
        }
    }

    #[test]
    fn a_bare_map_goes_where_the_rooms_setting_says() {
        let random = || 54_321;
        let elsewhere = Some(("battleon", Some(1)));
        assert_eq!(
            resolve("yulgar", Rooms::Public, elsewhere, random),
            "yulgar"
        );
        assert_eq!(
            resolve(" yulgar ", Rooms::default(), elsewhere, random),
            "yulgar-54321"
        );
        assert_eq!(
            resolve("yulgar", pinned(4242), elsewhere, random),
            "yulgar-4242"
        );
        assert_eq!(
            resolve("yulgar", Rooms::default(), None, random),
            "yulgar-54321"
        );
    }

    #[test]
    fn a_random_private_room_keeps_the_player_in_a_private_room_of_the_same_map() {
        let random = || 54_321;
        for (room, expected) in [
            (Some(1_001), "Yulgar"),
            (Some(20_000), "Yulgar"),
            (Some(1_000), "Yulgar-54321"),
            (None, "Yulgar-54321"),
        ] {
            assert_eq!(
                resolve("Yulgar", Rooms::default(), Some(("yulgar", room)), random),
                expected,
                "{room:?}"
            );
        }
        assert_eq!(
            resolve(
                "yulgar",
                pinned(4242),
                Some(("yulgar", Some(20_000))),
                random
            ),
            "yulgar-4242"
        );
    }

    #[test]
    fn a_map_with_its_own_room_is_never_rewritten() {
        for rooms in [Rooms::Public, Rooms::default(), pinned(4242)] {
            assert_eq!(
                resolve("bossmap-7", rooms, None, || unreachable!()),
                "bossmap-7"
            );
            assert_eq!(resolve("", rooms, None, || unreachable!()), "");
        }
    }

    #[test]
    fn the_destination_reads_the_players_map_and_room() {
        let loader = FakeLoader::new(|callback, args| {
            Ok(match callback {
                "player.getMap" => Value::String("yulgar".into()),
                "flash.getGameObject" if args == ["world.strAreaName".into()] => {
                    Value::String(r#""yulgar-20000""#.into())
                }
                other => panic!("unexpected call {other}"),
            })
        });
        assert_eq!(
            destination(&loader, "yulgar", Rooms::default()),
            Ok("yulgar".into())
        );
        assert_eq!(
            destination(&loader, "yulgar", pinned(4242)),
            Ok("yulgar-4242".into())
        );
        let rooms_read = loader.count("flash.getGameObject");
        assert_eq!(
            destination(&loader, "yulgar-7", Rooms::default()),
            Ok("yulgar-7".into())
        );
        destination(&loader, "battleon", Rooms::default()).unwrap();
        assert_eq!(loader.count("flash.getGameObject"), rooms_read);
    }

    #[test]
    fn random_rooms_are_high_private_rooms() {
        for _ in 0..1_000 {
            assert!(RANDOM.contains(&random()));
        }
    }

    #[test]
    fn a_private_room_is_a_whole_number_from_1001_to_99999() {
        for (input, number) in [("1001", 1_001), (" 4242 ", 4_242), ("99999", 99_999)] {
            assert_eq!(
                PrivateRoom::parse(input).map(PrivateRoom::get),
                Some(number)
            );
        }
        for input in ["", "0", "1000", "+5000", "12a", "100000", "-1"] {
            assert_eq!(PrivateRoom::parse(input), None, "{input}");
        }
    }
}
