use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::bridge::{self, Event, Loader, Subscription};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Extra {
    CopyMenus,
    ChatRecall,
    BankSearch,
    Percentages,
    HideMusicButton,
    DamageText,
    AuraText,
    Timers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    Interface,
    Combat,
}

impl Group {
    pub(crate) const ALL: [Group; 2] = [Group::Interface, Group::Combat];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Group::Interface => "Interface",
            Group::Combat => "Combat",
        }
    }
}

impl Extra {
    pub(crate) const ALL: [Extra; 8] = [
        Extra::CopyMenus,
        Extra::ChatRecall,
        Extra::BankSearch,
        Extra::Percentages,
        Extra::HideMusicButton,
        Extra::DamageText,
        Extra::AuraText,
        Extra::Timers,
    ];

    pub(crate) fn group(self) -> Group {
        match self {
            Extra::CopyMenus
            | Extra::ChatRecall
            | Extra::BankSearch
            | Extra::Percentages
            | Extra::HideMusicButton => Group::Interface,
            Extra::DamageText | Extra::AuraText | Extra::Timers => Group::Combat,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Extra::CopyMenus => "Right-click menus",
            Extra::ChatRecall => "Chat recall",
            Extra::BankSearch => "Bank search",
            Extra::Percentages => "HP and MP percentages",
            Extra::HideMusicButton => "Hide music button",
            Extra::DamageText => "Damage text",
            Extra::AuraText => "Aura text",
            Extra::Timers => "Timers",
        }
    }

    pub(crate) fn tooltip(self) -> &'static str {
        match self {
            Extra::CopyMenus => {
                "Adds contextual menus to monsters, items, shops, quests, and drops for more actions."
            }
            Extra::ChatRecall => {
                "Press Up and Down in the chat box to reuse what you've entered. Clears on disconnect."
            }
            Extra::BankSearch => {
                "Search the bank by enhancement or boost, like forge or dmg undead."
            }
            Extra::Percentages => "Shows percentages on your and your target's HP and MP bars.",
            Extra::HideMusicButton => "Hides the Music On/Off button at the top of some maps.",
            Extra::DamageText => "Redraws damage numbers as hits land, stacked above each target.",
            Extra::AuraText => {
                "Redraws aura popups beside you and your target, without the repeats from refreshes."
            }
            Extra::Timers => {
                "Redraws skill cooldowns and the aura HUD with a plain wipe instead of the clock sweep."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Extras {
    copy_menus: bool,
    chat_recall: bool,
    bank_search: bool,
    percentages: bool,
    hide_music_button: bool,
    damage_text: bool,
    aura_text: bool,
    timers: bool,
}

impl Extras {
    pub(crate) fn enabled(self, extra: Extra) -> bool {
        match extra {
            Extra::CopyMenus => self.copy_menus,
            Extra::ChatRecall => self.chat_recall,
            Extra::BankSearch => self.bank_search,
            Extra::Percentages => self.percentages,
            Extra::HideMusicButton => self.hide_music_button,
            Extra::DamageText => self.damage_text,
            Extra::AuraText => self.aura_text,
            Extra::Timers => self.timers,
        }
    }

    pub(crate) fn toggled(mut self, extra: Extra) -> Self {
        let enabled = match extra {
            Extra::CopyMenus => &mut self.copy_menus,
            Extra::ChatRecall => &mut self.chat_recall,
            Extra::BankSearch => &mut self.bank_search,
            Extra::Percentages => &mut self.percentages,
            Extra::HideMusicButton => &mut self.hide_music_button,
            Extra::DamageText => &mut self.damage_text,
            Extra::AuraText => &mut self.aura_text,
            Extra::Timers => &mut self.timers,
        };
        *enabled = !*enabled;
        self
    }
}

pub(crate) struct Store {
    extras: watch::Sender<Extras>,
}

impl Store {
    pub(crate) fn new(extras: Extras) -> Self {
        Self {
            extras: watch::Sender::new(extras),
        }
    }

    pub(crate) fn get(&self) -> Extras {
        *self.extras.borrow()
    }

    pub(crate) fn watch(&self) -> watch::Receiver<Extras> {
        self.extras.subscribe()
    }

    pub(crate) fn toggle(&self, extra: Extra) -> Extras {
        self.extras
            .send_modify(|extras| *extras = extras.toggled(extra));
        self.get()
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
            .and_then(|()| bridge::extras::set_chat_recall_enabled(&loader, extras.chat_recall))
            .and_then(|()| bridge::extras::set_bank_search_enabled(&loader, extras.bank_search))
            .and_then(|()| bridge::extras::set_percentages_enabled(&loader, extras.percentages))
            .and_then(|()| {
                bridge::extras::set_hide_music_button_enabled(&loader, extras.hide_music_button)
            })
            .and_then(|()| bridge::extras::set_damage_text_enabled(&loader, extras.damage_text))
            .and_then(|()| bridge::extras::set_aura_text_enabled(&loader, extras.aura_text))
            .and_then(|()| bridge::extras::set_timers_enabled(&loader, extras.timers));
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

    #[test]
    fn extras_start_off_and_a_toggle_reaches_every_watcher() {
        let store = Store::new(Extras::default());
        let mut running = store.watch();
        for extra in Extra::ALL {
            assert!(!store.get().enabled(extra));
        }
        let toggled = store.toggle(Extra::ChatRecall);
        assert!(toggled.enabled(Extra::ChatRecall));
        assert!(!toggled.enabled(Extra::CopyMenus));
        assert_eq!(*running.borrow_and_update(), toggled);
        assert!(!store.toggle(Extra::ChatRecall).enabled(Extra::ChatRecall));
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
                let store = Store::new(Extras::default());
                tokio::task::spawn_local(run(loader, events.subscribe(wants), store.watch()));
                yield_now().await;

                store.toggle(Extra::ChatRecall);
                yield_now().await;
                assert!(applied.borrow().is_empty());

                *loaded.borrow_mut() = true;
                events.publish(&Event::Loaded);
                yield_now().await;
                assert_eq!(
                    applied.borrow_mut().drain(..).collect::<Vec<_>>(),
                    [
                        ("extras.setCopyMenusEnabled", false),
                        ("extras.setChatRecallEnabled", true),
                        ("extras.setBankSearchEnabled", false),
                        ("extras.setPercentagesEnabled", false),
                        ("extras.setHideMusicButtonEnabled", false),
                        ("extras.setDamageTextEnabled", false),
                        ("extras.setAuraTextEnabled", false),
                        ("extras.setTimersEnabled", false)
                    ]
                );

                store.toggle(Extra::CopyMenus);
                yield_now().await;
                assert_eq!(
                    *applied.borrow(),
                    [
                        ("extras.setCopyMenusEnabled", true),
                        ("extras.setChatRecallEnabled", true),
                        ("extras.setBankSearchEnabled", false),
                        ("extras.setPercentagesEnabled", false),
                        ("extras.setHideMusicButtonEnabled", false),
                        ("extras.setDamageTextEnabled", false),
                        ("extras.setAuraTextEnabled", false),
                        ("extras.setTimersEnabled", false)
                    ]
                );
            })
            .await;
    }
}
