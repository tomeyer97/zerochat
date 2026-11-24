//! `project` on whole runs: the ledger events of a small graph in, the run view out. The events
//! come from the shared builders in crate::test_events.

use serde_json::{Value, json};

use super::*;
use crate::engine_lines::reported_duration;
use crate::pricing::Usage;
use crate::source::{Liveness, LocalFacts};
use crate::test_events::{completed, event, failed, log, started, terminal, verified};

/// The graph every test runs: a loop of at most 2 rounds in which `worker` (1 attempt) does the
/// work and `review` (a verifier, 2 attempts, 60s timeout) accepts or rejects it; when the
/// rounds run out, the run fails at `change_attempts_exhausted`.
fn graph() -> Value {
    json!({
        "root": {
            "kind": "seq",
            "name": "run",
            "children": [
                {
                    "kind": "loop",
                    "name": "change_loop",
                    "maxIterations": 2,
                    "body": {
                        "kind": "seq",
                        "name": "it",
                        "children": [
                            {
                                "kind": "step",
                                "name": "worker",
                                "worker": "builtin.agent.worker@1",
                                "instructions": "Do it",
                                "attempts": 1,
                            },
                            {
                                "kind": "verifier",
                                "name": "review",
                                "worker": "builtin.agent.reviewer@1",
                                "attempts": 2,
                                "timeoutMs": 60000,
                            },
                        ],
                    },
                },
                {"kind": "fail", "name": "change_attempts_exhausted", "reason": "change_attempts_exhausted"},
            ],
        },
    })
}

/// A reviewer's outcome that rejects the work, saying why.
fn rejected() -> Value {
    json!({
        "status": "verifier",
        "output": null,
        "signals": {"verdict": "rejected"},
        "diagnostic": {"message": "missing tests"},
        "artifacts": [],
    })
}

/// A minimal admitted run of `graph()`: both nodes on `claude-haiku-4-5` with the Claude
/// harness on Anthropic (the run's default lane), Zeroshot's snapshot saying `phase` (and `terminal`, when it has one), and these
/// events.
fn data(events: Vec<Event>, phase: &str, terminal: Value, local: Option<LocalFacts>) -> RunData {
    RunData {
        run_id: "01a1a5e8-c91b-7073-b3e9-fdbd462600af".into(),
        title: "t".into(),
        admitted: json!({
            "graph": graph(),
            "runtime": {
                "harness": "claude",
                "provider": "anthropic",
                "nodes": {
                    "worker": {"model": "claude-haiku-4-5"},
                    "review": {"model": "claude-haiku-4-5"},
                },
            },
        }),
        snapshot: json!({"phase": phase, "terminal": terminal}),
        events,
        local,
        live_usage: Default::default(),
    }
}

/// Two rounds of work, each rejected by the review: the loop runs out.
fn exhausted_run() -> RunData {
    let events = vec![
        log(1, None, 1_000, "system", "Preparing execution environment"),
        event(2, json!({"kind": "run_started"})),
        started(3, 1, "worker", 1),
        log(4, Some(1), 2_000, "output", "working"),
        event(
            5,
            json!({"kind": "token_usage_observed", "execution": 1, "usage": {"inputTokens": 1000, "outputTokens": 1000}}),
        ),
        completed(6, 1, "worker", verified()),
        started(7, 2, "review", 1),
        log(8, Some(2), 3_000, "output", "reviewing"),
        completed(9, 2, "review", rejected()),
        started(10, 3, "worker", 1),
        log(11, Some(3), 4_000, "output", "again"),
        completed(12, 3, "worker", verified()),
        started(13, 4, "review", 1),
        log(14, Some(4), 5_000, "output", "still no"),
        completed(15, 4, "review", rejected()),
        terminal(16, "failed", "change_attempts_exhausted"),
    ];
    data(events, "finished", Value::Null, None)
}

#[test]
fn rounds_verdicts_and_work_failure() {
    let view = project("local", &exhausted_run(), 10_000);
    assert_eq!(view.status, "failed");
    let rounds: Vec<_> = view
        .executions
        .iter()
        .map(|e| (e.node.as_str(), e.round, state_name(e.state)))
        .collect();
    assert_eq!(
        rounds,
        [
            ("worker", 1, "succeeded"),
            ("review", 1, "rejected"),
            ("worker", 2, "succeeded"),
            ("review", 2, "rejected")
        ]
    );
    let failure = view.failure.unwrap();
    assert_eq!(failure.class, "work");
    assert_eq!(failure.node.as_deref(), Some("review"));
    assert!(failure.title.contains("2 of 2 rounds"), "{}", failure.title);
    assert_eq!(failure.cause, "not accepted after 2 rounds");
    assert_eq!(failure.feedback.len(), 2);
    assert_eq!(
        (failure.feedback[0].round, failure.feedback[0].text.as_str()),
        (2, "missing tests")
    );
    // F09: a replay reruns the same reviewer; the fix in the graph leads, not a replay.
    assert_eq!(failure.primary, None);
    assert_eq!(
        failure.fix.as_deref(),
        Some(
            "Raise `maxIterations` on `change_loop` in your graph (now 2), or change what `review` asks for; then start a new run."
        )
    );
    assert_eq!(failure.into_run_ms, Some(4_000));
    assert!(view.executions[0].cost_usd.unwrap() > 0.0);
    assert_eq!(view.executions[0].cost_source, Some(CostSource::Estimate));
    assert_eq!(view.nodes["review"].rounds, 2);
}

#[test]
fn retries_share_a_round() {
    let events = vec![
        started(1, 1, "review", 1),
        log(2, Some(1), 1_000, "error", "boom"),
        completed(3, 1, "review", failed("crash")),
        started(4, 2, "review", 2),
    ];
    let view = project("local", &data(events, "running", Value::Null, None), 2_000);
    assert_eq!(view.executions[1].round, 1);
    assert_eq!(view.executions[1].attempt, 2);
    assert!(
        view.pressures
            .iter()
            .any(|p| p.kind == PressureKind::Attempts && p.text == "last attempt (2 of 2)")
    );
}

#[test]
fn stalled_when_controller_is_gone() {
    let events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 1_000, "output", "hi"),
    ];
    let local = LocalFacts {
        controller_pid: Some(1),
        controller_alive: false,
        has_checkpoints: true,
        ..Default::default()
    };
    let view = project(
        "local",
        &data(events, "running", Value::Null, Some(local)),
        5_000,
    );
    assert_eq!(view.status, "stalled");
    assert_eq!(view.executions[0].state, ExecState::Interrupted);
    let failure = view.failure.unwrap();
    assert_eq!(failure.class, "orchestration");
    assert_eq!(failure.cause, "controller lost");
    // One name for this state everywhere: the sidebar's cause and the card heading now agree,
    // and neither says "stopped" (that word is reserved for a deliberate force stop).
    assert_eq!(failure.title, "Controller lost");
    assert!(
        failure.detail.is_empty(),
        "no reassurance sentence: {}",
        failure.detail
    );
    assert!(
        view.pressures.is_empty(),
        "a stalled run has no live limits"
    );
    assert!(
        !failure.actions.contains(&Action::Replay),
        "Zeroshot refuses to replay an unfinished run: {:?}",
        failure.actions
    );
}

/// A run whose controller runs in a sandbox elsewhere (pb, the ProgramBench runner, runs it in a
/// Docker container): its pid is not in this machine's process table, which proves nothing. Liveness is `unknown`, and the run reads as Zeroshot
/// recorded it (running) — not stalled, no "controller lost", no Resume.
#[test]
fn a_controller_elsewhere_is_unknown_not_lost() {
    let events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 1_000, "output", "hi"),
    ];
    let mut local = LocalFacts {
        controller_pid: Some(77),
        controller_alive: false,
        controller_liveness: Liveness::Dead,
        has_checkpoints: true,
        ..Default::default()
    };
    local.agent_processes = vec![crate::source::AgentProcess {
        pid: 4242,
        name: "claude".into(),
    }];
    local.controller_elsewhere();
    assert_eq!(
        (local.controller_liveness, local.controller_alive),
        (Liveness::Unknown, true)
    );
    assert!(local.agent_processes.is_empty());
    let view = project(
        "sandboxed",
        &data(events, "running", Value::Null, Some(local)),
        5_000,
    );
    assert_eq!(view.status, "running");
    assert!(
        view.failure.is_none(),
        "{:?}",
        view.failure.map(|f| f.cause)
    );
    assert_eq!(view.executions[0].state, ExecState::Running);

    // One this machine does see alive stays alive.
    let mut seen = LocalFacts {
        controller_pid: Some(77),
        controller_alive: true,
        controller_liveness: Liveness::Alive,
        ..Default::default()
    };
    seen.controller_elsewhere();
    assert_eq!(seen.controller_liveness, Liveness::Alive);
}

#[test]
fn stalled_run_with_an_orphaned_agent_says_so() {
    let events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 1_000, "output", "hi"),
    ];
    let local = LocalFacts {
        controller_pid: Some(1),
        controller_alive: false,
        agent_processes: vec![crate::source::AgentProcess {
            pid: 4242,
            name: "claude".into(),
        }],
        ..Default::default()
    };
    let view = project(
        "local",
        &data(events, "running", Value::Null, Some(local)),
        5_000,
    );
    let failure = view.failure.unwrap();
    assert!(failure.detail.contains("pid 4242"), "{}", failure.detail);
    assert!(!failure.detail.contains("Nothing"), "{}", failure.detail);
}

#[test]
fn live_timeout_pressure() {
    let events = vec![
        started(1, 1, "review", 1),
        log(2, Some(1), 1_000, "output", "hi"),
    ];
    let view = project("local", &data(events, "running", Value::Null, None), 55_000);
    let timeout = view
        .pressures
        .iter()
        .find(|p| p.kind == PressureKind::Timeout)
        .unwrap();
    assert_eq!(timeout.level, PressureLevel::Warning);
    assert!(
        timeout.text.starts_with("timeout in 6s"),
        "{}",
        timeout.text
    );
    let early = project(
        "local",
        &data(
            vec![
                started(1, 1, "review", 1),
                log(2, Some(1), 1_000, "output", "hi"),
            ],
            "running",
            Value::Null,
            None,
        ),
        5_000,
    );
    assert!(
        early.pressures.is_empty(),
        "a timeout far away is not under pressure"
    );
    assert_eq!(view.executions[0].duration_ms, Some(54_000));
}

#[test]
fn crash_error_output_drops_the_wrapper_and_the_enum_line() {
    // The enum summary line only repeats the title tooltip, and the provider's own
    // sentence is wrapped in generic "Node x failed: crash after Ys: ..." context.
    let events = vec![
        started(1, 1, "worker", 1),
        log(
            2,
            Some(1),
            1_000,
            "error",
            "Zeroshot reason worker_failed · node outcome crash: declared_failure",
        ),
        log(
            3,
            Some(1),
            1_000,
            "error",
            "Node worker failed: crash after 184.2s: node execution failed: Claude provider failure: You've hit your session limit · resets 2:03pm (UTC); provider process exited with status 1",
        ),
        completed(4, 1, "worker", failed("crash")),
        terminal(5, "failed", "worker_failed"),
    ];
    let view = project(
        "local",
        &data(events, "finished", Value::Null, None),
        10_000,
    );
    let failure = view.failure.unwrap();
    assert_eq!(
        failure.last_errors,
        vec!["You've hit your session limit · resets 2:03pm (UTC)".to_owned()]
    );
}

#[test]
fn malformed_stream_crash_names_the_cause_in_the_title() {
    // A malformed API stream must not read as a generic "crashed" / "exited without a
    // result"; the headline carries the cause, and the raw stream-debug dump is dropped.
    let events = vec![
        started(1, 1, "worker", 1),
        log(
            2,
            Some(1),
            1_000,
            "error",
            "Node worker failed: crash after 33.2s: node execution failed: API Error: API returned an empty or malformed response (HTTP 200) from the provider. From chunk: [ \"event: content_block_delta\" ]",
        ),
        completed(3, 1, "worker", failed("crash")),
        terminal(4, "failed", "worker_failed"),
    ];
    let view = project(
        "local",
        &data(events, "finished", Value::Null, None),
        10_000,
    );
    let failure = view.failure.unwrap();
    // F12: the provider's fault, named, with Resume as the next step.
    assert_eq!(failure.class, "provider");
    assert_eq!(failure.title, "Anthropic sent worker a malformed response");
    assert_eq!(failure.primary, Some(Action::Resume));
    assert_eq!(
        failure.last_errors,
        vec![
            "API Error: API returned an empty or malformed response (HTTP 200) from the provider"
                .to_owned()
        ]
    );
}

#[test]
fn reported_durations() {
    assert_eq!(
        reported_duration("Node worker failed: timeout after 45.4s: node timed out"),
        Some(45_400)
    );
    assert_eq!(
        reported_duration("Node code failed: crash after 950ms"),
        Some(950)
    );
    assert_eq!(reported_duration("Run failed: worker_failed"), None);
}

/// A worker that fails on an unknown model, and a run Zeroshot closes only a minute later:
/// - 1 000: the run starts (environment prepared);
/// - 11 000: the worker's first output (its approximate start, 5.4s before it failed);
/// - 12 000: the worker fails (`crash after 5.4s`), and the graph ends;
/// - 12 100 to 70 000: Active Memory, Zeroshot's own memory agent, runs its postmortem;
/// - 72 000: Zeroshot closes the run, 60s after the failure.
fn unknown_model_run() -> RunData {
    let error = "Node worker failed: crash after 5.4s: node execution failed: Claude provider failure: There's an issue with the selected model (claude-haiku-9-9). It may not exist";
    let events = vec![
        log(1, None, 1_000, "system", "Preparing execution environment"),
        started(2, 1, "worker", 1),
        log(
            3,
            Some(1),
            11_000,
            "output",
            "There's an issue with the selected model (claude-haiku-9-9).",
        ),
        log(4, Some(1), 12_000, "error", error),
        completed(5, 1, "worker", failed("crash")),
        event(
            6,
            json!({"kind": "primary_graph_completed", "result": {"status": "failed", "reason": "worker_failed"}}),
        ),
        log(
            7,
            None,
            12_100,
            "system",
            "Active Memory postmortem started",
        ),
        log(
            8,
            None,
            70_000,
            "system",
            "Active Memory postmortem completed",
        ),
        log(9, None, 72_000, "error", "Run failed: worker_failed"),
        terminal(10, "failed", "worker_failed"),
    ];
    data(events, "finished", Value::Null, None)
}

#[test]
fn the_run_log_strips_fences_and_structured_output_dumps() {
    // Raw markdown fences, the StructuredOutput call/result envelope, and a
    // below-threshold rate-limit ping are not run-timeline facts.
    let events = vec![
        log(
            1,
            None,
            1_000,
            "system",
            "[a_graph] ```json\n{\"lessons\":[]}\n```",
        ),
        log(
            2,
            None,
            1_100,
            "system",
            "[a_graph] {\"id\":\"t1\",\"name\":\"StructuredOutput\",\"input\":{\"response\":{}}}",
        ),
        log(
            3,
            None,
            1_200,
            "system",
            "[a_graph] {\"toolUseId\":\"t1\",\"content\":\"Structured output provided successfully\",\"isError\":false}",
        ),
        log(
            4,
            None,
            1_300,
            "system",
            "[a_graph] Claude rate limit: status=allowed_warning, type=seven_day, utilization=10%, resetsAt=2026-11-11T02:00:00Z",
        ),
    ];
    let view = project("local", &data(events, "running", Value::Null, None), 2_000);
    let lines: Vec<&str> = view.run_log.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(lines, ["{\"lessons\":[]}", "reported its result"]);
}

#[test]
fn the_run_log_has_node_lifecycle_lines() {
    // "worker started" / "worker failed" belong in the run's own timeline, not only
    // the background postmortem agents.
    let view = project("local", &unknown_model_run(), 100_000);
    let texts: Vec<&str> = view.run_log.iter().map(|l| l.text.as_str()).collect();
    assert!(
        texts.iter().any(|t| t.starts_with("worker started")),
        "{texts:?}"
    );
    assert!(texts.contains(&"worker failed: crashed"), "{texts:?}");
    // Node lifecycle lines are not attributed to a background agent.
    assert!(
        view.run_log
            .iter()
            .filter(|l| l.text.starts_with("worker "))
            .all(|l| l.agent.is_none())
    );
}

#[test]
fn one_duration_and_the_time_the_node_failed() {
    let view = project("local", &unknown_model_run(), 100_000);
    let worker = &view.executions[0];
    assert_eq!(
        worker.duration_ms,
        Some(5_400),
        "Zeroshot's own duration wins over inferred times"
    );
    assert_eq!(worker.ended_at, Some(12_000));
    let failure = view.failure.unwrap();
    assert_eq!(failure.class, "setup");
    assert_eq!(failure.cause, "unknown model");
    assert_eq!(
        failure.title, "Unknown model claude-haiku-9-9",
        "the exact id, to search the config for"
    );
    // F09: a replay reruns the unknown model; the fix leads (what to change, where).
    assert_eq!(failure.primary, None);
    assert_eq!(
        failure.fix.as_deref(),
        Some(
            "Set a model Anthropic knows for `worker` in your runtime file, then start a new run."
        )
    );
    assert!(
        !failure.actions.contains(&Action::Resume),
        "resuming reuses the unknown model"
    );
    assert_eq!(failure.at, Some(12_000));
    assert_eq!(failure.into_run_ms, Some(11_000));
    assert_eq!(failure.closed_after_ms, Some(60_000));
    assert!(failure.closed_by_postmortem);
}

#[test]
fn force_stopped_run_is_stopped_not_refused() {
    let events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 1_000, "output", "working"),
        event(3, json!({"kind": "force_stop_requested"})),
        log(
            4,
            Some(1),
            2_000,
            "error",
            "Node worker failed: refusal after 1.0s",
        ),
        completed(5, 1, "worker", failed("refusal")),
        terminal(6, "failed", "force_stopped"),
    ];
    let view = project("local", &data(events, "finished", Value::Null, None), 3_000);
    assert_eq!(view.status, "stopped");
    assert_eq!(view.executions[0].state, ExecState::Interrupted);
    let failure = view.failure.unwrap();
    assert_eq!(
        (failure.class, failure.cause.as_str()),
        ("stopped", "stopped")
    );
    assert!(failure.last_errors.iter().all(|l| !l.contains("refusal")));
}

#[test]
fn a_resumed_run_keeps_its_own_failure() {
    let events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 1_000, "output", "hi"),
        log(3, Some(1), 90_000, "output", "last words"),
        log(4, None, 700_000, "error", "Run failed: runtime_lost"),
        terminal(5, "failed", "runtime_lost"),
    ];
    let local = LocalFacts {
        resumed_as: Some("01a1a657-eeb1-7000-8000-000000000000".into()),
        has_checkpoints: true,
        ..Default::default()
    };
    let view = project(
        "local",
        &data(events, "finished", Value::Null, Some(local)),
        800_000,
    );
    assert_eq!(
        view.ended_at,
        Some(90_000),
        "elapsed ends when the run was last active, not at the resume"
    );
    let failure = view.failure.unwrap();
    assert_eq!(failure.cause, "controller lost");
    assert_eq!(failure.at, Some(90_000));
    assert!(
        !failure.actions.contains(&Action::Resume),
        "already resumed"
    );
    assert_eq!(
        view.resumed_as.as_deref(),
        Some("01a1a657-eeb1-7000-8000-000000000000")
    );
}

#[test]
fn live_usage_from_the_session_log_until_the_ledger_has_it() {
    let events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 1_000, "output", "hi"),
    ];
    let mut run = data(events, "running", Value::Null, None);
    let view = project("local", &run, 2_000);
    assert_eq!(view.executions[0].usage_source, UsageSource::Pending);
    assert!(view.usage_pending);
    run.live_usage.insert(
        1,
        Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_read_input_tokens: 1000,
            cache_creation_input_tokens: 0,
        },
    );
    let view = project("local", &run, 2_000);
    assert_eq!(view.executions[0].usage_source, UsageSource::SessionLog);
    assert_eq!(view.usage.total(), 1030);
    assert!(view.usage_live && view.cost_usd.is_some());
}

#[test]
fn rate_limit_notices_become_pressures() {
    let events = vec![
        started(1, 1, "worker", 1),
        log(
            2,
            Some(1),
            1_000,
            "system",
            "Claude rate limit: status=rejected, type=five_hour, resetsAt=2026-11-04T12:00:00Z",
        ),
    ];
    let view = project("local", &data(events, "running", Value::Null, None), 2_000);
    let limit = view
        .pressures
        .iter()
        .find(|p| p.kind == PressureKind::RateLimit)
        .unwrap();
    assert_eq!(limit.level, PressureLevel::Critical);
    assert_eq!(limit.text, "5-hour limit hit");
    assert_eq!(limit.resets_at, Some(1_793_793_600_000));
}

#[test]
fn healthy_limits_are_not_shown_and_a_shared_loop_round_shows_once() {
    let events = vec![
        started(1, 1, "worker", 1),
        log(
            2,
            Some(1),
            1_000,
            "system",
            "Claude rate limit: status=allowed, type=seven_day, resetsAt=2026-11-11T02:00:00Z",
        ),
    ];
    let view = project("local", &data(events, "running", Value::Null, None), 2_000);
    assert!(view.pressures.is_empty(), "{:?}", view.pressures);
}

#[test]
fn a_successful_call_clears_its_retry_pressure() {
    // A survivable retry is not a pressing limit once the agent is working again.
    let retry = log(
        2,
        Some(1),
        1_000,
        "system",
        "Claude API retry 2/10: rate_limit",
    );
    let still_retrying = project(
        "local",
        &data(
            vec![started(1, 1, "worker", 1), retry.clone()],
            "running",
            Value::Null,
            None,
        ),
        2_000,
    );
    assert!(
        still_retrying
            .pressures
            .iter()
            .any(|p| p.kind == PressureKind::Retry),
        "{:?}",
        still_retrying.pressures
    );

    let success = event(
        3,
        json!({"kind": "token_usage_observed", "execution": 1, "usage": {"inputTokens": 10, "outputTokens": 10}}),
    );
    let resolved = project(
        "local",
        &data(
            vec![started(1, 1, "worker", 1), retry, success],
            "running",
            Value::Null,
            None,
        ),
        2_000,
    );
    assert!(
        !resolved
            .pressures
            .iter()
            .any(|p| p.kind == PressureKind::Retry),
        "{:?}",
        resolved.pressures
    );
}

#[test]
fn a_tool_call_after_a_retry_clears_its_pressure_too() {
    // `token_usage_observed` only arrives once per turn, so a long turn with many tool
    // calls kept showing "API retry … /10" the whole time even though the agent was visibly
    // working again. Any real activity (a tool call, not just the final usage tally) clears it.
    let retry = log(
        2,
        Some(1),
        1_000,
        "system",
        "Claude API retry 2/10: rate_limit",
    );
    let tool_use = event(
        3,
        json!({"kind": "safe_log", "execution": 1, "timestamp": 2_000, "stream": "output", "logKind": "tool_use", "line": "{\"id\":\"t1\",\"name\":\"Read\",\"input\":{}}"}),
    );
    let view = project(
        "local",
        &data(
            vec![started(1, 1, "worker", 1), retry, tool_use],
            "running",
            Value::Null,
            None,
        ),
        3_000,
    );
    assert!(
        !view.pressures.iter().any(|p| p.kind == PressureKind::Retry),
        "{:?}",
        view.pressures
    );
}

#[test]
fn retry_pressure_text_is_humanised() {
    // The raw engine enum ("Claude API retry 2/10: rate_limit") is not shown as is.
    let retry = log(
        2,
        Some(1),
        1_000,
        "system",
        "Claude API retry 2/10: rate_limit",
    );
    let view = project(
        "local",
        &data(
            vec![started(1, 1, "worker", 1), retry],
            "running",
            Value::Null,
            None,
        ),
        2_000,
    );
    let pressure = view
        .pressures
        .iter()
        .find(|p| p.kind == PressureKind::Retry)
        .unwrap();
    assert_eq!(pressure.text, "retried 2 of 10 (rate limit)");
}

#[test]
fn a_rate_limit_warning_is_amber_only_above_the_real_threshold() {
    // 33% used is account info, not something pressing on this run.
    let events = vec![
        started(1, 1, "worker", 1),
        log(
            2,
            Some(1),
            1_000,
            "system",
            "Claude rate limit: status=allowed_warning, type=seven_day, utilization=33%, resetsAt=2026-11-11T02:00:00Z",
        ),
    ];
    let low = project("local", &data(events, "running", Value::Null, None), 2_000);
    // `info`-level rate-limit pressures are filtered out before reaching the view (see below),
    // so a 33% warning shows nothing on the run page at all.
    assert!(
        !low.pressures
            .iter()
            .any(|p| p.kind == PressureKind::RateLimit),
        "{:?}",
        low.pressures
    );

    let events = vec![
        started(1, 1, "worker", 1),
        log(
            2,
            Some(1),
            1_000,
            "system",
            "Claude rate limit: status=allowed_warning, type=seven_day, utilization=93%, resetsAt=2026-11-11T02:00:00Z",
        ),
    ];
    let high = project("local", &data(events, "running", Value::Null, None), 2_000);
    let pressure = high
        .pressures
        .iter()
        .find(|p| p.kind == PressureKind::RateLimit)
        .unwrap();
    assert_eq!(pressure.level, PressureLevel::Warning);
    assert_eq!(pressure.text, "7-day limit: 93% used");
}

#[test]
fn background_agents_from_prefixed_lines() {
    let events = vec![
        log(
            1,
            None,
            1_000,
            "system",
            "[active_memory_graph] Claude API retry 1/10: authentication_failed",
        ),
        log(
            2,
            None,
            2_000,
            "system",
            "[active_memory_graph] Claude API retry 2/10: authentication_failed",
        ),
    ];
    let view = project("local", &data(events, "running", Value::Null, None), 3_000);
    assert_eq!(view.background_agents.len(), 1);
    assert_eq!(view.background_agents[0].retries, 2);
}

#[test]
fn verdict_reading() {
    assert_eq!(
        verdict_accepted(&json!({"verdict": "accepted"})),
        Some(true)
    );
    assert_eq!(
        verdict_accepted(&json!({"verdict": "accepted", "delivery": "conflict"})),
        Some(false)
    );
    assert_eq!(verdict_accepted(&json!({"kind": "weird"})), None);
}

#[test]
fn durations_read_naturally() {
    assert_eq!(human_ms(45_000), "45s");
    assert_eq!(human_ms(250_000), "4m 10s");
    assert_eq!(human_ms(1_800_000), "30m");
    assert_eq!(human_ms(3_600_000), "1h");
    assert_eq!(human_ms(7_500_000), "2h 05m");
}

/// Two background agents (`a_graph`, `a_repo`: names Zeroshot gives its Active Memory agents)
/// stream deltas that interleave.
#[test]
fn background_deltas_join_per_agent_in_time_order() {
    let events = vec![
        log(1, None, 1_000, "output", "[a_graph] The user is instruct"),
        log(2, None, 1_050, "output", "[a_repo] Checking"),
        log(3, None, 1_100, "output", "[a_graph] ing me."),
        log(4, None, 1_150, "output", "[a_repo]  the repo."),
        log(
            5,
            None,
            1_200,
            "system",
            "[a_graph] {\"harness\":\"claude\",\"sessionId\":\"66425ab0\"}",
        ),
        log(
            6,
            None,
            1_300,
            "system",
            "[a_repo] Claude rate limit: status=allowed, type=seven_day, resetsAt=2026-11-11T02:00:00Z",
        ),
        log(
            7,
            None,
            1_400,
            "output",
            "[a_repo] {\"content\":\"Structured output provided successfully\",\"isError\":false,\"toolUseId\":\"toolu_1\"}",
        ),
        log(8, None, 900, "system", "Preparing execution environment"),
        log(
            9,
            None,
            2_000,
            "system",
            "Active Memory a_repo findings rejected: cited references do not prove it",
        ),
        log(10, None, 2_100, "system", "Active Memory a_repo failed"),
        log(11, None, 2_200, "system", "Active Memory a_graph completed"),
    ];
    let view = project("local", &data(events, "running", Value::Null, None), 3_000);
    let lines: Vec<(Option<&str>, &str)> = view
        .run_log
        .iter()
        .map(|l| (l.agent.as_deref(), l.text.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            (None, "Preparing execution environment"),
            (Some("a_graph"), "The user is instructing me."),
            (Some("a_repo"), "Checking the repo."),
            // The harness's own "it answered" tool result says nothing on its own and is
            // dropped, same as the routine "status=allowed" rate-limit ping.
            (
                None,
                "Active Memory a_repo findings rejected: cited references do not prove it"
            ),
            (None, "Active Memory a_repo failed"),
            (None, "Active Memory a_graph completed"),
        ]
    );
    let repo = view
        .background_agents
        .iter()
        .find(|a| a.name == "a_repo")
        .unwrap();
    assert_eq!(repo.outcome, Some("failed"));
    assert_eq!(repo.last_line, "Checking the repo.");
    assert!(repo.note.as_deref().unwrap().contains("cited references"));
    assert_eq!(
        view.background_agents
            .iter()
            .find(|a| a.name == "a_graph")
            .unwrap()
            .outcome,
        Some("completed")
    );
}

/// A replay of `parent` from its `review` node: the worker's execution is reused from the
/// parent, the review runs here. The parent also lists this run as its successor (`resumedAs`)
/// and as a replay.
fn replay_with_a_reused_worker() -> RunData {
    let prior = event(
        1,
        json!({
            "kind": "prior_execution",
            "execution": {
                "occurrence": {"node": "worker", "mapIndices": []},
                "node_instance": 1,
                "execution": 1,
                "attempt": 1,
                "input": {},
                "state": {"Settled": {"outcome": {"status": "verified"}}},
            },
        }),
    );
    let events = vec![
        prior,
        log(2, None, 50_000, "system", "Resuming"),
        started(3, 2, "review", 1),
        log(4, Some(2), 51_000, "output", "ok"),
    ];
    let local = LocalFacts {
        resumed_from: Some("parent".into()),
        resumed_as: Some("child".into()),
        replays: vec!["child".into()],
        ..Default::default()
    };
    let mut run = data(events, "running", Value::Null, Some(local));
    run.admitted["replayOf"] = json!({"runId": "parent", "node": "review"});
    run
}

#[test]
fn reused_executions_have_no_times_of_their_own() {
    let view = project("local", &replay_with_a_reused_worker(), 52_000);
    let worker = &view.executions[0];
    assert!(worker.reused);
    assert_eq!(
        (
            worker.started_at,
            worker.ended_at,
            worker.duration_ms,
            worker.times_approximate
        ),
        (None, None, None, false)
    );
    assert_eq!(worker.reused_from.as_deref(), Some("parent"));
    assert_eq!(
        view.executions[1].started_at,
        Some(51_000),
        "the first node that runs here waits for the environment"
    );
}

#[test]
fn lineage_is_named_once() {
    let view = project("local", &replay_with_a_reused_worker(), 52_000);
    assert_eq!(
        view.resumed_from, None,
        "a replay is not also shown as resumed from its parent"
    );
    assert_eq!(
        view.resumed_as, None,
        "a replay successor is listed once, under replays"
    );
    assert_eq!(view.replays, ["child"]);
}

#[test]
fn credentials_failure_names_the_fix_once() {
    let line = "Node worker failed: crash after 10.5s: node execution failed: Claude provider failure: Failed to authenticate. API Error: 401 OAuth access token is invalid.";
    let inner = "Claude provider failure: Failed to authenticate. API Error: 401 OAuth access token is invalid.";
    let events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 1_000, "error", inner),
        log(3, Some(1), 2_000, "error", line),
        completed(4, 1, "worker", failed("crash")),
        terminal(5, "failed", "worker_failed"),
    ];
    let mut run = data(events, "finished", Value::Null, None);
    run.admitted["runtime"]["nodes"]["worker"]["connections"] =
        json!({"anthropic": ["CLAUDE_CODE_OAUTH_TOKEN"]});
    let failure = project("local", &run, 3_000).failure.unwrap();
    assert_eq!(
        failure.title, "Anthropic credentials rejected",
        "the company is the provider's"
    );
    assert_eq!(
        failure.fix.as_deref(),
        Some("Update the `anthropic` connection, then resume.")
    );
    assert_eq!(
        failure.fix_command.as_deref(),
        Some("zeroshot connection set anthropic --field CLAUDE_CODE_OAUTH_TOKEN")
    );
    assert_eq!(failure.primary, Some(Action::Resume));
    // The wrapper ("Node worker failed: crash after …: node execution failed: … provider
    // failure: ") is stripped; only the provider's own sentence is shown, once.
    assert_eq!(
        failure.last_errors,
        ["Failed to authenticate. API Error: 401 OAuth access token is invalid."],
        "the provider error is shown once, unwrapped"
    );
}

#[test]
fn a_sibling_cancelled_for_a_force_stop_is_interrupted_not_failed() {
    let events = vec![
        started(1, 1, "worker", 1),
        started(2, 2, "review", 1),
        log(
            3,
            Some(2),
            1_000,
            "error",
            "Node review failed: refusal after 1.0s",
        ),
        completed(4, 2, "review", failed("refusal")),
        event(5, json!({"kind": "force_stop_requested"})),
        completed(6, 1, "worker", failed("refusal")),
        terminal(7, "failed", "force_stopped"),
    ];
    let view = project("local", &data(events, "finished", Value::Null, None), 3_000);
    assert_eq!(view.status, "stopped");
    assert!(
        view.executions
            .iter()
            .all(|e| e.state == ExecState::Interrupted),
        "{:?}",
        view.executions.iter().map(|e| e.state).collect::<Vec<_>>()
    );
}

// ---------- Zeroshot 16.1 ----------

fn vector(seq: u64, vector: Value) -> Event {
    Event {
        seq,
        body: json!({"kind": "vector", "vector": vector}),
    }
}

fn steered_16_1() -> Vec<Event> {
    let run = "01a1a5e8-c91b-7073-b3e9-fdbd462600af";
    vec![
        event(1, json!({"kind": "run_started", "at": 1_000})),
        event(
            2,
            json!({"kind": "node_started", "at": 1_500, "reference": {"node": "worker", "nodeInstance": 1, "execution": 1}, "occurrence": {"node": "worker", "mapIndices": []}, "attempt": 1, "input": {}}),
        ),
        log(
            3,
            Some(1),
            2_000,
            "system",
            "Claude tool call: Bash id=toolu_1 status=unknown\n{\"command\":\"sleep 20\"}",
        ),
        vector(
            4,
            json!({"id": "s1", "createdAt": 2_100, "kind": "steer", "text": "Write BANANA instead.", "mode": "once"}),
        ),
        vector(
            5,
            json!({"id": "s2", "createdAt": 2_200, "kind": "steer", "node": "worker", "text": "Only you: skip step 3.", "mode": "once"}),
        ),
        log(
            6,
            Some(1),
            3_000,
            "system",
            "Claude tool completed: Bash id=toolu_1 status=success\n(Bash completed with no output)",
        ),
        vector(
            7,
            json!({"id": "r1", "createdAt": 3_001, "kind": "steer_receipt", "steerId": "s1", "conversation": format!("{run}/execution/1"), "execution": "nv2-x", "nodeInstance": 1}),
        ),
    ]
}

#[test]
fn sixteen_one_tool_lines_steers_and_receipts() {
    let view = project(
        "local",
        &data(steered_16_1(), "running", Value::Null, None),
        10_000,
    );
    assert!(
        view.features.exact_times,
        "node_started carries `at` (from recorded_at_ms)"
    );
    assert!(
        view.features.tool_calls,
        "16.1's plain-text tool lines are tool calls"
    );
    let worker = &view.executions[0];
    assert_eq!(worker.tool_calls, 1);
    assert_eq!(worker.started_at, Some(1_500));
    let texts: Vec<&str> = view.run_log.iter().map(|l| l.text.as_str()).collect();
    assert!(
        texts.contains(&"Message to all agents: Write BANANA instead."),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"Message to worker: Only you: skip step 3."),
        "{texts:?}"
    );
    assert!(texts.contains(&"worker received the message"), "{texts:?}");
}

#[test]
fn sixteen_one_suspension_is_neither_stalled_nor_failed() {
    let mut events = steered_16_1();
    events.push(event(8, json!({"kind": "request_deferred", "reference": {"node": "worker", "nodeInstance": 1, "execution": 1}})));
    events.push(event(9, json!({"kind": "suspend_requested"})));
    events.push(event(
        10,
        json!({"kind": "suspended", "suspension": {"reason": "request"}}),
    ));
    events.push(event(
        11,
        json!({"kind": "node_instances_reserved", "through": 1}),
    ));
    let dead = LocalFacts {
        controller_alive: false,
        controller_liveness: Liveness::Dead,
        ..Default::default()
    };
    let view = project(
        "local",
        &data(events, "suspended", Value::Null, Some(dead)),
        10_000,
    );
    assert_eq!(view.status, "suspended");
    assert_ne!(
        view.executions[0].state,
        ExecState::Running,
        "nothing runs in a suspended run"
    );
    assert!(view.failure.is_none());
    let texts: Vec<&str> = view.run_log.iter().map(|l| l.text.as_str()).collect();
    assert!(texts.contains(&"worker waits for an answer"), "{texts:?}");
    assert!(texts.contains(&"Suspended (request)"), "{texts:?}");
}

#[test]
fn older_patched_ledgers_are_told_apart_from_16_1() {
    assert!(!older_patched_ledger(&steered_16_1()));
    let fork = vec![event(
        1,
        json!({"kind": "safe_log", "execution": 1, "timestamp": 1, "stream": "output", "line": "hi", "logKind": "text"}),
    )];
    assert!(older_patched_ledger(&fork));
    let fork_cost = vec![event(
        1,
        json!({"kind": "token_usage_observed", "execution": 1, "usage": {}, "costUsd": "0.1"}),
    )];
    assert!(older_patched_ledger(&fork_cost));
    let stock_with_at = vec![event(
        1,
        json!({"kind": "node_started", "at": 5, "reference": {"execution": 1}}),
    )];
    assert!(
        !older_patched_ledger(&stock_with_at),
        "`at` alone is the recorded time zerochat fills in"
    );
}

/// A run whose `worker` failed with the provider's `error` text, on the run-level lane
/// `runtime` (`harness`, `provider`; `None` leaves the key out).
fn failed_on_lane(error: &str, harness: Option<&str>, provider: Option<&str>) -> RunData {
    let events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 1_000, "error", error),
        completed(3, 1, "worker", failed("crash")),
        terminal(4, "failed", "worker_failed"),
    ];
    let mut run = data(events, "finished", Value::Null, None);
    let runtime = run.admitted["runtime"].as_object_mut().unwrap();
    runtime.remove("harness");
    runtime.remove("provider");
    if let Some(h) = harness {
        runtime.insert("harness".into(), json!(h));
    }
    if let Some(p) = provider {
        runtime.insert("provider".into(), json!(p));
    }
    run
}

/// Provider error texts, one per cause whose headline names the company or the harness.
const PROVIDER_ERRORS: [&str; 10] = [
    "API Error: 401 OAuth access token is invalid.",
    "API Error: 403 permission_error: does not have permission",
    "You've hit your usage limit · resets 9:30am (UTC)",
    "API Error: 429 rate_limit_error",
    "API Error: 529 overloaded_error",
    "StreamNoEventsError: stream ended unexpectedly",
    "APIConnectionTimeoutError: Request timed out.",
    "connect ECONNREFUSED 127.0.0.1:443",
    "spawn claude ENOENT: no such file or directory",
    "There's an issue with the selected model (claude-haiku-9-9).",
];

#[test]
fn a_node_lane_wins_over_the_run_default() {
    let events = vec![started(1, 1, "worker", 1), started(2, 2, "review", 1)];
    let mut run = data(events, "running", Value::Null, None);
    run.admitted["runtime"]["nodes"]["worker"] =
        json!({"model": "gpt-5-codex", "lane": {"harness": "codex", "provider": "openai"}});
    let view = project("local", &run, 2_000);
    let lane = |i: usize| {
        (
            view.executions[i].harness.as_deref(),
            view.executions[i].provider.as_deref(),
            view.executions[i].model.as_deref(),
        )
    };
    assert_eq!(
        lane(0),
        (Some("codex"), Some("openai"), Some("gpt-5-codex")),
        "the node's own lane"
    );
    assert_eq!(
        lane(1),
        (Some("claude"), Some("anthropic"), Some("claude-haiku-4-5")),
        "the run's default lane"
    );
    let worker = view
        .graph_nodes
        .iter()
        .find(|n| n.name == "worker")
        .unwrap();
    assert_eq!(
        (worker.harness.as_deref(), worker.provider.as_deref()),
        (Some("codex"), Some("openai"))
    );
    // The run header's lanes: one per distinct lane, in graph order, with its nodes.
    /// (harness, provider, model, nodes) of one lane.
    type Lane<'a> = (
        Option<&'a str>,
        Option<&'a str>,
        Option<&'a str>,
        Vec<&'a str>,
    );
    let lanes: Vec<Lane> = view
        .lanes
        .iter()
        .map(|l| {
            (
                l.harness.as_deref(),
                l.provider.as_deref(),
                l.model.as_deref(),
                l.nodes.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        lanes,
        [
            (
                Some("codex"),
                Some("openai"),
                Some("gpt-5-codex"),
                vec!["worker"]
            ),
            (
                Some("claude"),
                Some("anthropic"),
                Some("claude-haiku-4-5"),
                vec!["review"]
            ),
        ]
    );
}

#[test]
fn one_lane_when_every_node_shares_it() {
    let view = project(
        "local",
        &data(
            vec![started(1, 1, "worker", 1)],
            "running",
            Value::Null,
            None,
        ),
        2_000,
    );
    assert_eq!(view.lanes.len(), 1, "{:?}", view.lanes);
    assert_eq!(
        view.lanes[0].nodes,
        ["worker", "review"],
        "a node that has not run yet shows its configured lane"
    );
}

#[test]
fn the_older_patched_zeroshot_lane_record_is_still_read() {
    // The fork recorded the lane per execution (`node_started.provider`); no runtime lane.
    let mut start = started(1, 1, "worker", 1);
    start.body["provider"] = json!({"harness": "copilot", "provider": "github", "model": "gpt-5"});
    let mut run = data(vec![start], "running", Value::Null, None);
    run.admitted["runtime"]
        .as_object_mut()
        .unwrap()
        .remove("provider");
    let e = &project("local", &run, 2_000).executions[0];
    assert_eq!(
        (
            e.harness.as_deref(),
            e.provider.as_deref(),
            e.model.as_deref()
        ),
        (Some("copilot"), Some("github"), Some("gpt-5"))
    );
}

#[test]
fn a_missing_lane_is_unknown_not_claude() {
    let view = project("local", &failed_on_lane("boom", None, None), 5_000);
    let e = &view.executions[0];
    assert_eq!((e.harness.as_deref(), e.provider.as_deref()), (None, None));
    assert_eq!(crate::harness_name::label(e.harness.as_deref()), "unknown");
}

#[test]
fn a_node_is_priced_on_its_own_lane() {
    // A cache write costs 2x on Claude Code (1-hour cache) and 1.25x elsewhere (pricing.rs):
    // the same tokens on a Copilot lane must not be priced as the run's Claude.
    let usage = |seq: u64, execution: u64| {
        event(
            seq,
            json!({"kind": "token_usage_observed", "execution": execution, "usage": {"inputTokens": 0, "outputTokens": 0, "cacheReadInputTokens": 0, "cacheCreationInputTokens": 1_000_000}}),
        )
    };
    let events = vec![
        started(1, 1, "worker", 1),
        usage(2, 1),
        started(3, 2, "review", 1),
        usage(4, 2),
    ];
    let mut run = data(events, "running", Value::Null, None);
    run.admitted["runtime"]["nodes"]["worker"]["lane"] =
        json!({"harness": "copilot", "provider": "anthropic"});
    let view = project("local", &run, 9_000);
    let (worker, review) = (
        view.executions[0].cost_usd.unwrap(),
        view.executions[1].cost_usd.unwrap(),
    );
    assert!(
        worker < review,
        "copilot lane {worker} vs claude lane {review}"
    );
}

#[test]
fn an_unknown_or_missing_harness_is_never_claude_in_a_headline() {
    for (harness, provider) in [
        (Some("hermes"), Some("nous")),
        (None, None),
        (Some("pi"), None),
        (Some("opencode"), Some("opencode")),
    ] {
        for error in PROVIDER_ERRORS {
            let failure = project("local", &failed_on_lane(error, harness, provider), 5_000)
                .failure
                .unwrap();
            let words = format!(
                "{} | {} | {}",
                failure.title,
                failure.fix.unwrap_or_default(),
                failure.detail
            );
            assert!(
                !words.contains("Claude"),
                "{harness:?}/{provider:?} {error:?}: {words}"
            );
        }
    }
}

#[test]
fn the_company_comes_from_the_provider() {
    let title = |error: &str, harness: &str, provider: Option<&str>| {
        project(
            "local",
            &failed_on_lane(error, Some(harness), provider),
            5_000,
        )
        .failure
        .unwrap()
        .title
    };
    let limited = "API Error: 429 rate_limit_error";
    assert_eq!(
        title(limited, "copilot", Some("anthropic")),
        "worker was rate limited by Anthropic",
        "Copilot on Anthropic is Anthropic"
    );
    assert_eq!(
        title(limited, "codex", Some("openai")),
        "worker was rate limited by OpenAI"
    );
    assert_eq!(
        title(limited, "copilot", Some("github")),
        "worker was rate limited by GitHub"
    );
    assert_eq!(
        title(limited, "claude", Some("openrouter")),
        "worker was rate limited by OpenRouter"
    );
    assert_eq!(
        title(limited, "hermes", Some("nous")),
        "worker was rate limited by the provider"
    );
    assert_eq!(
        title(limited, "claude", None),
        "worker was rate limited by the provider"
    );
    let down = "API Error: 529 overloaded_error";
    assert_eq!(
        title(down, "hermes", None),
        "The provider was overloaded or unavailable (worker, 529)"
    );
    assert_eq!(
        title("You've hit your usage limit", "pi", Some("x")),
        "worker hit the provider's usage limit"
    );
    assert_eq!(
        title("You've hit your usage limit", "claude", Some("anthropic")),
        "worker hit the Anthropic usage limit"
    );
    assert_eq!(
        title(
            "API Error: 401 OAuth access token is invalid.",
            "hermes",
            None
        ),
        "The provider credentials rejected"
    );
    assert_eq!(
        title("StreamNoEventsError", "hermes", None),
        "The provider's answer to worker was cut off"
    );
}

#[test]
fn a_missing_cli_names_the_lane_harness_as_it_is() {
    let error = "spawn codex ENOENT: no such file or directory";
    let failure = project("local", &failed_on_lane(error, Some("hermes"), None), 5_000)
        .failure
        .unwrap();
    assert_eq!(failure.title, "worker could not start the hermes CLI");
    let failure = project("local", &failed_on_lane(error, None, None), 5_000)
        .failure
        .unwrap();
    assert_eq!(failure.title, "worker could not start the unknown CLI");
}

#[test]
fn failure_headlines_keep_the_exact_model_id() {
    let title = |error: &str, model: &str| {
        let mut run = failed_on_lane(error, Some("claude"), Some("anthropic"));
        run.admitted["runtime"]["nodes"]["worker"]["model"] = json!(model);
        project("local", &run, 5_000).failure.unwrap().title
    };
    let unknown = "API Error: 404 {\"type\":\"error\",\"error\":{\"type\":\"not_found_error\",\"message\":\"model: claude-haiku-9-9-does-not-exist\"}}";
    assert_eq!(
        title(unknown, "claude-haiku-9-9-does-not-exist"),
        "Unknown model claude-haiku-9-9-does-not-exist"
    );
    let denied = "API Error: 403 permission_error: does not have permission";
    assert_eq!(
        title(denied, "claude-haiku-4-5"),
        "No access to claude-haiku-4-5"
    );
    assert_eq!(
        title(denied, "claude-opus-4-1-20250805"),
        "No access to claude-opus-4-1-20250805",
        "dated ids too"
    );
    assert_eq!(
        title(denied, "us.anthropic.claude-haiku-4-5-v1:0"),
        "No access to us.anthropic.claude-haiku-4-5-v1:0"
    );
}

/// A `worker` that hit Zeroshot's node timeout, with these log lines (at 2 000…) before it.
fn timed_out_after(lines: &[(&str, &str)]) -> RunData {
    let timed: Vec<(u64, &str, &str)> = lines
        .iter()
        .enumerate()
        .map(|(i, (stream, text))| (2_000 + i as u64 * 1_000, *stream, *text))
        .collect();
    timed_out_with(&timed)
}

/// A `worker` (60 s timeout) that hit it at 60 000, with these log lines `(at, stream, text)`.
fn timed_out_with(lines: &[(u64, &str, &str)]) -> RunData {
    let mut events = vec![started(1, 1, "worker", 1)];
    for (i, (at, stream, text)) in lines.iter().enumerate() {
        events.push(log(2 + i as u64, Some(1), *at, stream, text));
    }
    let n = events.len() as u64;
    events.push(log(
        n + 1,
        Some(1),
        60_000,
        "error",
        "Node worker failed: timeout after 58.0s: node execution failed",
    ));
    events.push(completed(n + 2, 1, "worker", failed("timeout")));
    events.push(terminal(n + 3, "failed", "worker_failed"));
    let mut run = data(events, "finished", Value::Null, None);
    run.admitted["graph"]["root"]["children"][0]["body"]["children"][0]["timeoutMs"] =
        json!(60_000);
    run
}

#[test]
fn a_node_timeout_leads_with_the_fix_not_a_replay() {
    // F09: an agent that kept working until its own limit: a replay times out again.
    let working = [
        (2_000, "output", "working"),
        (50_000, "output", "still working"),
        (58_000, "output", "nearly there"),
    ];
    let failure = project("local", &timed_out_with(&working), 90_000)
        .failure
        .unwrap();
    assert_eq!(
        (failure.class, failure.cause.as_str()),
        ("agent", "timed out after 1m")
    );
    assert_eq!(failure.primary, None, "no replay as the fix for a timeout");
    assert_eq!(
        failure.fix.as_deref(),
        Some(
            "Raise `timeoutMs` on `worker` in your graph (now 1m), or give it a smaller task; then start a new run."
        )
    );
    assert!(!failure.actions.contains(&Action::Resume));
}

#[test]
fn a_timeout_while_the_api_was_retrying_is_the_providers() {
    // F12: API retry lines before the timeout: the provider stopped answering, not the agent.
    let retrying = [
        ("output", "working"),
        ("system", "Claude API retry 1/10: unknown"),
    ];
    let failure = project("local", &timed_out_after(&retrying), 90_000)
        .failure
        .unwrap();
    assert_eq!(
        (failure.class, failure.cause.as_str()),
        ("provider", "provider not responding")
    );
    assert_eq!(
        failure.title,
        "Anthropic stopped answering worker (timed out after 1m)"
    );
    assert_eq!(failure.primary, Some(Action::Resume));
    assert!(failure.fix.is_none());
    // A provider error line before the timeout is the same evidence.
    let cut = [("error", "StreamNoEventsError: stream ended unexpectedly")];
    let failure = project("local", &timed_out_after(&cut), 90_000)
        .failure
        .unwrap();
    assert_eq!(
        (failure.class, failure.primary),
        ("provider", Some(Action::Resume))
    );
}

#[test]
fn a_timeout_after_a_model_request_nobody_answered_is_the_providers() {
    // F12 (the lab's chaos-hang): a tool result, then no answer for 55 s until the timeout.
    let call =
        "Claude tool call: Bash id=toolu_1 status=unknown\n{\"command\":\"python3 -m unittest\"}";
    let result = "Claude tool completed: Bash id=toolu_1 status=completed\nOK";
    let notice = "Claude rate limit: status=allowed, type=seven_day, resetsAt=2026-11-25T02:00:00Z";
    let hung = [
        (2_000, "output", "Let me run the tests."),
        (3_000, "system", call),
        (4_000, "system", notice),
        (5_000, "system", result),
    ];
    let failure = project("local", &timed_out_with(&hung), 90_000)
        .failure
        .unwrap();
    assert_eq!(
        (failure.class, failure.cause.as_str()),
        ("provider", "provider not responding")
    );
    assert_eq!(
        failure.title,
        "Anthropic stopped answering worker (timed out after 1m)"
    );
    assert_eq!(
        failure.detail,
        "Its last model request had no answer for 55s when the timeout hit. Usually temporary: resume."
    );
    assert_eq!(failure.primary, Some(Action::Resume));
    assert!(failure.fix.is_none());

    // The same silence while a tool runs (a long build or test) is the agent's own time.
    let running = [
        (2_000, "output", "Let me run the tests."),
        (5_000, "system", call),
    ];
    let failure = project("local", &timed_out_with(&running), 90_000)
        .failure
        .unwrap();
    assert_eq!((failure.class, failure.primary), ("agent", None));

    // A short wait for the model before the timeout is the agent working until its limit.
    let busy = [(2_000, "system", call), (40_000, "system", result)];
    let failure = project("local", &timed_out_with(&busy), 90_000)
        .failure
        .unwrap();
    assert_eq!((failure.class, failure.primary), ("agent", None));
}

#[test]
fn provider_faults_are_classed_provider_with_resume() {
    // F11, F12: limits, outages and broken streams are the provider's, and Resume is the way.
    for error in [
        "You've hit your usage limit · resets 9:30am (UTC)",
        "API Error: 429 rate_limit_error",
        "API Error: 529 overloaded_error",
        "StreamNoEventsError: stream ended unexpectedly",
        "APIConnectionTimeoutError: Request timed out.",
        "Unexpected end of stream: malformed API response",
    ] {
        let failure = project(
            "local",
            &failed_on_lane(error, Some("claude"), Some("anthropic")),
            5_000,
        )
        .failure
        .unwrap();
        assert_eq!(
            (failure.class, failure.primary),
            ("provider", Some(Action::Resume)),
            "{error}"
        );
        assert!(
            failure.title.contains("Anthropic"),
            "names the provider: {}",
            failure.title
        );
    }
}

#[test]
fn no_access_and_unknown_model_lead_with_the_fix() {
    for error in [
        "API Error: 403 permission_error: does not have permission",
        "There's an issue with the selected model (claude-x-9).",
    ] {
        let failure = project(
            "local",
            &failed_on_lane(error, Some("claude"), Some("anthropic")),
            5_000,
        )
        .failure
        .unwrap();
        assert_eq!((failure.class, failure.primary), ("setup", None), "{error}");
        assert!(
            failure
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("`worker` in your runtime file")),
            "{:?}",
            failure.fix
        );
    }
}

#[test]
fn a_resumed_run_that_fails_again_does_not_lead_with_the_same_replay() {
    // F09 (G9): a run that was itself a resume or replay already had its unchanged second try.
    let events = vec![
        started(1, 1, "worker", 1),
        log(
            2,
            Some(1),
            1_000,
            "error",
            "prompt is too long: context window exceeded",
        ),
        completed(3, 1, "worker", failed("crash")),
        terminal(4, "failed", "worker_failed"),
    ];
    let first = LocalFacts {
        has_checkpoints: true,
        ..Default::default()
    };
    let failure = project(
        "local",
        &data(events.clone(), "finished", Value::Null, Some(first)),
        5_000,
    )
    .failure
    .unwrap();
    assert_eq!(
        failure.primary,
        Some(Action::Replay),
        "a first failure may lead with a replay"
    );
    let again = LocalFacts {
        has_checkpoints: true,
        resumed_from: Some("01a1a657-eeb1-7000-8000-000000000000".into()),
        ..Default::default()
    };
    let failure = project(
        "local",
        &data(events, "finished", Value::Null, Some(again)),
        5_000,
    )
    .failure
    .unwrap();
    assert_eq!(
        failure.primary, None,
        "the same replay again is not the way forward"
    );
    assert!(
        failure.actions.contains(&Action::Replay),
        "still offered, not as the primary"
    );
}

// ---------- F18, F19, F36: run numbers and pressures ----------

#[test]
fn the_run_ends_at_its_outcome_not_when_zeroshot_closes_it() {
    // F19: the graph ended at 12 000; the postmortem kept Zeroshot closing the run until 72 000.
    let view = project("local", &unknown_model_run(), 100_000);
    assert_eq!(
        view.ended_at,
        Some(12_000),
        "the list and the header count to the outcome"
    );
    let failure = view.failure.unwrap();
    assert_eq!(
        failure.closed_after_ms,
        Some(60_000),
        "the closing is still told apart"
    );
}

#[test]
fn a_succeeded_run_ends_at_its_outcome_while_zeroshot_closes_it_and_after() {
    let mut events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 5_000, "output", "done"),
        completed(3, 1, "worker", verified()),
        event(
            4,
            json!({"kind": "primary_graph_completed", "result": {"status": "succeeded"}, "at": 6_000}),
        ),
        log(
            5,
            None,
            30_000,
            "system",
            "Active Memory postmortem started",
        ),
    ];
    // Still closing: phase `running`, the status final already.
    let closing = project(
        "local",
        &data(events.clone(), "running", Value::Null, None),
        40_000,
    );
    assert_eq!(
        (closing.status.as_str(), closing.ended_at),
        ("succeeded", Some(6_000))
    );
    events.push(log(
        6,
        None,
        60_000,
        "system",
        "Active Memory postmortem completed",
    ));
    events.push(event(
        7,
        json!({"kind": "terminal", "result": {"status": "succeeded"}, "at": 61_000}),
    ));
    let closed = project(
        "local",
        &data(events, "finished", Value::Null, None),
        100_000,
    );
    assert_eq!(
        closed.ended_at,
        Some(6_000),
        "the same duration once Zeroshot closed it"
    );
}

/// F18: the worker stalls at 5 000 (its controller died); the run is resumed at 300 000, when
/// Zeroshot fails the open execution and closes the run.
fn stalled_then_closed(live_usage: bool) -> RunData {
    let events = vec![
        started(1, 1, "worker", 1),
        log(2, Some(1), 1_000, "output", "working"),
        log(3, Some(1), 5_000, "output", "still working"),
        log(
            4,
            Some(1),
            300_000,
            "error",
            "Node worker failed: crash: run runtime_lost",
        ),
        event(
            5,
            json!({"kind": "node_completed", "at": 300_001, "completion": {"reference": {"node": "worker", "nodeInstance": 1, "execution": 1}, "outcome": failed("crash")}}),
        ),
        log(6, None, 300_002, "error", "Run failed: runtime_lost"),
        terminal(7, "failed", "runtime_lost"),
    ];
    let local = LocalFacts {
        resumed_as: Some("01a1a657-eeb1-7000-8000-000000000000".into()),
        has_checkpoints: true,
        ..Default::default()
    };
    let mut run = data(events, "finished", Value::Null, Some(local));
    if live_usage {
        run.live_usage.insert(
            1,
            Usage {
                input_tokens: 10,
                output_tokens: 20,
                cache_read_input_tokens: 1000,
                cache_creation_input_tokens: 0,
            },
        );
    }
    run
}

#[test]
fn an_execution_closed_with_a_lost_run_was_interrupted_at_its_last_activity() {
    let view = project("local", &stalled_then_closed(false), 400_000);
    let worker = &view.executions[0];
    assert_eq!(
        worker.state,
        ExecState::Interrupted,
        "Zeroshot's close is not the agent failing"
    );
    assert_eq!(
        (
            worker.last_activity_at,
            worker.ended_at,
            worker.duration_ms.is_some_and(|d| d < 10_000)
        ),
        (Some(5_000), Some(5_000), true)
    );
    assert_eq!(
        view.ended_at,
        Some(5_000),
        "the run's duration ends when it was last active too"
    );
    assert_eq!(view.failure.unwrap().at, Some(5_000));
}

#[test]
fn an_interrupted_execution_keeps_its_session_log_usage_after_the_close() {
    let view = project("local", &stalled_then_closed(true), 400_000);
    let worker = &view.executions[0];
    assert_eq!(
        worker.usage_source,
        UsageSource::SessionLog,
        "marked as from the provider's log"
    );
    assert_eq!(view.usage.total(), 1030);
    assert!(view.cost_usd.is_some() && view.cost_estimated && view.usage_live);
}

#[test]
fn rate_limit_words_in_tool_output_or_agent_text_are_not_pressure() {
    let tool = "Claude tool completed: Bash id=toolu_013Q status=success\nHTTP 429 rate_limit_error: You have hit your usage limit. Rate limit exceeded, retry-after 60";
    let events = vec![
        started(1, 1, "worker", 1),
        // Zeroshot 16.1: tool lines are plain `system` lines.
        log(2, Some(1), 1_000, "system", tool),
        // The older patched Zeroshot: a structured tool result.
        event(
            3,
            json!({"kind": "safe_log", "execution": 1, "timestamp": 2_000, "stream": "system", "logKind": "tool_result", "line": "{\"toolUseId\":\"t1\",\"content\":\"429 Too Many Requests: overloaded_error, quota exceeded\"}"}),
        ),
        // The agent's own words, even when they quote Zeroshot's notices.
        log(
            4,
            Some(1),
            3_000,
            "output",
            "The client retries on rate_limit_error and usage limit errors.",
        ),
        log(
            5,
            Some(1),
            3_500,
            "output",
            "Claude API retry 1/3: rate_limit",
        ),
        log(
            6,
            Some(1),
            3_600,
            "output",
            "Claude rate limit: status=rejected, type=five_hour, resetsAt=2026-11-04T12:00:00Z",
        ),
    ];
    let view = project("local", &data(events, "running", Value::Null, None), 4_000);
    assert!(
        view.pressures
            .iter()
            .all(|p| !matches!(p.kind, PressureKind::RateLimit | PressureKind::Retry)),
        "{:?}",
        view.pressures
    );
}

#[test]
fn a_structured_limit_event_shows_a_title_never_the_raw_line() {
    let raw = "Claude tool completed: Bash id=toolu_1 status=success HTTP 429";
    let events = vec![
        started(1, 1, "worker", 1),
        event(
            2,
            json!({"kind": "safe_log", "execution": 1, "timestamp": 1_000, "stream": "system", "logKind": "rate_limit", "line": raw}),
        ),
        event(
            3,
            json!({"kind": "safe_log", "execution": 1, "timestamp": 1_500, "stream": "system", "logKind": "retry", "line": "retrying after a hiccup"}),
        ),
    ];
    let view = project("local", &data(events, "running", Value::Null, None), 2_000);
    let texts: Vec<(&PressureKind, &str)> = view
        .pressures
        .iter()
        .map(|p| (&p.kind, p.text.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            (&PressureKind::RateLimit, "rate limit hit"),
            (&PressureKind::Retry, "retrying the provider's API")
        ]
    );
    assert!(
        view.pressures
            .iter()
            .all(|p| p.node.as_deref() == Some("worker") && p.execution == Some(1)),
        "the agent and its line"
    );
}
