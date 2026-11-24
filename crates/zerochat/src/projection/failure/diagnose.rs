//! What a provider's own error text says went wrong. Zeroshot often reports these as a generic
//! `crash`, so the error lines are the only place the real cause shows.
//!
//! The needles are lower-case pieces of Claude, Codex, Copilot and Node error text; when a
//! provider changes its wording, the `RULES` table is the place to follow it.

use serde::Serialize;

use super::http_status::http_statuses;

pub struct Diagnosis {
    pub kind: DiagnosisKind,
    pub model: Option<String>,
    /// The HTTP status the error carried, when one did (shown for `ProviderDown`).
    pub http: Option<u16>,
    /// The provider's reset phrase, e.g. `9:30am (UTC)`.
    pub resets: Option<String>,
}

/// The causes `diagnose` recognises in a provider's error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosisKind {
    Permission,
    Credentials,
    UsageLimit,
    RateLimit,
    Model,
    ProviderDown,
    StreamCut,
    ProviderTimeout,
    MalformedStream,
    Context,
    Disk,
    Network,
    MissingCli,
}

impl DiagnosisKind {
    /// The cause's name, as the failure classification matches it.
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosisKind::Permission => "permission",
            DiagnosisKind::Credentials => "credentials",
            DiagnosisKind::UsageLimit => "usage_limit",
            DiagnosisKind::RateLimit => "rate_limit",
            DiagnosisKind::Model => "model",
            DiagnosisKind::ProviderDown => "provider_down",
            DiagnosisKind::StreamCut => "stream_cut",
            DiagnosisKind::ProviderTimeout => "provider_timeout",
            DiagnosisKind::MalformedStream => "malformed_stream",
            DiagnosisKind::Context => "context",
            DiagnosisKind::Disk => "disk",
            DiagnosisKind::Network => "network",
            DiagnosisKind::MissingCli => "missing_cli",
        }
    }
}

impl Serialize for DiagnosisKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The HTTP status that names a cause on its own, whatever the text says.
#[derive(Clone, Copy)]
enum Status {
    None,
    Code(u16),
    /// Any 5xx.
    ServerError,
}

/// One cause: it matches when the error text carries its status, or contains one of its needles
/// (and then, when `and_one_of` is not empty, one of those too).
struct Rule {
    kind: DiagnosisKind,
    status: Status,
    needles: &'static [&'static str],
    and_one_of: &'static [&'static str],
}

const fn rule(kind: DiagnosisKind, status: Status, needles: &'static [&'static str]) -> Rule {
    Rule {
        kind,
        status,
        needles,
        and_one_of: &[],
    }
}

/// The causes in the order they are tried: the first that matches wins (a 403 is a permission
/// problem even though its text says "Failed to authenticate"; a usage limit also mentions a
/// limit, so it comes before the rate limit).
const RULES: &[Rule] = &[
    // 403: the credentials work, but this account may not use the model (or the resource).
    rule(
        DiagnosisKind::Permission,
        Status::Code(403),
        &[
            "permission_error",
            "does not have permission",
            "not have access to this model",
        ],
    ),
    rule(
        DiagnosisKind::Credentials,
        Status::Code(401),
        &[
            "failed to authenticate",
            "invalid x-api-key",
            "oauth access token is invalid",
            "authentication_failed",
            "authentication_error",
            "not logged in",
            "please run /login",
        ],
    ),
    rule(
        DiagnosisKind::UsageLimit,
        Status::None,
        &[
            "usage limit",
            "usage_limit",
            "session limit",
            "weekly limit",
            "hit your",
            "reached your",
            "limit reached",
            "credit balance is too low",
            "quota",
        ],
    ),
    rule(
        DiagnosisKind::RateLimit,
        Status::Code(429),
        &["rate_limit_error", "rate limit"],
    ),
    rule(
        DiagnosisKind::Model,
        Status::None,
        &[
            "not_found_error",
            "model not found",
            "invalid model",
            "unknown model",
            "model_not_found",
            "does not exist",
            "issue with the selected model",
            "unrecognized_model",
        ],
    ),
    rule(
        DiagnosisKind::ProviderDown,
        Status::ServerError,
        &[
            "overloaded_error",
            "overloaded",
            "bad gateway",
            "service unavailable",
            "service temporarily unavailable",
            "gateway timeout",
            "internal server error",
        ],
    ),
    // The answer stopped mid-stream (or came back empty): a network or provider blip.
    rule(
        DiagnosisKind::StreamCut,
        Status::None,
        &[
            "streamnoeventserror",
            "no_events",
            "stream ended unexpectedly",
            "premature close",
            "socket hang up",
        ],
    ),
    rule(
        DiagnosisKind::ProviderTimeout,
        Status::None,
        &[
            "request timed out",
            "request timeout",
            "timed out waiting for",
            "apiconnectiontimeouterror",
        ],
    ),
    rule(
        DiagnosisKind::MalformedStream,
        Status::None,
        &[
            "empty or malformed response",
            "malformed response",
            "malformed api response",
            "unexpected end of stream",
            "invalid sse",
            "invalid event stream",
        ],
    ),
    rule(
        DiagnosisKind::Context,
        Status::None,
        &[
            "prompt is too long",
            "context length",
            "context window",
            "maximum context",
        ],
    ),
    rule(
        DiagnosisKind::Disk,
        Status::None,
        &["enospc", "no space left on device"],
    ),
    rule(
        DiagnosisKind::Network,
        Status::None,
        &[
            "econnrefused",
            "etimedout",
            "enotfound",
            "getaddrinfo",
            "network error",
            "connection refused",
            "connection reset",
            "econnreset",
        ],
    ),
    // A missing file is a missing CLI only when the line names an agent CLI (or spawning one).
    Rule {
        kind: DiagnosisKind::MissingCli,
        status: Status::None,
        needles: &[
            "executable file not found",
            "no such file or directory",
            "command not found",
        ],
        and_one_of: &["claude", "codex", "copilot", "spawn"],
    },
];

/// What a provider's own error text says went wrong: the first of `RULES` that matches, with
/// the model, HTTP status or reset time the text names.
pub fn diagnose(lines: &[String]) -> Option<Diagnosis> {
    let text = lines.join("\n");
    let lower = text.to_ascii_lowercase();
    let codes = http_statuses(&text);
    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    let server_error = codes.iter().copied().find(|c| (500..600).contains(c));
    let matches = |rule: &&Rule| {
        let by_status = match rule.status {
            Status::None => false,
            Status::Code(code) => codes.contains(&code),
            Status::ServerError => server_error.is_some(),
        };
        (by_status || has(rule.needles)) && (rule.and_one_of.is_empty() || has(rule.and_one_of))
    };
    let kind = RULES.iter().find(matches)?.kind;

    let mut diagnosis = Diagnosis {
        kind,
        model: None,
        http: None,
        resets: None,
    };
    match kind {
        DiagnosisKind::Permission => {
            diagnosis.model = selected_model(&text);
            diagnosis.http = Some(403);
        }
        DiagnosisKind::Credentials => diagnosis.http = codes.contains(&401).then_some(401),
        DiagnosisKind::UsageLimit => diagnosis.resets = resets_phrase(lines),
        DiagnosisKind::RateLimit => diagnosis.http = Some(429),
        DiagnosisKind::Model => {
            diagnosis.model = selected_model(&text).or_else(|| model_field(&text))
        }
        DiagnosisKind::ProviderDown => diagnosis.http = server_error,
        _ => {}
    }
    Some(diagnosis)
}

/// The model Claude names in `There's an issue with the selected model (claude-x-9)`.
fn selected_model(text: &str) -> Option<String> {
    text.split("selected model (")
        .nth(1)
        .and_then(|r| r.split(')').next())
        .map(str::to_owned)
}

/// The model of a JSON error body: `"model":"…"`.
fn model_field(text: &str) -> Option<String> {
    text.split("\"model\":\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .map(str::to_owned)
}

/// The usage limit's reset phrase, from the last line that has one: `… resets 9:30am (UTC); …`
/// → `9:30am (UTC)`.
fn resets_phrase(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .rev()
        .find_map(|l| l.split("resets ").nth(1))
        .map(|r| {
            r.split([';', '\n'])
                .next()
                .unwrap_or("")
                .trim()
                .trim_end_matches('.')
                .to_owned()
        })
        .filter(|r| !r.is_empty())
}
