pub(crate) mod auras;
pub(crate) mod auth;
pub(crate) mod bank;
pub(crate) mod combat;
pub(crate) mod counter_attack;
pub(crate) mod drops;
pub(crate) mod enhancements;
pub(crate) mod events;
pub(crate) mod house;
pub(crate) mod inventory;
pub(crate) mod packets;
pub(crate) mod player;
pub(crate) mod players;
pub(crate) mod quests;
pub(crate) mod shops;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod world;

use std::{cell::Cell, rc::Rc, time::Duration};

use serde::{Deserialize, Serialize, de::IgnoredAny};
use tokio::time::{Instant, sleep, sleep_until};

use crate::bridge::{
    self, CallError, Event, Format, GameAction, Loader, Packet, Packets, Subscription,
};
use events::{GameEvent, Kind, Myself};

const POLL: Duration = Duration::from_millis(100);
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const ACTION_TIMEOUT: Duration = Duration::from_millis(3_500);
const AFTER_COMBAT_RETRY: Duration = Duration::from_secs(1);
const AFTER_COMBAT_LIMIT: Duration = Duration::from_secs(10);
const AFTER_COMBAT: Duration = Duration::from_millis(1_500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum State {
    Dead,
    Idle,
    Combat,
}

impl State {
    pub(crate) fn from_game(state: i32) -> Option<Self> {
        match state {
            0 => Some(State::Dead),
            1 => Some(State::Idle),
            2 => Some(State::Combat),
            _ => None,
        }
    }
}

pub(crate) fn has_avatar(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(bridge::auth::is_logged_in(loader)?
        && !bridge::flash::is_null(loader, "world.myAvatar.dataLeaf")?
        && !bridge::flash::is_null(loader, "world.myAvatar.objData")?)
}

pub(crate) fn ready(loader: &impl Loader) -> Result<bool, CallError> {
    Ok(auth::in_game(loader)? && bridge::player::is_loaded(loader)?)
}

pub(crate) async fn wait_until_ready(loader: &impl Loader) -> Result<bool, CallError> {
    if !bridge::auth::is_logged_in(loader)? {
        return Ok(false);
    }
    Ok(
        poll(POLL, READY_TIMEOUT, || Ok(ready(loader)?.then_some(())))
            .await?
            .is_some(),
    )
}

pub(crate) async fn wait_for_action(
    loader: &impl Loader,
    action: GameAction,
) -> Result<bool, CallError> {
    Ok(poll(POLL, ACTION_TIMEOUT, || {
        Ok(bridge::world::is_action_available(loader, action)?.then_some(()))
    })
    .await?
    .is_some())
}

#[derive(Deserialize)]
struct WarningLine {
    _cmd: IgnoredAny,
    _room: IgnoredAny,
    message: String,
}

impl Packet for WarningLine {
    const FORMAT: Format = Format::Str;
    const CMD: &'static str = "warning";
}

#[derive(Clone, Default)]
pub(crate) struct CombatCooldown(Rc<Cell<Fight>>);

#[derive(Clone, Copy, Default)]
enum Fight {
    #[default]
    Calm,
    Fighting,
    Left(Instant),
}

impl CombatCooldown {
    pub(crate) fn wants(event: &Event) -> bool {
        Kind::PlayerState.wants(event)
    }

    pub(crate) async fn track(self, loader: impl Loader, events: Subscription) {
        loop {
            let Ok(event) = events.next().await else {
                continue;
            };
            let mut states = Vec::new();
            Kind::PlayerState.read(&event, &Myself(&loader), &mut states);
            for event in states {
                if let GameEvent::PlayerState {
                    is_self: true,
                    state,
                    ..
                } = event
                {
                    self.observe(state);
                }
            }
        }
    }

    fn observe(&self, state: State) {
        let fight = match (self.0.get(), state) {
            (_, State::Combat) => Fight::Fighting,
            (Fight::Fighting, _) => Fight::Left(Instant::now()),
            (fight, _) => fight,
        };
        self.0.set(fight);
    }

    async fn settle(&self) {
        if let Fight::Left(at) = self.0.get() {
            sleep_until(at + AFTER_COMBAT).await;
        }
    }
}

// The server doesn't announce how long this cooldown lasts, and its warning doesn't say which
// request it refused, so `attempt` must be safe to repeat.
pub(crate) async fn retry_after_combat(
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    cooldown: &CombatCooldown,
    mut attempt: impl AsyncFnMut() -> Result<bool, CallError>,
) -> Result<bool, CallError> {
    cooldown.settle().await;
    let deadline = Instant::now() + AFTER_COMBAT_LIMIT;
    loop {
        let warnings = Packets::<WarningLine>::subscribe(subscribe);
        let refused = warnings.answer(AFTER_COMBAT_LIMIT, |warning| {
            (warning.message == "You cannot do that too soon after combat.").then_some(())
        });
        tokio::select! {
            result = attempt() => return result,
            Some(()) = refused => {}
        }
        if Instant::now() + AFTER_COMBAT_RETRY > deadline {
            return Ok(false);
        }
        sleep(AFTER_COMBAT_RETRY).await;
    }
}

pub(crate) async fn arrive(
    loader: &impl Loader,
    within: Duration,
    mut arrived: impl FnMut() -> Result<Option<bool>, CallError>,
) -> Result<bool, CallError> {
    let ended = poll(POLL, within, || {
        if !bridge::auth::is_logged_in(loader)? {
            return Ok(Some(false));
        }
        arrived()
    })
    .await?;
    Ok(ended.unwrap_or(false))
}

pub(crate) async fn poll<T>(
    every: Duration,
    within: Duration,
    mut check: impl FnMut() -> Result<Option<T>, CallError>,
) -> Result<Option<T>, CallError> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(value) = check()? {
            return Ok(Some(value));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        sleep(every.min(deadline - now)).await;
    }
}
