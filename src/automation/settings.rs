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
