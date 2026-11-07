pub(crate) mod auth;
pub(crate) mod combat;
pub(crate) mod player;
#[cfg(test)]
pub(crate) mod testing;

use std::time::Duration;

use tokio::time::{Instant, sleep};

use crate::bridge::CallError;

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
