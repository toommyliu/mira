pub(crate) mod auth;
pub(crate) mod combat;
pub(crate) mod inventory;
pub(crate) mod player;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod world;

use std::time::Duration;

use serde::Serialize;
use tokio::time::{Instant, sleep};

use crate::bridge::{self, CallError, GameAction, Loader};

const POLL: Duration = Duration::from_millis(100);
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const ACTION_TIMEOUT: Duration = Duration::from_secs(2);

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
