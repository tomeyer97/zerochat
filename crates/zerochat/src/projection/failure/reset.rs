//! When a usage limit resets, from the provider's phrase (`resets 9:30am (UTC)`).

const HOUR_MS: u64 = 3_600_000;
const MINUTE_MS: u64 = 60_000;
pub(super) const DAY_MS: u64 = 24 * HOUR_MS;

/// `9:30am (UTC)`, `11am (UTC)` or `21:05 (UTC)` → the next such moment after `after` (Unix ms).
/// A time in any other zone (or none) is not guessed at.
pub fn parse_reset(text: &str, after: u64) -> Option<u64> {
    let lower = text.to_ascii_lowercase();
    if !lower.contains("utc") {
        return None;
    }
    let (hour, minute) = parse_clock(&lower)?;
    Some(next_time_of_day(hour * HOUR_MS + minute * MINUTE_MS, after))
}

/// The 12- or 24-hour clock time `text` starts with (`9:30am`, `11am`, `21:05`) → (hour, minute)
/// on a 24-hour clock.
fn parse_clock(text: &str) -> Option<(u64, u64)> {
    let clock: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, ':' | 'a' | 'p' | 'm'))
        .collect();
    let pm = clock.ends_with("pm");
    let am = clock.ends_with("am");
    let digits = clock.trim_end_matches("am").trim_end_matches("pm");
    let (h, m) = match digits.split_once(':') {
        Some((h, m)) => (h.parse::<u64>().ok()?, m.parse::<u64>().ok()?),
        None => (digits.parse::<u64>().ok()?, 0),
    };
    let hour = match (am, pm, h) {
        (true, _, 12) => 0,
        (_, true, 12) => 12,
        (_, true, h) => h + 12,
        (_, _, h) => h,
    };
    (hour <= 23 && m <= 59).then_some((hour, m))
}

/// The first moment after `after` (Unix ms) whose UTC time of day is `time_of_day_ms`.
fn next_time_of_day(time_of_day_ms: u64, after: u64) -> u64 {
    let today = after / DAY_MS * DAY_MS + time_of_day_ms;
    if today <= after {
        today + DAY_MS
    } else {
        today
    }
}
