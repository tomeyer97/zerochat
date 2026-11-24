//! A zsf state built with the zsf library: feature `calc` with four slices.
//! 01 checkpoint merged, not approved · 02 pending, depends on 01 · 03 failed after two
//! attempts (worktree kept, notes, an artifact) · 04 pending, depends on 03.
//! The server tests use it too (`crate::features::fixture`).

use std::path::{Path, PathBuf};

use zsf::model::*;
use zsf::store::Feature;

pub const RUN_03_A2: &str = "01a1ab88-e21f-7c41-bc66-66d8cf6e6923";

fn slice(id: &str, deps: &[&str], checkpoint: bool) -> SlicePlan {
    SlicePlan {
        id: id.into(),
        title: format!("Slice {id}"),
        spec: PathBuf::from(format!("specs/{id}.md")),
        deps: deps.iter().map(|d| d.to_string()).collect(),
        checkpoint,
        leases: vec![],
        commit_title: None,
        description: None,
        graph: None,
        runtime: None,
        args: None,
        hooks: Hooks::default(),
    }
}

fn attempt(
    dir: &Path,
    id: &str,
    n: u32,
    kind: AttemptKind,
    run: &str,
    result: AttemptResult,
) -> Attempt {
    Attempt {
        n,
        kind,
        fresh: false,
        run_id: Some(run.into()),
        earlier_runs: vec![],
        task_file: dir.join(format!("tasks/{id}-a{n}.md")),
        log_file: None,
        started_ms: 1_000 * n as u64,
        ended_ms: Some(1_000 * n as u64 + 500),
        resumed_ms: None,
        result: Some(result),
        reason: None,
        resume_tried: false,
        launch_key: None,
        backup: None,
    }
}

/// Builds the feature under `root` (`root/calc`); returns the feature.
pub fn build(root: &Path) -> Feature {
    let dir = root.join("calc");
    std::fs::create_dir_all(dir.join("specs")).unwrap();
    std::fs::create_dir_all(dir.join("tasks")).unwrap();
    std::fs::create_dir_all(dir.join("artifacts/03/gate")).unwrap();
    let worktree = dir.join("worktrees/03-slice-03");
    std::fs::create_dir_all(&worktree).unwrap();
    let config: FeatureConfig = toml::from_str(&format!(
        "name = \"calc\"\nrepo = \"{}\"\nbranch = \"calc\"\nbase = \"main\"\n",
        root.join("no-repo").display()
    ))
    .unwrap();
    let feature = Feature {
        dir: dir.clone(),
        config,
    };
    feature.save_config().unwrap();
    let plan = Plan {
        slices: vec![
            slice("01", &[], true),
            slice("02", &["01"], false),
            slice("03", &[], false),
            slice("04", &["03"], false),
        ],
    };
    feature.save_plan(&plan).unwrap();
    for id in ["01", "02", "03", "04"] {
        std::fs::write(
            dir.join(format!("specs/{id}.md")),
            format!("# {id}\nDo {id}.\n"),
        )
        .unwrap();
    }
    let notes = "Use the existing parser.";
    std::fs::write(dir.join("tasks/03-a1.md"), "# 03\nDo 03.\n").unwrap();
    std::fs::write(
        dir.join("tasks/03-a2.md"),
        format!("# 03\nDo 03.\n\n## Notes from the developer\n\n{notes}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("artifacts/03/gate/out.txt"),
        "gate: missing NOTES.md\n",
    )
    .unwrap();
    let mut state = State::default();
    state.slices.insert(
        "01".into(),
        SliceState {
            status: SliceStatus::Merged,
            commit: Some("abc123".into()),
            // A checkpoint (see the plan) not approved yet: it waits for a person.
            approved: false,
            attempts: vec![attempt(
                &dir,
                "01",
                1,
                AttemptKind::First,
                "01a1ab83-0000-7000-8000-000000000001",
                AttemptResult::Merged,
            )],
            ..Default::default()
        },
    );
    state.slices.insert(
        "03".into(),
        SliceState {
            status: SliceStatus::Failed,
            reason: Some("gate failed".into()),
            notes: vec![notes.into()],
            worktree: Some(worktree),
            attempts: vec![
                attempt(
                    &dir,
                    "03",
                    1,
                    AttemptKind::First,
                    "01a1ab83-0000-7000-8000-000000000031",
                    AttemptResult::GateFailed,
                ),
                attempt(
                    &dir,
                    "03",
                    2,
                    AttemptKind::ManualRetry,
                    RUN_03_A2,
                    AttemptResult::GateFailed,
                ),
            ],
            ..Default::default()
        },
    );
    state.slices.insert(
        "04".into(),
        SliceState {
            status: SliceStatus::Pending,
            ..Default::default()
        },
    );
    feature.save_state(&mut state).unwrap();
    for (slice, kind) in [
        (None, EventKind::Initialized),
        (Some("01"), EventKind::Merged),
        (Some("03"), EventKind::GateFailed),
        (Some("01"), EventKind::CheckpointWaiting),
    ] {
        feature
            .append_event(&Event {
                ts: 5,
                slice: slice.map(str::to_owned),
                attempt: None,
                kind,
                run_id: None,
                detail: String::new(),
            })
            .unwrap();
    }
    feature
}
