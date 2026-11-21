use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::de::IgnoredAny;

use crate::accounts::AccountId;

pub(crate) fn file(skua_scripts: &Path, account: &AccountId) -> PathBuf {
    skua_scripts
        .with_file_name("options")
        .join(format!("CBO_Storage({}).txt", account.as_str()))
}

pub(crate) fn advanced_skills(skua_scripts: &Path) -> PathBuf {
    skua_scripts.join("Skills").join("AdvancedSkills.json")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Options {
    lines: Vec<String>,
}

impl Options {
    pub(crate) fn parse(text: &str) -> Self {
        Self {
            lines: text.lines().map(str::to_owned).collect(),
        }
    }

    pub(crate) fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let temporary = path.with_extension("txt.tmp");
        fs::write(&temporary, self.text())?;
        fs::rename(temporary, path)
    }

    pub(crate) fn text(&self) -> String {
        self.lines.iter().map(|line| format!("{line}\n")).collect()
    }

    fn line(&self, key: &str) -> Option<usize> {
        self.lines.iter().position(|line| line.starts_with(key))
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        let value = self.lines[self.line(key)?].split(": ").nth(1)?;
        (value != "fail" && !value.trim().is_empty()).then_some(value)
    }

    pub(crate) fn set(&mut self, key: &str, value: Option<&str>) {
        let value = value.filter(|value| !value.trim().is_empty());
        match (self.line(key), value) {
            (Some(index), Some(value)) => self.lines[index] = format!("{key}: {value}"),
            (Some(index), None) => {
                self.lines.remove(index);
            }
            (None, Some(value)) => self.lines.push(format!("{key}: {value}")),
            (None, None) => {}
        }
    }

    pub(crate) fn switch(&self, key: &str, default: bool) -> bool {
        self.get(key).map_or(default, |value| value == "True")
    }

    pub(crate) fn set_switch(&mut self, key: &str, on: bool) {
        self.set(key, Some(if on { "True" } else { "False" }));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Switch(bool),
    Number(i32),
    Text(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Field {
    pub(crate) key: &'static str,
    pub(crate) label: &'static str,
    pub(crate) description: Option<&'static str>,
    pub(crate) kind: Kind,
}

const fn switch(
    key: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    default: bool,
) -> Field {
    Field {
        key,
        label,
        description,
        kind: Kind::Switch(default),
    }
}

const fn number(
    key: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    default: i32,
) -> Field {
    Field {
        key,
        label,
        description,
        kind: Kind::Number(default),
    }
}

pub(crate) const PRIVATE_ROOMS: &str = "PrivateRooms";
pub(crate) const ROOM: &str = "PrivateRoomNr";

pub(crate) const GENERAL: &[Field] = &[
    switch(
        PRIVATE_ROOMS,
        "Private rooms",
        Some("Join private rooms instead of public ones."),
        true,
    ),
    number(
        ROOM,
        "Room",
        Some("Above 99999 picks a random room."),
        100_000,
    ),
    switch(
        "PublicDifficult",
        "Public rooms for hard fights",
        Some("Join public rooms where the enemy is tough."),
        false,
    ),
    Field {
        key: "StopLocationSelect",
        label: "Map after stopping",
        description: Some("Home or house goes to your house."),
        kind: Kind::Text("whitemap"),
    },
    switch(
        "BankMiscAC",
        "Bank misc. AC items",
        Some("Bank misc. AC items before the bot starts."),
        false,
    ),
    switch(
        "BankUnenhancedACGear",
        "Bank unenhanced AC gear",
        Some("Bank AC gear that isn't enhanced before the bot starts."),
        false,
    ),
    switch(
        "LoggerInChat",
        "Log in chat",
        Some("Also write the bot's log to the game's chat."),
        true,
    ),
    switch(
        "MessageBoxCheck",
        "Hide message boxes",
        Some("Show a message box only when it's necessary."),
        false,
    ),
    switch(
        "RestCheck",
        "Rest after kills",
        Some("Rest after killing a monster."),
        false,
    ),
    switch(
        "AntiLag",
        "Anti-lag",
        Some("Turn on lag killer, hide monsters and run at 10 FPS."),
        true,
    ),
    switch("DisableBestGear", "Disable best gear", None, false),
    switch("DisableAutoEnhance", "Disable auto enhance", None, false),
];

pub(crate) const TIMING: &[Field] = &[
    number("ActionDelayNr", "Action delay", Some("Milliseconds."), 700),
    number(
        "ExitCombatNr",
        "Exit combat delay",
        Some("Milliseconds."),
        1600,
    ),
    number("HuntDelayNr", "Hunt delay", Some("Milliseconds."), 1000),
    number("QuestTriesNr", "Accept and complete tries", None, 20),
    number("QuestMaxNr", "Loaded quests limit", None, 150),
];

pub(crate) const BOOSTS: &[Field] = &[
    switch(
        "doGoldBoost",
        "Gold boosts",
        Some("Use gold boosts when farming gold."),
        false,
    ),
    switch(
        "doClassBoost",
        "Class boosts",
        Some("Use class boosts when ranking up a class."),
        false,
    ),
    switch(
        "doRepBoost",
        "Reputation boosts",
        Some("Use reputation boosts when farming reputation."),
        false,
    ),
    switch(
        "doExpBoost",
        "Experience boosts",
        Some("Use experience boosts when farming experience."),
        false,
    ),
];

pub(crate) const OTHER: &[Field] = &[
    switch(
        "Nation_SellMemVoucher",
        "Sell Voucher of Nulgath",
        Some("Sell the member voucher when it isn't needed."),
        false,
    ),
    switch(
        "Nation_ReturnPolicyDuringSupplies",
        "Swindles Return during Supplies",
        None,
        false,
    ),
    switch(
        "UltraAlteonForSupplies",
        "Ultra Alteon for Supplies",
        Some("Farm Supplies from Ultra Alteon instead of Escherion."),
        false,
    ),
    switch(
        "PvP_SoloPvPBoss",
        "Kill Bludrut Brawl ads first",
        Some("Kill the brawlers and restorers before the boss."),
        false,
    ),
    switch(
        "BCO_Story_TestBot",
        "Story test mode",
        Some("For bot creators. Skips the completed-quest checks except on one-time quests."),
        false,
    ),
];

pub(crate) const SECTIONS: [(&str, &[Field]); 4] = [
    ("General", GENERAL),
    ("Timing", TIMING),
    ("Boosts", BOOSTS),
    ("Other", OTHER),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Solo,
    Farm,
    Dodge,
    Boss,
}

impl Role {
    pub(crate) const ALL: [Role; 4] = [Role::Solo, Role::Farm, Role::Dodge, Role::Boss];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Role::Solo => "Solo",
            Role::Farm => "Farm",
            Role::Dodge => "Dodge",
            Role::Boss => "Boss",
        }
    }

    fn number(self) -> u8 {
        match self {
            Role::Solo => 1,
            Role::Farm => 2,
            Role::Dodge => 3,
            Role::Boss => 4,
        }
    }

    pub(crate) fn class_key(self) -> String {
        format!("{}ClassSelect", self.label())
    }

    pub(crate) fn mode_key(self) -> String {
        format!("{}ModeSelect", self.label())
    }

    pub(crate) fn gear_on_key(self) -> String {
        format!("{}EquipCheck", self.label())
    }

    pub(crate) fn gear_key(self, gear: Gear) -> String {
        format!("{}{}Select", gear.key(), self.number())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gear {
    Helm,
    Armor,
    Cape,
    Weapon,
    Pet,
    GroundItem,
}

impl Gear {
    pub(crate) const ALL: [Gear; 6] = [
        Gear::Helm,
        Gear::Armor,
        Gear::Cape,
        Gear::Weapon,
        Gear::Pet,
        Gear::GroundItem,
    ];

    fn key(self) -> &'static str {
        match self {
            Gear::Helm => "Helm",
            Gear::Armor => "Armor",
            Gear::Cape => "Cape",
            Gear::Weapon => "Weapon",
            Gear::Pet => "Pet",
            Gear::GroundItem => "GroundItem",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Gear::GroundItem => "Ground item",
            gear => gear.key(),
        }
    }

    pub(crate) fn slot(self) -> &'static str {
        match self {
            Gear::Helm => "he",
            Gear::Armor => "co",
            Gear::Cape => "ba",
            Gear::Weapon => "Weapon",
            Gear::Pet => "pe",
            Gear::GroundItem => "mi",
        }
    }
}

pub(crate) const CLASS_SLOT: &str = "ar";

pub(crate) const MODES: [&str; 8] = [
    "Base", "Atk", "Def", "Farm", "Solo", "Supp", "Dodge", "Ultra",
];

pub(crate) type ClassModes = HashMap<String, Vec<&'static str>>;

pub(crate) fn class_modes(json: &str) -> serde_json::Result<ClassModes> {
    let classes: HashMap<String, HashMap<String, IgnoredAny>> = serde_json::from_str(json)?;
    Ok(classes
        .into_iter()
        .map(|(class, modes)| {
            let known = MODES
                .into_iter()
                .filter(|mode| modes.contains_key(*mode))
                .collect();
            (class, known)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_the_way_core_bots_reads_them() {
        let options = Options::parse(
            "ActionDelayNr: 900\r\nPrivateRooms: False\r\nSoloClassSelect: \r\nFarmClassSelect: fail\r\nStopLocationSelect: Home\r\n",
        );
        assert_eq!(options.get("ActionDelay"), Some("900"));
        assert_eq!(options.get("QuestMaxNr"), None);
        assert!(!options.switch(PRIVATE_ROOMS, true));
        assert!(options.switch("LoggerInChat", true));
        assert_eq!(options.get("SoloClassSelect"), None);
        assert_eq!(options.get("FarmClassSelect"), None);
        assert_eq!(options.get("StopLocationSelect"), Some("Home"));
    }

    #[test]
    fn a_change_keeps_every_other_line_where_it_was() {
        let mut options = Options::parse(
            "HelmDodgeSelect: Old Helm\nPrivateRooms: True\nSomethingNew: 1\nSoloClassSelect: Legion Revenant\n",
        );
        options.set_switch(PRIVATE_ROOMS, false);
        options.set(&Role::Solo.class_key(), None);
        options.set(&Role::Boss.gear_key(Gear::GroundItem), Some("Blade of Awe"));
        options.set(&Role::Farm.mode_key(), Some("  "));
        assert_eq!(
            options.text(),
            "HelmDodgeSelect: Old Helm\nPrivateRooms: False\nSomethingNew: 1\nGroundItem4Select: Blade of Awe\n"
        );
    }

    #[test]
    fn saving_creates_the_options_folder_and_reloads_the_same_lines() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let home =
            std::env::temp_dir().join(format!("mira-core-bots-{:x}", u64::from_ne_bytes(random)));
        let path = file(&home.join("Scripts"), &AccountId::of("Hero"));
        assert_eq!(path, home.join("options").join("CBO_Storage(hero).txt"));
        assert_eq!(Options::load(&path).unwrap(), Options::default());
        let mut options = Options::default();
        options.set(&Role::Farm.class_key(), Some("ArchPaladin"));
        options.save(&path).unwrap();
        assert_eq!(Options::load(&path).unwrap(), options);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_class_offers_only_the_modes_core_bots_can_read() {
        let modes = class_modes(
            r#"{"ArchPaladin":{"Base":{},"Farm":{"skills":[]},"Custom":{}},"Rogue":{}}"#,
        )
        .unwrap();
        assert_eq!(modes["ArchPaladin"], ["Base", "Farm"]);
        assert!(modes["Rogue"].is_empty());
    }
}
