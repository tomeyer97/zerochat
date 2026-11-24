// Each tests/*.rs compiles its own copy of this module and uses only some helpers; the rest
// would warn as dead code in every binary that does not need them.
#![allow(dead_code)]
//! Test helpers: a temp git repository, a feature on it, and a scripted fake Zeroshot.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use zsf::engine::{Engine, Tick};
use zsf::hooks::HookEnv;
use zsf::model::{
    Action, EventKind, FeatureConfig, Plan, SlicePlan, SliceState, SliceStatus, State,
};
use zsf::store::Feature;
use zsf::zeroshot::{LaunchRequest, RunStatus, Zeroshot};

// Terminal statuses and reasons in Zeroshot's vocabulary (what `zeroshot status` reports).
pub const SUCCEEDED: &str = "succeeded";
pub const FAILED: &str = "failed";

/// Polls `cond` until it holds; fails the test after 30 seconds.
pub fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---------------------------------------------------------------------------------------------
// The plan: `setup_plan(&[plan_slice("01").checkpoint(), plan_slice("02").after(&["01"])], …)`.

/// A slice `id` titled `Slice <id>`, its spec at `specs/<id>.md` (written by [`setup`]).
pub fn plan_slice(id: &str) -> SlicePlan {
    SlicePlan {
        id: id.to_owned(),
        title: format!("Slice {id}"),
        spec: PathBuf::from(format!("specs/{id}.md")),
        ..Default::default()
    }
}

/// Slices `01`, `02`, … with no dependencies.
pub fn independent(ids: &[&str]) -> Vec<SlicePlan> {
    ids.iter().map(|id| plan_slice(id)).collect()
}

/// Builder methods for the slices of a test plan.
pub trait SliceBuilder {
    fn after(self, deps: &[&str]) -> Self;
    fn checkpoint(self) -> Self;
    fn leases(self, leases: &[&str]) -> Self;
    fn launch_hook(self, command: &str) -> Self;
}

impl SliceBuilder for SlicePlan {
    fn after(mut self, deps: &[&str]) -> Self {
        self.deps = deps.iter().map(|d| d.to_string()).collect();
        self
    }

    fn checkpoint(mut self) -> Self {
        self.checkpoint = true;
        self
    }

    fn leases(mut self, leases: &[&str]) -> Self {
        self.leases = leases.iter().map(|l| l.to_string()).collect();
        self
    }

    fn launch_hook(mut self, command: &str) -> Self {
        self.hooks.launch = Some(command.to_owned());
        self
    }
}

// ---------------------------------------------------------------------------------------------
// The fake Zeroshot.

/// What the fake run of one attempt does.
#[derive(Clone, Debug)]
pub enum Run {
    /// Writes the files (uncommitted, like an agent) and ends with this terminal status.
    Ends {
        files: Vec<(String, String)>,
        status: &'static str,
        reason: Option<&'static str>,
    },
    /// Writes the files and keeps running until the test finishes it.
    Runs { files: Vec<(String, String)> },
    /// The launch itself fails.
    LaunchFails(&'static str),
}

pub fn succeeds(files: &[(&str, &str)]) -> Run {
    Run::Ends {
        files: owned(files),
        status: SUCCEEDED,
        reason: None,
    }
}

pub fn fails(reason: &'static str) -> Run {
    fails_after_writing(&[], reason)
}

/// Writes the files, then fails (the work stays in the worktree).
pub fn fails_after_writing(files: &[(&str, &str)], reason: &'static str) -> Run {
    Run::Ends {
        files: owned(files),
        status: FAILED,
        reason: Some(reason),
    }
}

pub fn runs(files: &[(&str, &str)]) -> Run {
    Run::Runs {
        files: owned(files),
    }
}

fn owned(files: &[(&str, &str)]) -> Vec<(String, String)> {
    files
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[derive(Clone, Debug)]
pub struct Launch {
    pub slice: String,
    pub attempt: u32,
    pub run_id: String,
    pub task: String,
    pub worktree: PathBuf,
    /// Files in the worktree root when the run started.
    pub files_at_start: Vec<String>,
    /// The launch key, and whether state.json already held it when the run started.
    pub key: Option<String>,
    pub key_saved_before: bool,
}

#[derive(Default)]
pub struct FakeState {
    pub next: u32,
    pub scripts: BTreeMap<String, VecDeque<Run>>,
    pub runs: BTreeMap<String, RunStatus>,
    pub run_slice: BTreeMap<String, String>,
    pub launches: Vec<Launch>,
    pub resumes: Vec<String>,
    /// Runs whose resume fails (with this message); others resume into a running successor.
    pub resume_fails: BTreeMap<String, String>,
    pub force_stopped: Vec<String>,
    /// Launch key → run (a launch with a known key returns its run, like `--submission-key`).
    pub keys: BTreeMap<String, String>,
    /// Run → successor made by a resume.
    pub successors: BTreeMap<String, String>,
    /// Most runs running at once (counting the one just started).
    pub max_running: usize,
}

impl FakeState {
    fn new_run_id(&mut self) -> String {
        self.next += 1;
        format!("run-{:03}", self.next)
    }

    /// The slice's next scripted run; unscripted slices write `<slice>.txt` and succeed.
    fn next_script(&mut self, slice: &str) -> Run {
        self.scripts
            .get_mut(slice)
            .and_then(|q| q.pop_front())
            .unwrap_or_else(|| succeeds(&[(&format!("{slice}.txt"), &format!("slice {slice}\n"))]))
    }
}

#[derive(Default)]
pub struct Fake {
    pub state: Mutex<FakeState>,
}

impl Fake {
    pub fn script(&self, slice: &str, runs: Vec<Run>) {
        self.state
            .lock()
            .unwrap()
            .scripts
            .entry(slice.to_owned())
            .or_default()
            .extend(runs);
    }

    pub fn launches(&self) -> Vec<Launch> {
        self.state.lock().unwrap().launches.clone()
    }

    pub fn launched_slices(&self) -> Vec<String> {
        self.launches().into_iter().map(|l| l.slice).collect()
    }

    /// Launched slices, sorted (slices launched in the same tick start on parallel threads).
    pub fn launched_set(&self) -> Vec<String> {
        let mut v = self.launched_slices();
        v.sort();
        v
    }

    pub fn last_run(&self, slice: &str) -> String {
        self.launches()
            .iter()
            .rev()
            .find(|l| l.slice == slice)
            .map(|l| l.run_id.clone())
            .expect("a run")
    }

    pub fn set(&self, run_id: &str, status: RunStatus) {
        self.state
            .lock()
            .unwrap()
            .runs
            .insert(run_id.to_owned(), status);
    }

    /// The run ends with this terminal status (and reason).
    pub fn finish(&self, run_id: &str, status: &str, reason: Option<&str>) {
        self.set(
            run_id,
            RunStatus::Finished {
                status: status.to_owned(),
                reason: reason.map(str::to_owned),
            },
        );
    }

    /// Runs resumed so far, in order.
    pub fn resumes(&self) -> Vec<String> {
        self.state.lock().unwrap().resumes.clone()
    }

    pub fn resume_count(&self) -> usize {
        self.state.lock().unwrap().resumes.len()
    }

    /// Resuming `run` will fail with this message.
    pub fn fail_resume(&self, run: &str, message: &str) {
        self.state
            .lock()
            .unwrap()
            .resume_fails
            .insert(run.to_owned(), message.to_owned());
    }

    /// The successor a resume made of `run`.
    pub fn successor_of(&self, run: &str) -> String {
        self.state.lock().unwrap().successors[run].clone()
    }

    /// Someone resumed `run` by hand into `successor` (zsf did not).
    pub fn adopt(&self, run: &str, successor: &str) {
        self.state
            .lock()
            .unwrap()
            .successors
            .insert(run.to_owned(), successor.to_owned());
    }

    pub fn force_stopped(&self) -> Vec<String> {
        self.state.lock().unwrap().force_stopped.clone()
    }

    /// Most runs that were running at once.
    pub fn max_running(&self) -> usize {
        self.state.lock().unwrap().max_running
    }

    /// Zeroshot forgets every launch, run and launch key (as if no launch had reached it).
    pub fn forget_everything(&self) {
        let mut st = self.state.lock().unwrap();
        st.keys.clear();
        st.launches.clear();
        st.runs.clear();
    }
}

/// Plays a scripted run in the worktree: writes its files and returns its first status.
fn apply_script(script: Run, worktree: &Path) -> Result<RunStatus> {
    let (files, status) = match script {
        Run::LaunchFails(why) => bail!("{why}"),
        Run::Ends {
            files,
            status,
            reason,
        } => (
            files,
            RunStatus::Finished {
                status: status.to_owned(),
                reason: reason.map(str::to_owned),
            },
        ),
        Run::Runs { files } => (files, RunStatus::Running),
    };
    for (path, content) in files {
        std::fs::write(worktree.join(path), content).unwrap();
    }
    Ok(status)
}

/// Names in the worktree root (without dot files), sorted.
fn visible_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// Whether the writer's saved state.json already has an attempt of `slice` with this key.
fn key_saved(req: &LaunchRequest, key: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(req.env.state_dir.join("state.json")) else {
        return false;
    };
    let state: State = serde_json::from_str(&text).unwrap();
    state.slices.get(&req.env.slice).is_some_and(|s| {
        s.attempts
            .iter()
            .any(|a| a.launch_key.as_deref() == Some(key))
    })
}

impl Fake {
    /// Records a started run: its status, its launch key and what the launch saw.
    fn record_launch(
        st: &mut FakeState,
        req: &LaunchRequest,
        status: RunStatus,
        files_at_start: Vec<String>,
    ) -> String {
        let run_id = st.new_run_id();
        st.runs.insert(run_id.clone(), status);
        st.run_slice.insert(run_id.clone(), req.env.slice.clone());
        let key = req.submission_key.clone();
        let key_saved_before = key.as_deref().is_some_and(|k| key_saved(req, k));
        if let Some(k) = &key {
            st.keys.insert(k.clone(), run_id.clone());
        }
        let running = st
            .runs
            .values()
            .filter(|r| **r == RunStatus::Running)
            .count();
        st.max_running = st.max_running.max(running);
        st.launches.push(Launch {
            slice: req.env.slice.clone(),
            attempt: req.env.attempt,
            run_id: run_id.clone(),
            task: std::fs::read_to_string(&req.task_file).unwrap(),
            worktree: req.worktree.clone(),
            files_at_start,
            key,
            key_saved_before,
        });
        run_id
    }
}

impl Zeroshot for Fake {
    fn launch(&self, req: &LaunchRequest) -> Result<String> {
        let mut st = self.state.lock().unwrap();
        // A known submission key returns its run, like `zeroshot run --submission-key`.
        if let Some(run) = req.submission_key.as_ref().and_then(|k| st.keys.get(k)) {
            return Ok(run.clone());
        }
        let script = st.next_script(&req.env.slice);
        let files_at_start = visible_files(&req.worktree);
        let status = apply_script(script, &req.worktree)?;
        Ok(Self::record_launch(&mut st, req, status, files_at_start))
    }

    fn status(&self, run_id: &str) -> RunStatus {
        self.state
            .lock()
            .unwrap()
            .runs
            .get(run_id)
            .cloned()
            .unwrap_or(RunStatus::NotStarted)
    }

    fn resume(&self, run_id: &str, _cwd: &Path, _env: &HookEnv, _log: &Path) -> Result<String> {
        let mut st = self.state.lock().unwrap();
        st.resumes.push(run_id.to_owned());
        if let Some(e) = st.resume_fails.get(run_id) {
            bail!("{e}");
        }
        let id = st.new_run_id();
        st.runs.insert(id.clone(), RunStatus::Running);
        st.successors.insert(run_id.to_owned(), id.clone());
        Ok(id)
    }

    fn find_run(&self, submission_key: &str) -> Option<String> {
        self.state.lock().unwrap().keys.get(submission_key).cloned()
    }

    fn successor(&self, run_id: &str) -> Option<String> {
        self.state.lock().unwrap().successors.get(run_id).cloned()
    }

    fn force_stop(&self, run_id: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        st.force_stopped.push(run_id.to_owned());
        st.runs.insert(
            run_id.to_owned(),
            RunStatus::Finished {
                status: "stopped".into(),
                reason: Some("force_stopped".into()),
            },
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// The repository and the feature.

pub struct Env {
    pub tmp: tempfile::TempDir,
    pub repo: PathBuf,
    pub root: PathBuf,
    pub fake: Arc<Fake>,
    pub feature: Feature,
}

pub const BRANCH: &str = "feature/x";

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

pub fn new_repo(dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "Test"),
        ("user.email", "test@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join("README.md"), "toy\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    repo
}

/// A feature `feat` on a fresh repository. Slices: (id, deps, checkpoint); new tests read
/// better with [`setup_plan`].
pub fn setup(
    slices: &[(&str, &[&str], bool)],
    configure: impl FnOnce(&mut FeatureConfig) + 'static,
) -> Env {
    let slices: Vec<SlicePlan> = slices
        .iter()
        .map(|(id, deps, checkpoint)| SlicePlan {
            checkpoint: *checkpoint,
            ..plan_slice(id).after(deps)
        })
        .collect();
    setup_plan(&slices, configure)
}

/// A feature `feat` on a fresh repository with these slices (see [`plan_slice`]); each spec
/// says `Do slice <id>.`.
pub fn setup_plan(
    slices: &[SlicePlan],
    configure: impl FnOnce(&mut FeatureConfig) + 'static,
) -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let repo = new_repo(tmp.path());
    std::fs::create_dir_all(tmp.path().join("specs")).unwrap();
    for s in slices {
        std::fs::write(
            tmp.path().join(&s.spec),
            format!("# {id}: Slice {id}\n\nDo slice {id}.\n", id = s.id),
        )
        .unwrap();
    }
    let plan = Plan {
        slices: slices.to_vec(),
    };
    let plan_path = tmp.path().join("plan.toml");
    std::fs::write(&plan_path, toml::to_string(&plan).unwrap()).unwrap();
    let root = tmp.path().join("state");
    let done = zsf::setup::init(
        &root,
        zsf::setup::InitOptions {
            name: "feat".into(),
            repo: repo.clone(),
            branch: BRANCH.into(),
            base: "main".into(),
            plan: Some(plan_path),
            refined: None,
            checkpoints: vec![],
            configure: Box::new(move |c| {
                c.limits.poll_secs = 1;
                c.limits.start_grace_secs = 3600;
                configure(c);
            }),
        },
    )
    .unwrap();
    Env {
        tmp,
        repo,
        root,
        fake: Arc::new(Fake::default()),
        feature: done.feature,
    }
}

impl Env {
    pub fn engine(&self) -> Engine {
        let feature = Feature::open(&self.root, "feat").unwrap();
        Engine::open(feature, self.fake.clone()).unwrap()
    }

    /// A started engine.
    pub fn started(&self) -> Engine {
        let mut e = self.engine();
        e.apply(&Action::Start).unwrap();
        e
    }

    /// The slice's newest run succeeds, and the engine runs until idle.
    pub fn succeed(&self, e: &mut Engine, slice: &str) -> Tick {
        self.fake
            .finish(&self.fake.last_run(slice), SUCCEEDED, None);
        e.run_until_idle().unwrap()
    }

    /// The slice's newest run fails for this reason, and the engine runs until idle.
    pub fn fail(&self, e: &mut Engine, slice: &str, reason: &str) -> Tick {
        self.fake
            .finish(&self.fake.last_run(slice), FAILED, Some(reason));
        e.run_until_idle().unwrap()
    }

    pub fn plan(&self) -> Plan {
        self.feature.load_plan().unwrap()
    }

    /// Commit subjects on the feature branch since main, oldest first.
    pub fn merged_subjects(&self) -> Vec<String> {
        let log = git(
            &self.repo,
            &[
                "log",
                "--reverse",
                "--format=%s",
                &format!("main..{BRANCH}"),
            ],
        );
        log.lines().map(str::to_owned).collect()
    }

    pub fn events(&self) -> Vec<zsf::model::Event> {
        self.feature.events()
    }

    /// The kinds of every event, oldest first.
    pub fn all_event_kinds(&self) -> Vec<EventKind> {
        self.events().iter().map(|e| e.kind).collect()
    }

    /// The kinds of the slice's events, oldest first.
    pub fn event_kinds(&self, slice: &str) -> Vec<EventKind> {
        self.events()
            .iter()
            .filter(|e| e.slice.as_deref() == Some(slice))
            .map(|e| e.kind)
            .collect()
    }
}

pub fn slice<'a>(engine: &'a Engine, id: &str) -> &'a SliceState {
    &engine.state.slices[id]
}

pub fn status(engine: &Engine, id: &str) -> SliceStatus {
    slice(engine, id).status
}
