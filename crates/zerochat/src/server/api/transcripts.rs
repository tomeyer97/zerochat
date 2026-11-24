//! One execution's transcript. The ledger always has one; for a local run whose agent ran with
//! Claude Code or Copilot, the harness's own session log is richer (every tool call and reply)
//! and is shown instead, merged with what only the ledger knows (steers, retries, rate limits).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::Json;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::clock::now_ms;
use crate::discovery::{Target, TargetKind};
use crate::harness_name::Harness;
use crate::projection::Execution;
use crate::source::RunData;
use crate::transcript::{self, Transcript};

use crate::server::error::{ApiError, ApiResult};
use crate::server::state::AppState;

#[derive(Deserialize)]
pub(crate) struct TranscriptQuery {
    #[serde(default)]
    source: Option<String>,
}

pub(crate) async fn run_transcript(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id, execution)): Path<(String, String, u64)>,
    Query(query): Query<TranscriptQuery>,
) -> ApiResult<Json<Value>> {
    let target = state.target(&target_id)?;
    let data = state.run(&target, &run_id).await?;
    let view = state.project(&target, data.clone());
    let execution_view = view
        .executions
        .iter()
        .find(|e| e.id == execution)
        .ok_or_else(|| {
            ApiError::not_found("execution_not_found", "This run has no such execution.")
        })?;
    let instructions = view
        .graph_nodes
        .iter()
        .find(|n| n.name == execution_view.node)
        .and_then(|n| n.instructions.clone());
    let ledger = transcript::from_ledger(&data.events, execution);

    let native = match NativeLog::of(&target, execution_view) {
        Some(log) if query.source.as_deref() != Some("ledger") => log.read(
            &state.home,
            &data,
            execution_view,
            instructions.as_deref(),
            &ledger,
        ),
        _ => None,
    };
    let (result, source_note) = match native {
        Some(Ok(mut native)) => {
            transcript::merge_native_with_ledger(&mut native, ledger.items);
            (native, None)
        }
        // Why the richer source was not used; shown in Details, not in the chat.
        Some(Err(note)) => (ledger, Some(note)),
        None => (ledger, None),
    };
    Ok(Json(json!({
        "execution": execution_view,
        "instructions": instructions,
        "transcript": result,
        "sourceNote": source_note,
    })))
}

/// A harness whose own session log, on this machine, can replace the ledger transcript.
#[derive(Clone, Copy)]
enum NativeLog {
    Claude,
    Copilot,
}

impl NativeLog {
    /// The session log to look for: only for a local run, and only Claude Code and Copilot.
    fn of(target: &Target, execution: &Execution) -> Option<NativeLog> {
        if target.kind != TargetKind::Local {
            return None;
        }
        match Harness::of(execution.harness.as_deref())? {
            Harness::Claude => Some(NativeLog::Claude),
            Harness::Copilot => Some(NativeLog::Copilot),
            Harness::Codex => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            NativeLog::Claude => "Claude",
            NativeLog::Copilot => "Copilot",
        }
    }

    /// The execution's transcript from this harness's session log, or why it is not available
    /// (None: nothing to say, the ledger is simply what there is).
    fn read(
        self,
        home: &std::path::Path,
        data: &RunData,
        execution: &Execution,
        instructions: Option<&str>,
        ledger: &Transcript,
    ) -> Option<Result<Transcript, String>> {
        let local = data.local.as_ref();
        let workspace = local.and_then(|l| l.workspace.as_deref());
        // Still running: its log is matched up to now.
        let ended_at = execution.ended_at.or(Some(now_ms()));
        let session_id = execution.session_id.as_deref();
        // The messages people sent this execution (16.1 `vector` steers, or the older patched
        // Zeroshot's `logKind: "steer"` lines): a native session log shows them as user messages.
        let steers = transcript::steer_texts(ledger);
        // The session log found, and read.
        let found = match self {
            // Claude Code files its sessions by working directory.
            NativeLog::Claude => transcript::find_claude_session_of(
                home,
                workspace?,
                session_id,
                execution.started_at,
                ended_at,
                instructions,
            )
            .map(|path| transcript::from_claude_session(&path, &steers)),
            NativeLog::Copilot => {
                let retained = local.map(|l| {
                    l.run_dir
                        .join("transcripts")
                        .join(format!("execution-{}", execution.id))
                });
                // A resumed Copilot session's log also holds the earlier executions' turns.
                let since = execution.started_at.map(|t| t.saturating_sub(5_000));
                transcript::find_copilot_session(
                    &transcript::copilot_home(home),
                    retained.as_deref(),
                    workspace,
                    session_id,
                    execution.started_at,
                    ended_at,
                )
                .map(|path| transcript::from_copilot_session(&path, &steers, since))
            }
        };
        let name = self.name();
        Some(match found {
            Some(read) => {
                read.map_err(|error| format!("{name}'s session log could not be read: {error}"))
            }
            None if session_id.is_some() => Err(format!(
                "{name}'s session log for this node is not on this machine."
            )),
            None => Err(format!(
                "{name}'s session log could not be matched to this node (this Zeroshot does not record the session id)."
            )),
        })
    }
}
