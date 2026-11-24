//! The clock: "now" in Unix milliseconds, with a fixed time for golden tests.

use std::time::{SystemTime, UNIX_EPOCH};

/// Now, in Unix ms (the test clock's time when a test set one).
pub fn now_ms() -> u64 {
    #[cfg(test)]
    if let Some(now) = test_clock::get() {
        return now;
    }
    wall_ms()
}

/// Now by the system clock, in Unix ms, even under a test clock (`0` before 1970).
pub fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Test seam: a fixed "now" for golden tests of time-dependent responses (unset: the real clock).
#[cfg(test)]
pub mod test_clock {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NOW: AtomicU64 = AtomicU64::new(0);
    pub fn set(ms: u64) {
        NOW.store(ms, Ordering::SeqCst);
    }
    pub fn get() -> Option<u64> {
        Some(NOW.load(Ordering::SeqCst)).filter(|n| *n > 0)
    }
}
