use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::actions::{self, cli::timeout};
use crate::clock::now_ms;
use crate::discovery::{Target, TargetKind};
use crate::projection;
use crate::source::RunData;
use crate::validate;

use crate::server::args::{cli_args, valid_id, valid_run_id};
use crate::server::error::{ApiError, ApiResult};
use crate::server::state::AppState;

/// Configured sources (`TargetKind::Observe`: pb and the like) never accept actions from
/// zerochat: there is no controller to drive, and for an `observe`-policy source (pb) doing so
/// would disturb the sandbox and accounting that produced the run.
pub(crate) fn require_actions(target: &Target) -> ApiResult<()> {
    if target.kind != TargetKind::Observe {
        return Ok(());
    }
    let message = if target.actions_allowed {
        "zerochat does not act on configured sources yet; this one only observes runs."
    } else {
        "This is an observe-only source; actions are refused (it would disturb the sandbox and accounting that produced it)."
    };
    Err(ApiError::unavailable("actions_refused", message))
}

#[derive(Deserialize)]
pub(crate) struct SteerBody {
    pub(crate) execution: u64,
    pub(crate) message: String,
}

/// The largest message Zeroshot accepts (`zeroshot steer`: 262,144 bytes of UTF-8).
pub(crate) const MAX_STEER_BYTES: usize = 256 * 1024;

/// Sends a person's message to the agents of a running run (`zeroshot steer --mode once`).
/// With a Zeroshot that can steer one node (`steer_node`) it goes to the clicked execution's
/// node only; otherwise every agent conversation of the run receives it, which the composer
/// says before sending. The answer is Zeroshot's receipt: delivery happens later, at each
/// agent's next tool call or prompt, and shows in the chat as `steer_receipt` ledger events.
pub(crate) async fn steer(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
    Json(body): Json<SteerBody>,
) -> ApiResult<Json<Value>> {
    let target = state.target(&target_id)?;
    require_actions(&target)?;
    valid_run_id(&run_id)?;
    let message = body.message.trim();
    if message.is_empty() {
        return Err(ApiError::bad_request(
            "empty_message",
            "Type a message first.",
        ));
    }
    if message.len() > MAX_STEER_BYTES {
        return Err(ApiError::bad_request(
            "message_too_long",
            format!("Messages are limited to {} KiB.", MAX_STEER_BYTES / 1024),
        ));
    }
    if message
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
    {
        return Err(ApiError::bad_request(
            "invalid_message",
            "Messages cannot contain control characters (tabs and line breaks are fine).",
        ));
    }
    let caps = state.capabilities(&target).await;
    if !caps.iter().any(|c| c == "steer") {
        return Err(ApiError::unavailable(
            "steer_unsupported",
            "This Zeroshot cannot send messages to running agents: that needs Zeroshot 16.1 or later (`zeroshot steer`).",
        ));
    }
    let data = state.run(&target, &run_id).await?;
    refuse_older_ledger(&data, "send it messages")?;
    let view = state.project(&target, data);
    let execution = view
        .executions
        .iter()
        .find(|e| e.id == body.execution)
        .ok_or_else(|| {
            ApiError::not_found("execution_not_found", "This run has no such execution.")
        })?;
    let node = caps
        .iter()
        .any(|c| c == "steer_node")
        .then_some(execution.node.as_str());
    let dir = private_temp_dir("steer").map_err(|e| ApiError::internal(e.into()))?;
    let file = dir.join("message.txt");
    let written = write_private(&file, message.as_bytes());
    let result = match written {
        Ok(()) => {
            state
                .cli
                .json(
                    &actions::steer_args(&run_id, node, &file, target.cli_name.as_deref()),
                    timeout::STEER,
                )
                .await
        }
        Err(error) => Err(error.into()),
    };
    let _ = std::fs::remove_dir_all(&dir);
    let receipt = result.map_err(steer_error)?;
    let (steer_id, received) = actions::steer_receipt(&receipt).unwrap_or_default();
    Ok(Json(json!({
        "status": "sent",
        "scope": if node.is_some() { "node" } else { "run" },
        "node": node,
        "steerId": steer_id,
        "receivedExecutions": received,
    })))
}

/// Zeroshot's refusals of a steer, in words a person can act on (the CLI's own text otherwise).
pub(crate) fn steer_error(error: anyhow::Error) -> ApiError {
    let mut api = ApiError::from_cli(error);
    if let Some(text) = steer_refusal(&api.code, &api.message) {
        // Zeroshot refused this message for this run (16.1 says `kind: target`, `code:
        // invalid_request`): the request, not the gateway, is at fault.
        api.message = text.to_owned();
        api.status = StatusCode::BAD_REQUEST;
    }
    api
}

pub(crate) fn steer_refusal(code: &str, message: &str) -> Option<&'static str> {
    let m = message.to_ascii_lowercase();
    if m.contains("successor") {
        Some("This run was resumed as a newer run; message that one instead.")
    } else if m.contains("before its graph ends")
        || m.contains("finished")
        || m.contains("stopping")
    {
        Some("This run is no longer running, so no agent can receive a message.")
    } else if m.contains("control character") {
        Some("Messages cannot contain control characters (tabs and line breaks are fine).")
    } else if m.contains("steer node must name an agent") {
        Some("This node is not an agent of the run, so it cannot receive messages.")
    } else if m.contains("human") && m.contains("agent") {
        Some(
            "Zeroshot refuses messages from a process that an agent started; run zerochat yourself, outside any Zeroshot run.",
        )
    } else if code == "invalid_request" {
        Some("Zeroshot refused the message for this run.")
    } else {
        None
    }
}

/// Zeroshot 16.1 refuses to read a ledger written by the older patched Zeroshot (it rejects the
/// extra observe fields), so it can neither resume, stop nor message such a run. Refuse up front
/// with a reason, instead of Zeroshot's "run ledger storage is corrupt". Only a Zeroshot with
/// `steer` (16.1 and later) is this strict; an older one still drives its own runs.
pub(crate) fn refuse_older_ledger(data: &RunData, what: &str) -> ApiResult<()> {
    if projection::older_patched_ledger(&data.events) {
        return Err(ApiError::unavailable(
            "ledger_unreadable",
            format!(
                "This run was recorded by an older, patched Zeroshot whose ledger this Zeroshot cannot read, so zerochat cannot {what}. Use the Zeroshot that recorded it."
            ),
        ));
    }
    Ok(())
}

/// A fresh directory only this user can read, under the system temporary directory.
pub(crate) fn private_temp_dir(what: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "zerochat-{what}-{}-{}",
        std::process::id(),
        now_ms()
    ));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&dir)?;
    Ok(dir)
}

pub(crate) fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(path)?, bytes)
}

pub(crate) async fn stop(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
    Json(_): Json<Value>,
) -> ApiResult<Json<Value>> {
    let target = state.target(&target_id)?;
    require_actions(&target)?;
    valid_run_id(&run_id)?;
    refuse_older_ledger_on_16(&state, &target, &run_id, "stop it").await?;
    let args = cli_args(
        &["force-stop"],
        Vec::new(),
        target.cli_name.as_deref(),
        &[&run_id],
    );
    state
        .cli
        .json(&args, timeout::STOP)
        .await
        .map(Json)
        .map_err(ApiError::from_cli)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResumeBody {
    pub(crate) checkpoint_id: Option<String>,
}

pub(crate) async fn resume(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
    Json(body): Json<ResumeBody>,
) -> ApiResult<Json<Value>> {
    let target = state.target(&target_id)?;
    require_actions(&target)?;
    let checkpoint = body
        .checkpoint_id
        .as_deref()
        .map(|id| valid_id(id, "checkpoint id"))
        .transpose()?;
    valid_run_id(&run_id)?;
    refuse_older_ledger_on_16(&state, &target, &run_id, "resume it").await?;
    let args = actions::resume_args(&run_id, checkpoint, target.cli_name.as_deref());
    state
        .cli
        .json(&args, timeout::RESUME)
        .await
        .map(Json)
        .map_err(ApiError::from_cli)
}

/// [`refuse_older_ledger`] when the configured Zeroshot is 16.1 or later (it has `steer`).
pub(crate) async fn refuse_older_ledger_on_16(
    state: &AppState,
    target: &Target,
    run_id: &str,
    what: &str,
) -> ApiResult<()> {
    if !state
        .capabilities(target)
        .await
        .iter()
        .any(|c| c == "steer")
    {
        return Ok(());
    }
    match state.run(target, run_id).await {
        Ok(data) => refuse_older_ledger(&data, what),
        // Unreadable here: let Zeroshot answer for the run.
        Err(_) => Ok(()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReplayBody {
    pub(crate) checkpoint_id: String,
    pub(crate) graph: Option<Value>,
    pub(crate) runtime: Option<Value>,
    pub(crate) initial_input: Option<Value>,
    // Accepted and ignored: a resume keeps the run's title.
    #[allow(dead_code)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) validate_only: bool,
}

/// A replay is a resume from a checkpoint, unchanged: Zeroshot cannot replay a run with another
/// graph, runtime or input (only the older patched Zeroshot could), so a request that changes
/// any of them is refused, and the dialog does not offer it.
pub(crate) async fn replay(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
    Json(body): Json<ReplayBody>,
) -> ApiResult<Json<Value>> {
    let target = state.target(&target_id)?;
    require_actions(&target)?;
    valid_run_id(&run_id)?;
    valid_id(&body.checkpoint_id, "checkpoint id")?;
    if let Some(problem) = body.runtime.as_ref().and_then(runtime_problem) {
        return Err(ApiError::bad_request("invalid_runtime", problem));
    }
    if body.graph.is_some() || body.runtime.is_some() || body.initial_input.is_some() {
        return Err(ApiError::unavailable(
            "replay_unsupported",
            "Zeroshot resumes a checkpoint unchanged; changing the prompt, model or graph means starting a new run.",
        ));
    }
    if body.validate_only {
        return Ok(Json(json!({"valid": true, "mode": "resume"})));
    }
    resume(
        State(state),
        Path((target_id, run_id)),
        Json(ResumeBody {
            checkpoint_id: Some(body.checkpoint_id),
        }),
    )
    .await
}

/// Model, effort and harness names in a replacement runtime must be plain identifiers
/// (`validate::RUNTIME_NAME`): they reach the provider CLI's argv.
pub(crate) fn runtime_problem(runtime: &Value) -> Option<String> {
    pub(crate) const NAME_KEYS: [&str; 3] = ["model", "effort", "harness"];
    let bindings = runtime["nodes"].as_object().into_iter().flatten();
    let mut names = bindings.flat_map(|(node, binding)| {
        NAME_KEYS
            .into_iter()
            .filter_map(move |key| Some((node, key, binding.get(key)?.as_str()?)))
    });
    names.find_map(|(node, key, value)| {
        if validate::RUNTIME_NAME.accepts(value) {
            return None;
        }
        Some(format!(
            "The {key} for {node} is not a valid name: {value:?}."
        ))
    })
}

/// The keys `zeroshot run --graph` accepts. The admitted graph also carries keys admission
/// computed (such as `bounds`), which a replacement graph must not repeat.
// Kept, with its test, for replay with changes should Zeroshot gain it again (see `replay`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const GRAPH_KEYS: [&str; 4] = ["profile", "initialInput", "policy", "root"];

#[cfg_attr(not(test), allow(dead_code))]
pub fn submission_graph(graph: Value) -> Value {
    match graph {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(k, _)| GRAPH_KEYS.contains(&k.as_str()))
                .collect(),
        ),
        other => other,
    }
}
