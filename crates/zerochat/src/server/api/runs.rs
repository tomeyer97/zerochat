//! `GET /api/runs/{target}/{run}` (the run view) and `.../events` (the live change stream).

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::Json;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures::stream::{Stream, StreamExt};
use serde_json::{Value, json};

use crate::discovery::Target;
use crate::source::RunData;

use crate::server::cost::benchmark_cost;
use crate::server::error::{ApiError, ApiResult};
use crate::server::state::AppState;

/// How often the change stream reads the run again.
const SSE_POLL: Duration = Duration::from_millis(700);
/// How often the change stream sends a keep-alive comment when nothing changed.
const SSE_KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The run view, plus what the run page needs around it: the target's capabilities, kind and
/// name, and the run's context (pb/zsf). A benchmark run shows pb's cost (`cost` module).
pub(crate) async fn run_view(
    State(state): State<Arc<AppState>>,
    Path((target, run)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let target = state.target(&target)?;
    let data = state.run(&target, &run).await?;
    let view = state.project(&target, data);
    let capabilities = state.capabilities(&target).await;
    let context = state.context(&run).await;
    let mut value = serde_json::to_value(&view).map_err(|e| ApiError::internal(e.into()))?;
    value["capabilities"] = json!(capabilities);
    value["targetKind"] = json!(target.kind);
    value["targetName"] = json!(target.name);
    if let Some(context) = &context {
        benchmark_cost(&mut value, context);
    }
    value["context"] = json!(context);
    Ok(Json(value))
}

/// `changed` whenever the run has new events (or its controller state changes), so the page
/// refetches; `ping` every few seconds otherwise.
pub(crate) async fn run_events(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
) -> ApiResult<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>> {
    let target = state.target(&target_id)?;
    state.run(&target, &run_id).await?;
    let stream = run_change_stream(state, target, run_id);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(SSE_KEEP_ALIVE)))
}

/// What, when it moves, makes the page refetch the run.
#[derive(PartialEq)]
struct ChangeMarker {
    /// The last event's sequence number.
    cursor: u64,
    phase: String,
    controller_alive: bool,
}

impl ChangeMarker {
    fn of(data: &RunData) -> Self {
        Self {
            cursor: data.events.last().map(|e| e.seq).unwrap_or(0),
            phase: data.snapshot["phase"].as_str().unwrap_or("").to_owned(),
            controller_alive: data
                .local
                .as_ref()
                .is_none_or(|l| l.controller_liveness.is_alive()),
        }
    }
}

/// Reads the run every `SSE_POLL` until the page goes away, and sends `changed` when its
/// `ChangeMarker` moved, or `history_error` when it cannot be read.
fn run_change_stream(
    state: Arc<AppState>,
    target: Target,
    run_id: String,
) -> impl Stream<Item = Result<SseEvent, Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<SseEvent>(4);
    tokio::spawn(async move {
        let mut last: Option<ChangeMarker> = None;
        loop {
            let event = match state.run(&target, &run_id).await {
                Ok(data) => changed_event(&mut last, ChangeMarker::of(&data)),
                Err(error) => Some(
                    SseEvent::default()
                        .event("history_error")
                        .data(json!({"code": error.code, "message": error.message}).to_string()),
                ),
            };
            if let Some(event) = event {
                if tx.send(event).await.is_err() {
                    return;
                }
            } else if tx.is_closed() {
                return;
            }
            tokio::time::sleep(SSE_POLL).await;
        }
    });
    tokio_stream::wrappers::ReceiverStream::new(rx).map(Ok)
}

/// `changed` when `marker` differs from the `last` one sent (and remembers it), else nothing.
fn changed_event(last: &mut Option<ChangeMarker>, marker: ChangeMarker) -> Option<SseEvent> {
    if last.as_ref() == Some(&marker) {
        return None;
    }
    let event = SseEvent::default()
        .event("changed")
        .data(json!({"cursor": marker.cursor, "phase": marker.phase}).to_string());
    *last = Some(marker);
    Some(event)
}
