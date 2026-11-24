//! The wording for a failed agent: one arm per cause, each with its class, cause, headline, what
//! to fix and the action that fits. Edit the copy here; how the cause is found is in
//! diagnose.rs.

use serde_json::Value;

use super::super::model::{Action, Execution};
use super::super::words::human_ms;
use super::diagnose::{DiagnosisKind, diagnose};
use super::explain::{Case, Class, Explanation};
use super::reset::parse_reset;

/// Explains why the culprit `e` failed: the cause its provider's error text names (`diagnose`),
/// else the one Zeroshot's error code and reason name.
pub(super) fn explain_agent_failure(c: &Case, e: &Execution) -> Explanation {
    let code = e
        .error
        .as_ref()
        .and_then(|v| v["code"].as_str())
        .unwrap_or("error");
    let why = e
        .error
        .as_ref()
        .and_then(|v| v["reason"].as_str())
        .unwrap_or("");
    let diagnosis = diagnose(c.last_errors);
    let kind = match (code, &diagnosis) {
        // A node timeout while the provider was not answering (`provider_hang`) is the
        // provider's; otherwise Zeroshot's own timeout wins over whatever the provider said.
        ("timeout", _) if provider_hang(c, e, diagnosis.as_ref().map(|d| d.kind)) => {
            "provider_hang"
        }
        ("timeout", _) => "timeout",
        (_, Some(d)) => d.kind.as_str(),
        (_, None) => zeroshot_kind(code, why),
    };
    let (who, company) = (&c.who, c.company);
    // Where a node's settings live, for a fix that names them.
    let node = &e.node;
    match kind {
        "credentials" => {
            let (fix, command) = credentials_fix(c.f.runtime, &e.node);
            Explanation::new(
                Class::Setup,
                "credentials rejected",
                format!("{} credentials rejected", company.subject()),
            )
            .fix(fix)
            .fix_command(command)
            .primary(Action::Resume)
        }
        "permission" => {
            let model = e
                .model
                .clone()
                .or_else(|| diagnosis.as_ref().and_then(|d| d.model.clone()));
            let title = match model {
                // The exact id, as configured: the one to look for and fix.
                Some(m) => format!("No access to {m}"),
                None => format!("{} denied access", company.subject()),
            };
            // A replay reruns the same model: the fix leads (no primary action).
            Explanation::new(Class::Setup, "no access to model", title)
                .fix(format!(
                    "Set a model this account may use for `{node}` in your runtime file, or use credentials that \
                     may use this one; then start a new run."
                ))
                .resume_does_not_help()
        }
        "model" => {
            let model = diagnosis
                .as_ref()
                .and_then(|d| d.model.clone())
                .or_else(|| e.model.clone());
            let title = match model {
                Some(m) => format!("Unknown model {m}"),
                None => "Unknown model".to_owned(),
            };
            let fix = format!(
                "Set a model {} knows for `{node}` in your runtime file, then start a new run.",
                company.object()
            );
            Explanation::new(Class::Setup, "unknown model", title)
                .fix(fix)
                .resume_does_not_help()
        }
        "usage_limit" => {
            let resets = diagnosis.as_ref().and_then(|d| d.resets.as_deref());
            let resets_at = resets.and_then(|r| parse_reset(r, c.at.unwrap_or(c.f.now_ms)));
            // The Resume button already says when; nothing here says more.
            Explanation::new(
                Class::Provider,
                "usage limit",
                format!("{who} hit {}", company.the("usage limit")),
            )
            .primary(Action::Resume)
            .resets_at(resets_at)
        }
        "rate_limit" => Explanation::new(
            Class::Provider,
            "rate limited",
            format!("{who} was rate limited by {}", company.object()),
        )
        .primary(Action::Resume),
        "provider_down" => {
            let http = diagnosis
                .as_ref()
                .and_then(|d| d.http)
                .map(|c| format!(", {c}"))
                .unwrap_or_default();
            let title = format!(
                "{} was overloaded or unavailable ({who}{http})",
                company.subject()
            );
            Explanation::new(Class::Provider, "provider unavailable", title)
                .detail("Usually temporary: resume later.")
                .primary(Action::Resume)
        }
        "stream_cut" => Explanation::new(
            Class::Provider,
            "response cut off",
            format!(
                "{} answer to {who} was cut off",
                company.subject_possessive()
            ),
        )
        .detail("The connection dropped mid-answer (network or provider); resume to retry.")
        .primary(Action::Resume),
        "provider_timeout" => {
            let title = format!(
                "{} stopped answering {who} (request timed out)",
                company.subject()
            );
            Explanation::new(Class::Provider, "provider not responding", title)
                .primary(Action::Resume)
        }
        "provider_hang" => {
            let after = c
                .info
                .and_then(|n| n.timeout_ms)
                .map(|t| format!(" (timed out after {})", human_ms(t)))
                .unwrap_or_default();
            let title = format!("{} stopped answering {who}{after}", company.subject());
            let waited = match unanswered_for(c, e) {
                Some(ms) => format!(
                    "Its last model request had no answer for {} when the timeout hit.",
                    human_ms(ms)
                ),
                None => "The agent was waiting on the API when its timeout hit.".to_owned(),
            };
            Explanation::new(Class::Provider, "provider not responding", title)
                .detail(format!("{waited} Usually temporary: resume."))
                .primary(Action::Resume)
        }
        "context" => Explanation::new(
            Class::Agent,
            "context window full",
            format!("{who} ran out of context window"),
        )
        .primary(Action::Replay),
        "disk" => Explanation::new(
            Class::Setup,
            "disk full",
            format!("{who} ran out of disk space"),
        )
        .fix("Free disk space on the machine running the agents, then resume.")
        .primary(Action::Resume),
        "network" => Explanation::new(
            Class::Agent,
            "network error",
            format!("{who} could not reach {}", company.object()),
        )
        .primary(Action::Resume),
        "missing_cli" => {
            // The harness as its lane names it; never a guess (harness_name.rs `label`).
            let harness = crate::harness_name::label(c.harness);
            Explanation::new(Class::Setup, "agent CLI missing", format!("{who} could not start the {harness} CLI"))
                .fix(format!(
                    "Install the `{harness}` CLI on the machine running the agents (it must be on PATH), then resume."
                ))
                .primary(Action::Resume)
        }
        "input" => Explanation::new(
            Class::Agent,
            "asked for input",
            format!("{who} stopped to ask for input a headless run cannot give"),
        ),
        "policy" => Explanation::new(
            Class::Setup,
            "blocked by policy",
            format!("{who} was blocked by a tool or permission policy"),
        ),
        "malformed" => Explanation::new(
            Class::Agent,
            "malformed result",
            format!("{who} did not return a result in the required format"),
        )
        .primary(Action::Replay),
        "timeout" => {
            let (cause, title, now) = match c.info.and_then(|n| n.timeout_ms) {
                Some(t) => (
                    format!("timed out after {}", human_ms(t)),
                    format!("{who} timed out after {}", human_ms(t)),
                    format!(" (now {})", human_ms(t)),
                ),
                None => (
                    "timed out".to_owned(),
                    format!("{who} timed out"),
                    String::new(),
                ),
            };
            // A replay keeps the timeout, so it times out again: the fix leads (no primary).
            let fix = format!(
                "Raise `timeoutMs` on `{node}` in your graph{now}, or give it a smaller task; then start a new run."
            );
            Explanation::new(Class::Agent, cause, title)
                .fix(fix)
                .resume_does_not_help()
        }
        "crash" => Explanation::new(Class::Agent, "crashed", format!("{who} crashed"))
            .detail("The agent process exited without a result.")
            .primary(Action::Resume),
        "malformed_stream" => Explanation::new(
            Class::Provider,
            "malformed API response",
            format!("{} sent {who} a malformed response", company.subject()),
        )
        .detail("A broken stream from the provider or the network. Usually temporary: resume.")
        .primary(Action::Resume),
        "refusal" => Explanation::new(Class::Agent, "refused", format!("{who} refused the task")),
        _ => Explanation::new(
            Class::Agent,
            format!("failed ({code})"),
            format!("{who} failed ({code})"),
        ),
    }
}

/// Whether a node timeout `e` hit while the provider was not answering, rather than while the
/// agent was still working. The ledger's evidence: the agent's API retry lines before the
/// timeout (`e.retries`, counted only for genuine retries), a provider fault in its error lines
/// (a request timeout, a cut or malformed stream, a 5xx, a network error), or a model request
/// left unanswered for long before the timeout (`unanswered_for`). A timeout with none of these
/// is the node's own limit: the agent was still working.
fn provider_hang(c: &Case, e: &Execution, diagnosed: Option<DiagnosisKind>) -> bool {
    use DiagnosisKind::*;
    e.retries > 0
        || matches!(
            diagnosed,
            Some(ProviderTimeout | StreamCut | MalformedStream | ProviderDown | Network)
        )
        || unanswered_for(c, e).is_some()
}

/// The shortest silence that reads as a model request nobody answered: a model starts answering
/// within seconds, and even a long answer shows within this.
const UNANSWERED_MIN_MS: u64 = 45_000;

/// How long the model owed `e` an answer when it ended, when that reads as the provider not
/// answering: no tool was running, and nothing came for at least `UNANSWERED_MIN_MS` and a third
/// of the node's timeout (an agent that was working shows lines until close to its limit).
fn unanswered_for(c: &Case, e: &Execution) -> Option<u64> {
    let silence = e.ended_at?.checked_sub(e.model_turn_since?)?;
    let timeout = c.info.and_then(|n| n.timeout_ms).unwrap_or(0);
    (silence >= UNANSWERED_MIN_MS && silence.saturating_mul(3) >= timeout).then_some(silence)
}

/// The cause Zeroshot's error `code` and `reason` name, when the provider's text names none.
fn zeroshot_kind(code: &str, why: &str) -> &'static str {
    match (code, why) {
        (_, "authentication_required") => "credentials",
        (_, "interactive_input_required") => "input",
        (_, "policy_denied") => "policy",
        (_, "malformed_result") | ("malformed", _) => "malformed",
        ("timeout", _) => "timeout",
        ("crash", _) => "crash",
        ("refusal", _) => "refusal",
        _ => "other",
    }
}

/// The fix for rejected credentials and the `zeroshot connection set` command that makes it:
/// for the connection the node's runtime binding names (e.g. `anthropic` →
/// CLAUDE_CODE_OAUTH_TOKEN), else for the target's provider credentials in general.
fn credentials_fix(runtime: &Value, node: &str) -> (String, String) {
    let connections = runtime["nodes"][node]["connections"].as_object();
    let first_env = |envs: &Value| {
        envs.as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    match connections
        .and_then(|m| m.iter().next())
        .map(|(key, envs)| (key, first_env(envs)))
    {
        Some((key, env)) if !env.is_empty() => (
            format!("Update the `{key}` connection, then resume."),
            format!("zeroshot connection set {key} --field {env}"),
        ),
        _ => (
            "Update the provider credentials for this target, then resume.".to_owned(),
            "zeroshot connection set".to_owned(),
        ),
    }
}
