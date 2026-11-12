use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::bridge::{self, Event, Loader, Subscription};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Extra {
    CopyMenus,
    ChatHistory,
    BankSearch,
}

impl Extra {
    pub(crate) const ALL: [Extra; 3] = [Extra::CopyMenus, Extra::ChatHistory, Extra::BankSearch];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Extra::CopyMenus => "Right-click menus",
            Extra::ChatHistory => "Chat history",
            Extra::BankSearch => "Bank search",
        }
    }

    pub(crate) fn tooltip(self) -> &'static str {
        match self {
            Extra::CopyMenus => {
                "Adds contextual menus to monsters, items, shops, quests, and drops for more actions."
            }
            Extra::ChatHistory => {
                "Press Up and Down in the chat box to bring back messages you've sent."
            }
            Extra::BankSearch => {
                "Search the bank by enhancement or boost, like forge or dmg undead."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Extras {
    copy_menus: bool,
    chat_history: bool,
    bank_search: bool,
}

impl Extras {
    pub(crate) fn enabled(self, extra: Extra) -> bool {
        match extra {
            Extra::CopyMenus => self.copy_menus,
            Extra::ChatHistory => self.chat_history,
            Extra::BankSearch => self.bank_search,
        }
    }

    fn toggle(&mut self, extra: Extra) {
        let enabled = match extra {
            Extra::CopyMenus => &mut self.copy_menus,
            Extra::ChatHistory => &mut self.chat_history,
            Extra::BankSearch => &mut self.bank_search,
        };
        *enabled = !*enabled;
    }
}

pub(crate) struct Store {
    path: PathBuf,
    extras: watch::Sender<Extras>,
}

impl Store {
    pub(crate) fn open(path: PathBuf) -> Self {
        let extras = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %path.display(), "couldn't read AS3 extras");
                Extras::default()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Extras::default(),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "couldn't read AS3 extras");
                Extras::default()
            }
        };
        Self {
            path,
            extras: watch::Sender::new(extras),
        }
    }

    pub(crate) fn get(&self) -> Extras {
        *self.extras.borrow()
    }

    pub(crate) fn watch(&self) -> watch::Receiver<Extras> {
        self.extras.subscribe()
    }

    pub(crate) fn toggle(&self, extra: Extra) -> io::Result<()> {
        self.extras.send_modify(|extras| extras.toggle(extra));
        let temporary = self.path.with_extension("json.tmp");
        let mut bytes = serde_json::to_vec_pretty(&self.get()).map_err(io::Error::other)?;
        bytes.push(b'\n');
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, &self.path)
    }
}

pub(super) fn wants(event: &Event) -> bool {
    *event == Event::Loaded
}

pub(super) async fn run<L: Loader>(
    loader: L,
    loads: Subscription,
    mut extras: watch::Receiver<Extras>,
) {
    loop {
        tokio::select! {
            _ = loads.next() => {}
            changed = extras.changed() => if changed.is_err() {
                return;
            },
        }
        let extras = *extras.borrow_and_update();
        let applied = bridge::extras::set_copy_menus_enabled(&loader, extras.copy_menus)
            .and_then(|()| bridge::extras::set_chat_history_enabled(&loader, extras.chat_history))
            .and_then(|()| bridge::extras::set_bank_search_enabled(&loader, extras.bank_search));
        if let Err(error) = applied {
            tracing::debug!(%error, "AS3 extras will apply once the loader is ready");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use tokio::task::{LocalSet, yield_now};

    use super::*;
    use crate::bridge::{CallError, Events, Value};
    use crate::domain::testing::FakeLoader;

    fn temporary_path() -> PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        std::env::temp_dir().join(format!("mira-as3-{:x}.json", u64::from_ne_bytes(random)))
    }

    #[test]
    fn extras_start_off_and_each_toggle_survives_a_restart() {
        let path = temporary_path();
        let store = Store::open(path.clone());
        let mut running = store.watch();
        for extra in Extra::ALL {
            assert!(!store.get().enabled(extra));
        }

        store.toggle(Extra::ChatHistory).unwrap();
        assert!(running.borrow_and_update().enabled(Extra::ChatHistory));
        let reopened = Store::open(path.clone()).get();
        assert!(reopened.enabled(Extra::ChatHistory));
        assert!(!reopened.enabled(Extra::CopyMenus));

        store.toggle(Extra::CopyMenus).unwrap();
        store.toggle(Extra::ChatHistory).unwrap();
        let reopened = Store::open(path.clone()).get();
        assert!(reopened.enabled(Extra::CopyMenus));
        assert!(!reopened.enabled(Extra::ChatHistory));

        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Store::open(path.clone()).get(), Extras::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn the_loader_gets_the_saved_choices_once_loaded_and_every_change_after() {
        LocalSet::new()
            .run_until(async {
                let loaded = Rc::new(RefCell::new(false));
                let applied = Rc::new(RefCell::new(Vec::new()));
                let loader = FakeLoader::new({
                    let loaded = loaded.clone();
                    let applied = applied.clone();
                    move |callback, args| {
                        if !*loaded.borrow() {
                            return Err(CallError::Unavailable(callback));
                        }
                        let [Value::Bool(enabled)] = args else {
                            panic!("the setter takes a flag: {args:?}");
                        };
                        applied.borrow_mut().push((callback, *enabled));
                        Ok(Value::Undefined)
                    }
                });
                let events = Events::default();
                let path = temporary_path();
                let store = Store::open(path.clone());
                tokio::task::spawn_local(run(loader, events.subscribe(wants), store.watch()));
                yield_now().await;

                store.toggle(Extra::ChatHistory).unwrap();
                yield_now().await;
                assert!(applied.borrow().is_empty());

                *loaded.borrow_mut() = true;
                events.publish(&Event::Loaded);
                yield_now().await;
                assert_eq!(
                    applied.borrow_mut().drain(..).collect::<Vec<_>>(),
                    [
                        ("extras.setCopyMenusEnabled", false),
                        ("extras.setChatHistoryEnabled", true),
                        ("extras.setBankSearchEnabled", false)
                    ]
                );

                store.toggle(Extra::CopyMenus).unwrap();
                yield_now().await;
                assert_eq!(
                    *applied.borrow(),
                    [
                        ("extras.setCopyMenusEnabled", true),
                        ("extras.setChatHistoryEnabled", true),
                        ("extras.setBankSearchEnabled", false)
                    ]
                );
                fs::remove_file(path).unwrap();
            })
            .await;
    }
}
