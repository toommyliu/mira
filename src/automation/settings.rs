use tokio::sync::watch;

use super::{auto_attack, auto_relogin, auto_zone, boosts, drops, game_settings, quests};

#[derive(Clone, Default)]
pub(crate) struct Automation {
    relogin: watch::Sender<auto_relogin::Settings>,
    zone: watch::Sender<auto_zone::Settings>,
    attack: watch::Sender<auto_attack::Settings>,
    game: watch::Sender<game_settings::Settings>,
    frame_rate: watch::Sender<Option<game_settings::FrameRate>>,
    quests: watch::Sender<quests::State>,
    drops: watch::Sender<drops::State>,
    boosts: watch::Sender<boosts::State>,
}

impl Automation {
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
        self.attack.borrow().clone()
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

    pub(crate) fn game_settings(&self) -> game_settings::Settings {
        self.game.borrow().clone()
    }

    pub(crate) fn watch_game_settings(&self) -> watch::Receiver<game_settings::Settings> {
        self.game.subscribe()
    }

    pub(crate) fn update_game_settings(
        &self,
        change: impl FnOnce(&mut game_settings::Settings),
    ) -> game_settings::Settings {
        update(&self.game, |settings| {
            change(settings);
            *settings = std::mem::take(settings).normalized();
        })
    }

    pub(crate) fn quests(&self) -> quests::State {
        self.quests.borrow().clone()
    }

    pub(crate) fn watch_quests(&self) -> watch::Receiver<quests::State> {
        self.quests.subscribe()
    }

    pub(crate) fn update_quests(&self, change: impl FnOnce(&mut quests::State)) -> quests::State {
        update(&self.quests, change)
    }

    pub(crate) fn drops(&self) -> drops::State {
        self.drops.borrow().clone()
    }

    pub(crate) fn watch_drops(&self) -> watch::Receiver<drops::State> {
        self.drops.subscribe()
    }

    pub(crate) fn update_drops(&self, change: impl FnOnce(&mut drops::State)) -> drops::State {
        update(&self.drops, change)
    }

    pub(crate) fn boosts(&self) -> boosts::State {
        self.boosts.borrow().clone()
    }

    pub(crate) fn watch_boosts(&self) -> watch::Receiver<boosts::State> {
        self.boosts.subscribe()
    }

    pub(crate) fn update_boosts(&self, change: impl FnOnce(&mut boosts::State)) -> boosts::State {
        update(&self.boosts, change)
    }

    pub(crate) fn copy_environment_from(&self, source: &Automation) {
        let quests = source.quests();
        self.update_quests(|state| {
            state.quests = quests.quests;
            state.config = quests::Config {
                enabled: state.config.enabled,
                ..quests.config
            };
        });
        let drops = source.drops();
        self.update_drops(|state| {
            state.items = drops.items;
            state.sounds = drops.sounds;
            state.config = drops::Config {
                enabled: state.config.enabled,
                ..drops.config
            };
        });
        let boosts = source.boosts();
        self.update_boosts(|state| state.items = boosts.items);
    }

    pub(crate) fn frame_rate(&self) -> Option<u32> {
        self.frame_rate.borrow().map(|rate| rate.fps)
    }

    pub(crate) fn watch_frame_rate(&self) -> watch::Receiver<Option<game_settings::FrameRate>> {
        self.frame_rate.subscribe()
    }

    pub(crate) fn set_frame_rate(&self, fps: u32) -> Option<u32> {
        if self.game.borrow().rendering.minimal() {
            return self.frame_rate();
        }
        let (min, max) = game_settings::FrameRate::RANGE;
        let fps = fps.clamp(min, max);
        self.frame_rate.send_replace(Some(game_settings::FrameRate {
            fps,
            by: game_settings::Writer::Mira,
        }));
        Some(fps)
    }

    pub(crate) fn observe_frame_rate(&self, fps: f64) {
        let fps = fps.round() as u32;
        self.frame_rate.send_if_modified(|current| {
            if current.is_some_and(|rate| rate.fps == fps) {
                return false;
            }
            *current = Some(game_settings::FrameRate {
                fps,
                by: game_settings::Writer::Game,
            });
            true
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{automation::quests::Registered, domain::inventory::ItemQuery};

    #[test]
    fn copying_an_environment_replaces_lists_and_rules_but_keeps_each_switch() {
        let source = Automation::default();
        source.update_quests(|state| {
            state.register([Registered {
                quest_id: 7,
                reward_item_id: Some(50),
            }]);
            state.config.accept_rewards = true;
        });
        source.update_drops(|state| {
            state.items = vec![ItemQuery::Name("Gem".into())];
            state.set_sound(&ItemQuery::Name("Gem".into()), true);
            state.config.reject_others = true;
        });
        source.update_boosts(|state| {
            state.items = vec![ItemQuery::Name("XP Boost".into())];
            state.config.enabled = false;
        });

        let target = Automation::default();
        target.update_drops(|state| {
            state.items = vec![ItemQuery::Name("Cape".into())];
            state.config.enabled = false;
        });
        target.copy_environment_from(&source);

        assert_eq!(target.quests(), source.quests());
        let drops = target.drops();
        assert_eq!(
            (drops.items, drops.sounds),
            (source.drops().items, source.drops().sounds)
        );
        assert!(drops.config.reject_others);
        assert!(!drops.config.enabled, "the target's pause stays");
        assert_eq!(target.boosts().items, source.boosts().items);
        assert!(target.boosts().config.enabled);
    }
}
