//! What the run page reads: the run view and its parts, as the API serialises them. Their
//! TypeScript twins are generated into web/src/generated/projection.ts (`make types`).

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use super::graph::GraphNode;
use crate::pricing::Usage;
use crate::source::LocalFacts;

/// One execution of a node: a visit or a retry of one.
///
/// The fields are flat on the wire (the web and server.rs read them directly); they come in
/// seven groups: identity, timing, lifecycle, I/O, usage and cost, lineage, and the agent's
/// runtime with its log counts.
#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    // Identity.
    pub id: u64,
    pub node: String,
    pub node_instance: u64,
    pub attempt: u64,
    #[ts(type = "Json")]
    pub map_indices: Value,
    /// 1-based visit number of this node (retries of one visit share it).
    pub round: u64,

    // Timing: ledger positions, then wall-clock times (Unix ms).
    pub start_seq: u64,
    pub end_seq: Option<u64>,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    /// True when times are inferred from neighbouring log timestamps (stock Zeroshot).
    pub times_approximate: bool,
    pub duration_ms: Option<u64>,
    pub last_activity_at: Option<u64>,

    // Lifecycle: where it is, and how it ended.
    pub state: ExecState,
    #[ts(type = "Json | null")]
    pub outcome: Option<Value>,
    #[ts(type = "JsonObject | null")]
    pub signals: Option<Value>,
    pub accepted: Option<bool>,
    #[ts(type = "Json | null")]
    pub diagnostic: Option<Value>,
    #[ts(type = "ExecutionError | null")]
    pub error: Option<Value>,
    pub void_reason: Option<String>,

    // Input and output, as Zeroshot recorded them.
    #[ts(type = "Json")]
    pub input: Value,
    #[ts(type = "Json | null")]
    pub output: Option<Value>,

    // Usage and cost.
    pub usage: Usage,
    pub usage_source: UsageSource,
    pub cost_usd: Option<f64>,
    pub cost_source: Option<CostSource>,

    // Lineage.
    /// Imported from the run this one replays or resumes (not run again).
    pub reused: bool,
    /// The run a reused execution comes from (it ran there; this run has no times for it).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reused_from: Option<String>,

    // The agent's runtime (its node's lane: graph.rs `lane_field`) and what its log showed.
    /// The exact model id, as configured (or as the older patched Zeroshot reported it).
    pub model: Option<String>,
    pub harness: Option<String>,
    /// Not on the wire (the transcript API serialises executions as they were): the run page
    /// reads a node's provider from `GraphNode::provider` and `RunView::lanes`.
    #[serde(skip)]
    pub provider: Option<String>,
    pub session_id: Option<String>,
    #[ts(type = "WorkspaceSnapshot | null")]
    pub workspace_start: Option<Value>,
    #[ts(type = "WorkspaceSnapshot | null")]
    pub workspace_end: Option<Value>,
    pub log_lines: u64,
    pub tool_calls: u64,
    /// API retries and rate-limit waits this execution survived: shown in its chat
    /// (merged into the transcript) and as a count in Details.
    pub retries: u64,
    pub error_lines: Vec<String>,
    /// Zeroshot's own duration from its failure line (`failed: timeout after 45.4s`).
    #[serde(skip)]
    pub(super) reported_ms: Option<u64>,
    /// Since when the agent's model owed an answer, as of its last line: from its start, a tool
    /// result or its own text, with no tool call running (`None` while one runs). A timeout
    /// long after it is the provider not answering (failure/agent_copy.rs `provider_hang`).
    #[serde(skip)]
    pub(super) model_turn_since: Option<u64>,
}

/// Where an execution's token usage comes from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// Zeroshot's own count.
    #[default]
    Ledger,
    /// Live, from the provider's own log until the ledger has it.
    SessionLog,
    /// Not counted until the node's turn ends.
    Pending,
}

/// Where an execution's cost comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    /// The provider reported it.
    Provider,
    /// Estimated from the tokens (`pricing::estimate_usd`), shown with `~`.
    Estimate,
}

/// `Default` only lets a node's stats start empty: every node has an execution, whose state
/// replaces it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ExecState {
    #[default]
    Running,
    Succeeded,
    Accepted,
    Rejected,
    Failed,
    Voided,
    /// Was running when the run stopped (force stop or lost runtime).
    Interrupted,
}

/// A state's wire name, as `serde(rename_all = "snake_case")` writes it; the goldens check both
/// agree. Only the tests need it.
#[cfg(test)]
pub fn state_name(state: ExecState) -> &'static str {
    match state {
        ExecState::Running => "running",
        ExecState::Succeeded => "succeeded",
        ExecState::Accepted => "accepted",
        ExecState::Rejected => "rejected",
        ExecState::Failed => "failed",
        ExecState::Voided => "voided",
        ExecState::Interrupted => "interrupted",
    }
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NodeStats {
    /// The state of the node's latest execution.
    pub state: ExecState,
    pub executions: Vec<u64>,
    pub rounds: u64,
    pub usage: Usage,
    pub cost_usd: Option<f64>,
    /// Part of `cost_usd` is estimated from tokens (shown with `~`).
    pub cost_estimated: bool,
    pub elapsed_ms: u64,
    pub active_since: Option<u64>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Pressure {
    pub kind: PressureKind,
    pub level: PressureLevel,
    pub text: String,
    pub execution: Option<u64>,
    pub node: Option<String>,
    pub at: Option<u64>,
    /// When a limit resets (Unix ms).
    pub resets_at: Option<u64>,
}

/// What presses on a live run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PressureKind {
    /// A node is on its last attempt.
    Attempts,
    /// A node has used most of its time.
    Timeout,
    /// A loop is in its last round.
    Rounds,
    /// A provider rate or usage limit.
    RateLimit,
    /// The provider's API is being retried.
    Retry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PressureLevel {
    /// Account info, not pressing on the run: never shown as a pressure.
    Info,
    Warning,
    Critical,
}

/// A reviewer's rejection, for the failure card of a run whose work was not accepted.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    pub node: String,
    pub round: u64,
    pub execution: u64,
    pub verdict: String,
    pub text: String,
}

/// Why a run failed, as the run page's failure card shows it.
///
/// The backend writes the user-facing copy (cause, title, detail, fix) and picks the actions;
/// the web only lays them out. Wording belongs here, not in the web.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    /// work | agent | provider | setup | orchestration | delivery | stopped
    #[ts(type = "FailureClass")]
    pub class: &'static str,
    /// The cause in 2-4 words, for run lists (`credentials rejected`, `timed out after 45s`).
    pub cause: String,
    /// One short headline.
    pub title: String,
    /// At most one sentence of context that the headline does not already say.
    pub detail: String,
    /// What to fix before retrying, when retrying unchanged would fail the same way.
    pub fix: Option<String>,
    /// The command that makes the fix, shown on its own line with a copy button.
    pub fix_command: Option<String>,
    pub reason: String,
    /// The graph's `fail` node the run ended in, when one matches the reason.
    pub exit: Option<String>,
    pub node: Option<String>,
    /// `step`, `verifier`, … of the failing node.
    pub node_kind: Option<String>,
    pub execution: Option<u64>,
    pub round: Option<u64>,
    pub attempt: Option<u64>,
    /// When the deciding node failed (or the controller was last seen).
    pub at: Option<u64>,
    pub into_run_ms: Option<u64>,
    /// How long after `at` the run was closed, when that took a while (Active Memory postmortem).
    pub closed_after_ms: Option<u64>,
    pub closed_by_postmortem: bool,
    /// Nodes whose work finished and was kept before the failure, in order.
    pub kept: Vec<String>,
    /// Rejections, newest round first (for `work` failures).
    pub feedback: Vec<Feedback>,
    /// When a usage limit resets (Unix ms).
    pub resets_at: Option<u64>,
    pub last_errors: Vec<String>,
    /// The one action that fits the cause best, or none when retrying unchanged would fail the
    /// same way.
    pub primary: Option<Action>,
    /// Every available action: only `replay` and `resume` (the variants of a replay are presets
    /// of the replay button, see `Action::base`).
    pub actions: Vec<Action>,
}

/// What a person can do about a failed run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Run again from a checkpoint.
    Replay,
    /// A replay with another model (the model was the cause).
    // Not chosen while no Zeroshot can replay with changes (a replay reruns the same model; the
    // failure's fix leads instead); kept on the wire for the web's replay presets.
    #[allow(dead_code)]
    ReplayModel,
    /// A replay with a longer timeout (the node timed out).
    #[allow(dead_code)]
    ReplayTimeout,
    /// Continue the same run.
    Resume,
}

impl Action {
    /// The action that offers this one: a replay with a preset is still a replay.
    pub fn base(self) -> Action {
        match self {
            Action::ReplayModel | Action::ReplayTimeout => Action::Replay,
            other => other,
        }
    }
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub seq: u64,
    pub at: u64,
    pub stream: String,
    /// The background agent that wrote it (`[name] …` prefix, removed from `text`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent: Option<String>,
    /// Streamed deltas of one agent are joined into one line.
    pub text: String,
}

/// An agent Zeroshot runs outside the graph (e.g. Active Memory maintenance), seen only through
/// its prefixed log lines and run-level token usage.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAgent {
    pub name: String,
    pub lines: u64,
    pub first_at: u64,
    pub last_at: u64,
    /// The last thing it said (deltas joined, tool results as their text).
    pub last_line: String,
    pub retries: u64,
    pub errors: u64,
    /// `failed` / `completed`, from the run's own `Active Memory <name> …` lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "'failed' | 'completed'")]
    pub outcome: Option<&'static str>,
    /// Why it failed (`findings rejected: …`), when the run log says.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

/// What the ledger's Zeroshot records, so the page knows which figures are exact.
#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Features {
    pub exact_times: bool,
    pub tool_calls: bool,
    pub provider_cost: bool,
    pub workspace_snapshots: bool,
}

/// A lane the run's agents ran on (harness, provider, exact model id) and the nodes on it, as
/// the run header lists them. `None`: the ledger does not say.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Lane {
    pub harness: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub nodes: Vec<String>,
}

/// A run as the run page shows it. The server's run handler (server.rs) adds `capabilities`,
/// `targetKind`, `targetName`, `context` and, for a pb run, pb's own cost (`costSource`,
/// `zeroshotCostUsd`, `zeroshotCostEstimated`); web/src/types.ts `RunView` adds them too.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub target: String,
    pub run_id: String,
    pub title: String,
    pub phase: String,
    /// running | succeeded | failed | stopping | admitted | stalled
    pub status: String,
    pub created_at: Option<u64>,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub last_activity_at: Option<u64>,
    #[ts(type = "RunTerminal | null")]
    pub terminal: Value,
    #[ts(type = "JsonObject | null")]
    pub graph: Value,
    #[ts(type = "JsonObject | null")]
    pub runtime: Value,
    #[ts(type = "Json")]
    pub initial_input: Value,
    #[ts(type = "JsonObject | null")]
    pub source: Value,
    pub graph_nodes: Vec<GraphNode>,
    /// The lanes its agents ran on (or will run on), in graph order: one unless nodes differ.
    pub lanes: Vec<Lane>,
    pub nodes: BTreeMap<String, NodeStats>,
    pub executions: Vec<Execution>,
    pub usage: Usage,
    pub cost_usd: Option<f64>,
    pub cost_estimated: bool,
    pub failure: Option<Failure>,
    pub pressures: Vec<Pressure>,
    pub run_log: Vec<LogLine>,
    pub background_agents: Vec<BackgroundAgent>,
    /// Tokens of the graph's nodes only; `usage` is everything (nodes + background agents).
    pub node_usage: Usage,
    pub background_usage: Usage,
    /// Some of `usage` is live from a provider's session log, not yet counted by Zeroshot.
    pub usage_live: bool,
    /// A running execution's usage is not known until its turn ends.
    pub usage_pending: bool,
    pub cursor: u64,
    pub features: Features,
    pub local: Option<LocalFacts>,
    #[ts(type = "ReplayOf | null")]
    pub replay_of: Option<Value>,
    /// Successor (resume) and replays started from this run, and the run this resumed.
    pub resumed_from: Option<String>,
    pub resumed_as: Option<String>,
    pub replays: Vec<String>,
}
