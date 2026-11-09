use std::time::Duration;

use serde::Serialize;
use tokio::{
    sync::watch,
    time::{Instant, sleep_until},
};

use crate::{
    bridge::{CallError, Event, Loader, Subscription, settings},
    domain::auth,
};

const TICK: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    pub(crate) infinite_range: bool,
    pub(crate) provoke_cell: bool,
    pub(crate) enemy_magnet: bool,
    pub(crate) skip_cutscenes: bool,
    pub(crate) hide_players: bool,
    pub(crate) lag_killer: bool,
    pub(crate) animations: bool,
    pub(crate) collisions: bool,
    pub(crate) ads: bool,
    pub(crate) walk_speed: u32,
    pub(crate) custom_name: Option<String>,
    pub(crate) custom_guild: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            infinite_range: false,
            provoke_cell: false,
            enemy_magnet: false,
            skip_cutscenes: false,
            hide_players: false,
            lag_killer: false,
            animations: true,
            collisions: true,
            ads: true,
            walk_speed: 8,
            custom_name: None,
            custom_guild: None,
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
        let default = Self::default();
        self.lag_killer != default.lag_killer || self.walk_speed != default.walk_speed
    }
}

pub(crate) async fn run(loader: impl Loader, mut settings: watch::Receiver<Settings>) {
    let mut desired = settings.borrow_and_update().clone();
    let mut applied = Settings::default();
    let mut next_tick = None;
    loop {
        let ticking = desired != applied || desired.acts() || desired.overrides_the_game();
        next_tick = ticking.then(|| next_tick.unwrap_or_else(|| Instant::now() + TICK));
        tokio::select! {
            changed = settings.changed() => {
                if changed.is_err() {
                    return;
                }
                desired = settings.borrow_and_update().clone();
                step(&loader, &desired, &mut applied, false);
            }
            () = sleep_until(next_tick.unwrap_or_else(Instant::now)), if ticking => {
                next_tick = None;
                step(&loader, &desired, &mut applied, true);
            }
        }
    }
}

pub(crate) fn wants(event: &Event) -> bool {
    matches!(event, Event::AdHidden { .. })
}

pub(crate) async fn log_hidden_ads(events: Subscription) {
    loop {
        if let Ok(Event::AdHidden { ad, by }) = events.next().await {
            tracing::info!(ad, by, "ad hidden");
        }
    }
}

fn step(loader: &impl Loader, desired: &Settings, applied: &mut Settings, tick: bool) {
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
        &desired.lag_killer,
        &mut applied.lag_killer,
        tick && desired.lag_killer != default.lag_killer,
        |on| settings::set_lag_killer_enabled(loader, *on),
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
    ) -> (Sent, Rc<FakeLoader>, watch::Sender<Settings>) {
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
        tokio::task::spawn_local(run(loader.clone(), receiver));
        (sent, loader, sender)
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
                let (sent, loader, _settings) =
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
                let (sent, _loader, settings) =
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
                let (sent, _loader, settings) =
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
                let (sent, _loader, _settings) = start(
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
    async fn changes_arriving_faster_than_a_tick_do_not_postpone_actions() {
        LocalSet::new()
            .run_until(async {
                let (sent, _loader, settings) = start(
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
