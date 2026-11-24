//! features (zsf state → views): the allowed-action tables for every slice status × job ×
//! worktree and every feature state, `waiting_for_you`, the task diff (`line_diff`), and the
//! list/detail readers called directly at the fixed "now".

use serde_json::{Value, json};
use zsf::model::{Job, Plan, SlicePlan, SliceState, SliceStatus, State};

use super::check;
use super::world::{NOW_MS, world};
use crate::features;

fn plan(checkpoint: bool) -> SlicePlan {
    SlicePlan {
        id: "01".into(),
        title: "t".into(),
        spec: "s.md".into(),
        deps: vec![],
        checkpoint,
        leases: vec![],
        commit_title: None,
        description: None,
        graph: None,
        runtime: None,
        args: None,
        hooks: Default::default(),
    }
}

#[test]
fn slice_action_table() {
    let statuses = [
        SliceStatus::Pending,
        SliceStatus::Launching,
        SliceStatus::Running,
        SliceStatus::Integrating,
        SliceStatus::WaitingForReview,
        SliceStatus::Merged,
        SliceStatus::Failed,
        SliceStatus::Interrupted,
        SliceStatus::Skipped,
    ];
    let jobs = [None, Some(Job::Resume)];
    let existing = world().root.clone();
    let mut rows = Vec::new();
    for status in statuses {
        for checkpoint in [false, true] {
            for approved in [false, true] {
                for job in &jobs {
                    for (attempts, worktree, changed) in [
                        (0, false, false),
                        (1, false, false),
                        (1, true, false),
                        (1, true, true),
                    ] {
                        let mut s = SliceState {
                            status,
                            approved,
                            job: job.clone(),
                            ..Default::default()
                        };
                        if attempts > 0 {
                            s.attempts = serde_json::from_value(json!([{"n": 1, "kind": "first", "task_file": "t.md", "started_ms": 1}])).unwrap();
                        }
                        if worktree {
                            s.worktree = Some(existing.clone());
                        }
                        let actions = features::slice_actions(&plan(checkpoint), &s, changed);
                        rows.push(json!({"status": status.as_str(), "display": s.display_status(checkpoint), "checkpoint": checkpoint, "approved": approved, "job": job.is_some(), "attempts": attempts, "worktree": worktree, "worktreeChanged": changed, "actions": actions}));
                    }
                }
            }
        }
    }
    check("features/slice-actions", &json!(rows));
}

#[test]
fn feature_action_table() {
    let mut rows = Vec::new();
    for active in [false, true] {
        for paused in [false, true] {
            for all_finished in [false, true] {
                for daemon in [false, true] {
                    for spawn in [false, true] {
                        for readable in [false, true] {
                            let state = State {
                                active,
                                paused: paused
                                    .then(|| "2 infrastructure failures in a row".to_owned()),
                                ..Default::default()
                            };
                            rows.push(json!({"active": active, "paused": paused, "allFinished": all_finished, "daemonAlive": daemon, "canSpawn": spawn, "readable": readable,
                                "actions": features::feature_actions(&state, all_finished, daemon, spawn, readable)}));
                        }
                    }
                }
            }
        }
    }
    check("features/feature-actions", &json!(rows));
}

#[test]
fn waiting_and_task_diff() {
    let w = world();
    let feature = zsf::store::Feature::open(&w.zsf, "calc").unwrap();
    let plan: Plan = feature.load_plan().unwrap();
    let state = feature.load_state().unwrap();
    let (approve, failed) = features::waiting_for_you(&plan, &state);
    let diffs: Vec<Value> = [
        ("a\nb\nc\n", "a\nb\nc\n"),
        ("a\nb\nc\n", "a\nB\nc\nd\n"),
        ("", "new\n"),
        ("old\n", ""),
        ("# Add farewell()\n\nImplement farewell() in calc/.\n", "# Add farewell()\n\nImplement farewell() in calc/.\n\n## Notes from the developer\n\nKeep farewell() next to greet().\n"),
    ]
    .iter()
    .map(|(a, b)| json!({"old": a, "new": b, "diff": features::line_diff(a, b, "x-a1.md", "x-a2.md")}))
    .collect();
    check(
        "features/waiting-and-line-diff",
        &json!({"approve": approve, "failed": failed, "lineDiff": diffs, "needsYou": features::needs_you_count(&w.zsf), "names": features::names(&w.zsf)}),
    );
}

#[test]
fn list_and_detail_direct() {
    let w = world();
    let roots: features::Roots = vec![
        ("local".to_owned(), w.state.clone()),
        ("observe".to_owned(), w.observe.clone()),
    ];
    let runs = features::RunFactsCache::default();
    let ctx = features::Ctx {
        root: &w.zsf,
        roots: &roots,
        runs: &runs,
        zsf_bin: None,
        now: NOW_MS,
    };
    let list = features::list(&ctx);
    let details: Vec<Value> = ["calc", "paused", "fresh", "broken", "missing"].iter().map(|n| json!({"name": n, "detail": features::detail(&ctx, n).map_err(|e| format!("{e:#}"))})).collect();
    // A zsf program present: `start` is offered.
    let bin = w.root.join("work/succeeded/repo/README.md");
    let with_bin = features::Ctx {
        root: &w.zsf,
        roots: &roots,
        runs: &runs,
        zsf_bin: Some(&bin),
        now: NOW_MS,
    };
    let calc_with_bin = features::detail(&with_bin, "calc")
        .unwrap()
        .map(|d| json!(d.actions));
    check(
        "features/direct",
        &json!({"list": list, "details": details, "calcActionsWithZsf": calc_with_bin}),
    );
}

/// F38: a run whose agents are done while Zeroshot is still finishing it (its postmortem) is
/// `closing`: zsf takes the slice up only after, so the slice says that instead of "checks next".
#[test]
fn a_run_in_its_postmortem_is_closing() {
    let w = world();
    let roots: features::Roots = vec![
        ("local".to_owned(), w.state.clone()),
        ("observe".to_owned(), w.observe.clone()),
    ];
    let runs = features::RunFactsCache::default();
    let id = |name: &str| {
        let path = super::world::testdata().join(format!("ledgers/synthetic/{name}.json"));
        serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap()["runId"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let facts = |name: &str| runs.get(&roots, &id(name), NOW_MS).unwrap();
    let closing = facts("closing-postmortem");
    assert!(closing.closing && closing.ended_at.is_some(), "{closing:?}");
    for done in ["succeeded", "diag-code-timeout", "running-live"] {
        assert!(!facts(done).closing, "{done}");
    }
}

/// A slice waiting for review (calc 14) through the API: its view (status, actions, the diff's
/// numbers, the PR its run recorded), the diff against the feature branch (what Approve checks and
/// merges), and Request changes (`retry`) refused without notes. Pinned under `features/`, not
/// `api/`: the web goldens of other slices read every snapshot under `api/post/` and every diff
/// under `api/features/`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn waiting_for_review_api() {
    let api = super::api::Api::start().await;
    let detail = api.get("/api/features/calc").await;
    let slice = detail["body"]["slices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "14")
        .unwrap()
        .clone();
    let view: serde_json::Map<String, Value> = [
        "id",
        "status",
        "actions",
        "worktreeChanges",
        "prUrl",
        "commit",
        "mergedAt",
    ]
    .iter()
    .map(|k| (k.to_string(), slice[*k].clone()))
    .collect();
    let path = "/api/features/calc/slices/14/actions";
    check(
        "features/waiting-for-review",
        &json!({
            "slice": view,
            "worktree": api.get("/api/features/calc/slices/14/worktree").await,
            "requestChangesWithoutNotes": api.post(path, r#"{"action":"retry"}"#).await,
            "requestChangesBlankNotes": api.post(path, r#"{"action":"retry","notes":"  \n "}"#).await,
        }),
    );
}
