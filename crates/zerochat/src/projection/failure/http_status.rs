//! HTTP status codes in provider error text.

/// Words right before a number that make it a status code: `API Error: 503`, `status 429`,
/// `HTTP 401`, `"status":529`, `code=500` (after dropping ` :=("` in between).
const STATUS_LABELS: [&str; 10] = [
    "api error",
    "status",
    "status code",
    "http",
    "http/1.1",
    "http/2",
    "\"status\"",
    "error code",
    "statuscode",
    "code",
];

/// Reason phrases right after a number that make it a status code: `503 Service Unavailable`
/// (after dropping ` -)` in between).
const STATUS_PHRASES: [&str; 12] = [
    "bad request",
    "unauthorized",
    "forbidden",
    "not found",
    "too many requests",
    "internal server error",
    "bad gateway",
    "service unavailable",
    "service temporarily unavailable",
    "gateway timeout",
    "gateway time-out",
    "overloaded",
];

/// HTTP status codes that appear *as status codes* in provider error text, in order, each once:
/// a three-digit number from 100 to 599 standing on its own, with a status label before it or a
/// reason phrase after it. Never any three-digit number (`crash after 401.3s`, a pid, a line
/// number).
pub fn http_statuses(text: &str) -> Vec<u16> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 <= lower.len() {
        if !three_digits_alone_at(lower.as_bytes(), i) {
            i += 1;
            continue;
        }
        let code: u16 = lower[i..i + 3].parse().unwrap_or(0);
        let before = lower[..i].trim_end_matches([' ', ':', '=', '(', '"']);
        let after = lower[i + 3..].trim_start_matches([' ', '-', ')']);
        let labelled = STATUS_LABELS.iter().any(|l| before.ends_with(l));
        let phrased = STATUS_PHRASES.iter().any(|p| after.starts_with(p));
        if (100..600).contains(&code) && (labelled || phrased) && !out.contains(&code) {
            out.push(code);
        }
        i += 3;
    }
    out
}

/// Whether `bytes[i..i + 3]` are three digits not part of a longer word or number (no letter,
/// digit or `.` touches them on either side).
fn three_digits_alone_at(bytes: &[u8], i: usize) -> bool {
    let part_of_number = |b: &u8| b.is_ascii_alphanumeric() || *b == b'.';
    bytes[i..i + 3].iter().all(u8::is_ascii_digit)
        && !i
            .checked_sub(1)
            .and_then(|p| bytes.get(p))
            .is_some_and(part_of_number)
        && !bytes.get(i + 3).is_some_and(part_of_number)
}
