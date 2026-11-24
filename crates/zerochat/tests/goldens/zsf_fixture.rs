//! zsf features for the goldens, modelled on the end-to-end test lab's `live` plan (the `calc` toy
//! repository), with one slice in every status zsf has (plus `waiting_for_approval`, an older
//! zsf's merged unapproved checkpoint, an accepted merge, a conflict re-run, a queued retry, a
//! stopped slice with a live worktree), every attempt kind and result, and every event kind.
//! Slices point at synthetic runs of the local state root, so a slice's run status and cost come
//! from a real projection; 14 (waiting for review after changes were requested and a gate
//! re-run) points at runs no state root has, so the runs' own goldens stay as they are.
//!
//! Also: `paused` (paused after infrastructure failures, active), `broken` (an invalid
//! feature.toml) and `fresh` (a plan nothing has started yet).

use std::path::Path;

use serde_json::{Value, json};

use super::world::git;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn run_of(state: &Path, scenario: &str) -> String {
    let path = super::world::testdata().join(format!("ledgers/synthetic/{scenario}.json"));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let id = v["runId"].as_str().unwrap().to_owned();
    let _ = state;
    id
}

const T: u64 = 1_795_000_000_000;

/// The runs of the `calc` feature's slice 14 (one part of the fixture feature, in zsf's words):
/// no state root has them, so their run status and cost stay unknown (see the module comment).
const RUN_14: [&str; 3] = [
    "01a1ab90-0000-7000-8000-000000000141",
    "01a1ab90-0000-7000-8000-000000000142",
    "01a1ab90-0000-7000-8000-000000000143",
];

/// Returns `(slice, squash commit)` of the merged slices.
pub fn build(root: &Path, zsf: &Path, state: &Path) -> Vec<(String, String)> {
    // The feature repository: main, and the feature branch with one squash commit per merged slice.
    let repo = root.join("work/calc/repo");
    std::fs::create_dir_all(repo.join("calc")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    write(
        &repo.join("calc/ops.py"),
        "def add(a, b):\n    return a + b\n",
    );
    write(
        &repo.join("calc/greet.py"),
        "def greet(name):\n    return 'hi ' + name\n",
    );
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "calc"]);
    git(&repo, &["checkout", "-q", "-b", "calc"]);
    let mut commits = Vec::new();
    let mut ops = String::from("def add(a, b):\n    return a + b\n");
    for (slice, title, file, add) in [
        (
            "01",
            "Add sub()",
            "calc/ops.py",
            "\n\ndef sub(a, b):\n    return a - b\n",
        ),
        (
            "02",
            "Add mul()",
            "calc/ops.py",
            "\n\ndef mul(a, b):\n    return a * b\n",
        ),
        ("03", "Friendlier greeting", "calc/greet.py", ""),
        ("11", "Hand-finished tidy-up", "calc/ops.py", "\n\n# tidy\n"),
    ] {
        if file == "calc/greet.py" {
            write(
                &repo.join(file),
                "def greet(name):\n    return f'Hello, {name}!'\n",
            );
        } else {
            ops.push_str(add);
            write(&repo.join(file), &ops);
        }
        git(&repo, &["add", "-A"]);
        git(
            &repo,
            &["commit", "-q", "-m", &format!("{title}\n\nSlice: {slice}")],
        );
        commits.push((slice.to_owned(), git(&repo, &["rev-parse", "HEAD"])));
    }
    git(&repo, &["checkout", "-q", "main"]);

    let dir = zsf.join("calc");
    let worktree_04 = dir.join("worktrees/04-add-farewell");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "zsf/calc/04-add-farewell",
            worktree_04.to_str().unwrap(),
            "calc",
        ],
    );
    write(
        &worktree_04.join("calc/greet.py"),
        "def greet(name):\n    return f'Hello, {name}!'\n\n\ndef farewell(name):\n    return f'Bye, {name}!'\n",
    );
    let worktree_14 = dir.join("worktrees/14-add-max");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "zsf/calc/14-add-max",
            worktree_14.to_str().unwrap(),
            "calc",
        ],
    );
    let ops_14 = std::fs::read_to_string(worktree_14.join("calc/ops.py")).unwrap();
    write(
        &worktree_14.join("calc/ops.py"),
        &format!("{ops_14}\n\ndef max2(a, b):\n    return a if a >= b else b\n"),
    );
    // 14's run delivered a pull request and recorded its link where the graph's runs do
    // (`.zeroshot-pr/PR.md`, a declared artifact directory, git-excluded so it is not part of the diff).
    write(&repo.join(".git/info/exclude"), "/.zeroshot-pr/\n");
    write(
        &worktree_14.join(".zeroshot-pr/PR.md"),
        "https://github.com/example/calc/pull/14\n\nAdd max2().\n",
    );

    write(
        &dir.join("feature.toml"),
        &format!(
            "name = \"calc\"\nrepo = \"{}\"\nbranch = \"calc\"\nbase = \"main\"\nplan = \"plan.toml\"\nartifacts = [\"NOTES.md\", \".zeroshot-pr\"]\n\n[launch]\ngraph = \"graph.json\"\nruntime = \"runtime.json\"\n\n[hooks]\ngate = \"./test.sh\"\n\n[limits]\nmax_parallel = 2\n\n[env]\nZEROSHOT_STATE_DIR = \"{}\"\n",
            repo.display(),
            state.display()
        ),
    );
    let slices = [
        ("01", "Add sub()", vec![], false),
        ("02", "Add mul()", vec![], false),
        ("03", "Friendlier greeting", vec![], true),
        ("04", "Add farewell()", vec!["03"], false),
        ("05", "Add square()", vec!["02"], false),
        (
            "06",
            "Document the API",
            vec!["01", "02", "04", "05"],
            false,
        ),
        ("07", "Drop the legacy alias", vec![], false),
        ("08", "Add div()", vec![], false),
        ("09", "Add pow()", vec![], false),
        ("10", "Add mod()", vec![], false),
        ("11", "Hand-finished tidy-up", vec![], false),
        ("12", "Add neg()", vec![], false),
        ("13", "Add abs()", vec!["12"], false),
        ("14", "Add max()", vec![], true),
    ];
    let mut plan = String::new();
    for (id, title, deps, checkpoint) in &slices {
        plan.push_str(&format!("[[slice]]\nid = \"{id}\"\ntitle = \"{title}\"\nspec = \"specs/{id}.md\"\ndeps = {deps:?}\n"));
        if *checkpoint {
            plan.push_str("checkpoint = true\n");
        }
        plan.push('\n');
        // 14's spec is not on disk: its text would be new to the web's Markdown goldens.
        if *id != "14" {
            write(
                &dir.join(format!("specs/{id}.md")),
                &format!("# {title}\n\nImplement {title} in calc/.\n"),
            );
        }
    }
    write(&dir.join("plan.toml"), &plan);

    let task = |id: &str, n: u32| {
        dir.join(format!("tasks/{id}-a{n}.md"))
            .display()
            .to_string()
    };
    let log = |id: &str, n: u32| {
        dir.join(format!("runs/{id}-a{n}.log"))
            .display()
            .to_string()
    };
    let attempt = |id: &str,
                   n: u32,
                   kind: &str,
                   scenario: Option<&str>,
                   start: u64,
                   end: Option<u64>,
                   result: Option<&str>,
                   extra: Value|
     -> Value {
        let mut a = json!({"n": n, "kind": kind, "task_file": task(id, n), "log_file": log(id, n), "started_ms": start});
        if let Some(s) = scenario {
            a["run_id"] = json!(run_of(state, s));
        }
        if let Some(e) = end {
            a["ended_ms"] = json!(e);
        }
        if let Some(r) = result {
            a["result"] = json!(r);
        }
        for (k, v) in extra.as_object().unwrap() {
            a[k] = v.clone();
        }
        a
    };
    let commit = |slice: &str| commits.iter().find(|(s, _)| s == slice).unwrap().1.clone();
    let st = json!({
        "version": 1,
        "active": false,
        "consecutive_infra": 0,
        "note": "waiting for you: review 03, review 14, fix 04, fix 12",
        "updated_ms": T + 3_000_000,
        "slices": {
            "01": {"status": "merged", "branch": "zsf/calc/01-add-sub", "commit": commit("01"), "infra_failures": 0, "conflict_reruns": 0,
                   "attempts": [attempt("01", 1, "first", Some("succeeded"), T, Some(T + 300_000), Some("merged"), json!({"launch_key": "zsf-calc-01-a1"}))]},
            "02": {"status": "merged", "branch": "zsf/calc/02-add-mul", "commit": commit("02"), "infra_failures": 0, "conflict_reruns": 1,
                   "auto_notes": ["The previous attempt's work conflicted with 01; it is re-applied on the new tip."],
                   "attempts": [attempt("02", 1, "first", Some("delivery-failed"), T, Some(T + 200_000), Some("conflict"), json!({"backup": "zsf-backup/calc/02-a1"})),
                                attempt("02", 2, "rerun", Some("succeeded-stock"), T + 210_000, Some(T + 400_000), Some("merged"), json!({}))]},
            "03": {"status": "merged", "branch": "zsf/calc/03-friendlier-greeting", "commit": commit("03"), "infra_failures": 0, "conflict_reruns": 0,
                   "attempts": [attempt("03", 1, "first", Some("map-parallel"), T, Some(T + 100_000), Some("merged"), json!({}))]},
            "04": {"status": "failed", "reason": "gate failed: ./test.sh exited 1", "notes": ["Keep farewell() next to greet()."], "worktree": worktree_04.display().to_string(),
                   "branch": "zsf/calc/04-add-farewell", "infra_failures": 0, "conflict_reruns": 0,
                   "attempts": [attempt("04", 1, "first", Some("graph-ended-other"), T + 500_000, Some(T + 600_000), Some("gate_failed"), json!({"reason": "gate failed"})),
                                attempt("04", 2, "manual-retry", Some("voided"), T + 700_000, Some(T + 800_000), Some("gate_failed"), json!({"reason": "gate failed"}))]},
            "05": {"status": "running", "branch": "zsf/calc/05-add-square", "infra_failures": 0, "conflict_reruns": 0,
                   "attempts": [attempt("05", 1, "first", Some("stalled-live-usage"), T + 900_000, None, None, json!({}))]},
            "07": {"status": "skipped", "reason": "not needed any more", "infra_failures": 0, "conflict_reruns": 0, "attempts": []},
            "08": {"status": "interrupted", "reason": "controller lost", "infra_failures": 1, "conflict_reruns": 0,
                   "attempts": [attempt("08", 1, "first", Some("stalled"), T + 100_000, Some(T + 400_000), Some("infra_failed"), json!({"reason": "controller lost", "resume_tried": true, "earlier_runs": [run_of(state, "resumed-original")]}))]},
            "09": {"status": "launching", "infra_failures": 0, "conflict_reruns": 0, "job": {"job": "launch", "kind": "infra-retry", "fresh": false},
                   "attempts": [attempt("09", 1, "first", Some("diag-provider-down"), T, Some(T + 50_000), Some("infra_failed"), json!({}))]},
            "10": {"status": "integrating", "infra_failures": 0, "conflict_reruns": 0, "job": {"job": "integrate", "accept": false},
                   "attempts": [attempt("10", 1, "first", Some("codex-estimate"), T, Some(T + 50_000), None, json!({}))]},
            "11": {"status": "merged", "accepted": true, "commit": commit("11"), "infra_failures": 0, "conflict_reruns": 0,
                   "attempts": [attempt("11", 1, "first", Some("force-stopped"), T, Some(T + 50_000), Some("stopped"), json!({}))]},
            "12": {"status": "failed", "reason": "not accepted after 2 rounds", "infra_failures": 0, "conflict_reruns": 0,
                   "attempts": [attempt("12", 1, "first", Some("work-exhausted"), T, Some(T + 50_000), Some("quality_failed"), json!({}))]},
            "13": {"status": "pending", "retry": {"fresh": true}, "infra_failures": 0, "conflict_reruns": 0,
                   "attempts": [attempt("13", 1, "first", None, T, Some(T + 1_000), Some("launch_failed"), json!({"reason": "zeroshot run: exit 2"}))]},
            // A checkpoint: reviewed, changes requested; the retry approved, its gate failed; the
            // gate re-run waits for review again (approval cleared).
            "14": {"status": "waiting_for_review", "branch": "zsf/calc/14-add-max", "worktree": worktree_14.display().to_string(), "infra_failures": 0, "conflict_reruns": 0, "gate_reruns": 1,
                   "notes": ["Name it max2(); no sorting."],
                   "auto_notes": ["The merge gate failed on attempt 2: fix what it reports, build and test, and commit. The end of its output:\n\n```text\nFAIL: test_max2_equal\n```"],
                   "attempts": [attempt("14", 1, "first", None, T + 1_000_000, Some(T + 1_100_000), Some("changes_requested"), json!({"run_id": RUN_14[0], "reason": "changes requested: Name it max2(); no sorting."})),
                                attempt("14", 2, "manual-retry", None, T + 1_200_000, Some(T + 1_300_000), Some("gate_failed"), json!({"run_id": RUN_14[1], "reason": "FAIL: test_max2_equal"})),
                                attempt("14", 3, "gate-rerun", None, T + 1_400_000, Some(T + 1_500_000), None, json!({"run_id": RUN_14[2]}))]}
        }
    });
    write(
        &dir.join("state.json"),
        &serde_json::to_string_pretty(&st).unwrap(),
    );
    let kinds = [
        "initialized",
        "daemon_started",
        "started",
        "launched",
        "launch_failed",
        "run_finished",
        "gate_failed",
        "merged",
        "conflict",
        "quality_failed",
        "run_stopped",
        "infra_failure",
        "interrupted",
        "resumed",
        "resume_failed",
        "paused",
        "unpaused",
        "checkpoint_waiting",
        "approved",
        "retried",
        "accepted",
        "skipped",
        "edited",
        "waiting",
        "done",
        "integration_failed",
        "hook_failed",
        "request_rejected",
        "stopped",
        "daemon_stopped",
        "something_newer",
    ];
    let mut events = String::new();
    for (i, kind) in kinds.iter().enumerate() {
        let slice = ["01", "02", "03", "04", "05", "07", "08", "11", "12"][i % 9];
        let mut e = json!({"ts": T + i as u64 * 10_000, "kind": kind});
        if i > 2 {
            e["slice"] = json!(slice);
            e["attempt"] = json!(1);
        }
        if *kind == "launched" || *kind == "run_finished" {
            e["run_id"] = json!(run_of(state, "succeeded"));
        }
        e["detail"] = json!(format!("{kind} detail"));
        events.push_str(&e.to_string());
        events.push('\n');
    }
    events.push_str("not json\n");
    write(&dir.join("events.jsonl"), &events);
    write(
        &dir.join("tasks/04-a1.md"),
        "# Add farewell()\n\nImplement farewell() in calc/.\n",
    );
    write(
        &dir.join("tasks/04-a2.md"),
        "# Add farewell()\n\nImplement farewell() in calc/.\n\n## Notes from the developer\n\nKeep farewell() next to greet().\n",
    );
    write(
        &dir.join("tasks/01-a1.md"),
        "# Add sub()\n\nImplement sub() in calc/.\n",
    );
    write(
        &dir.join("tasks/14-a1.md"),
        "# Add max()\n\nImplement max() in calc/.\n",
    );
    write(
        &dir.join("tasks/14-a2.md"),
        "# Add max()\n\nImplement max() in calc/.\n\n## Notes from the developer\n\nName it max2(); no sorting.\n",
    );
    write(
        &dir.join("tasks/14-a3.md"),
        "# Add max()\n\nImplement max() in calc/.\n\n## Earlier attempt (noted by zsf)\n\nThe merge gate failed on attempt 2: fix what it reports, build and test, and commit. The end of its output:\n\n```text\nFAIL: test_max2_equal\n```\n\n## Notes from the developer\n\nName it max2(); no sorting.\n",
    );
    write(&dir.join("runs/04-a1.log"), "launched run\n");
    write(&dir.join("runs/04-a1.gate.log"), "FAIL: test_farewell\n");
    write(
        &dir.join("artifacts/04/NOTES.md"),
        "farewell() lives in greet.py\n",
    );
    write(
        &dir.join("artifacts/04/gate/out.txt"),
        "gate: missing NOTES.md\n",
    );

    // paused: active, paused after two infrastructure failures in a row.
    let paused = zsf.join("paused");
    write(
        &paused.join("feature.toml"),
        &format!(
            "name = \"paused\"\nrepo = \"{}\"\nbranch = \"paused\"\nbase = \"main\"\n\n[env]\nZEROSHOT_STATE_DIR = \"{}\"\n",
            repo.display(),
            state.display()
        ),
    );
    write(
        &paused.join("plan.toml"),
        "[[slice]]\nid = \"01\"\ntitle = \"Retry me\"\nspec = \"specs/01.md\"\n",
    );
    write(&paused.join("specs/01.md"), "# Retry me\n");
    write(
        &paused.join("state.json"),
        &serde_json::to_string_pretty(&json!({"version": 1, "active": true, "paused": "2 infrastructure failures in a row", "consecutive_infra": 2, "updated_ms": T,
            "slices": {"01": {"status": "interrupted", "reason": "provider unavailable", "infra_failures": 2, "conflict_reruns": 0,
                "attempts": [attempt("01", 1, "first", Some("diag-provider-down"), T, Some(T + 1), Some("infra_failed"), json!({})), attempt("01", 2, "infra-retry", Some("runtime-lost"), T + 2, Some(T + 3), Some("infra_failed"), json!({}))]}}}))
        .unwrap(),
    );
    // broken: zsf cannot read it.
    write(
        &zsf.join("broken/feature.toml"),
        "name = \"broken\"\nnot_a_field = 1\n",
    );
    // fresh: nothing has run yet.
    let fresh = zsf.join("fresh");
    write(
        &fresh.join("feature.toml"),
        &format!(
            "name = \"fresh\"\nrepo = \"{}\"\nbranch = \"fresh\"\nbase = \"main\"\n\n[env]\nZEROSHOT_STATE_DIR = \"{}\"\n",
            repo.display(),
            state.display()
        ),
    );
    write(
        &fresh.join("plan.toml"),
        "[[slice]]\nid = \"01\"\ntitle = \"First\"\nspec = \"specs/01.md\"\n\n[[slice]]\nid = \"02\"\ntitle = \"Second\"\nspec = \"specs/02.md\"\ndeps = [\"01\"]\n",
    );
    commits
}
