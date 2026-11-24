//! The long work of a slice — worktree + launch, gate + integration, resume — run on job threads
//! without access to the state. Results go back to the engine, which alone changes the state.
//!
//! Sections: the names zsf gives a slice's worktree, branch and attempt files; the texts the
//! agent reads; then one section per job (launch, integrate, resume).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};

use crate::git::{self, git};
use crate::hooks::{self, HookEnv};
use crate::model::{Attempt, AttemptKind, Carry, SLICE_TRAILER, SlicePlan, SliceState, slug};
use crate::store::{Feature, expand_home};
use crate::time::now_ms;
use crate::zeroshot::{LaunchRequest, Zeroshot};

// `copy_dir` lives in `sys`; this path stays for the callers that use it from here.
pub(crate) use crate::sys::copy_dir;

/// How much of a failed hook's output goes into reasons and events (characters, from the end).
const HOOK_OUTPUT_TAIL: usize = 400;
/// The gate's output is the reason a slice failed, so it keeps more.
const GATE_OUTPUT_TAIL: usize = 600;

/// The commit trailer naming the Zeroshot run that made the change.
const RUN_TRAILER: &str = "Zeroshot-Run";

/// What a job thread may use.
#[derive(Clone)]
pub struct Ctx {
    pub feature: Feature,
    pub zeroshot: Arc<dyn Zeroshot>,
    /// Serialises every change to the main repository (worktrees, refs, the merge).
    pub repo_lock: Arc<Mutex<()>>,
}

impl Ctx {
    pub fn repo(&self) -> PathBuf {
        self.feature.resolve(&self.feature.config.repo)
    }

    pub fn prefix(&self) -> String {
        self.feature
            .config
            .commit_prefix
            .clone()
            .unwrap_or_else(|| self.feature.config.name.clone())
    }

    pub fn env(
        &self,
        slice: &SlicePlan,
        attempt: u32,
        worktree: Option<&Path>,
        branch: &str,
    ) -> HookEnv {
        HookEnv {
            feature: self.feature.config.name.clone(),
            slice: slice.id.clone(),
            attempt,
            worktree: worktree.map(Path::to_path_buf),
            branch: self.feature.config.branch.clone(),
            slice_branch: branch.to_owned(),
            context: self.feature.context_dir(),
            run_id: None,
            task_file: None,
            launch_key: None,
            state_dir: self.feature.dir.clone(),
            extra: self
                .feature
                .config
                .env
                .iter()
                .map(|(k, v)| (k.clone(), expand_home(v)))
                .collect(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.repo_lock.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// ---------------------------------------------------------------- names

/// The slice's worktree: `<worktrees>/<id>-<slug>`.
pub fn worktree_path(feature: &Feature, slice: &SlicePlan) -> PathBuf {
    feature
        .worktrees_dir()
        .join(format!("{}-{}", slice.id, slug(&slice.title)))
}

/// The slice's working branch: `zsf/<feature branch>/<id>-<slug>`.
pub fn slice_branch(feature: &Feature, slice: &SlicePlan) -> String {
    format!(
        "zsf/{}/{}-{}",
        feature.config.branch,
        slice.id,
        slug(&slice.title)
    )
}

/// The branch a conflict keeps attempt `n`'s work on: `zsf-backup/<feature branch>/<id>-a<n>`.
fn backup_branch(feature: &Feature, slice: &str, n: u32) -> String {
    format!("zsf-backup/{}/{slice}-a{n}", feature.config.branch)
}

/// The files of attempt `n` of a slice, in the feature's state directory.
pub struct AttemptFiles {
    /// The task the attempt was given: `tasks/<id>-a<n>.md` (its input `tasks/<id>-a<n>.input.json`).
    pub task: PathBuf,
    /// The launch's output: `runs/<id>-a<n>.log`.
    pub log: PathBuf,
    /// The gate hook's output: `runs/<id>-a<n>.gate.log`.
    pub gate_log: PathBuf,
    /// `zeroshot resume`'s output: `runs/<id>-a<n>.resume.log`.
    pub resume_log: PathBuf,
}

impl AttemptFiles {
    pub fn new(feature: &Feature, slice: &str, n: u32) -> AttemptFiles {
        let runs = feature.runs_dir();
        AttemptFiles {
            task: feature.tasks_dir().join(format!("{slice}-a{n}.md")),
            log: runs.join(format!("{slice}-a{n}.log")),
            gate_log: runs.join(format!("{slice}-a{n}.gate.log")),
            resume_log: runs.join(format!("{slice}-a{n}.resume.log")),
        }
    }
}

/// A launch key unique to this attempt: `zsf-<feature>-<slice>-a<n>-<now in ms, base 36>`.
pub fn launch_key(feature: &str, slice: &str, n: u32) -> String {
    format!("zsf-{feature}-{slice}-a{n}-{}", base36(now_ms()))
}

fn base36(mut value: u64) -> String {
    let mut digits = String::new();
    while value > 0 {
        digits.insert(0, char::from_digit((value % 36) as u32, 36).unwrap_or('0'));
        value /= 36;
    }
    digits
}

// ---------------------------------------------------------------- texts for the agent

const AUTO_NOTES_HEADING: &str = "## Earlier attempt (noted by zsf)";
const DEVELOPER_NOTES_HEADING: &str = "## Notes from the developer";

/// The task an attempt gets: the spec, where the context is, zsf's notes about earlier attempts,
/// and the developer's notes (each under its own heading, so nobody mistakes one for the other).
pub fn task_text(
    spec: &str,
    context: Option<&str>,
    notes: &[String],
    auto_notes: &[String],
) -> String {
    let mut text = spec.trim_end().to_owned();
    text.push('\n');
    if let Some(context) = context {
        text.push_str(&format!(
            "\n---\n\nReference documents for the whole feature: {context} \
             (read-only; do not commit them).\n"
        ));
    }
    for (heading, notes) in [
        (AUTO_NOTES_HEADING, auto_notes),
        (DEVELOPER_NOTES_HEADING, notes),
    ] {
        if notes.is_empty() {
            continue;
        }
        text.push_str(&format!("\n{heading}\n"));
        for note in notes {
            text.push('\n');
            text.push_str(note.trim());
            text.push('\n');
        }
    }
    text
}

/// zsf's note for a conflict re-run whose earlier work did not apply onto the new tip.
fn carried_work_note(carry: &Carry, conflicts: &[String]) -> String {
    let files = if conflicts.is_empty() {
        "the reported files".to_owned()
    } else {
        conflicts.join(", ")
    };
    format!(
        "This slice was already implemented in a previous attempt: commit {commit} (branch \
         {backup}). Other slices merged meanwhile and it no longer applies cleanly. Start by \
         re-applying it with `git cherry-pick {commit}`, resolve the conflicts in {files} keeping \
         both sides' intent, build and test, and commit. Do not re-implement the slice from \
         scratch.",
        commit = carry.commit,
        backup = carry.backup,
    )
}

/// zsf's note for a gate re-run: the merge gate failed on attempt `n` with this output (its tail).
pub fn gate_note(n: u32, output: &str) -> String {
    format!(
        "The merge gate failed on attempt {n}: fix what it reports, build and test, and commit. \
         The end of its output:\n\n```text\n{}\n```",
        output.trim()
    )
}

// ---------------------------------------------------------------- launch

/// What a launch job gets: a snapshot of the slice's plan, state and attempt, taken by the
/// engine, because job threads never read the state.
pub struct LaunchJob {
    pub slice: SlicePlan,
    pub n: u32,
    pub kind: AttemptKind,
    pub fresh: bool,
    pub notes: Vec<String>,
    /// The texts of zsf's own notes (the previous work after a conflict, the gate's output).
    pub auto_notes: Vec<String>,
    /// The earlier work a conflict re-run starts from (only for [`AttemptKind::Rerun`]).
    pub carry: Option<Carry>,
    /// The worktree of earlier attempts, if any.
    pub worktree: Option<PathBuf>,
    pub task_file: PathBuf,
    pub log_file: PathBuf,
    /// The attempt's `--submission-key`.
    pub launch_key: Option<String>,
}

#[derive(Debug)]
pub struct LaunchDone {
    pub worktree: Option<PathBuf>,
    pub branch: Option<String>,
    /// Notes this launch added (a carried attempt that did not apply cleanly).
    pub notes_added: Vec<String>,
    pub result: Result<String, String>,
}

impl LaunchJob {
    pub fn new(
        feature: &Feature,
        slice: SlicePlan,
        state: &SliceState,
        attempt: &Attempt,
        kind: AttemptKind,
        fresh: bool,
    ) -> LaunchJob {
        let log_file = attempt
            .log_file
            .clone()
            .unwrap_or_else(|| AttemptFiles::new(feature, &slice.id, attempt.n).log);
        LaunchJob {
            n: attempt.n,
            kind,
            fresh,
            notes: state.notes.clone(),
            auto_notes: state.auto_notes.iter().map(|n| n.text.clone()).collect(),
            carry: state.carry.clone().filter(|_| kind == AttemptKind::Rerun),
            worktree: state.worktree.clone(),
            task_file: attempt.task_file.clone(),
            log_file,
            launch_key: attempt.launch_key.clone(),
            slice,
        }
    }

    pub fn run(self, ctx: &Ctx) -> LaunchDone {
        // `launch` records the worktree and notes as it goes: the engine keeps them even when
        // the launch itself fails.
        let mut done = LaunchDone {
            worktree: None,
            branch: None,
            notes_added: Vec::new(),
            result: Err("not launched".to_owned()),
        };
        done.result = self.launch(ctx, &mut done).map_err(|e| format!("{e:#}"));
        done
    }

    fn launch(&self, ctx: &Ctx, done: &mut LaunchDone) -> Result<String> {
        let branch = slice_branch(&ctx.feature, &self.slice);
        let worktree = self
            .worktree
            .clone()
            .unwrap_or_else(|| worktree_path(&ctx.feature, &self.slice));
        let new_tree = self.needs_new_worktree(&worktree);
        if new_tree {
            let _guard = ctx.lock();
            remove_worktree(ctx, &self.slice.id, &worktree, &branch);
            create_worktree(ctx, &worktree, &branch)?;
        }
        done.worktree = Some(worktree.clone());
        done.branch = Some(branch.clone());
        if self.kind == AttemptKind::Rerun
            && let Some(carry) = &self.carry
            && let Some(note) = carry_previous_work(&worktree, carry)?
        {
            done.notes_added.push(note);
        }
        let input_file = self.write_task(ctx, &worktree, &done.notes_added)?;
        let hooks = self.slice.hooks.over(&ctx.feature.config.hooks);
        let mut env = ctx.env(&self.slice, self.n, Some(&worktree), &branch);
        env.task_file = Some(self.task_file.clone());
        env.launch_key = self.launch_key.clone();
        if new_tree && let Some(prepare) = &hooks.prepare {
            self.run_prepare_hook(prepare, &worktree, &env)?;
        }
        let request = self.launch_request(ctx, worktree, input_file, hooks.launch.clone(), env);
        ctx.zeroshot.launch(&request)
    }

    /// A first attempt, a conflict re-run and a fresh retry start from the branch tip; other
    /// attempts (a retry, a gate re-run) continue in the worktree they had, when it is still there.
    fn needs_new_worktree(&self, worktree: &Path) -> bool {
        matches!(self.kind, AttemptKind::First | AttemptKind::Rerun)
            || self.fresh
            || !worktree.is_dir()
    }

    /// Copies the context into the worktree and writes the task file and its `input.json`
    /// (returns the latter's path).
    fn write_task(&self, ctx: &Ctx, worktree: &Path, notes_added: &[String]) -> Result<PathBuf> {
        let context_note = copy_context(ctx, worktree)?;
        let mut auto_notes = self.auto_notes.clone();
        auto_notes.extend(notes_added.iter().cloned());
        let spec_path = ctx.feature.spec_path(&self.slice.spec);
        let spec = std::fs::read_to_string(&spec_path)
            .with_context(|| format!("cannot read the spec {}", spec_path.display()))?;
        let text = task_text(&spec, context_note.as_deref(), &self.notes, &auto_notes);
        crate::store::write_atomic(&self.task_file, text.as_bytes())?;
        let input_file = self.task_file.with_extension("input.json");
        let input = serde_json::to_string(&serde_json::json!({ "task": text }))?;
        crate::store::write_atomic(&input_file, input.as_bytes())?;
        Ok(input_file)
    }

    fn run_prepare_hook(&self, prepare: &str, worktree: &Path, env: &HookEnv) -> Result<()> {
        let out = hooks::run(prepare, worktree, env, Some(&self.log_file))?;
        if !out.ok {
            bail!("the prepare hook failed: {}", out.tail(HOOK_OUTPUT_TAIL));
        }
        Ok(())
    }

    /// The slice's graph, runtime and args win over the feature's `[launch]` defaults.
    fn launch_request(
        &self,
        ctx: &Ctx,
        worktree: PathBuf,
        input_file: PathBuf,
        hook: Option<String>,
        env: HookEnv,
    ) -> LaunchRequest {
        let cfg = &ctx.feature.config;
        let resolve = |p: &Option<PathBuf>| p.as_ref().map(|p| ctx.feature.resolve(p));
        LaunchRequest {
            title: format!("{} {}: {}", ctx.prefix(), self.slice.id, self.slice.title),
            worktree,
            task_file: self.task_file.clone(),
            input_file,
            graph: resolve(&self.slice.graph).or_else(|| resolve(&cfg.launch.graph)),
            runtime: resolve(&self.slice.runtime).or_else(|| resolve(&cfg.launch.runtime)),
            args: self
                .slice
                .args
                .clone()
                .unwrap_or_else(|| cfg.launch.args.clone()),
            hook,
            env,
            log_file: self.log_file.clone(),
            timeout: Duration::from_secs(cfg.limits.launch_timeout_secs.max(1)),
            submission_key: self.launch_key.clone(),
        }
    }
}

fn create_worktree(ctx: &Ctx, worktree: &Path, branch: &str) -> Result<()> {
    let repo = ctx.repo();
    if let Some(parent) = worktree.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let feature_branch = &ctx.feature.config.branch;
    let tip = git::rev_parse(&repo, &format!("refs/heads/{feature_branch}"))
        .map_err(|_| anyhow!("the feature branch {feature_branch} does not exist"))?;
    let worktree = worktree.to_string_lossy();
    git(
        &repo,
        &["worktree", "add", "-q", "-B", branch, &worktree, &tip],
    )?;
    Ok(())
}

/// Saves the declared artifacts, then removes the worktree and its branch (call under the repo lock).
pub fn remove_worktree(ctx: &Ctx, slice: &str, worktree: &Path, branch: &str) {
    let repo = ctx.repo();
    if worktree.is_dir() {
        save_artifacts(&ctx.feature, slice, worktree);
        let _ = git::run(
            &repo,
            &["worktree", "remove", "--force", &worktree.to_string_lossy()],
        );
        if worktree.is_dir() {
            let _ = std::fs::remove_dir_all(worktree);
        }
    }
    let _ = git::run(&repo, &["worktree", "prune"]);
    let _ = git::run(&repo, &["branch", "-D", branch]);
}

/// Re-applies the previous attempt's finished work onto the fresh worktree of a conflict re-run.
/// Applies cleanly: committed, the run starts from it. Conflicts: the tree stays clean and the
/// returned note tells the worker what to re-apply.
fn carry_previous_work(worktree: &Path, carry: &Carry) -> Result<Option<String>> {
    let picked = git::run(worktree, &["cherry-pick", "--no-commit", &carry.commit])?;
    if picked.ok {
        let message = format!(
            "zsf: previous attempt re-applied onto the new branch tip ({})",
            carry.backup
        );
        git(
            worktree,
            &[
                "commit",
                "-q",
                "--no-verify",
                "--allow-empty",
                "-m",
                &message,
            ],
        )?;
        return Ok(None);
    }
    let conflicts = conflicted_files(worktree);
    let _ = git::run(worktree, &["cherry-pick", "--abort"]);
    let _ = git::run(worktree, &["reset", "-q", "--hard", "HEAD"]);
    Ok(Some(carried_work_note(carry, &conflicts)))
}

/// The files with unresolved conflicts in `dir` (one per line of git's output).
fn conflicted_files(dir: &Path) -> Vec<String> {
    git::run(dir, &["diff", "--name-only", "--diff-filter=U"])
        .map(|o| {
            o.stdout
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Copies the context into the worktree (git-excluded) and says where it is for the task.
fn copy_context(ctx: &Ctx, worktree: &Path) -> Result<Option<String>> {
    let Some(context) = ctx.feature.context_dir() else {
        return Ok(None);
    };
    match ctx.feature.config.launch.context_in_worktree() {
        None => Ok(Some(format!("`{}`", context.display()))),
        Some(rel) => {
            let rel = safe_relative(rel)?;
            let dest = worktree.join(rel);
            if dest.exists() {
                std::fs::remove_dir_all(&dest).ok();
            }
            copy_dir(&context, &dest)?;
            git::exclude(worktree, &format!("/{}/", rel.display()))?;
            Ok(Some(format!("`{}/` in this repository", rel.display())))
        }
    }
}

fn safe_relative(rel: &str) -> Result<&Path> {
    let path = Path::new(rel);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        bail!("{rel:?} must be a relative path inside the worktree");
    }
    Ok(path)
}

/// Copies the feature's declared artifacts from a worktree to `artifacts/<slice>/`.
/// Best effort: the artifacts are a copy for a person, so a file that cannot be copied is left
/// out rather than failing the slice.
pub fn save_artifacts(feature: &Feature, slice: &str, worktree: &Path) {
    for rel in &feature.config.artifacts {
        let Ok(rel) = safe_relative(rel) else {
            continue;
        };
        let source = worktree.join(rel);
        let dest = feature.artifacts_dir().join(slice).join(rel);
        if source.is_dir() {
            let _ = copy_dir(&source, &dest);
        } else if source.is_file() {
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::copy(&source, &dest);
        }
    }
}

// ---------------------------------------------------------------- integrate

/// How often the merge rebases and tries to move the feature branch again when the branch moved
/// between the rebase and the move (another writer of the repository, such as a person pushing).
const MAX_FAST_FORWARD_TRIES: usize = 3;

pub struct IntegrateJob {
    pub slice: SlicePlan,
    pub n: u32,
    pub worktree: PathBuf,
    pub run_id: Option<String>,
    /// Run the gate hook first (not for `accept`).
    pub gate: bool,
}

#[derive(Debug, PartialEq)]
pub enum Integrated {
    Merged {
        commit: String,
        warning: Option<String>,
    },
    GateFailed(String),
    Empty,
    Conflict {
        reason: String,
        carry: Carry,
    },
    Failed(String),
}

impl IntegrateJob {
    pub fn run(self, ctx: &Ctx) -> Integrated {
        if !self.worktree.is_dir() {
            return Integrated::Failed(format!("the worktree {} is gone", self.worktree.display()));
        }
        let branch = slice_branch(&ctx.feature, &self.slice);
        let mut env = ctx.env(&self.slice, self.n, Some(&self.worktree), &branch);
        env.run_id = self.run_id.clone();
        save_artifacts(&ctx.feature, &self.slice.id, &self.worktree);
        let hooks = self.slice.hooks.over(&ctx.feature.config.hooks);
        if self.gate
            && let Some(gate) = &hooks.gate
            && let Some(failed) = self.run_gate(ctx, gate, &env)
        {
            return failed;
        }
        let guard = ctx.lock();
        let result = self
            .merge(ctx, &env, hooks.check_after_rebase.as_deref())
            .unwrap_or_else(|e| Integrated::Failed(format!("{e:#}")));
        if let Integrated::Merged { .. } = &result {
            remove_worktree(ctx, &self.slice.id, &self.worktree, &branch);
        }
        drop(guard);
        match result {
            Integrated::Merged { commit, .. } => Integrated::Merged {
                commit,
                warning: hooks
                    .after_merge
                    .as_ref()
                    .and_then(|hook| run_after_merge(ctx, hook, &env)),
            },
            other => other,
        }
    }

    /// Runs the gate hook; `Some` when it failed.
    fn run_gate(&self, ctx: &Ctx, gate: &str, env: &HookEnv) -> Option<Integrated> {
        let log = AttemptFiles::new(&ctx.feature, &self.slice.id, self.n).gate_log;
        match hooks::run(gate, &self.worktree, env, Some(&log)) {
            Ok(out) if out.ok => None,
            Ok(out) => Some(Integrated::GateFailed(out.tail(GATE_OUTPUT_TAIL))),
            Err(e) => Some(Integrated::GateFailed(format!("{e:#}"))),
        }
    }

    /// Squashes the slice's work into one commit and fast-forwards the feature branch to it,
    /// rebasing first when the branch moved.
    fn merge(&self, ctx: &Ctx, env: &HookEnv, check: Option<&str>) -> Result<Integrated> {
        let wt = &self.worktree;
        let repo = ctx.repo();
        let feature_ref = format!("refs/heads/{}", ctx.feature.config.branch);
        commit_leftovers(wt)?;
        let head = git::rev_parse(wt, "HEAD")?;
        let fork = git(wt, &["merge-base", "HEAD", &feature_ref])?;
        if head == fork {
            // Merged by an earlier integration that was cut short (daemon restart)?
            let message = git(wt, &["log", "-1", "--format=%B", &head])?;
            if has_trailer(&message, SLICE_TRAILER, &self.slice.id) {
                return Ok(Integrated::Merged {
                    commit: head,
                    warning: None,
                });
            }
            return Ok(Integrated::Empty);
        }
        self.squash(ctx, &fork)?;
        for _ in 0..MAX_FAST_FORWARD_TRIES {
            let tip = git::rev_parse(&repo, &feature_ref)?;
            let base = git(wt, &["merge-base", "HEAD", &feature_ref])?;
            if tip != base
                && let Some(conflict) = self.rebase_onto(ctx, env, check, &base, &tip)?
            {
                return Ok(conflict);
            }
            let new_head = git::rev_parse(wt, "HEAD")?;
            // Moves the branch only if nobody moved it meanwhile.
            if git::run(&repo, &["update-ref", &feature_ref, &new_head, &tip])?.ok {
                return Ok(Integrated::Merged {
                    commit: new_head,
                    warning: None,
                });
            }
        }
        bail!("the feature branch kept moving while merging")
    }

    /// Replaces the slice's commits since `fork` with one commit carrying the slice's message.
    fn squash(&self, ctx: &Ctx, fork: &str) -> Result<()> {
        let wt = &self.worktree;
        git(wt, &["reset", "-q", "--soft", fork])?;
        let committed = git::run_input(
            wt,
            &["commit", "-q", "--no-verify", "-F", "-"],
            Some(&self.message(ctx)),
        )?;
        if !committed.ok {
            bail!("commit failed: {}", committed.stderr);
        }
        Ok(())
    }

    /// Rebases the squashed commit from `base` onto the branch `tip`, then runs the
    /// check-after-rebase hook. `Some` is the conflict that stops the merge.
    fn rebase_onto(
        &self,
        ctx: &Ctx,
        env: &HookEnv,
        check: Option<&str>,
        base: &str,
        tip: &str,
    ) -> Result<Option<Integrated>> {
        let wt = &self.worktree;
        if !git::run(wt, &["rebase", "-q", tip])?.ok {
            let files = conflicted_files(wt);
            let _ = git::run(wt, &["rebase", "--abort"]);
            let slices = slices_between(&ctx.repo(), base, tip, &files);
            return self
                .conflict(ctx, conflict_reason(&slices, &files))
                .map(Some);
        }
        if let Some(check) = check {
            let out = hooks::run(check, wt, env, None)?;
            if !out.ok {
                let reason = format!("check after rebase failed: {}", out.tail(HOOK_OUTPUT_TAIL));
                return self.conflict(ctx, reason).map(Some);
            }
        }
        Ok(None)
    }

    fn conflict(&self, ctx: &Ctx, reason: String) -> Result<Integrated> {
        let head = git::rev_parse(&self.worktree, "HEAD")?;
        let backup = backup_branch(&ctx.feature, &self.slice.id, self.n);
        git(&ctx.repo(), &["branch", "-f", &backup, &head])?;
        Ok(Integrated::Conflict {
            reason,
            carry: Carry {
                commit: head,
                backup,
            },
        })
    }

    fn message(&self, ctx: &Ctx) -> String {
        let title =
            self.slice.commit_title.clone().unwrap_or_else(|| {
                format!("{} {}: {}", ctx.prefix(), self.slice.id, self.slice.title)
            });
        let mut message = format!("{}\n\n", title.trim());
        if let Some(description) = self
            .slice
            .description
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            message.push_str(description);
            message.push_str("\n\n");
        }
        message.push_str(&format!("{SLICE_TRAILER}: {}\n", self.slice.id));
        if let Some(run) = &self.run_id {
            message.push_str(&format!("{RUN_TRAILER}: {run}\n"));
        }
        message
    }
}

/// Commits what the run left uncommitted in the worktree, so the merge takes all of it.
fn commit_leftovers(wt: &Path) -> Result<()> {
    if git::is_dirty(wt)? {
        git(wt, &["add", "-A"])?;
        git(
            wt,
            &[
                "commit",
                "-q",
                "--no-verify",
                "-m",
                "zsf: leftovers at the end of the run",
            ],
        )?;
    }
    Ok(())
}

/// Runs the after-merge hook in the main repository (the worktree is gone); `Some` is the
/// warning when it failed (the merge stands).
fn run_after_merge(ctx: &Ctx, hook: &str, env: &HookEnv) -> Option<String> {
    let mut env = env.clone();
    env.worktree = None;
    match hooks::run(hook, &ctx.repo(), &env, None) {
        Ok(out) if out.ok => None,
        Ok(out) => Some(format!(
            "after_merge hook failed: {}",
            out.tail(HOOK_OUTPUT_TAIL)
        )),
        Err(e) => Some(format!("after_merge hook failed: {e:#}")),
    }
}

/// The `Slice:` trailers of the commits in `from..to` that touch `files` (all commits when empty),
/// oldest first, each once.
pub fn slices_between(repo: &Path, from: &str, to: &str, files: &[String]) -> Vec<String> {
    let range = format!("{from}..{to}");
    let mut args = vec!["--reverse", range.as_str()];
    if !files.is_empty() {
        args.push("--");
        args.extend(files.iter().map(String::as_str));
    }
    let Ok(commits) = git::commits(repo, &args) else {
        return Vec::new();
    };
    let prefix = format!("{SLICE_TRAILER}: ");
    let mut slices: Vec<String> = Vec::new();
    for commit in &commits {
        for line in commit.message.lines() {
            if let Some(id) = line.trim().strip_prefix(prefix.as_str()).map(str::trim)
                && !id.is_empty()
                && !slices.iter().any(|s| s == id)
            {
                slices.push(id.to_owned());
            }
        }
    }
    slices
}

/// `conflicts with 02 in calc/ops.py`: who moved the branch under the slice, and where.
pub fn conflict_reason(slices: &[String], files: &[String]) -> String {
    const FILES_SHOWN: usize = 3;
    let with = if slices.is_empty() {
        "the branch tip".to_owned()
    } else {
        slices.join(", ")
    };
    let mut text = format!("conflicts with {with}");
    if !files.is_empty() {
        let shown: Vec<&str> = files.iter().take(FILES_SHOWN).map(String::as_str).collect();
        text.push_str(&format!(" in {}", shown.join(", ")));
        if files.len() > FILES_SHOWN {
            text.push_str(&format!(" and {} more files", files.len() - FILES_SHOWN));
        }
    }
    text
}

/// The commit `message` has the trailer `key: value`.
pub fn has_trailer(message: &str, key: &str, value: &str) -> bool {
    message
        .lines()
        .any(|l| l.trim() == format!("{key}: {value}"))
}

// ---------------------------------------------------------------- resume

pub struct ResumeJob {
    pub slice: SlicePlan,
    pub n: u32,
    pub run_id: String,
    pub worktree: PathBuf,
    pub branch: String,
}

impl ResumeJob {
    pub fn run(self, ctx: &Ctx) -> Result<String, String> {
        let mut env = ctx.env(&self.slice, self.n, Some(&self.worktree), &self.branch);
        env.run_id = Some(self.run_id.clone());
        let log = AttemptFiles::new(&ctx.feature, &self.slice.id, self.n).resume_log;
        let cwd = if self.worktree.is_dir() {
            self.worktree.clone()
        } else {
            ctx.repo()
        };
        ctx.zeroshot
            .resume(&self.run_id, &cwd, &env, &log)
            .map_err(|e| format!("{e:#}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carry() -> Carry {
        Carry {
            commit: "abc123".to_owned(),
            backup: "zsf-backup/calc/02-a1".to_owned(),
        }
    }

    #[test]
    fn carried_work_note_names_the_commit_and_the_files() {
        let files = ["calc/ops.py".to_owned(), "my notes.md".to_owned()];
        assert_eq!(
            carried_work_note(&carry(), &files),
            "This slice was already implemented in a previous attempt: commit abc123 (branch \
             zsf-backup/calc/02-a1). Other slices merged meanwhile and it no longer applies \
             cleanly. Start by re-applying it with `git cherry-pick abc123`, resolve the \
             conflicts in calc/ops.py, my notes.md keeping both sides' intent, build and test, \
             and commit. Do not re-implement the slice from scratch."
        );
        assert!(carried_work_note(&carry(), &[]).contains("conflicts in the reported files "));
    }

    #[test]
    fn task_text_puts_context_and_notes_under_their_headings() {
        let text = task_text(
            "Spec\n\n",
            Some("`ctx/`"),
            &["dev note".to_owned()],
            &["zsf note".to_owned()],
        );
        assert_eq!(
            text,
            "Spec\n\n---\n\nReference documents for the whole feature: `ctx/` (read-only; do not \
             commit them).\n\n## Earlier attempt (noted by zsf)\n\nzsf note\n\n## Notes from the \
             developer\n\ndev note\n"
        );
    }

    #[test]
    fn gate_note_quotes_the_gate_output() {
        assert_eq!(
            gate_note(2, "FAIL: test_sub\n"),
            "The merge gate failed on attempt 2: fix what it reports, build and test, and commit. \
             The end of its output:\n\n```text\nFAIL: test_sub\n```"
        );
        // An older zsf stored the bare text: this is how its cause is told.
        assert!(gate_note(1, "x").starts_with(crate::model::GATE_NOTE_START));
    }

    #[test]
    fn conflict_reason_names_three_files_at_most() {
        let files: Vec<String> = ["a", "b", "c", "d", "e"].map(String::from).to_vec();
        assert_eq!(
            conflict_reason(&["02".to_owned()], &files),
            "conflicts with 02 in a, b, c and 2 more files"
        );
        assert_eq!(conflict_reason(&[], &[]), "conflicts with the branch tip");
    }

    #[test]
    fn launch_keys_end_in_base36_time() {
        assert_eq!(base36(0), "");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert!(launch_key("calc", "02", 1).starts_with("zsf-calc-02-a1-"));
    }
}
