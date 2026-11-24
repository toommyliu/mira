use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use gpui::{App, Global};
use serde::{Deserialize, Serialize};

use crate::{
    accounts::AccountId,
    automation::{auto_relogin, game_settings, scripts, settings::Automation},
    data_file,
    domain::profiles::same_name,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct AccountSettings {
    pub(crate) scripts: scripts::Settings,
    pub(crate) options: Options,
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

const SETTINGS: &str = "settings.json";

pub(crate) struct Store {
    root: PathBuf,
    settings: HashMap<AccountId, AccountSettings>,
}

impl Global for Store {}

impl Store {
    pub(crate) fn open(root: PathBuf) -> Self {
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(%error, folder = %root.display(), "couldn't read character settings");
                }
                return Self {
                    root,
                    settings: HashMap::new(),
                };
            }
        };
        let settings = entries
            .filter_map(|entry| {
                let folder = entry.ok()?.path();
                let path = folder.join(SETTINGS);
                let name = folder.file_name()?.to_str()?;
                path.is_file()
                    .then(|| (AccountId::of(name), data_file::load_json(&path)))
            })
            .collect();
        Self { root, settings }
    }

    pub(crate) fn get(&self, character: &AccountId) -> AccountSettings {
        self.settings.get(character).cloned().unwrap_or_default()
    }

    pub(crate) fn change(
        &mut self,
        character: &AccountId,
        edit: impl FnOnce(&mut AccountSettings),
    ) -> io::Result<()> {
        let path = self.folder(character)?.join(SETTINGS);
        let settings = self.settings.entry(character.clone()).or_default();
        let before = settings.clone();
        edit(settings);
        if *settings == before {
            return Ok(());
        }
        data_file::write_json(&path, settings)
    }

    pub(crate) fn rename(&mut self, from: &AccountId, to: &AccountId) -> io::Result<()> {
        let (old, new) = (self.folder(from)?, self.folder(to)?);
        if !old.try_exists()? {
            return Ok(());
        }
        remove(&new)?;
        fs::rename(old, &new)?;
        if let Some(settings) = self.settings.remove(from) {
            self.settings.insert(to.clone(), settings);
        } else {
            self.settings.remove(to);
        }
        Ok(())
    }

    pub(crate) fn forget(&mut self, character: &AccountId) -> io::Result<()> {
        self.settings.remove(character);
        remove(&self.folder(character)?)
    }

    pub(crate) fn folder(&self, character: &AccountId) -> io::Result<PathBuf> {
        let name = character.as_str();
        if name.is_empty() || name.starts_with('.') || name.contains(['/', '\0']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name:?} can't name a folder"),
            ));
        }
        Ok(self.root.join(name))
    }

    fn move_auto_attack_profile(&mut self, from: &str, to: Option<&str>) -> io::Result<()> {
        let using: Vec<AccountId> = self
            .settings
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
}

fn remove(folder: &Path) -> io::Result<()> {
    match fs::remove_dir_all(folder) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

pub(crate) fn init(root: PathBuf, cx: &mut App) {
    cx.set_global(Store::open(root));
}

pub(crate) fn folder(character: &AccountId, cx: &App) -> io::Result<PathBuf> {
    cx.global::<Store>().folder(character)
}

pub(crate) fn get(account: &AccountId, cx: &App) -> AccountSettings {
    cx.global::<Store>().get(account)
}

pub(crate) fn change(account: &AccountId, cx: &mut App, edit: impl FnOnce(&mut AccountSettings)) {
    if let Err(error) = cx.global_mut::<Store>().change(account, edit) {
        tracing::error!(%error, "couldn't save account settings");
    }
}

pub(crate) fn rename(from: &AccountId, to: &AccountId, cx: &mut App) {
    if let Err(error) = cx.global_mut::<Store>().rename(from, to) {
        tracing::error!(%error, "couldn't move a character's settings and shared objects to the new username");
    }
}

pub(crate) fn forget(account: &AccountId, cx: &mut App) {
    if let Err(error) = cx.global_mut::<Store>().forget(account) {
        tracing::error!(%error, "couldn't delete a removed account's settings and shared objects");
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
    use crate::{
        automation::game_settings::RenderingMode,
        domain::rooms::{PrivateRoom, Rooms},
    };

    fn temporary_folder() -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        std::env::temp_dir().join(format!("mira-characters-{:x}", u64::from_ne_bytes(random)))
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
        assert_eq!(store.get(&alice).scripts, scripts::Settings::default());
        assert!(store.get(&alice).scripts.safe_start_stop);
        assert_eq!(
            store.get(&alice).scripts.rooms,
            Rooms::Private { room: None }
        );
        store
            .change(&alice, |settings| {
                settings.scripts.restart_after_reconnect = true;
                settings.scripts.rooms = Rooms::Private {
                    room: PrivateRoom::parse("4242"),
                };
                settings.options.game.hide_players = true;
                settings.options.game.rendering.set(RenderingMode::Minimal);
                settings.options.frame_rate = Some(30);
                settings.options.relogin.delay_seconds = 20;
            })
            .unwrap();
        assert_eq!(files(&folder), ["alice"]);
        store
            .change(&bob, |settings| settings.scripts.safe_start_stop = false)
            .unwrap();
        assert_eq!(files(&folder), ["alice", "bob"]);
        assert_eq!(files(&folder.join("alice")), ["settings.json"]);
        let reopened = Store::open(folder.clone());
        let saved = reopened.get(&AccountId::of("ALICE"));
        assert!(saved.scripts.restart_after_reconnect);
        assert_eq!(
            saved.scripts.rooms,
            Rooms::Private {
                room: PrivateRoom::parse("4242")
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
    fn a_damaged_file_resets_only_its_account_and_survives_a_save() {
        let folder = temporary_folder();
        for (name, contents) in [
            (
                "alice",
                r#"{"scripts": {"rooms": {"kind": "private", "room": 1000}}}"#,
            ),
            (
                "bob",
                r#"{"scripts": {"safeStartStop": false}, "unknown": 1}"#,
            ),
            ("carol", "{ not json"),
        ] {
            fs::create_dir_all(folder.join(name)).unwrap();
            fs::write(folder.join(name).join(SETTINGS), contents).unwrap();
        }
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
        let mut store = store;
        store
            .change(&AccountId::of("carol"), |settings| {
                settings.scripts.safe_start_stop = false
            })
            .unwrap();
        assert!(data_file::backed_up(&folder.join("carol"), "{ not json"));
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
    fn a_character_folder_follows_a_username_change_and_goes_away_with_the_account() {
        let folder = temporary_folder();
        let shared = folder.join("alice/shared-objects/localhost/AQLite_Data.sol");
        fs::create_dir_all(shared.parent().unwrap()).unwrap();
        fs::write(&shared, "alice's own").unwrap();
        fs::create_dir_all(folder.join("robert")).unwrap();
        fs::write(
            folder.join("robert").join(SETTINGS),
            r#"{"options": {"frameRate": 12}}"#,
        )
        .unwrap();
        let mut store = Store::open(folder.clone());
        let (alice, alicia, bob, robert) = (
            AccountId::of("alice"),
            AccountId::of("alicia"),
            AccountId::of("bob"),
            AccountId::of("robert"),
        );
        store
            .change(&alice, |settings| settings.options.frame_rate = Some(30))
            .unwrap();
        store.rename(&alice, &alicia).unwrap();
        store.rename(&bob, &robert).unwrap();
        assert_eq!(store.get(&alicia).options.frame_rate, Some(30));
        assert_eq!(store.get(&robert).options.frame_rate, Some(12));
        assert_eq!(files(&folder), ["alicia", "robert"]);
        assert_eq!(
            fs::read_to_string(folder.join("alicia/shared-objects/localhost/AQLite_Data.sol"))
                .unwrap(),
            "alice's own"
        );
        assert_eq!(
            Store::open(folder.clone()).get(&alicia).options.frame_rate,
            Some(30)
        );
        store.forget(&alicia).unwrap();
        store.forget(&alicia).unwrap();
        assert_eq!(store.get(&alicia), AccountSettings::default());
        assert_eq!(files(&folder), ["robert"]);
        assert!(store.folder(&AccountId::of("..")).is_err());
        fs::remove_dir_all(folder).unwrap();
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
}
