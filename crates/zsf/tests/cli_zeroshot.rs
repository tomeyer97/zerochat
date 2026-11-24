//! The real Zeroshot boundary (`ZeroshotCli`) against a fake `zeroshot` script
//! (tests/fixtures/fake-zeroshot.sh) and hand-written ledgers: launch arguments, the launch
//! hook, ledger status, and resume.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::*;
use serde_json::json;
use zsf::engine::Engine;
use zsf::model::{Action, SliceStatus};
use zsf::store::Feature;
use zsf::zeroshot::ledger::{
    CONTROLLER_READY_FILE, LEDGER_FILE, RECOVERY_FILE, RUNS_DIR, STORED_RUN_QUERY,
};

const FAKE_ZEROSHOT: &str = include_str!("fixtures/fake-zeroshot.sh");

/// A launch hook that prints its own run id line among other output, and saves the launch key.
const LAUNCH_HOOK: &str = r#"echo preparing; echo $ZSF_LAUNCH_KEY > hooked.txt; echo "{\"runId\":\"hook-$ZSF_SLICE-$ZSF_ATTEMPT\"}""#;

/// Writes the run's ledger as Zeroshot does: `<state>/runs/<run>/runs.sqlite3`, one row in
/// the table `STORED_RUN_QUERY` reads, holding `{"admitted": …, "snapshot": <snapshot>}`.
fn ledger(state: &Path, run_id: &str, snapshot: &str) {
    assert!(STORED_RUN_QUERY.contains("stored_json FROM v2_runs"));
    let dir = state.join(RUNS_DIR).join(run_id);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(LEDGER_FILE);
    let _ = std::fs::remove_file(&path);
    let snapshot: serde_json::Value = serde_json::from_str(snapshot).unwrap();
    let stored = json!({ "admitted": { "title": "t" }, "snapshot": snapshot });
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute_batch("CREATE TABLE v2_runs (stored_json TEXT)")
        .unwrap();
    c.execute("INSERT INTO v2_runs VALUES (?1)", [stored.to_string()])
        .unwrap();
}

const SUCCEEDED_RUN: &str = r#"{"phase":"finished","terminal":{"status":"succeeded"}}"#;
const RUNNING_RUN: &str = r#"{"phase":"running","terminal":null}"#;

/// Three slices launched through the fake `zeroshot` (01 and 03 by `zeroshot run`, 02 by a
/// launch hook), none of them with a ledger yet.
struct CliEnv {
    env: Env,
    engine: Engine,
    /// Where the fake records its calls.
    fake_dir: PathBuf,
    /// Zeroshot's state directory (the feature's `ZEROSHOT_STATE_DIR`).
    zs_state: PathBuf,
}

fn cli_env() -> CliEnv {
    let env = setup_plan(
        &[
            plan_slice("01"),
            plan_slice("02").launch_hook(LAUNCH_HOOK),
            plan_slice("03"),
        ],
        |c| c.limits.max_parallel = 3,
    );
    let tmp = env.tmp.path();
    let fake_dir = tmp.join("fake");
    let zs_state = tmp.join("zs");
    std::fs::create_dir_all(&fake_dir).unwrap();
    let script = tmp.join("zeroshot");
    std::fs::write(&script, FAKE_ZEROSHOT).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(tmp.join("graph.json"), "{}").unwrap();
    std::fs::write(tmp.join("runtime.json"), "{}").unwrap();

    let mut feature = Feature::open(&env.root, "feat").unwrap();
    let launch = &mut feature.config.launch;
    launch.zeroshot = Some(script.to_string_lossy().into_owned());
    launch.graph = Some(tmp.join("graph.json"));
    launch.runtime = Some(tmp.join("runtime.json"));
    let vars = &mut feature.config.env;
    vars.insert(
        "ZEROSHOT_STATE_DIR".into(),
        zs_state.to_string_lossy().into_owned(),
    );
    vars.insert(
        "FAKE_ZS_DIR".into(),
        fake_dir.to_string_lossy().into_owned(),
    );
    feature.save_config().unwrap();

    let zeroshot = zsf::daemon::zeroshot_for(&feature);
    let mut engine = Engine::open(feature, zeroshot).unwrap();
    engine.apply(&Action::Start).unwrap();
    engine.run_until_idle().unwrap();
    CliEnv {
        env,
        engine,
        fake_dir,
        zs_state,
    }
}

impl CliEnv {
    fn run_id(&self, slice: &str) -> String {
        self.engine.state.slices[slice].attempts[0]
            .run_id
            .clone()
            .unwrap()
    }

    /// What the fake recorded for the launch that made `run_id` (`args`, `cwd`).
    fn recorded(&self, what: &str, run_id: &str) -> String {
        // The fake numbers launches 1, 2 in call order and ends each run id with its number.
        let n = if run_id.ends_with('1') { 1 } else { 2 };
        std::fs::read_to_string(self.fake_dir.join(format!("{what}-{n}"))).unwrap()
    }
}

#[test]
fn launches_with_zeroshot_run_in_the_worktree() {
    let cli = cli_env();
    let e = &cli.engine;
    let tmp = cli.env.tmp.path();
    let run1 = cli.run_id("01");
    // No ledger yet: still starting, within the grace period.
    assert_eq!(e.state.slices["01"].status, SliceStatus::Running);

    // zeroshot run --title … --graph … --runtime-config … --input … --detach, in the worktree.
    let args: Vec<String> = cli
        .recorded("args", &run1)
        .lines()
        .map(str::to_owned)
        .collect();
    let pos = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("{flag} missing in {args:?}"))
    };
    assert_eq!(args[0], "run");
    assert_eq!(args[pos("--title") + 1], "feat 01: Slice 01");
    assert_eq!(Path::new(&args[pos("--graph") + 1]), tmp.join("graph.json"));
    assert_eq!(
        Path::new(&args[pos("--runtime-config") + 1]),
        tmp.join("runtime.json")
    );
    assert!(args.contains(&"--detach".to_owned()));
    let key = e.state.slices["01"].attempts[0].launch_key.clone().unwrap();
    assert!(key.starts_with("zsf-feat-01-a1-"), "{key}");
    assert_eq!(args[pos("--submission-key") + 1], key);
    let input: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args[pos("--input") + 1]).unwrap()).unwrap();
    assert!(input["task"].as_str().unwrap().contains("Do slice 01."));
    let cwd = cli.recorded("cwd", &run1);
    assert_eq!(
        Path::new(cwd.trim()),
        e.state.slices["01"].worktree.as_deref().unwrap()
    );
}

#[test]
fn a_launch_hook_replaces_zeroshot_run() {
    let cli = cli_env();
    let s2 = &cli.engine.state.slices["02"];
    assert_eq!(s2.attempts[0].run_id.as_deref(), Some("hook-02-1"));
    let hooked = std::fs::read_to_string(s2.worktree.as_ref().unwrap().join("hooked.txt")).unwrap();
    assert_eq!(
        hooked.trim(),
        s2.attempts[0].launch_key.as_deref().unwrap(),
        "ZSF_LAUNCH_KEY reaches the launch hook"
    );
}

#[test]
fn reads_the_ledger_and_resumes_a_dead_controller() {
    let mut cli = cli_env();
    let (run1, run3) = (cli.run_id("01"), cli.run_id("03"));
    let zs_state = cli.zs_state.clone();

    // 01 and the hook's run finish; 03's controller is dead, so it is resumed once.
    ledger(&zs_state, &run1, SUCCEEDED_RUN);
    ledger(&zs_state, "hook-02-1", SUCCEEDED_RUN);
    ledger(&zs_state, &run3, RUNNING_RUN);
    std::fs::write(
        zs_state
            .join(RUNS_DIR)
            .join(&run3)
            .join(CONTROLLER_READY_FILE),
        r#"{"pid":999999999}"#,
    )
    .unwrap();
    let e = &mut cli.engine;
    e.run_until_idle().unwrap();
    assert_eq!(e.state.slices["01"].status, SliceStatus::Merged);
    assert_eq!(e.state.slices["02"].status, SliceStatus::Merged);
    let a3 = &e.state.slices["03"].attempts[0];
    assert_eq!(
        a3.run_id.as_deref(),
        Some(format!("{run3}-resumed").as_str())
    );
    assert_eq!(a3.earlier_runs, std::slice::from_ref(&run3));
    // Before resuming, zsf let Zeroshot look at the lost run (with the feature's state directory).
    let observed = std::fs::read_to_string(cli.fake_dir.join(format!("observed-{run3}"))).unwrap();
    assert_eq!(Path::new(observed.trim()), zs_state);

    ledger(&zs_state, &format!("{run3}-resumed"), SUCCEEDED_RUN);
    let tick = cli.engine.run_until_idle().unwrap();
    assert!(tick.done);
    let body = git(&cli.env.repo, &["log", "-1", "--format=%B", BRANCH]);
    assert!(
        body.contains(&format!("Zeroshot-Run: {run3}-resumed")),
        "{body}"
    );
}

#[test]
fn finds_runs_by_launch_key_and_successors_in_the_state_directory() {
    use zsf::zeroshot::Zeroshot;
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path();
    let recovery = |run: &str, doc: &str| {
        std::fs::write(state.join(RUNS_DIR).join(run).join(RECOVERY_FILE), doc).unwrap()
    };
    // Admitted (ledger row) with the key; a failed submission with the same key has no row.
    ledger(
        state,
        "r-admitted",
        r#"{"runId":"r-admitted","phase":"running"}"#,
    );
    recovery(
        "r-admitted",
        r#"{"submission":{"submissionKey":"zsf-feat-01-a1-x"},"workspace":"/w","successorRunId":"r-next"}"#,
    );
    std::fs::create_dir_all(state.join(RUNS_DIR).join("r-unadmitted")).unwrap();
    recovery(
        "r-unadmitted",
        r#"{"submission":{"submissionKey":"zsf-feat-02-a1-x"},"workspace":"/w"}"#,
    );
    let cli = zsf::zeroshot::ZeroshotCli::new("zeroshot", state.to_path_buf());
    assert_eq!(
        cli.find_run("zsf-feat-01-a1-x").as_deref(),
        Some("r-admitted")
    );
    assert_eq!(cli.find_run("zsf-feat-02-a1-x"), None);
    assert_eq!(cli.find_run("zsf-feat-01-a1"), None, "keys match exactly");
    // The successor counts once its own ledger admitted it.
    assert_eq!(cli.successor("r-admitted"), None);
    ledger(state, "r-next", r#"{"runId":"r-next","phase":"running"}"#);
    assert_eq!(cli.successor("r-admitted").as_deref(), Some("r-next"));
    assert_eq!(
        zsf::zeroshot::already_admitted(
            "zeroshot: submission key already identifies a different admitted run (01a1ab80-74e9)\n"
        )
        .as_deref(),
        Some("01a1ab80-74e9")
    );
}
