use anyhow::{Context, Result};
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current time in milliseconds since the Unix epoch.
pub fn now_ms() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?;
    Ok(elapsed.as_millis() as u64)
}

/// Returns the current time in milliseconds since the Unix epoch (UTC).
/// Alias for now_ms() for backward compatibility.
pub fn now_ms_utc() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}
