use std::time::Duration;

pub(crate) fn up_to(max: Duration) -> Duration {
    let mut bytes = [0; 8];
    let random = match getrandom::getrandom(&mut bytes) {
        Ok(()) => u64::from_le_bytes(bytes),
        Err(_) => 0,
    };
    let max = u64::try_from(max.as_millis()).unwrap_or(u64::MAX).max(1);
    Duration::from_millis(random % max)
}
