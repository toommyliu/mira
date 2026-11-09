use std::{collections::BTreeMap, io, path::Path};

use serde::{Deserialize, Serialize};

const MAX_ROOM: u32 = 99_999;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub(crate) struct ConfigError(String);

#[derive(Debug, PartialEq)]
pub(crate) struct Config {
    pub(crate) players: Vec<String>,
    pub(crate) room: u32,
    items: BTreeMap<String, String>,
    sets: BTreeMap<String, Set>,
}

#[derive(Debug, PartialEq)]
struct Set {
    default: Gear,
    players: Vec<Option<Gear>>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Gear {
    safe_class: Option<String>,
    safe_pot: Option<String>,
    class: Option<String>,
    weapon: Option<String>,
    cape: Option<String>,
    helm: Option<String>,
    armor: Option<String>,
    pet: Option<String>,
    pots: Option<Vec<String>>,
    scroll: Option<String>,
}

#[derive(Deserialize)]
struct File {
    players: Option<Vec<String>>,
    room: Option<String>,
    #[serde(default)]
    items: BTreeMap<String, String>,
    #[serde(default)]
    sets: BTreeMap<String, BTreeMap<String, Gear>>,
}

pub(crate) fn check_name(name: &str) -> Result<(), ConfigError> {
    let mut characters = name.chars();
    let valid = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && characters.all(|rest| rest.is_ascii_alphanumeric() || matches!(rest, '.' | '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(ConfigError(format!(
            "\"{name}\" isn't an army name. Names start with a letter or digit and use only letters, digits, '.', '_' and '-'."
        )))
    }
}

impl Config {
    pub(crate) fn load(folder: &Path, name: &str) -> Result<Self, ConfigError> {
        check_name(name)?;
        let file = format!("{name}.yaml");
        let path = folder.join(&file);
        let source = std::fs::read_to_string(&path).map_err(|error| {
            ConfigError(match error.kind() {
                io::ErrorKind::NotFound => format!("{} doesn't exist", path.display()),
                _ => format!("couldn't read {}: {error}", path.display()),
            })
        })?;
        Self::parse(&source).map_err(|message| ConfigError(format!("{file}: {message}")))
    }

    fn parse(source: &str) -> Result<Self, String> {
        let options = serde_saphyr::options! { with_snippet: false };
        let file: File = serde_saphyr::from_str_with_options(source, options)
            .map_err(|error| error.to_string())?;
        let players = parse_players(file.players)?;
        let room = parse_room(file.room)?;
        let items = file
            .items
            .into_iter()
            .map(|(alias, item)| (alias.trim().to_owned(), item.trim().to_owned()))
            .collect();
        let sets = file
            .sets
            .into_iter()
            .map(|(name, entries)| {
                let set = parse_set(&name, entries, players.len())?;
                Ok((name, set))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            players,
            room,
            items,
            sets,
        })
    }

    pub(crate) fn seat_of(&self, username: &str) -> Option<usize> {
        let username = username.trim().to_lowercase();
        self.players
            .iter()
            .position(|player| player.to_lowercase() == username)
    }

    pub(crate) fn gear(&self, set: &str, player: usize) -> Option<Gear> {
        let set = self.sets.get(set)?;
        let mut gear = set.default.clone();
        if let Some(Some(own)) = set.players.get(player) {
            gear.apply(own);
        }
        gear.resolve(&self.items);
        Some(gear)
    }
}

fn parse_players(players: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let players: Vec<String> = players
        .ok_or("players is required")?
        .into_iter()
        .map(|player| player.trim().to_owned())
        .collect();
    if players.is_empty() {
        return Err("players is empty".into());
    }
    for (index, player) in players.iter().enumerate() {
        if player.is_empty() {
            return Err(format!("players has an empty name at player{}", index + 1));
        }
        if players[..index]
            .iter()
            .any(|earlier| earlier.to_lowercase() == player.to_lowercase())
        {
            return Err(format!("players lists {player} twice"));
        }
    }
    Ok(players)
}

fn parse_room(room: Option<String>) -> Result<u32, String> {
    let room = room.ok_or("room is required")?;
    room.trim()
        .parse::<u32>()
        .ok()
        .filter(|room| (1..=MAX_ROOM).contains(room))
        .ok_or_else(|| format!("room must be a whole number from 1 to {MAX_ROOM}"))
}

fn parse_set(name: &str, entries: BTreeMap<String, Gear>, size: usize) -> Result<Set, String> {
    let mut set = Set {
        default: Gear::default(),
        players: vec![None; size],
    };
    for (key, gear) in entries {
        if key == "default" {
            set.default = gear;
            continue;
        }
        let number = key
            .strip_prefix("player")
            .and_then(|number| number.parse::<usize>().ok())
            .filter(|number| *number >= 1)
            .ok_or_else(|| {
                format!("sets.{name}.{key}: expected default or player1 to player{size}")
            })?;
        let Some(slot) = set.players.get_mut(number - 1) else {
            return Err(format!(
                "sets.{name}.{key}: the army has {size} player{}",
                if size == 1 { "" } else { "s" }
            ));
        };
        *slot = Some(gear);
    }
    Ok(set)
}

impl Gear {
    fn apply(&mut self, own: &Gear) {
        fn over<T: Clone>(field: &mut Option<T>, own: &Option<T>) {
            if own.is_some() {
                field.clone_from(own);
            }
        }
        over(&mut self.safe_class, &own.safe_class);
        over(&mut self.safe_pot, &own.safe_pot);
        over(&mut self.class, &own.class);
        over(&mut self.weapon, &own.weapon);
        over(&mut self.cape, &own.cape);
        over(&mut self.helm, &own.helm);
        over(&mut self.armor, &own.armor);
        over(&mut self.pet, &own.pet);
        over(&mut self.pots, &own.pots);
        over(&mut self.scroll, &own.scroll);
    }

    fn resolve(&mut self, items: &BTreeMap<String, String>) {
        let resolve = |item: &mut String| {
            let trimmed = item.trim();
            *item = items
                .get(trimmed)
                .map_or(trimmed, String::as_str)
                .to_owned();
        };
        for item in [
            &mut self.safe_class,
            &mut self.safe_pot,
            &mut self.class,
            &mut self.weapon,
            &mut self.cape,
            &mut self.helm,
            &mut self.armor,
            &mut self.pet,
            &mut self.scroll,
        ]
        .into_iter()
        .flatten()
        {
            resolve(item);
        }
        self.pots.iter_mut().flatten().for_each(resolve);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARMY: &str = r#"
players:
  - player1
  - player2
  - player3
room: 48213

items:
  sword: Example Sword

sets:
  boss:
    default:
      class: Example Class
      weapon: sword
      pots: [Example Potion]
    player3:
      class: Other Class
      pots: []
"#;

    fn error(source: &str) -> String {
        Config::parse(source).unwrap_err()
    }

    #[test]
    fn a_player_wears_the_default_gear_with_its_own_entry_over_it() {
        let config = Config::parse(ARMY).unwrap();
        assert_eq!(config.players, ["player1", "player2", "player3"]);
        assert_eq!(config.room, 48213);
        let default = config.gear("boss", 0).unwrap();
        assert_eq!(default.class.as_deref(), Some("Example Class"));
        assert_eq!(default.weapon.as_deref(), Some("Example Sword"));
        assert_eq!(default.pots, Some(vec!["Example Potion".to_owned()]));
        let own = config.gear("boss", 2).unwrap();
        assert_eq!(own.class.as_deref(), Some("Other Class"));
        assert_eq!(own.weapon.as_deref(), Some("Example Sword"));
        assert_eq!(own.pots, Some(vec![]));
        assert_eq!(config.gear("farm", 0), None);
    }

    #[test]
    fn a_value_that_isnt_an_alias_is_the_item_name() {
        let config = Config::parse(
            "players: [player1]\nroom: 1\nitems:\n  sword: Example Sword\nsets:\n  boss:\n    default:\n      weapon: \" Sword \"\n      cape: \" sword \"\n",
        )
        .unwrap();
        let gear = config.gear("boss", 0).unwrap();
        assert_eq!(gear.weapon.as_deref(), Some("Sword"));
        assert_eq!(gear.cape.as_deref(), Some("Example Sword"));
    }

    #[test]
    fn usernames_and_rooms_that_look_like_other_types_are_text() {
        let config = Config::parse("players: [1234, true, 007]\nroom: \"8421\"\n").unwrap();
        assert_eq!(config.players, ["1234", "true", "007"]);
        assert_eq!(config.room, 8421);
    }

    #[test]
    fn players_are_matched_ignoring_case_and_spaces() {
        let config = Config::parse("players: [Player1, player2]\nroom: 1\n").unwrap();
        assert_eq!(config.seat_of(" PLAYER2 "), Some(1));
        assert_eq!(config.seat_of("player3"), None);
    }

    #[test]
    fn unknown_top_level_keys_are_ignored() {
        let config = Config::parse("players: [player1]\nroom: 1\nnotes: anything\n").unwrap();
        assert_eq!(config.players, ["player1"]);
    }

    #[test]
    fn mistakes_in_the_roster_and_room_are_rejected() {
        assert_eq!(error("room: 1\n"), "players is required");
        assert_eq!(error("players: []\nroom: 1\n"), "players is empty");
        assert_eq!(
            error("players: [player1, PLAYER1]\nroom: 1\n"),
            "players lists PLAYER1 twice"
        );
        assert_eq!(error("players: [player1]\n"), "room is required");
        for room in ["0", "100000", "abc", "1.5"] {
            assert_eq!(
                error(&format!("players: [player1]\nroom: {room}\n")),
                "room must be a whole number from 1 to 99999",
                "{room}"
            );
        }
    }

    #[test]
    fn mistakes_inside_a_set_are_rejected() {
        let set = |entry: &str| {
            error(&format!(
                "players: [player1, player2]\nroom: 1\nsets:\n  boss:\n    {entry}\n"
            ))
        };
        assert!(
            set("default: { wepon: Sword }").contains("wepon"),
            "{}",
            set("default: { wepon: Sword }")
        );
        assert_eq!(
            set("deafult: { class: Example }"),
            "sets.boss.deafult: expected default or player1 to player2"
        );
        assert_eq!(
            set("player3: { class: Example }"),
            "sets.boss.player3: the army has 2 players"
        );
    }

    #[test]
    fn army_names_stay_inside_the_folder() {
        for name in ["ultras", "ultras-2", "Ultras_2.b"] {
            assert_eq!(check_name(name), Ok(()), "{name}");
        }
        for name in ["", "../ultras", "ultras/2", ".hidden", "ultras 2"] {
            assert!(check_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn a_missing_file_names_its_path() {
        let folder = std::env::temp_dir().join("mira-army-missing");
        let error = Config::load(&folder, "ultras").unwrap_err().to_string();
        assert!(error.contains("ultras.yaml doesn't exist"), "{error}");
    }
}
