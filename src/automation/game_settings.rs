use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::{
    sync::watch,
    time::{Instant, sleep_until},
};

use crate::{
    bridge::{CallError, Loader, settings},
    domain::auth,
};

const TICK: Duration = Duration::from_millis(500);
const MINIMAL_FPS: u32 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RenderingMode {
    #[default]
    Full,
    InterfaceOnly,
    Minimal,
}

impl RenderingMode {
    pub(crate) const ALL: [RenderingMode; 3] = [
        RenderingMode::Full,
        RenderingMode::InterfaceOnly,
        RenderingMode::Minimal,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            RenderingMode::Full => "Full",
            RenderingMode::InterfaceOnly => "Interface only",
            RenderingMode::Minimal => "Minimal",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "RenderingMode", from = "RenderingMode")]
pub(crate) struct Rendering {
    mode: RenderingMode,
    resume: RenderingMode,
}

impl Rendering {
    pub(crate) fn mode(self) -> RenderingMode {
        self.mode
    }

    pub(crate) fn set(&mut self, mode: RenderingMode) {
        if mode != RenderingMode::Minimal {
            self.resume = mode;
        }
        self.mode = mode;
    }

    pub(crate) fn resume(&mut self) {
        self.mode = self.resume;
    }

    pub(crate) fn minimal(self) -> bool {
        self.mode == RenderingMode::Minimal
    }

    fn shows_world(self) -> bool {
        self.mode == RenderingMode::Full
    }
}

impl From<RenderingMode> for Rendering {
    fn from(mode: RenderingMode) -> Self {
        let mut rendering = Rendering::default();
        rendering.set(mode);
        rendering
    }
}

impl From<Rendering> for RenderingMode {
    fn from(rendering: Rendering) -> Self {
        rendering.mode()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) infinite_range: bool,
    pub(crate) provoke_cell: bool,
    pub(crate) enemy_magnet: bool,
    pub(crate) skip_cutscenes: bool,
    pub(crate) hide_players: bool,
    pub(crate) rendering: Rendering,
    pub(crate) animations: bool,
    pub(crate) collisions: bool,
    pub(crate) ads: bool,
    pub(crate) walk_speed: u32,
    pub(crate) custom_name: Option<String>,
    pub(crate) custom_guild: Option<String>,
    pub(crate) anti_counter: bool,
    pub(crate) anti_afk: bool,
    pub(crate) auto_zone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Writer {
    Game,
    Mira,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameRate {
    pub(crate) fps: u32,
    pub(crate) by: Writer,
}

impl FrameRate {
    pub(crate) const RANGE: (u32, u32) = (1, 60);
    pub(crate) const DEFAULT: u32 = 24;
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            infinite_range: false,
            provoke_cell: false,
            enemy_magnet: false,
            skip_cutscenes: false,
            hide_players: false,
            rendering: Rendering::default(),
            animations: true,
            collisions: true,
            ads: true,
            walk_speed: 8,
            custom_name: None,
            custom_guild: None,
            anti_counter: true,
            anti_afk: false,
            auto_zone: false,
        }
    }
}

impl Settings {
    pub(crate) const WALK_SPEEDS: (u32, u32) = (1, 99);

    pub(crate) fn normalized(self) -> Self {
        let text = |value: Option<String>| {
            value
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        Self {
            walk_speed: self
                .walk_speed
                .clamp(Self::WALK_SPEEDS.0, Self::WALK_SPEEDS.1),
            custom_name: text(self.custom_name),
            custom_guild: text(self.custom_guild),
            ..self
        }
    }

    fn acts(&self) -> bool {
        self.infinite_range || self.provoke_cell || self.enemy_magnet || self.skip_cutscenes
    }

    fn overrides_the_game(&self) -> bool {
        !self.rendering.shows_world() || self.walk_speed != Self::default().walk_speed
    }
}

pub(crate) async fn run(
    loader: impl Loader,
    mut settings: watch::Receiver<Settings>,
    mut frame_rate: watch::Receiver<Option<FrameRate>>,
) {
    let mut desired = settings.borrow_and_update().clone();
    let mut applied = Settings::default();
    let mut stage = Stage::default();
    stage.observe(*frame_rate.borrow_and_update());
    stage.set_minimal(desired.rendering.minimal(), *frame_rate.borrow());
    let mut next_tick = None;
    loop {
        let ticking = desired != applied
            || desired.acts()
            || desired.overrides_the_game()
            || stage.send.is_some();
        next_tick = ticking.then(|| next_tick.unwrap_or_else(|| Instant::now() + TICK));
        tokio::select! {
            changed = settings.changed() => {
                if changed.is_err() {
                    return;
                }
                desired = settings.borrow_and_update().clone();
                step(&loader, &desired, &mut applied, false);
                stage.set_minimal(desired.rendering.minimal(), *frame_rate.borrow());
                apply_frame_rate(&loader, &mut stage.send);
            }
            changed = frame_rate.changed() => {
                if changed.is_err() {
                    return;
                }
                stage.observe(*frame_rate.borrow_and_update());
                apply_frame_rate(&loader, &mut stage.send);
            }
            () = sleep_until(next_tick.unwrap_or_else(Instant::now)), if ticking => {
                next_tick = None;
                step(&loader, &desired, &mut applied, true);
                apply_frame_rate(&loader, &mut stage.send);
            }
        }
    }
}

#[derive(Default)]
struct Stage {
    send: Option<u32>,
    minimal: bool,
    resume: Option<u32>,
}

impl Stage {
    fn observe(&mut self, rate: Option<FrameRate>) {
        let Some(FrameRate { fps, by }) = rate else {
            return;
        };
        match (self.minimal, by) {
            (false, Writer::Mira) => self.send = Some(fps),
            (false, Writer::Game) => {}
            (true, Writer::Mira) => self.resume = Some(fps),
            (true, Writer::Game) if fps != MINIMAL_FPS => {
                self.resume = Some(fps);
                self.send = Some(MINIMAL_FPS);
            }
            (true, Writer::Game) => {}
        }
    }

    fn set_minimal(&mut self, minimal: bool, current: Option<FrameRate>) {
        if minimal == self.minimal {
            return;
        }
        self.minimal = minimal;
        if minimal {
            self.resume = current.map(|rate| rate.fps);
            self.send = self.resume.map(|_| MINIMAL_FPS);
        } else {
            self.send = self.resume.take();
        }
    }
}

fn apply_frame_rate(loader: &impl Loader, requested: &mut Option<u32>) {
    let Some(fps) = *requested else {
        return;
    };
    match settings::set_frame_rate(loader, whole(fps)) {
        Ok(()) => *requested = None,
        Err(error) => tracing::debug!(%error, "couldn't apply the frame rate"),
    }
}

fn step(loader: &impl Loader, desired: &Settings, applied: &mut Settings, tick: bool) {
    applied.anti_counter = desired.anti_counter;
    applied.anti_afk = desired.anti_afk;
    applied.auto_zone = desired.auto_zone;
    sync(
        &desired.custom_name,
        &mut applied.custom_name,
        false,
        |name| match name {
            Some(name) => settings::set_custom_name(loader, name),
            None => settings::reset_custom_name(loader),
        },
    );
    sync(
        &desired.custom_guild,
        &mut applied.custom_guild,
        false,
        |guild| match guild {
            Some(guild) => settings::set_custom_guild(loader, guild),
            None => settings::reset_custom_guild(loader),
        },
    );
    match auth::in_game(loader) {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::debug!(%error, "game settings skipped a tick");
            return;
        }
    }
    let default = Settings::default();
    sync(
        &desired.hide_players,
        &mut applied.hide_players,
        false,
        |hide| settings::set_other_players_visible(loader, !hide),
    );
    sync(&desired.animations, &mut applied.animations, false, |on| {
        settings::set_animations_enabled(loader, *on)
    });
    sync(&desired.collisions, &mut applied.collisions, false, |on| {
        settings::set_collisions_enabled(loader, *on)
    });
    sync(&desired.ads, &mut applied.ads, false, |visible| {
        settings::set_ads_visible(loader, *visible)
    });
    sync(
        &desired.rendering,
        &mut applied.rendering,
        tick && !desired.rendering.shows_world(),
        |rendering| settings::set_world_visible(loader, rendering.shows_world()),
    );
    sync(
        &desired.walk_speed,
        &mut applied.walk_speed,
        tick && desired.walk_speed != default.walk_speed,
        |speed| settings::set_walk_speed(loader, whole(*speed)),
    );
    if !tick {
        return;
    }
    let actions: [(bool, fn(&_) -> Result<(), CallError>); 4] = [
        (desired.infinite_range, settings::infinite_range),
        (desired.provoke_cell, settings::provoke_cell),
        (desired.enemy_magnet, settings::enemy_magnet),
        (desired.skip_cutscenes, settings::skip_cutscenes),
    ];
    for (_, action) in actions.into_iter().filter(|(on, _)| *on) {
        if let Err(error) = action(loader) {
            tracing::debug!(%error, "a game settings action failed");
        }
    }
}

fn sync<T: Clone + PartialEq>(
    desired: &T,
    applied: &mut T,
    again: bool,
    send: impl FnOnce(&T) -> Result<(), CallError>,
) {
    if !again && desired == applied {
        return;
    }
    match send(desired) {
        Ok(()) => *applied = desired.clone(),
        Err(error) => tracing::debug!(%error, "couldn't apply a game setting"),
    }
}

fn whole(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use tokio::{task::LocalSet, time::sleep};

    use super::*;
    use crate::{bridge::Value, domain::testing::FakeLoader};

    type Sent = Rc<RefCell<Vec<(&'static str, Vec<Value>)>>>;

    fn start(
        in_game: Rc<RefCell<bool>>,
        settings: Settings,
    ) -> (
        Sent,
        Rc<FakeLoader>,
        watch::Sender<Settings>,
        watch::Sender<Option<FrameRate>>,
    ) {
        let sent = Sent::default();
        let log = sent.clone();
        let loader = Rc::new(FakeLoader::new(move |callback, args| {
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" => Value::Bool(*in_game.borrow()),
                _ => {
                    log.borrow_mut().push((callback, args.to_vec()));
                    Value::Null
                }
            })
        }));
        let (sender, receiver) = watch::channel(settings);
        let (frame_rate, frame_rates) = watch::channel(None);
        tokio::task::spawn_local(run(loader.clone(), receiver, frame_rates));
        (sent, loader, sender, frame_rate)
    }

    async fn ticks(count: u32) {
        sleep(TICK * count + TICK / 2).await;
    }

    fn drain(sent: &Sent) -> Vec<(&'static str, Vec<Value>)> {
        sent.borrow_mut().drain(..).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn the_defaults_leave_the_game_alone() {
        LocalSet::new()
            .run_until(async {
                let (sent, loader, _settings, _frame_rate) =
                    start(Rc::new(RefCell::new(true)), Settings::default());
                ticks(4).await;
                assert!(drain(&sent).is_empty());
                assert_eq!(loader.count("auth.isLoggedIn"), 0);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_loader_setting_is_sent_once_per_change() {
        LocalSet::new()
            .run_until(async {
                let (sent, _loader, settings, _frame_rate) =
                    start(Rc::new(RefCell::new(true)), Settings::default());
                tokio::task::yield_now().await;
                settings.send_modify(|settings| settings.hide_players = true);
                ticks(4).await;
                assert_eq!(
                    drain(&sent),
                    [("settings.setOtherPlayersVisible", vec![Value::Bool(false)])]
                );
                settings.send_modify(|settings| settings.custom_name = Some("player1".into()));
                settings.send_modify(|settings| settings.hide_players = false);
                settings.send_modify(|settings| settings.ads = false);
                ticks(2).await;
                assert_eq!(
                    drain(&sent),
                    [
                        (
                            "settings.setCustomName",
                            vec![Value::String("player1".into())]
                        ),
                        ("settings.setOtherPlayersVisible", vec![Value::Bool(true)]),
                        ("settings.setAdsVisible", vec![Value::Bool(false)]),
                    ]
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_value_the_game_owns_is_reapplied_until_set_back_to_its_default() {
        LocalSet::new()
            .run_until(async {
                let (sent, _loader, settings, _frame_rate) =
                    start(Rc::new(RefCell::new(true)), Settings::default());
                tokio::task::yield_now().await;
                settings.send_modify(|settings| settings.walk_speed = 20);
                ticks(3).await;
                let walk = ("settings.setWalkSpeed", vec![Value::Number(20.0)]);
                assert_eq!(
                    drain(&sent),
                    [walk.clone(), walk.clone(), walk.clone(), walk]
                );
                settings.send_modify(|settings| settings.walk_speed = 8);
                ticks(3).await;
                assert_eq!(
                    drain(&sent),
                    [("settings.setWalkSpeed", vec![Value::Number(8.0)])]
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn out_of_game_it_waits_then_applies_and_repeats_actions() {
        LocalSet::new()
            .run_until(async {
                let in_game = Rc::new(RefCell::new(false));
                let (sent, _loader, _settings, _frame_rate) = start(
                    in_game.clone(),
                    Settings {
                        provoke_cell: true,
                        collisions: false,
                        custom_name: Some("player1".into()),
                        ..Settings::default()
                    },
                );
                ticks(3).await;
                assert_eq!(
                    drain(&sent),
                    [(
                        "settings.setCustomName",
                        vec![Value::String("player1".into())]
                    )]
                );
                *in_game.borrow_mut() = true;
                ticks(2).await;
                let provoke = ("settings.provokeCell", vec![]);
                assert_eq!(
                    drain(&sent),
                    [
                        ("settings.setCollisionsEnabled", vec![Value::Bool(false)]),
                        provoke.clone(),
                        provoke,
                    ]
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_frame_rate_from_mira_is_applied_once_and_the_game_can_take_over() {
        LocalSet::new()
            .run_until(async {
                let (sent, _loader, _settings, frame_rate) =
                    start(Rc::new(RefCell::new(true)), Settings::default());
                tokio::task::yield_now().await;
                let mira = |fps| {
                    Some(FrameRate {
                        fps,
                        by: Writer::Mira,
                    })
                };
                let set = |fps: f64| ("settings.setFrameRate", vec![Value::Number(fps)]);
                frame_rate.send_replace(mira(15));
                ticks(3).await;
                assert_eq!(drain(&sent), [set(15.0)]);
                frame_rate.send_replace(Some(FrameRate {
                    fps: 60,
                    by: Writer::Game,
                }));
                ticks(2).await;
                assert!(drain(&sent).is_empty());
                frame_rate.send_replace(mira(15));
                ticks(1).await;
                assert_eq!(drain(&sent), [set(15.0)]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn minimal_rendering_holds_two_fps_then_restores_the_rate_it_replaced() {
        LocalSet::new()
            .run_until(async {
                let (sent, _loader, settings, frame_rate) =
                    start(Rc::new(RefCell::new(true)), Settings::default());
                let rate = |fps, by| Some(FrameRate { fps, by });
                let set = |fps: f64| ("settings.setFrameRate", vec![Value::Number(fps)]);
                let world = |visible| ("settings.setWorldVisible", vec![Value::Bool(visible)]);
                frame_rate.send_replace(rate(24, Writer::Game));
                tokio::task::yield_now().await;
                settings.send_modify(|settings| settings.rendering.set(RenderingMode::Minimal));
                ticks(2).await;
                assert_eq!(
                    drain(&sent),
                    [world(false), set(2.0), world(false), world(false)]
                );
                frame_rate.send_replace(rate(2, Writer::Game));
                frame_rate.send_replace(rate(30, Writer::Game));
                tokio::task::yield_now().await;
                frame_rate.send_replace(rate(48, Writer::Mira));
                tokio::task::yield_now().await;
                assert_eq!(drain(&sent), [set(2.0)]);
                settings.send_modify(|settings| settings.rendering.resume());
                ticks(2).await;
                assert_eq!(drain(&sent), [world(true), set(48.0)]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn frame_rates_set_during_minimal_rendering_are_ignored() {
        LocalSet::new()
            .run_until(async {
                let sent = Sent::default();
                let log = sent.clone();
                let loader = Rc::new(FakeLoader::new(move |callback, args| {
                    if callback == "settings.setFrameRate" {
                        log.borrow_mut().push((callback, args.to_vec()));
                    }
                    Ok(Value::Bool(true))
                }));
                let automation = crate::automation::settings::Automation::default();
                tokio::task::spawn_local(run(
                    loader,
                    automation.watch_game_settings(),
                    automation.watch_frame_rate(),
                ));
                automation.observe_frame_rate(48.0);
                tokio::task::yield_now().await;
                automation.update_game_settings(|settings| {
                    settings.rendering.set(RenderingMode::Minimal)
                });
                ticks(1).await;
                automation.observe_frame_rate(2.0);
                assert_eq!(automation.set_frame_rate(30), Some(2));
                assert_eq!(automation.frame_rate(), Some(2));
                tokio::task::yield_now().await;
                automation.update_game_settings(|settings| settings.rendering.resume());
                ticks(1).await;
                let set = |fps: f64| ("settings.setFrameRate", vec![Value::Number(fps)]);
                assert_eq!(drain(&sent), [set(2.0), set(48.0)]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn an_accounts_saved_frame_rate_returns_when_minimal_rendering_resumes() {
        LocalSet::new()
            .run_until(async {
                let sent = Sent::default();
                let log = sent.clone();
                let loader = Rc::new(FakeLoader::new(move |callback, args| {
                    if callback == "settings.setFrameRate" {
                        log.borrow_mut().push((callback, args.to_vec()));
                    }
                    Ok(Value::Bool(true))
                }));
                let automation = crate::automation::settings::Automation::default();
                tokio::task::spawn_local(run(
                    loader,
                    automation.watch_game_settings(),
                    automation.watch_frame_rate(),
                ));
                let mut options = crate::account_settings::Options::default();
                options.game.rendering.set(RenderingMode::Minimal);
                options.frame_rate = Some(15);
                options.apply_to(&automation);
                ticks(1).await;
                automation.update_game_settings(|settings| settings.rendering.resume());
                ticks(1).await;
                let sent = drain(&sent);
                assert_eq!(
                    sent.last(),
                    Some(&("settings.setFrameRate", vec![Value::Number(15.0)]))
                );
            })
            .await;
    }

    #[test]
    fn resuming_from_minimal_returns_to_the_mode_it_paused() {
        for mode in [RenderingMode::Full, RenderingMode::InterfaceOnly] {
            let mut rendering = Rendering::default();
            rendering.set(mode);
            rendering.set(RenderingMode::Minimal);
            assert_eq!(serde_json::to_value(rendering).unwrap(), "minimal");
            rendering.resume();
            assert_eq!(rendering.mode(), mode);
        }
        let mut rendering = Rendering::default();
        rendering.set(RenderingMode::InterfaceOnly);
        rendering.set(RenderingMode::Minimal);
        rendering.set(RenderingMode::Full);
        rendering.resume();
        assert_eq!(rendering.mode(), RenderingMode::Full);
    }

    #[tokio::test(start_paused = true)]
    async fn changes_arriving_faster_than_a_tick_do_not_postpone_actions() {
        LocalSet::new()
            .run_until(async {
                let (sent, _loader, settings, _frame_rate) = start(
                    Rc::new(RefCell::new(true)),
                    Settings {
                        provoke_cell: true,
                        ..Settings::default()
                    },
                );
                for speed in 0..20 {
                    sleep(TICK * 2 / 5).await;
                    settings.send_modify(|settings| settings.walk_speed = 10 + speed);
                }
                let provoked = drain(&sent)
                    .into_iter()
                    .filter(|(callback, _)| *callback == "settings.provokeCell")
                    .count();
                assert!(provoked >= 7, "provoked {provoked} times in 8 ticks");
            })
            .await;
    }
}
