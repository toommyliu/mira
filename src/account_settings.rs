use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use gpui::{App, Global};
use serde::{Deserialize, Serialize};

use crate::{
    accounts::AccountId,
    automation::{auto_relogin, game_settings, settings::Automation},
    domain::profiles::same_name,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct AccountSettings {
    pub(crate) scripts: Scripts,
    pub(crate) options: Options,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Scripts {
    pub(crate) safe_start_stop: bool,
    pub(crate) restart_after_reconnect: bool,
    pub(crate) rooms: Rooms,
}

impl Default for Scripts {
    fn default() -> Self {
        Self {
            safe_start_stop: true,
            restart_after_reconnect: false,
            rooms: Rooms::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum Rooms {
    Public,
    #[default]
    RandomPrivate,
    Specific {
        number: RoomNumber,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct RoomNumber(u32);

impl RoomNumber {
    pub(crate) fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if !input.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        input.parse::<u32>().ok()?.try_into().ok()
    }

    pub(crate) fn get(self) -> u32 {
        self.0
    }

    pub(crate) fn is_private(self) -> bool {
        self.0 > 1_000
    }
}

impl TryFrom<u32> for RoomNumber {
    type Error = &'static str;

    fn try_from(number: u32) -> Result<Self, Self::Error> {
        if (1..=99_999).contains(&number) {
            Ok(Self(number))
        } else {
            Err("a room number is from 1 to 99,999")
        }
    }
}

impl From<RoomNumber> for u32 {
    fn from(room: RoomNumber) -> Self {
        room.0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Options {
    pub(crate) game: game_settings::Settings,
    pub(crate) frame_rate: Option<u32>,
    pub(crate) relogin: auto_relogin::Settings,
    pub(crate) auto_attack_profile: Option<String>,
}

impl Options {
    pub(crate) fn apply_to(&self, automation: &Automation) {
        if let Some(fps) = self.frame_rate {
            automation.set_frame_rate(fps);
        }
        automation.update_game_settings(|settings| *settings = self.game.clone());
        automation.update_auto_attack(|settings| {
            settings.profile.clone_from(&self.auto_attack_profile);
        });
        automation.update_auto_relogin(|settings| {
            *settings = auto_relogin::Settings {
                enabled: settings.enabled,
                server: settings.server.take(),
                ..self.relogin.clone()
            }
        });
    }
}

pub(crate) struct Store {
    folder: PathBuf,
    accounts: HashMap<AccountId, AccountSettings>,
}

impl Global for Store {}

impl Store {
    pub(crate) fn open(folder: PathBuf) -> Self {
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(%error, folder = %folder.display(), "couldn't read account settings");
                }
                return Self {
                    folder,
                    accounts: HashMap::new(),
                };
            }
        };
        let accounts = entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.extension()? != "json" {
                    return None;
                }
                let username = path.file_stem()?.to_str()?;
                Some((AccountId::of(username), load(&path)))
            })
            .collect();
        Self { folder, accounts }
    }

    pub(crate) fn get(&self, account: &AccountId) -> AccountSettings {
        self.accounts.get(account).cloned().unwrap_or_default()
    }

    pub(crate) fn change(
        &mut self,
        account: &AccountId,
        edit: impl FnOnce(&mut AccountSettings),
    ) -> io::Result<()> {
        let path = self.path_of(account)?;
        let settings = self.accounts.entry(account.clone()).or_default();
        let before = settings.clone();
        edit(settings);
        if *settings == before {
            return Ok(());
        }
        fs::create_dir_all(&self.folder)?;
        let temporary = path.with_extension("json.tmp");
        let mut bytes = serde_json::to_vec_pretty(settings).map_err(io::Error::other)?;
        bytes.push(b'\n');
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, path)
    }

    fn move_auto_attack_profile(&mut self, from: &str, to: Option<&str>) -> io::Result<()> {
        let using: Vec<AccountId> = self
            .accounts
            .iter()
            .filter(|(_, settings)| {
                settings
                    .options
                    .auto_attack_profile
                    .as_deref()
                    .is_some_and(|name| same_name(name, from))
            })
            .map(|(account, _)| account.clone())
            .collect();
        for account in using {
            self.change(&account, |settings| {
                settings.options.auto_attack_profile = to.map(str::to_owned);
            })?;
        }
        Ok(())
    }

    fn path_of(&self, account: &AccountId) -> io::Result<PathBuf> {
        let name = account.as_str();
        if name.is_empty() || name.contains(['/', '\0']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name:?} can't name a settings file"),
            ));
        }
        Ok(self.folder.join(format!("{name}.json")))
    }
}

fn load(path: &Path) -> AccountSettings {
    let loaded = fs::read(path).and_then(|bytes| Ok(serde_json::from_slice(&bytes)?));
    loaded.unwrap_or_else(|error| {
        tracing::warn!(%error, path = %path.display(), "couldn't read an account's settings");
        AccountSettings::default()
    })
}

pub(crate) fn init(folder: PathBuf, cx: &mut App) {
    cx.set_global(Store::open(folder));
}

pub(crate) fn get(account: &AccountId, cx: &App) -> AccountSettings {
    cx.global::<Store>().get(account)
}

pub(crate) fn change(account: &AccountId, cx: &mut App, edit: impl FnOnce(&mut AccountSettings)) {
    if let Err(error) = cx.global_mut::<Store>().change(account, edit) {
        tracing::error!(%error, "couldn't save account settings");
    }
}

pub(crate) fn move_auto_attack_profile(from: &str, to: Option<&str>, cx: &mut App) {
    if let Err(error) = cx.global_mut::<Store>().move_auto_attack_profile(from, to) {
        tracing::error!(%error, "couldn't save account settings");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::game_settings::RenderingMode;

    fn temporary_folder() -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        std::env::temp_dir().join(format!(
            "mira-account-settings-{:x}",
            u64::from_ne_bytes(random)
        ))
    }

    fn files(folder: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn each_account_keeps_its_own_settings_file_across_restarts() {
        let folder = temporary_folder();
        let mut store = Store::open(folder.clone());
        let alice = AccountId::of("Alice");
        let bob = AccountId::of("bob");
        assert_eq!(store.get(&alice).scripts, Scripts::default());
        assert!(store.get(&alice).scripts.safe_start_stop);
        assert_eq!(store.get(&alice).scripts.rooms, Rooms::RandomPrivate);
        store
            .change(&alice, |settings| {
                settings.scripts.restart_after_reconnect = true;
                settings.scripts.rooms = Rooms::Specific {
                    number: RoomNumber::parse("4242").unwrap(),
                };
                settings.options.game.hide_players = true;
                settings.options.game.rendering.set(RenderingMode::Minimal);
                settings.options.frame_rate = Some(30);
                settings.options.relogin.delay_seconds = 20;
            })
            .unwrap();
        assert_eq!(files(&folder), ["alice.json"]);
        store
            .change(&bob, |settings| settings.scripts.safe_start_stop = false)
            .unwrap();
        assert_eq!(files(&folder), ["alice.json", "bob.json"]);
        let reopened = Store::open(folder.clone());
        let saved = reopened.get(&AccountId::of("ALICE"));
        assert!(saved.scripts.restart_after_reconnect);
        assert_eq!(
            saved.scripts.rooms,
            Rooms::Specific {
                number: RoomNumber::parse("4242").unwrap()
            }
        );
        assert!(saved.options.game.hide_players);
        assert!(saved.options.game.rendering.minimal());
        assert_eq!(saved.options.frame_rate, Some(30));
        assert_eq!(saved.options.relogin.delay_seconds, 20);
        let bob = reopened.get(&bob);
        assert!(!bob.scripts.safe_start_stop);
        assert!(!bob.scripts.restart_after_reconnect);
        assert!(!bob.options.game.hide_players);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn an_unchanged_value_does_not_write() {
        let folder = temporary_folder();
        let mut store = Store::open(folder.clone());
        store
            .change(&AccountId::of("alice"), |settings| {
                settings.scripts.safe_start_stop = true
            })
            .unwrap();
        assert!(!folder.exists());
    }

    #[test]
    fn a_damaged_file_resets_only_its_account_and_is_kept() {
        let folder = temporary_folder();
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("alice.json"),
            r#"{"scripts": {"rooms": {"kind": "specific", "number": 0}}}"#,
        )
        .unwrap();
        fs::write(
            folder.join("bob.json"),
            r#"{"scripts": {"safeStartStop": false}, "unknown": 1}"#,
        )
        .unwrap();
        fs::write(folder.join("carol.json"), "{ not json").unwrap();
        let store = Store::open(folder.clone());
        assert_eq!(
            store.get(&AccountId::of("alice")),
            AccountSettings::default()
        );
        assert!(!store.get(&AccountId::of("bob")).scripts.safe_start_stop);
        assert_eq!(
            store.get(&AccountId::of("carol")),
            AccountSettings::default()
        );
        assert_eq!(
            fs::read_to_string(folder.join("carol.json")).unwrap(),
            "{ not json"
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_username_that_cant_name_a_file_is_not_saved() {
        let folder = temporary_folder();
        let mut store = Store::open(folder.clone());
        let error = store
            .change(&AccountId::of("../escape"), |settings| {
                settings.scripts.safe_start_stop = false
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(!folder.exists());
    }

    #[test]
    fn auto_relogin_stays_off_and_keeps_its_server_when_an_account_loads() {
        let automation = Automation::default();
        automation.update_auto_relogin(|settings| settings.server = Some("Twilly".into()));
        let mut options = Options::default();
        options.relogin.enabled = true;
        options.relogin.server = Some("Yorumi".into());
        options.relogin.retries = 7;
        options.game.walk_speed = 20;
        options.apply_to(&automation);
        let relogin = automation.auto_relogin();
        assert!(!relogin.enabled);
        assert_eq!(relogin.server.as_deref(), Some("Twilly"));
        assert_eq!(relogin.retries, 7);
        assert_eq!(automation.game_settings().walk_speed, 20);
    }

    #[test]
    fn renaming_or_deleting_a_profile_follows_every_account_that_uses_it() {
        let folder = temporary_folder();
        let mut store = Store::open(folder.clone());
        let (alice, bob, carol) = (
            AccountId::of("alice"),
            AccountId::of("bob"),
            AccountId::of("carol"),
        );
        for (account, profile) in [(&alice, "Boss"), (&bob, "boss"), (&carol, "Solo")] {
            store
                .change(account, |settings| {
                    settings.options.auto_attack_profile = Some(profile.into())
                })
                .unwrap();
        }
        store
            .move_auto_attack_profile("BOSS", Some("Raid"))
            .unwrap();
        store.move_auto_attack_profile("Solo", None).unwrap();
        let reopened = Store::open(folder.clone());
        let profile = |account| reopened.get(account).options.auto_attack_profile;
        assert_eq!(profile(&alice).as_deref(), Some("Raid"));
        assert_eq!(profile(&bob).as_deref(), Some("Raid"));
        assert_eq!(profile(&carol), None);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_room_number_is_a_whole_number_from_1_to_99999() {
        for (input, number) in [("1", 1), (" 42 ", 42), ("1000", 1000), ("99999", 99_999)] {
            assert_eq!(RoomNumber::parse(input).map(RoomNumber::get), Some(number));
        }
        for input in ["", "0", "+5", "12a", "100000", "-1"] {
            assert_eq!(RoomNumber::parse(input), None, "{input}");
        }
        assert!(!RoomNumber::parse("1000").unwrap().is_private());
        assert!(RoomNumber::parse("1001").unwrap().is_private());
    }
}
