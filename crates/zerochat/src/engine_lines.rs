//! Zeroshot's own log lines about the provider: API retries, rate-limit notices and the
//! duration in a node's failure line. The run projection (pressures, run log) and the chat
//! transcript both read them.
//!
//! Each line is parsed into a struct first; how a view says it (`retried 2 of 10 (rate limit)`,
//! `5-hour limit OK`) is a separate step, so a change in Zeroshot's wording touches the parser
//! only.

use crate::projection::PressureLevel;

/// Starts an API retry line: `Claude API retry 2/10: rate_limit`.
pub const RETRY_PREFIX: &str = "Claude API retry";

/// Starts a rate-limit notice: `Claude rate limit: status=…, type=…, utilization=…%, resetsAt=…`.
pub const RATE_LIMIT_PREFIX: &str = "Claude rate limit: ";

/// What a rate-limit pressure says when Zeroshot's structured event (`logKind: rate_limit`)
/// carries a line this parser does not read: a title, never the raw line.
pub const RATE_LIMIT_TITLE: &str = "rate limit hit";

/// The same for an API retry (`logKind: retry`) whose line is not `Claude API retry n/max: …`.
pub const RETRY_TITLE: &str = "retrying the provider's API";

/// A rate-limit window is shown as pressing only from this share used: at 33% used, a 7-day
/// budget with days left is account info, not something pressing on the run.
const RATE_LIMIT_WARN_PCT: f64 = 80.0;

/// Whether a line is an API retry (`Claude API retry …`).
pub fn is_retry_line(text: &str) -> bool {
    text.starts_with(RETRY_PREFIX)
}

/// Whether a line is Zeroshot closing a run, not anything its agents did: `Run failed: …`, and
/// `Node worker failed: crash: run runtime_lost`, which Zeroshot writes for an execution that was
/// still open when it closed a lost run (`zeroshot resume` does, when it starts the successor).
pub fn is_closing_line(text: &str) -> bool {
    text.starts_with("Run failed")
        || (text.starts_with("Node ") && text.ends_with(": run runtime_lost"))
}

/// `Claude API retry 2/10: rate_limit`, parsed.
struct Retry<'a> {
    attempt: &'a str,
    max: &'a str,
    /// The engine's error enum (`rate_limit`, `unknown`, or empty).
    error: &'a str,
}

fn parse_retry(text: &str) -> Option<Retry<'_>> {
    let rest = text.strip_prefix(RETRY_PREFIX)?.strip_prefix(' ')?;
    let (counts, error) = rest.split_once(':')?;
    let (attempt, max) = counts.trim().split_once('/')?;
    Some(Retry {
        attempt,
        max,
        error: error.trim(),
    })
}

/// `Claude API retry 2/10: rate_limit` → `retried 2 of 10 (rate limit)`: the engine's enum stays
/// out of the UI, and the count is the fact worth keeping.
pub(crate) fn humanize_retry(text: &str) -> Option<String> {
    let Retry {
        attempt,
        max,
        error,
    } = parse_retry(text)?;
    let error_human = match error {
        "" | "unknown" => "a transient error".to_owned(),
        "rate_limit" => "rate limit".to_owned(),
        other => other.replace('_', " "),
    };
    Some(format!("retried {attempt} of {max} ({error_human})"))
}

/// A `Claude rate limit: …` notice, parsed. A field the line does not carry is empty.
struct RateLimitNotice {
    /// `allowed`, `allowed_warning`, or anything else for a limit that was hit.
    status: String,
    /// The window's type (`five_hour`, `seven_day`, `seven_day_opus`, …).
    window: String,
    /// Share of the window used, in percent.
    utilization: Option<f64>,
    resets_at: Option<u64>,
}

fn parse_rate_limit_notice(text: &str) -> Option<RateLimitNotice> {
    let rest = text.strip_prefix(RATE_LIMIT_PREFIX)?;
    let field = |name: &str| {
        let prefix = format!("{name}=");
        rest.split(", ")
            .find_map(|part| part.strip_prefix(&prefix))
            .unwrap_or("")
            .to_owned()
    };
    Some(RateLimitNotice {
        status: field("status"),
        window: field("type"),
        utilization: field("utilization")
            .trim_end_matches('%')
            .parse::<f64>()
            .ok(),
        resets_at: crate::transcript::parse_iso_millis(&field("resetsAt")),
    })
}

/// One rate-limit notice as both views show it: the window, how pressing it is, a plain-words
/// line and when it resets.
pub(crate) struct RateLimitLine {
    pub window: String,
    pub level: PressureLevel,
    pub text: String,
    pub resets_at: Option<u64>,
}

pub(crate) fn parse_rate_limit_line(text: &str) -> Option<RateLimitLine> {
    let notice = parse_rate_limit_notice(text)?;
    let window = window_name(&notice.window);
    let used = notice
        .utilization
        .map(|p| format!("{} used", format_pct(p)));
    let (level, text) = match notice.status.as_str() {
        "allowed" => (PressureLevel::Info, format!("{window} limit OK")),
        // Below the real threshold this is account info, not pressure on the run: keep it
        // out of sight (`info` is filtered out) but keep the number for when it does matter.
        "allowed_warning" if notice.utilization.is_some_and(|p| p < RATE_LIMIT_WARN_PCT) => (
            PressureLevel::Info,
            format!("{window} limit: {}", used.unwrap_or_default()),
        ),
        "allowed_warning" => (
            PressureLevel::Warning,
            format!(
                "{window} limit: {}",
                used.unwrap_or_else(|| "almost used".to_owned())
            ),
        ),
        _ => (PressureLevel::Critical, format!("{window} limit hit")),
    };
    Some(RateLimitLine {
        window,
        level,
        text,
        resets_at: notice.resets_at,
    })
}

/// `five_hour` → `5-hour`; an unknown window type with its `_`s as spaces.
fn window_name(window_type: &str) -> String {
    match window_type {
        "five_hour" => "5-hour".to_owned(),
        "seven_day" => "7-day".to_owned(),
        "seven_day_opus" => "7-day Opus".to_owned(),
        other => other.replace('_', " "),
    }
}

/// `33%`, `9.5%`: one decimal only when it is not a whole number.
fn format_pct(value: f64) -> String {
    if (value - value.round()).abs() < 0.05 {
        format!("{}%", value.round() as i64)
    } else {
        format!("{value:.1}%")
    }
}

/// Zeroshot's own duration from a node's failure line:
/// `Node worker failed: timeout after 45.4s: …` → 45 400 ms.
pub fn reported_duration(line: &str) -> Option<u64> {
    let rest = line.strip_prefix("Node ")?;
    let (_, rest) = rest.split_once(" failed: ")?;
    let (_, rest) = rest.split_once(" after ")?;
    let number: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let unit = &rest[number.len()..];
    let value: f64 = number.parse().ok()?;
    let ms = if unit.starts_with("ms") {
        value
    } else if unit.starts_with('s') {
        value * 1000.0
    } else if unit.starts_with('m') {
        value * 60_000.0
    } else {
        return None;
    };
    Some(ms.round() as u64)
}
