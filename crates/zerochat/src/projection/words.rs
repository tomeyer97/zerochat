//! Plain words for Zeroshot's codes and figures, shared by the run log, pressures and the
//! failure card. The web shows some of the same words: it imports those tables from
//! web/src/generated/projection.ts.

/// Zeroshot's execution error codes in plain words. The one table for both sides: the web
/// imports it from web/src/generated/projection.ts (`ERROR_WORDS`).
pub const ERROR_WORDS: [(&str, &str); 4] = [
    ("timeout", "timed out"),
    ("crash", "crashed"),
    ("refusal", "refused"),
    ("malformed", "gave malformed output"),
];

/// An error code in plain words (`ERROR_WORDS`); any other code with its `_`s as spaces.
pub(super) fn error_words(code: &str) -> String {
    ERROR_WORDS
        .iter()
        .find(|(c, _)| *c == code)
        .map_or_else(|| code.replace('_', " "), |(_, words)| (*words).to_owned())
}

/// The tool Zeroshot's harness has an agent call to hand in its result: not part of the work.
/// The web imports it from web/src/generated/projection.ts (`RESULT_TOOL`).
pub const RESULT_TOOL: &str = "StructuredOutput";

/// `45s`, `4m 10s`, `30m`, `2h 05m`, `1h`.
pub fn human_ms(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 if s.is_multiple_of(60) => format!("{}m", s / 60),
        60..3600 => format!("{}m {:02}s", s / 60, s % 60),
        _ if (s % 3600) / 60 == 0 => format!("{}h", s / 3600),
        _ => format!("{}h {:02}m", s / 3600, (s % 3600) / 60),
    }
}

/// `claude-haiku-4-5-20251001` → `haiku-4-5`: the short name graph cards show (format.ts
/// `shortModel`, its twin; web/src/contract.test.ts checks both give the same names,
/// `SHORT_MODEL_EXAMPLES`). Never in failure copy or Details: they show the exact id.
#[cfg(test)]
pub(crate) fn short_model(model: &str) -> String {
    let m = model.strip_prefix("claude-").unwrap_or(model);
    match m.rsplit_once('-') {
        Some((head, date)) if date.len() == 8 && date.chars().all(|c| c.is_ascii_digit()) => {
            head.to_owned()
        }
        _ => m.to_owned(),
    }
}
