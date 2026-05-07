//! Time utility functions for Parley.
//!
//! Provides shared timestamp utilities used across the codebase.

use anyhow::{Context, Result};
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current Unix timestamp in milliseconds.
///
/// # Errors
/// Returns an error if the system clock is before the Unix epoch.
pub fn now_ms() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?;
    Ok(elapsed.as_millis() as u64)
}

/// Returns the current Unix timestamp in milliseconds, or 0 if the clock is invalid.
///
/// This is a fallback variant that never fails, useful for UI rendering and logging.
pub fn now_ms_utc() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_ms_returns_valid_timestamp() {
        let ts = now_ms().expect("now_ms should succeed");
        assert!(ts > 0, "timestamp should be positive");
    }

    #[test]
    fn now_ms_utc_returns_valid_timestamp() {
        let ts = now_ms_utc();
        assert!(ts > 0, "timestamp should be positive");
    }

    #[test]
    fn now_ms_utc_never_fails() {
        // This function should never panic even with invalid clock
        let _ = now_ms_utc();
    }
}
