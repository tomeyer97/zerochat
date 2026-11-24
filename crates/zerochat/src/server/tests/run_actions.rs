//! Steer, stop, resume and replay: refused for observe sources, Zeroshot's refusals reworded, replay's checks.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{Value, json};

use crate::discovery::{Target, TargetKind};
use crate::server::api::run_actions::{
    ReplayBody, ResumeBody, SteerBody, replay, require_actions, resume, runtime_problem, steer,
    steer_refusal, stop, submission_graph,
};

use super::{observe_target, pb_state};

#[test]
fn runtime_problem_names_the_first_bad_name() {
    let ok = json!({"nodes": {"worker": {"model": "claude-haiku-4-5", "effort": "high", "harness": "claude", "timeout": 5}}});
    assert_eq!(runtime_problem(&ok), None);
    let bad = json!({"nodes": {"worker": {"model": "claude-haiku-4-5", "effort": "--x", "harness": "a b"}}});
    assert_eq!(
        runtime_problem(&bad).as_deref(),
        Some("The effort for worker is not a valid name: \"--x\".")
    );
    assert_eq!(
        runtime_problem(&json!({"nodes": {"w": {"model": 3}}})),
        None,
        "non-string values are not names"
    );
}

#[test]
fn zeroshot_steer_refusals_read_as_what_to_do() {
    // 16.1's own messages (zeroshot-official-steering.md §3).
    let finished = steer_refusal(
        "invalid_request",
        "steer requires an admitted, running or suspended run before its graph ends",
    );
    assert_eq!(
        finished,
        Some("This run is no longer running, so no agent can receive a message.")
    );
    assert_eq!(
        steer_refusal(
            "invalid_request",
            "run 01a… has successor 01b…; steer the successor"
        ),
        Some("This run was resumed as a newer run; message that one instead.")
    );
    assert_eq!(
        steer_refusal(
            "invalid_request",
            "steer text must not contain control characters"
        ),
        Some("Messages cannot contain control characters (tabs and line breaks are fine).")
    );
    assert_eq!(
        steer_refusal(
            "invalid_request",
            "steer node must name an agent in the admitted runtime plan"
        ),
        Some("This node is not an agent of the run, so it cannot receive messages.")
    );
    assert_eq!(
        steer_refusal("invalid_request", "something new"),
        Some("Zeroshot refused the message for this run.")
    );
    assert_eq!(
        steer_refusal("timeout", "no answer within 20s"),
        None,
        "other failures keep the CLI's words"
    );
}

#[test]
fn replacement_graphs_drop_admission_keys() {
    let graph = json!({"profile": "p", "initialInput": {}, "policy": {}, "root": {"kind": "seq"}, "bounds": {"maxNodes": 3}});
    let clean = submission_graph(graph);
    assert_eq!(
        clean
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        ["initialInput", "policy", "profile", "root"]
    );
}

#[test]
fn actions_are_refused_for_every_observe_target_regardless_of_policy() {
    assert!(
        require_actions(&observe_target(false)).is_err(),
        "observe policy refuses"
    );
    assert!(
        require_actions(&observe_target(true)).is_err(),
        "`all` policy still refuses: no controller wiring exists yet"
    );
    assert_eq!(
        require_actions(&observe_target(false)).unwrap_err().status,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[test]
fn actions_differ_by_policy_only_in_wording() {
    let observe = require_actions(&observe_target(false)).unwrap_err().message;
    let all = require_actions(&observe_target(true)).unwrap_err().message;
    assert_ne!(observe, all);
    assert!(observe.contains("observe-only"));
    assert!(all.contains("does not act"));
}

#[test]
fn local_and_direct_targets_are_unaffected() {
    let local = Target {
        id: "local".into(),
        name: "x".into(),
        kind: TargetKind::Local,
        origin: None,
        cli_name: None,
        roots: Vec::new(),
        actions_allowed: true,
    };
    let direct = Target {
        id: "target:lab".into(),
        name: "lab".into(),
        kind: TargetKind::Direct,
        origin: Some("http://x".into()),
        cli_name: Some("lab".into()),
        roots: Vec::new(),
        actions_allowed: true,
    };
    assert!(require_actions(&local).is_ok());
    assert!(require_actions(&direct).is_ok());
}

#[tokio::test]
async fn observe_source_from_config_has_no_action_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let state = pb_state(dir.path());
    let target = state
        .target("pb")
        .expect("pb source is discovered from config.toml");
    assert_eq!(target.kind, TargetKind::Observe);
    assert_eq!(state.capabilities(&target).await, Vec::<String>::new());
    assert!(require_actions(&target).is_err());
}

#[tokio::test]
async fn steer_stop_resume_replay_all_refuse_on_an_observe_source() {
    let dir = tempfile::tempdir().unwrap();
    let state = pb_state(dir.path());

    let steer_err = steer(
        State(state.clone()),
        Path(("pb".to_owned(), "run1".to_owned())),
        Json(SteerBody {
            execution: 1,
            message: "hi".into(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(steer_err.code, "actions_refused");

    let stop_err = stop(
        State(state.clone()),
        Path(("pb".to_owned(), "run1".to_owned())),
        Json(Value::Null),
    )
    .await
    .unwrap_err();
    assert_eq!(stop_err.code, "actions_refused");

    let resume_err = resume(
        State(state.clone()),
        Path(("pb".to_owned(), "run1".to_owned())),
        Json(ResumeBody {
            checkpoint_id: None,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(resume_err.code, "actions_refused");

    let replay_err = replay(
        State(state.clone()),
        Path(("pb".to_owned(), "run1".to_owned())),
        Json(ReplayBody {
            checkpoint_id: "c1".into(),
            graph: None,
            runtime: None,
            initial_input: None,
            title: None,
            validate_only: false,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(replay_err.code, "actions_refused");
}
