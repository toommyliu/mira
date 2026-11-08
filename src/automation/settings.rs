use std::{fs, io, ops::Deref, path::PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::{auto_attack, auto_relogin, auto_zone};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    auto_relogin: auto_relogin::Settings,
    auto_zone: auto_zone::Settings,
    auto_attack: auto_attack::Settings,
}

#[derive(Clone)]
pub(crate) struct Automation {
    relogin: watch::Sender<auto_relogin::Settings>,
    zone: watch::Sender<auto_zone::Settings>,
    attack: watch::Sender<auto_attack::Settings>,
}

impl Automation {
    fn new(settings: Settings) -> Self {
        Self {
            relogin: watch::Sender::new(settings.auto_relogin.normalized()),
            zone: watch::Sender::new(settings.auto_zone),
            attack: watch::Sender::new(settings.auto_attack),
        }
    }

    fn settings(&self) -> Settings {
        Settings {
            auto_relogin: self.auto_relogin(),
            auto_zone: self.auto_zone(),
            auto_attack: self.auto_attack(),
        }
    }

    pub(crate) fn auto_relogin(&self) -> auto_relogin::Settings {
        self.relogin.borrow().clone()
    }

    pub(crate) fn watch_auto_relogin(&self) -> watch::Receiver<auto_relogin::Settings> {
        self.relogin.subscribe()
    }

    pub(crate) fn update_auto_relogin(
        &self,
        change: impl FnOnce(&mut auto_relogin::Settings),
    ) -> auto_relogin::Settings {
        update(&self.relogin, |settings| {
            change(settings);
            *settings = std::mem::take(settings).normalized();
        })
    }

    pub(crate) fn auto_zone(&self) -> auto_zone::Settings {
        *self.zone.borrow()
    }

    pub(crate) fn watch_auto_zone(&self) -> watch::Receiver<auto_zone::Settings> {
        self.zone.subscribe()
    }

    pub(crate) fn update_auto_zone(
        &self,
        change: impl FnOnce(&mut auto_zone::Settings),
    ) -> auto_zone::Settings {
        update(&self.zone, change)
    }

    pub(crate) fn auto_attack(&self) -> auto_attack::Settings {
        *self.attack.borrow()
    }

    pub(crate) fn watch_auto_attack(&self) -> watch::Receiver<auto_attack::Settings> {
        self.attack.subscribe()
    }

    pub(crate) fn update_auto_attack(
        &self,
        change: impl FnOnce(&mut auto_attack::Settings),
    ) -> auto_attack::Settings {
        update(&self.attack, change)
    }
}

impl Default for Automation {
    fn default() -> Self {
        Self::new(Settings::default())
    }
}

fn update<T: Clone + PartialEq>(sender: &watch::Sender<T>, change: impl FnOnce(&mut T)) -> T {
    let mut stored = None;
    sender.send_if_modified(|current| {
        let before = current.clone();
        change(current);
        stored = Some(current.clone());
        *current != before
    });
    stored.expect("send_if_modified always runs its closure")
}

pub(crate) struct Store {
    path: PathBuf,
    automation: Automation,
}

impl Deref for Store {
    type Target = Automation;

    fn deref(&self) -> &Automation {
        &self.automation
    }
}

impl Store {
    pub(crate) fn open(path: PathBuf) -> Self {
        let settings = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Settings>(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %path.display(), "couldn't read automation settings");
                Settings::default()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "couldn't read automation settings");
                Settings::default()
            }
        };
        Self {
            path,
            automation: Automation::new(settings),
        }
    }

    pub(crate) fn change(&self, change: impl FnOnce(&Automation)) -> io::Result<()> {
        let before = self.automation.settings();
        change(&self.automation);
        if self.automation.settings() == before {
            return Ok(());
        }
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let temporary = self.path.with_extension("json.tmp");
        let mut bytes =
            serde_json::to_vec_pretty(&self.automation.settings()).map_err(io::Error::other)?;
        bytes.push(b'\n');
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_path() -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        std::env::temp_dir().join(format!(
            "lyra-automation-{:x}.json",
            u64::from_ne_bytes(random)
        ))
    }

    #[test]
    fn changes_survive_a_restart_and_reach_the_running_task() {
        let path = temporary_path();
        let store = Store::open(path.clone());
        assert_eq!(store.auto_relogin(), auto_relogin::Settings::default());
        let mut running = store.watch_auto_relogin();
        let changed = auto_relogin::Settings {
            enabled: true,
            delay_seconds: 30,
            ..Default::default()
        };
        store
            .change(|automation| {
                automation.update_auto_relogin(|settings| *settings = changed.clone());
            })
            .unwrap();
        assert!(running.has_changed().unwrap());
        assert_eq!(*running.borrow_and_update(), changed);
        assert_eq!(Store::open(path.clone()).auto_relogin(), changed);
        let zone = auto_zone::Settings {
            enabled: true,
            room: Some(auto_zone::Room::Queeniona),
        };
        store
            .change(|automation| {
                automation.update_auto_zone(|settings| *settings = zone);
                automation.update_auto_attack(|settings| settings.enabled = true);
            })
            .unwrap();
        let reopened = Store::open(path.clone());
        assert_eq!(reopened.auto_zone(), zone);
        assert!(reopened.auto_attack().enabled);
        assert_eq!(reopened.auto_relogin(), changed);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn an_unchanged_value_neither_writes_nor_wakes_the_task() {
        let path = temporary_path();
        let store = Store::open(path.clone());
        let running = store.watch_auto_relogin();
        store
            .change(|automation| {
                automation.update_auto_relogin(|settings| settings.delay_seconds = 3);
            })
            .unwrap();
        assert!(!running.has_changed().unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn a_damaged_or_out_of_range_file_loads_safe_values_and_is_kept() {
        let path = temporary_path();
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(
            Store::open(path.clone()).auto_relogin(),
            auto_relogin::Settings::default()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        fs::write(
            &path,
            r#"{"autoRelogin":{"enabled":true,"delaySeconds":99999},"unknown":1}"#,
        )
        .unwrap();
        let loaded = Store::open(path.clone()).auto_relogin();
        assert!(loaded.enabled);
        assert_eq!(
            loaded.delay_seconds,
            auto_relogin::Settings::MAX_DELAY_SECONDS
        );
        assert_eq!(loaded.retries, auto_relogin::Settings::default().retries);
        fs::remove_file(path).unwrap();
    }
}
