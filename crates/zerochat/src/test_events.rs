//! Ledger events for tests, in the shape Zeroshot 16.1 writes them: the one vocabulary the
//! projection, transcript and source tests build runs from. When the ledger's shape changes,
//! this is the place to follow it.

use serde_json::{Value, json};

use crate::source::Event;

/// Any event, from its body.
pub fn event(seq: u64, body: Value) -> Event {
    Event { seq, body }
}

/// `node_started`: an execution of `node` (instance 1, no map indices) begins its `attempt`.
pub fn started(seq: u64, execution: u64, node: &str, attempt: u64) -> Event {
    event(
        seq,
        json!({
            "kind": "node_started",
            "reference": {"node": node, "nodeInstance": 1, "execution": execution},
            "occurrence": {"node": node, "mapIndices": []},
            "attempt": attempt,
            "input": {"task": "t"},
        }),
    )
}

/// `node_completed`: the execution ends with `outcome` (see `verified`, `failed`).
pub fn completed(seq: u64, execution: u64, node: &str, outcome: Value) -> Event {
    event(
        seq,
        json!({
            "kind": "node_completed",
            "completion": {
                "reference": {"node": node, "nodeInstance": 1, "execution": execution},
                "outcome": outcome,
            },
        }),
    )
}

/// `safe_log`: one log line at `at` (Unix ms); `execution` is `None` for a run-level line.
pub fn log(seq: u64, execution: Option<u64>, at: u64, stream: &str, line: &str) -> Event {
    event(
        seq,
        json!({"kind": "safe_log", "execution": execution, "timestamp": at, "stream": stream, "line": line}),
    )
}

/// `terminal`: Zeroshot closed the run with `status` for `reason`.
pub fn terminal(seq: u64, status: &str, reason: &str) -> Event {
    event(
        seq,
        json!({"kind": "terminal", "result": {"status": status, "reason": reason}}),
    )
}

/// A step's outcome when it finished its work.
pub fn verified() -> Value {
    json!({"status": "verified", "output": null, "artifacts": []})
}

/// An execution's outcome when it failed with Zeroshot's error `code` (`crash`, `refusal`,
/// `timeout`, …) and the node declared the failure itself.
pub fn failed(code: &str) -> Value {
    json!({"status": "error", "code": code, "reason": "declared_failure"})
}
