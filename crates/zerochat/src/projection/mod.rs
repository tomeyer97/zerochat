//! Turns a run's definition and ledger events into what the UI shows: nodes, executions
//! (every visit and retry), time, tokens, dollars, limits under pressure, and a classified
//! failure that says what failed, when, why and what can be done.
//!
//! `project` folds the ledger: `Projector::new`, `feed` every event (fold.rs), `finish`
//! (finish.rs). The parts:
//! - model.rs: the view types the API serialises (`RunView` and its parts);
//! - graph.rs: the graph's nodes, flattened from its definition;
//! - clock.rs: when events happened;
//! - executions.rs: an execution's start, log lines, outcome and settling;
//! - run_log.rs and background.rs: the run's timeline and the agents outside the graph;
//! - pressures.rs: limits close to running out;
//! - status.rs: the run's status;
//! - totals.rs: tokens and dollars per node and for the run;
//! - failure.rs and failure/: why a failed run failed (`classify`, `diagnose`);
//! - words.rs: plain words for Zeroshot's codes and figures.

mod background;
mod clock;
mod executions;
mod failure;
mod finish;
mod fold;
mod graph;
mod model;
mod pressures;
mod run_log;
mod status;
mod totals;
mod words;

use crate::source::{Event, RunData};

pub use model::{ExecState, Execution, PressureLevel, RunView, UsageSource};

// The paths the generated TypeScript (api_types.rs) and the goldens name these by.
#[cfg(test)]
pub(crate) use crate::engine_lines::{humanize_retry, parse_rate_limit_line};
#[cfg(test)]
pub use executions::verdict_accepted;
#[cfg(test)]
pub use failure::{Diagnosis, diagnose, http_statuses, parse_reset};
#[cfg(test)]
pub use graph::{GraphNode, graph_nodes};
#[cfg(test)]
pub use model::{
    Action, BackgroundAgent, CostSource, Failure, Features, Feedback, Lane, LogLine, NodeStats,
    Pressure, PressureKind, state_name,
};
#[cfg(test)]
pub(crate) use words::short_model;
#[cfg(test)]
pub use words::{ERROR_WORDS, RESULT_TOOL, human_ms};

/// The run as the run page shows it, at `now_ms` (live figures run up to then).
pub fn project(target: &str, data: &RunData, now_ms: u64) -> RunView {
    let mut fold = fold::Projector::new(data);
    for event in &data.events {
        fold.feed(event);
    }
    fold.finish(target, data, now_ms)
}

/// Whether the ledger was written by the older patched Zeroshot (the `zerochat` fork): it adds
/// observe fields stock Zeroshot never writes (`logKind`, `provider`, `workspace`, `costUsd`,
/// `model`, `replayOf`). Not `at`: zerochat fills that in from 16.1's `recorded_at_ms`.
pub fn older_patched_ledger(events: &[Event]) -> bool {
    events.iter().any(|e| {
        let b = &e.body;
        match b["kind"].as_str().unwrap_or("") {
            "safe_log" => b.get("logKind").is_some(),
            "node_started" => b.get("provider").is_some() || b.get("workspace").is_some(),
            "node_completed" => {
                b.get("workspace").is_some() || b["completion"].get("workspace").is_some()
            }
            "token_usage_observed" => b.get("costUsd").is_some() || b.get("model").is_some(),
            _ => b.get("replayOf").is_some(),
        }
    })
}

/// This module's TypeScript (api_types.rs): its types in `web/src/generated/projection.ts`, in
/// file order, and the display tables the web shows too.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    /// Model ids `SHORT_MODEL_EXAMPLES` shortens (web/src/contract.test.ts checks format.ts `shortModel`).
    const MODEL_IDS: [&str; 6] = [
        "claude-haiku-4-5-20251001",
        "claude-sonnet-5",
        "claude-opus-4-1-20250805",
        "claude-3-5-sonnet-20241022",
        "gpt-5-codex",
        "sonnet",
    ];
    let q = |s: &str| serde_json::to_string(s).unwrap();
    let words: Vec<String> = ERROR_WORDS
        .iter()
        .map(|(code, words)| format!("  {code}: {},\n", q(words)))
        .collect();
    let examples: Vec<String> = MODEL_IDS
        .iter()
        .map(|id| format!("  [{}, {}],\n", q(id), q(&short_model(id))))
        .collect();
    crate::api_types::ts_types!(cfg, projection: GraphNode, ExecState, UsageSource, CostSource, Execution, NodeStats, PressureKind,
        PressureLevel, Pressure, Feedback, Action, Failure, LogLine, BackgroundAgent, Features, Lane, RunView)
    .with_consts(vec![
        format!("// projection.rs: ERROR_WORDS\n/** Zeroshot's execution error codes in plain words. */\nexport const ERROR_WORDS: Readonly<Record<string, string>> = {{\n{}}};", words.concat()),
        format!("// projection.rs: RESULT_TOOL\n/** The tool Zeroshot's harness has an agent call to hand in its result: not part of the work. */\nexport const RESULT_TOOL = {};", q(RESULT_TOOL)),
        format!("// projection.rs: short_model\n/** Model ids and their short names, as graph cards show them (projection.rs `short_model`; failure copy keeps the exact id). */\nexport const SHORT_MODEL_EXAMPLES: ReadonlyArray<readonly [string, string]> = [\n{}];", examples.concat()),
    ])
}

/// This module's value sets in `web/src/generated/shapes.ts` (api_types.rs `shape_fragments`).
#[cfg(test)]
pub(crate) const TS_SHAPES: &str = r#"/** `Failure::class` (projection.rs): what kind of thing failed. */
export type FailureClass = 'work' | 'agent' | 'provider' | 'setup' | 'orchestration' | 'delivery' | 'stopped';
"#;

#[cfg(test)]
mod tests;
