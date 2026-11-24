//! The writer: owns `state.json`, applies queued requests, checks running slices, starts ready
//! ones and turns job results into transitions. Long work runs on job threads ([`crate::jobs`]),
//! so the engine stays responsive and never holds anything while git or a launch is busy.
//!
//! Reading order: [`Engine::tick`] (one scheduler pass), then the sections below in the order a
//! slice meets them: running slices, starting slices, integration, requests.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};

use crate::jobs::{
    AttemptFiles, Ctx, IntegrateJob, Integrated, LaunchDone, LaunchJob, ResumeJob, gate_note,
    launch_key, save_artifacts, slice_branch, worktree_path,
};
use crate::model::*;
use crate::store::Feature;
use crate::time::now_ms;
use crate::zeroshot::{RunStatus, Zeroshot};

/// How often [`Engine::wait`] looks at the request queue and the stop flag while no job reports:
/// requests arrive as files, not on the job channel, so the wait has to look for them.
const REQUEST_POLL: Duration = Duration::from_millis(400);
/// How long [`Engine::drain_jobs`] waits for one job result before it gives up on the rest: a
/// guard against a hung job, far longer than any launch, gate or merge should take.
const JOB_RESULT_TIMEOUT: Duration = Duration::from_secs(3600);
/// [`Engine::run_until_idle`] waits this long for one job result, and ticks at most
/// [`MAX_IDLE_TICKS`] times, so a test whose feature never settles fails instead of hanging.
const IDLE_JOB_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_IDLE_TICKS: usize = 10_000;

/// Why Zeroshot finished a run that did not succeed, as far as zsf acts on it: the `reason`
/// Zeroshot records on a finished run (any other reason counts as an infrastructure failure).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinishReason {
    /// `change_attempts_exhausted`: the review loop gave up on the change (a quality failure).
    ChangeAttemptsExhausted,
    /// `force_stopped`: someone stopped the run.
    ForceStopped,
    /// `runtime_lost`: the controller died; resumable, like a controller found gone.
    RuntimeLost,
    Other,
}

impl FinishReason {
    fn parse(reason: Option<&str>) -> FinishReason {
        match reason {
            Some("change_attempts_exhausted") => FinishReason::ChangeAttemptsExhausted,
            Some("force_stopped") => FinishReason::ForceStopped,
            Some("runtime_lost") => FinishReason::RuntimeLost,
            _ => FinishReason::Other,
        }
    }
}

type FreeSpace = Arc<dyn Fn(&Path) -> f64 + Send + Sync>;

enum Outcome {
    Launch(LaunchDone),
    Integrate {
        accept: bool,
        result: Integrated,
    },
    Resume {
        old: String,
        result: Result<String, String>,
    },
}

/// A job's result, sent back from its thread.
struct Done {
    slice: String,
    outcome: Outcome,
}

/// The job threads (at most one per slice) and the channel their results come back on.
struct Jobs {
    threads: BTreeMap<String, JoinHandle<()>>,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    /// Results received but not applied yet.
    inbox: VecDeque<Done>,
}

impl Jobs {
    fn new() -> Jobs {
        let (tx, rx) = channel();
        Jobs {
            threads: BTreeMap::new(),
            tx,
            rx,
            inbox: VecDeque::new(),
        }
    }

    fn is_idle(&self) -> bool {
        self.threads.is_empty()
    }

    fn spawn(
        &mut self,
        slice: &str,
        ctx: Ctx,
        work: impl FnOnce(&Ctx) -> Outcome + Send + 'static,
    ) {
        let tx = self.tx.clone();
        let id = slice.to_owned();
        let handle = std::thread::spawn(move || {
            let outcome = work(&ctx);
            let _ = tx.send(Done { slice: id, outcome });
        });
        self.threads.insert(slice.to_owned(), handle);
    }

    /// Waits up to `timeout` for one result and puts it in the inbox.
    fn recv_into_inbox(&mut self, timeout: Duration) -> Result<(), RecvTimeoutError> {
        let done = self.rx.recv_timeout(timeout)?;
        self.inbox.push_back(done);
        Ok(())
    }

    /// Moves every result already sent into the inbox (without waiting).
    fn receive_sent(&mut self) {
        while let Ok(done) = self.rx.try_recv() {
            self.inbox.push_back(done);
        }
    }

    /// The next result to apply; its thread is joined (it has finished).
    fn next_result(&mut self) -> Option<Done> {
        let done = self.inbox.pop_front()?;
        if let Some(handle) = self.threads.remove(&done.slice) {
            let _ = handle.join();
        }
        Some(done)
    }
}

/// What one tick did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tick {
    /// Something changed (an event was written).
    pub changed: bool,
    /// Every slice is merged or skipped.
    pub done: bool,
}

pub struct Engine {
    pub feature: Feature,
    pub plan: Plan,
    pub state: State,
    zeroshot: Arc<dyn Zeroshot>,
    free_space: FreeSpace,
    repo_lock: Arc<Mutex<()>>,
    jobs: Jobs,
    /// An event was written during this tick (reported as [`Tick::changed`]).
    events_written: bool,
    /// `state` has changes not saved to `state.json` yet.
    unsaved: bool,
    /// Set when the writer is shutting down: no new slices start.
    pub stopping: bool,
}

impl Engine {
    /// Loads the plan and state. Only the writer (holding `writer.lock`) may call this.
    pub fn open(feature: Feature, zeroshot: Arc<dyn Zeroshot>) -> Result<Engine> {
        let plan = feature.load_plan()?;
        let mut state = feature.load_state()?;
        for slice in &plan.slices {
            state.slices.entry(slice.id.clone()).or_default();
        }
        Ok(Engine {
            feature,
            plan,
            state,
            zeroshot,
            free_space: Arc::new(crate::sys::free_gb),
            repo_lock: Arc::new(Mutex::new(())),
            jobs: Jobs::new(),
            events_written: false,
            unsaved: false,
            stopping: false,
        })
    }

    /// Replaces the free-disk probe (tests).
    pub fn set_free_space(&mut self, probe: impl Fn(&Path) -> f64 + Send + Sync + 'static) {
        self.free_space = Arc::new(probe);
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            feature: self.feature.clone(),
            zeroshot: self.zeroshot.clone(),
            repo_lock: self.repo_lock.clone(),
        }
    }

    pub fn jobs_running(&self) -> usize {
        self.jobs.threads.len()
    }

    pub fn save(&mut self) -> Result<()> {
        self.feature.save_state(&mut self.state)?;
        self.unsaved = false;
        Ok(())
    }

    fn save_if_unsaved(&mut self) -> Result<()> {
        if self.unsaved {
            self.save()?;
        }
        Ok(())
    }

    fn emit(&mut self, slice: Option<&str>, kind: EventKind, detail: impl Into<String>) {
        let (attempt, run_id) = match slice
            .and_then(|s| self.state.slices.get(s))
            .and_then(|s| s.last_attempt())
        {
            Some(a) => (Some(a.n), a.run_id.clone()),
            None => (None, None),
        };
        let event = Event {
            ts: now_ms(),
            slice: slice.map(str::to_owned),
            attempt,
            kind,
            run_id,
            detail: detail.into(),
        };
        if let Err(e) = self.feature.append_event(&event) {
            eprintln!("zsf: cannot write an event: {e:#}");
        }
        self.events_written = true;
        self.unsaved = true;
    }

    /// Writes a feature-level event (daemon start/stop).
    pub fn log_event(&mut self, kind: EventKind, detail: String) {
        self.emit(None, kind, detail);
    }

    fn slice(&mut self, id: &str) -> &mut SliceState {
        self.state.slices.entry(id.to_owned()).or_default()
    }

    fn plan_slice(&self, id: &str) -> Result<SlicePlan> {
        self.plan
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("no slice {id} in the plan"))
    }

    fn spawn(&mut self, slice: &str, work: impl FnOnce(&Ctx) -> Outcome + Send + 'static) {
        let ctx = self.ctx();
        self.jobs.spawn(slice, ctx, work);
    }

    // ------------------------------------------------------------ driving

    /// Redoes work cut short by a restart of the writer (call once when the daemon starts).
    pub fn recover(&mut self) {
        for (id, job) in self.interrupted_jobs() {
            match job {
                Job::Launch { kind, fresh } => self.recover_launch(&id, kind, fresh),
                Job::Integrate { accept } => self.dispatch_integrate(&id, accept),
                Job::Resume => self.recover_resume(&id),
            }
        }
    }

    /// The jobs the state still records for slices of the plan.
    fn interrupted_jobs(&self) -> Vec<(String, Job)> {
        self.state
            .slices
            .iter()
            .filter(|(id, _)| self.plan.get(id).is_some())
            .filter_map(|(id, s)| Some((id.clone(), s.job.clone()?)))
            .collect()
    }

    /// A launch cut short. It may have started the run before the writer died: then the run is
    /// found by the attempt's launch key and adopted; otherwise the launch runs again, reusing
    /// the attempt that never got a run.
    fn recover_launch(&mut self, id: &str, kind: AttemptKind, fresh: bool) {
        let pending = self.state.slices[id]
            .last_attempt()
            .filter(|a| a.run_id.is_none() && a.result.is_none())
            .cloned();
        let key = pending.as_ref().and_then(|a| a.launch_key.as_deref());
        match key.and_then(|key| self.zeroshot.find_run(key)) {
            Some(run_id) => self.adopt_run(id, kind, run_id),
            None => self.dispatch_launch(id, kind, fresh, pending.is_some()),
        }
    }

    fn adopt_run(&mut self, id: &str, kind: AttemptKind, run_id: String) {
        let paths = self.plan.get(id).map(|plan| {
            (
                worktree_path(&self.feature, plan),
                slice_branch(&self.feature, plan),
            )
        });
        let s = self.slice(id);
        s.job = None;
        s.status = SliceStatus::Running;
        if s.worktree.is_none()
            && let Some((worktree, branch)) = paths
        {
            s.worktree = Some(worktree);
            s.branch = Some(branch);
        }
        if let Some(a) = s.last_attempt_mut() {
            a.run_id = Some(run_id);
        }
        self.emit(
            Some(id),
            EventKind::Launched,
            format!(
                "{} attempt (run found by its launch key after a restart)",
                kind.as_str()
            ),
        );
    }

    /// A resume cut short: when it happened, its successor was not recorded yet; when it did
    /// not, it may be tried again (there is still only one successor).
    fn recover_resume(&mut self, id: &str) {
        let s = self.slice(id);
        s.job = None;
        s.status = SliceStatus::Running;
        let old = s.last_attempt().and_then(|a| a.run_id.clone());
        let new = old.as_deref().and_then(|old| self.zeroshot.successor(old));
        match (old, new) {
            (Some(old), Some(new)) => self.resumed(id, old, Ok(new)),
            _ => {
                if let Some(a) = self.slice(id).last_attempt_mut() {
                    a.resume_tried = false;
                }
            }
        }
    }

    /// One scheduler pass.
    pub fn tick(&mut self) -> Result<Tick> {
        self.events_written = false;
        self.collect();
        self.apply_requests();
        let mut done = false;
        if self.state.active && !self.stopping {
            self.check_running();
            if self.state.paused.is_none() {
                done = self.finish_if_done();
                if !done {
                    self.schedule();
                }
            }
        }
        self.save_if_unsaved()?;
        Ok(Tick {
            changed: self.events_written,
            done: done || self.all_finished(),
        })
    }

    /// Waits until a job finishes, a request arrives, `stop()` says so, or `timeout` passes.
    pub fn wait(&mut self, timeout: Duration, stop: &dyn Fn() -> bool) {
        let deadline = Instant::now() + timeout;
        loop {
            if stop() || self.feature.has_pending_requests() {
                return;
            }
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            match self
                .jobs
                .recv_into_inbox((deadline - now).min(REQUEST_POLL))
            {
                Err(RecvTimeoutError::Timeout) => {}
                // A job finished (or no job can report any more).
                Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Waits for every running job and applies the results (which may start more jobs).
    pub fn drain_jobs(&mut self) -> Result<()> {
        while !self.jobs.is_idle() {
            if self.jobs.recv_into_inbox(JOB_RESULT_TIMEOUT).is_err() {
                break;
            }
            self.collect();
        }
        self.save_if_unsaved()
    }

    /// Ticks until nothing changes and no job runs (tests and one-shot use).
    pub fn run_until_idle(&mut self) -> Result<Tick> {
        let mut last = Tick::default();
        for _ in 0..MAX_IDLE_TICKS {
            last = self.tick()?;
            if self.jobs.is_idle() && !last.changed && self.jobs.inbox.is_empty() {
                return Ok(last);
            }
            if !self.jobs.is_idle() {
                let _ = self.jobs.recv_into_inbox(IDLE_JOB_TIMEOUT);
            }
        }
        Ok(last)
    }

    /// Applies queued requests now and finishes the work they started (the CLI as the writer
    /// while no daemon runs). Does not schedule new slices.
    pub fn apply_requests_now(&mut self) -> Result<()> {
        self.apply_requests();
        self.drain_jobs()?;
        self.save_if_unsaved()
    }

    /// Applies the results of finished jobs. Results that arrive meanwhile wait for the next
    /// call.
    fn collect(&mut self) {
        self.jobs.receive_sent();
        while let Some(done) = self.jobs.next_result() {
            match done.outcome {
                Outcome::Launch(result) => self.launched(&done.slice, result),
                Outcome::Integrate { accept, result } => {
                    self.integrated(&done.slice, accept, result)
                }
                Outcome::Resume { old, result } => self.resumed(&done.slice, old, result),
            }
        }
    }

    /// Every slice is merged or skipped.
    pub fn all_finished(&self) -> bool {
        self.state.all_finished(&self.plan)
    }

    fn finish_if_done(&mut self) -> bool {
        let all = self.all_finished();
        if all && self.jobs.is_idle() {
            self.state.active = false;
            self.state.note = None;
            let count = |status| {
                self.state
                    .slices
                    .values()
                    .filter(|s| s.status == status)
                    .count()
            };
            let merged = count(SliceStatus::Merged);
            let skipped = match count(SliceStatus::Skipped) {
                0 => String::new(),
                skipped => format!(", {skipped} skipped"),
            };
            self.emit(
                None,
                EventKind::Done,
                format!(
                    "{merged} merged into {}{skipped}",
                    self.feature.config.branch
                ),
            );
        }
        all
    }

    // ------------------------------------------------------------ running slices

    fn check_running(&mut self) {
        let ids: Vec<String> = self
            .plan
            .slices
            .iter()
            .map(|s| s.id.clone())
            .filter(|id| {
                self.state
                    .slices
                    .get(id)
                    .is_some_and(|s| s.status == SliceStatus::Running && s.job.is_none())
            })
            .collect();
        for id in ids {
            let Some(attempt) = self.state.slices[&id].last_attempt().cloned() else {
                continue;
            };
            let Some(run_id) = attempt.run_id.clone() else {
                self.infra_failure(&id, "the launch recorded no run id".to_owned());
                continue;
            };
            match self.zeroshot.status(&run_id) {
                RunStatus::Running | RunStatus::Unreadable { .. } => {}
                RunStatus::NotStarted => {
                    let since = attempt.started_ms.max(attempt.resumed_ms.unwrap_or(0));
                    let grace = self.feature.config.limits.start_grace_secs;
                    if now_ms().saturating_sub(since) > grace * 1000 {
                        self.run_gone(&id, format!("no live controller {grace}s after the launch"));
                    }
                }
                RunStatus::Gone { why } => self.run_gone(&id, why),
                RunStatus::Finished { status, reason } => {
                    self.run_finished(&id, &run_id, &status, reason.as_deref())
                }
            }
        }
    }

    fn run_finished(&mut self, id: &str, run_id: &str, status: &str, reason: Option<&str>) {
        let succeeded = status == "succeeded";
        let why = FinishReason::parse(reason);
        if !succeeded && why == FinishReason::RuntimeLost {
            self.run_gone(
                id,
                format!(
                    "zeroshot recorded the run as {status}: {}",
                    reason.unwrap_or_default()
                ),
            );
            return;
        }
        let detail = match reason {
            Some(r) => format!("{status}: {r}"),
            None => status.to_owned(),
        };
        if let Some(a) = self.slice(id).last_attempt_mut() {
            a.ended_ms = Some(now_ms());
        }
        self.emit(Some(id), EventKind::RunFinished, detail);
        if succeeded {
            if self.needs_review(id) {
                self.wait_for_review(id);
            } else {
                self.dispatch_integrate(id, false);
            }
            return;
        }
        match why {
            FinishReason::ChangeAttemptsExhausted => self.stop_slice(
                id,
                AttemptResult::QualityFailed,
                format!("review loop exhausted (run {run_id})"),
                EventKind::QualityFailed,
            ),
            FinishReason::ForceStopped => self.stop_slice(
                id,
                AttemptResult::Stopped,
                format!("run {run_id} was force-stopped"),
                EventKind::RunStopped,
            ),
            FinishReason::RuntimeLost | FinishReason::Other => self.infra_failure(
                id,
                format!(
                    "run {run_id} {status}: {}",
                    reason.unwrap_or("no reason recorded")
                ),
            ),
        }
    }

    /// A checkpoint whose finished work no person has approved yet: it waits for review instead
    /// of merging.
    fn needs_review(&self, id: &str) -> bool {
        let checkpoint = self.plan.get(id).is_some_and(|p| p.checkpoint);
        checkpoint && !self.state.slices.get(id).is_some_and(|s| s.approved)
    }

    /// The checkpoint's run is done: it waits, not merged and with its worktree, for a person to
    /// approve (check and merge), request changes or skip it. Its attempt stays without a result
    /// until then.
    fn wait_for_review(&mut self, id: &str) {
        let s = self.slice(id);
        s.status = SliceStatus::WaitingForReview;
        s.job = None;
        s.reason = None;
        if let Some(worktree) = s.worktree.clone() {
            save_artifacts(&self.feature, id, &worktree);
        }
        self.state.consecutive_infra = 0;
        self.emit(
            Some(id),
            EventKind::CheckpointWaiting,
            "waiting for review, not merged (approve = check and merge)",
        );
    }

    /// The slice stops and waits for a person; its worktree stays.
    fn stop_slice(&mut self, id: &str, result: AttemptResult, reason: String, kind: EventKind) {
        close_attempt(self.slice(id), result, Some(reason.clone()));
        self.fail(id, reason.clone());
        self.emit(Some(id), kind, reason);
    }

    /// The slice is failed and waits for a person: its worktree stays, and the feature's
    /// declared artifacts are copied out of it.
    fn fail(&mut self, id: &str, reason: String) {
        let s = self.slice(id);
        mark_failed(s, reason);
        if let Some(worktree) = s.worktree.clone() {
            save_artifacts(&self.feature, id, &worktree);
        }
    }

    /// The run's controller is gone: resume it once, then count an infrastructure failure.
    fn run_gone(&mut self, id: &str, why: String) {
        let s = self.slice(id);
        let Some(a) = s.last_attempt_mut() else {
            return;
        };
        a.reason = Some(why.clone());
        let tried = a.resume_tried;
        let run_id = a.run_id.clone().unwrap_or_default();
        // Resumed before, but the successor never made it into the state (a writer crash).
        if let Some(new) = self.zeroshot.successor(&run_id) {
            self.resumed(id, run_id, Ok(new));
            return;
        }
        self.emit(Some(id), EventKind::Interrupted, why.clone());
        if tried {
            self.infra_failure(id, format!("run {run_id} lost again: {why}"));
            return;
        }
        let Ok(plan) = self.plan_slice(id) else {
            return;
        };
        let s = self.slice(id);
        let a = s.last_attempt_mut().expect("attempt");
        a.resume_tried = true;
        let n = a.n;
        s.job = Some(Job::Resume);
        let job = ResumeJob {
            slice: plan,
            n,
            run_id: run_id.clone(),
            worktree: s.worktree.clone().unwrap_or_default(),
            branch: s.branch.clone().unwrap_or_default(),
        };
        self.unsaved = true;
        self.spawn(id, move |ctx| Outcome::Resume {
            old: run_id,
            result: job.run(ctx),
        });
    }

    fn resumed(&mut self, id: &str, old: String, result: Result<String, String>) {
        let s = self.slice(id);
        s.job = None;
        match result {
            Ok(new) => {
                if let Some(a) = s.last_attempt_mut() {
                    a.earlier_runs.push(old.clone());
                    a.run_id = Some(new.clone());
                    a.resumed_ms = Some(now_ms());
                    a.reason = None;
                }
                s.status = SliceStatus::Running;
                self.emit(
                    Some(id),
                    EventKind::Resumed,
                    format!("resumed {old} as {new}"),
                );
            }
            Err(e) => {
                self.emit(Some(id), EventKind::ResumeFailed, e.clone());
                self.infra_failure(id, format!("run {old} lost; resume failed: {e}"));
            }
        }
    }

    /// Counts an infrastructure failure: the slice is launched again (up to `infra_retries`),
    /// waits as interrupted while the feature is paused or stopping, or fails.
    fn infra_failure(&mut self, id: &str, why: String) {
        let limits = self.feature.config.limits.clone();
        let s = self.slice(id);
        s.infra_failures += 1;
        s.job = None;
        let failures = s.infra_failures;
        if let Some(a) = s.last_attempt_mut() {
            // Unlike `close_attempt`, keeps a result recorded first (`launch_failed`).
            a.result.get_or_insert(AttemptResult::InfraFailed);
            a.reason = Some(why.clone());
            a.ended_ms.get_or_insert(now_ms());
        }
        self.state.consecutive_infra += 1;
        self.emit(
            Some(id),
            EventKind::InfraFailure,
            format!("#{failures}: {why}"),
        );
        self.pause_if_infra_keeps_failing(id, &why, limits.pause_after_infra);
        if failures > limits.infra_retries {
            self.fail(id, format!("infrastructure: {why}"));
        } else if self.state.paused.is_some() || self.stopping {
            let s = self.slice(id);
            s.status = SliceStatus::Interrupted;
            s.reason = Some(format!("infrastructure: {why}"));
        } else {
            self.dispatch_launch(id, AttemptKind::InfraRetry, false, false);
        }
    }

    fn pause_if_infra_keeps_failing(&mut self, id: &str, why: &str, pause_after: u32) {
        if pause_after == 0
            || self.state.consecutive_infra < pause_after
            || self.state.paused.is_some()
        {
            return;
        }
        let reason = format!(
            "{} infrastructure failures in a row (last: {id}: {why})",
            self.state.consecutive_infra
        );
        self.state.paused = Some(reason.clone());
        self.emit(None, EventKind::Paused, reason);
    }

    // ------------------------------------------------------------ starting slices

    fn deps_done(&self, slice: &SlicePlan) -> bool {
        slice.deps.iter().all(|d| self.dep_done(d))
    }

    /// Skipped, or merged and (for a checkpoint) approved ([`State::dep_done`]).
    pub fn dep_done(&self, dep: &str) -> bool {
        self.state.dep_done(&self.plan, dep)
    }

    /// At least one slot, whatever the config says.
    fn max_parallel(&self) -> usize {
        self.feature.config.limits.max_parallel.max(1) as usize
    }

    /// Why no slice can start now, as "when …" for a person (`None`: one can). The conditions
    /// under which [`Engine::tick`] calls [`Engine::schedule`], and the slot limit it applies.
    fn start_blocker(&self) -> Option<&'static str> {
        if self.state.paused.is_some() {
            Some("when the feature is unpaused")
        } else if !self.state.active || self.stopping {
            Some("when the feature is started")
        } else if self.state.active_count() >= self.max_parallel() {
            Some("when a slot is free")
        } else {
            None
        }
    }

    /// Starts slices while slots are free: interrupted ones first (in their own worktree), then
    /// pending ones in plan order. Writes a note when something keeps slices from starting.
    fn schedule(&mut self) {
        let max = self.max_parallel();
        let mut running = self.state.active_count();
        let order: Vec<SlicePlan> = self.plan.slices.clone();
        for slice in &order {
            if running >= max {
                break;
            }
            if self.is_interrupted(&slice.id) {
                self.dispatch_launch(&slice.id, AttemptKind::InfraRetry, false, false);
                running += 1;
            }
        }
        let mut note = None;
        for slice in &order {
            if running >= max {
                break;
            }
            if !self.is_pending(&slice.id) || !self.deps_done(slice) {
                continue;
            }
            if let Some(low) = self.low_disk(slice) {
                note = Some(low);
                break;
            }
            if let Some(lease) = self.busy_lease(slice) {
                note.get_or_insert(format!("{} waits for lease {lease}", slice.id));
                continue;
            }
            self.start_pending(&slice.id);
            running += 1;
        }
        if note.is_none() && self.state.active_count() == 0 && self.jobs.is_idle() {
            note = Some(self.waiting_note());
        }
        self.update_waiting_note(note);
    }

    fn is_interrupted(&self, id: &str) -> bool {
        self.state
            .slices
            .get(id)
            .is_some_and(|s| s.status == SliceStatus::Interrupted && s.job.is_none())
    }

    fn is_pending(&self, id: &str) -> bool {
        self.state
            .slices
            .get(id)
            .is_none_or(|s| s.status == SliceStatus::Pending && s.job.is_none())
    }

    /// The note when the disk is below `min_free_gb` (checked before each start).
    fn low_disk(&self, slice: &SlicePlan) -> Option<String> {
        let min = self.feature.config.limits.min_free_gb;
        if min > 0.0 {
            let free = (self.free_space)(&self.feature.worktrees_dir());
            if free < min {
                return Some(format!(
                    "disk below {min} GB ({free:.0} GB free) before {}",
                    slice.id
                ));
            }
        }
        None
    }

    /// The first of the slice's leases that active slices already hold up to its capacity.
    fn busy_lease<'a>(&self, slice: &'a SlicePlan) -> Option<&'a String> {
        slice.leases.iter().find(|lease| {
            let capacity = self.feature.config.leases.get(*lease).copied().unwrap_or(0);
            self.lease_holders(lease) >= capacity as usize
        })
    }

    /// How many active slices hold `lease`.
    fn lease_holders(&self, lease: &str) -> usize {
        self.plan
            .slices
            .iter()
            .filter(|p| p.leases.iter().any(|l| l == lease))
            .filter(|p| {
                self.state
                    .slices
                    .get(&p.id)
                    .is_some_and(|s| s.status.is_active())
            })
            .count()
    }

    /// Launches a pending slice: its queued retry, or its first attempt.
    fn start_pending(&mut self, id: &str) {
        match self.slice(id).retry.take() {
            Some(retry) => self.dispatch_launch(id, AttemptKind::ManualRetry, retry.fresh, false),
            None => self.dispatch_launch(id, AttemptKind::First, false, false),
        }
    }

    /// Records the feature's note (and a `waiting` event) when it changed.
    fn update_waiting_note(&mut self, note: Option<String>) {
        if note == self.state.note {
            return;
        }
        if let Some(text) = &note {
            self.emit(None, EventKind::Waiting, text.clone());
        }
        self.state.note = note;
        self.unsaved = true;
    }

    /// What a person has to do before anything can continue.
    pub fn waiting_note(&self) -> String {
        let mut wants = Vec::new();
        for p in &self.plan.slices {
            let Some(s) = self.state.slices.get(&p.id) else {
                continue;
            };
            if s.awaiting_approval(p.checkpoint) {
                wants.push(format!("review {}", p.id));
            } else if s.status == SliceStatus::Failed {
                wants.push(format!("fix {}", p.id));
            }
        }
        if wants.is_empty() {
            "nothing can start".to_owned()
        } else {
            format!("waiting for you: {}", wants.join(", "))
        }
    }

    /// Starts a launch job for the slice, in a new attempt unless `reuse_attempt` (a launch cut
    /// short by a restart, whose attempt never got a run).
    fn dispatch_launch(&mut self, id: &str, kind: AttemptKind, fresh: bool, reuse_attempt: bool) {
        let Ok(plan) = self.plan_slice(id) else {
            return;
        };
        let _ = std::fs::create_dir_all(self.feature.tasks_dir());
        let _ = std::fs::create_dir_all(self.feature.runs_dir());
        if !reuse_attempt {
            self.open_attempt(id, kind, fresh);
        }
        let s = self.slice(id);
        let attempt = s.last_attempt().expect("attempt").clone();
        s.status = SliceStatus::Launching;
        s.job = Some(Job::Launch { kind, fresh });
        s.reason = None;
        if fresh {
            s.worktree = None;
        }
        let s = s.clone();
        let job = LaunchJob::new(&self.feature, plan, &s, &attempt, kind, fresh);
        self.unsaved = true;
        // The attempt and its launch key are on disk before the run can exist: a writer that
        // dies before recording the run id finds the run again by the key (`recover`).
        if let Err(e) = self.save() {
            eprintln!("zsf: cannot save the state before a launch: {e:#}");
        }
        self.spawn(id, move |ctx| Outcome::Launch(job.run(ctx)));
    }

    /// Closes the slice's last attempt as abandoned (when it is still open) and adds the next.
    fn open_attempt(&mut self, id: &str, kind: AttemptKind, fresh: bool) {
        let n = self.state.slices.get(id).map_or(0, |s| s.attempts.len()) as u32 + 1;
        let files = AttemptFiles::new(&self.feature, id, n);
        let key = launch_key(&self.feature.config.name, id, n);
        let s = self.slice(id);
        if let Some(a) = s.last_attempt_mut() {
            a.result.get_or_insert(AttemptResult::Abandoned);
            a.ended_ms.get_or_insert(now_ms());
        }
        s.attempts.push(new_attempt(n, kind, fresh, files, key));
    }

    fn launched(&mut self, id: &str, done: LaunchDone) {
        let s = self.slice(id);
        s.job = None;
        if done.worktree.is_some() {
            s.worktree = done.worktree;
            s.branch = done.branch;
        }
        let carried = done
            .notes_added
            .into_iter()
            .map(|t| AutoNote::new(AutoNoteCause::Conflict, t));
        s.auto_notes.extend(carried);
        match done.result {
            Ok(run_id) => {
                if let Some(a) = s.last_attempt_mut() {
                    a.run_id = Some(run_id);
                    if a.kind == AttemptKind::Rerun {
                        s.carry = None;
                    }
                }
                let kind = s.last_attempt().map(|a| a.kind.as_str()).unwrap_or("");
                let detail = format!("{kind} attempt");
                s.status = SliceStatus::Running;
                self.emit(Some(id), EventKind::Launched, detail);
            }
            Err(reason) => {
                close_attempt(s, AttemptResult::LaunchFailed, Some(reason.clone()));
                self.emit(Some(id), EventKind::LaunchFailed, reason.clone());
                self.infra_failure(id, format!("launch failed: {reason}"));
            }
        }
    }

    // ------------------------------------------------------------ integration

    fn dispatch_integrate(&mut self, id: &str, accept: bool) {
        let Ok(plan) = self.plan_slice(id) else {
            return;
        };
        let s = self.slice(id);
        let Some(worktree) = s.worktree.clone() else {
            self.stop_slice(
                id,
                AttemptResult::IntegrationFailed,
                "no worktree to merge".to_owned(),
                EventKind::IntegrationFailed,
            );
            return;
        };
        let (n, run_id) = s
            .last_attempt()
            .map(|a| (a.n, a.run_id.clone()))
            .unwrap_or((0, None));
        s.status = SliceStatus::Integrating;
        s.job = Some(Job::Integrate { accept });
        let job = IntegrateJob {
            slice: plan,
            n,
            worktree,
            run_id,
            gate: !accept,
        };
        self.unsaved = true;
        self.spawn(id, move |ctx| Outcome::Integrate {
            accept,
            result: job.run(ctx),
        });
    }

    /// Applies an integration's result. An `accept` (a person's work in a stopped slice) leaves
    /// the attempt's result alone: it records what the run did.
    fn integrated(&mut self, id: &str, accept: bool, result: Integrated) {
        self.slice(id).job = None;
        match result {
            Integrated::Merged { commit, warning } => self.merged(id, accept, commit, warning),
            Integrated::GateFailed(output) => self.gate_failed(id, accept, output),
            Integrated::Empty => {
                let reason = if accept {
                    "nothing to merge: the worktree has no changes"
                } else {
                    "the run finished without any change"
                };
                let failure = (AttemptResult::Empty, EventKind::IntegrationFailed);
                self.not_merged(id, accept, failure, reason.to_owned(), reason.to_owned());
            }
            Integrated::Conflict { reason, carry } => self.conflicted(id, accept, reason, carry),
            Integrated::Failed(output) => {
                let reason = format!("merge failed: {output}");
                let failure = (
                    AttemptResult::IntegrationFailed,
                    EventKind::IntegrationFailed,
                );
                self.not_merged(id, accept, failure, output, reason);
            }
        }
    }

    fn merged(&mut self, id: &str, accept: bool, commit: String, warning: Option<String>) {
        let checkpoint = self.plan.get(id).is_some_and(|p| p.checkpoint);
        let s = self.slice(id);
        if !accept {
            close_attempt(s, AttemptResult::Merged, None);
        }
        // A person merging their own work in a checkpoint has reviewed it.
        if checkpoint && accept {
            s.approved = true;
        }
        s.status = SliceStatus::Merged;
        s.commit = Some(commit.clone());
        s.worktree = None;
        s.carry = None;
        s.reason = None;
        s.accepted = accept;
        self.state.consecutive_infra = 0;
        let short = short_commit(&commit);
        let kind = if accept {
            EventKind::Accepted
        } else {
            EventKind::Merged
        };
        let detail = format!("{short} on {}", self.feature.config.branch);
        self.emit(Some(id), kind, detail);
        if let Some(warning) = warning {
            self.emit(Some(id), EventKind::HookFailed, warning);
        }
    }

    /// The merge gate failed: the slice runs once more in its worktree with the gate's output as
    /// notes (up to `gate_reruns`; a checkpoint's re-run waits for review again), or fails.
    fn gate_failed(&mut self, id: &str, accept: bool, output: String) {
        let limit = self.feature.config.limits.gate_reruns;
        let s = self.slice(id);
        if accept || s.gate_reruns >= limit {
            let reason = format!("gate failed: {}", crate::status::one_line(&output));
            let failure = (AttemptResult::GateFailed, EventKind::GateFailed);
            self.not_merged(id, accept, failure, output, reason);
            return;
        }
        s.gate_reruns += 1;
        // The re-run changes the work: a person reviews it again.
        s.approved = false;
        let n = s.last_attempt().map_or(0, |a| a.n);
        close_attempt(s, AttemptResult::GateFailed, Some(output.clone()));
        s.auto_notes.push(AutoNote::new(
            AutoNoteCause::GateFailed,
            gate_note(n, &output),
        ));
        let detail = format!(
            "{}; running it once more with the gate output as notes",
            crate::status::one_line(&output)
        );
        self.emit(Some(id), EventKind::GateFailed, detail);
        self.dispatch_launch(id, AttemptKind::GateRerun, false, false);
    }

    /// An integration that merged nothing: the attempt keeps `detail` (also the event's), the
    /// slice fails with `reason`. No artifacts are copied: the integration job saved them
    /// before its gate.
    fn not_merged(
        &mut self,
        id: &str,
        accept: bool,
        (result, kind): (AttemptResult, EventKind),
        detail: String,
        reason: String,
    ) {
        let s = self.slice(id);
        if !accept {
            close_attempt(s, result, Some(detail.clone()));
        }
        mark_failed(s, reason);
        self.emit(Some(id), kind, detail);
    }

    /// The work conflicts with the moved branch tip. It is kept on a backup branch; the slice
    /// runs once more from the new tip carrying it (up to `conflict_reruns`), or fails.
    fn conflicted(&mut self, id: &str, accept: bool, reason: String, carry: Carry) {
        let rerun_limit = self.feature.config.limits.conflict_reruns;
        let s = self.slice(id);
        if !accept {
            close_attempt(s, AttemptResult::Conflict, Some(reason.clone()));
            if let Some(a) = s.last_attempt_mut() {
                a.backup = Some(carry.backup.clone());
            }
        }
        let kept = format!("{} ({})", carry.backup, short_commit(&carry.commit));
        let detail = format!("{reason}; previous work kept as {kept}");
        if !accept && s.conflict_reruns < rerun_limit {
            s.conflict_reruns += 1;
            s.carry = Some(carry);
            let detail = format!("{detail}; running it once more from the new tip");
            self.emit(Some(id), EventKind::Conflict, detail);
            self.dispatch_launch(id, AttemptKind::Rerun, false, false);
        } else {
            mark_failed(s, format!("conflict: {detail}"));
            self.emit(Some(id), EventKind::Conflict, detail);
        }
    }

    // ------------------------------------------------------------ requests

    fn apply_requests(&mut self) {
        for (path, request) in self.feature.pending_requests() {
            let (id, result) = match &request {
                Ok(request) => (request.id.clone(), self.apply(&request.action)),
                Err(e) => (
                    path.file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    Err(anyhow!("unreadable request: {e:#}")),
                ),
            };
            let answer = match result {
                Ok(message) => RequestResult {
                    id,
                    ok: true,
                    message,
                },
                Err(e) => {
                    let slice = request
                        .as_ref()
                        .ok()
                        .and_then(|r| r.action.slice())
                        .map(str::to_owned);
                    self.emit(
                        slice.as_deref(),
                        EventKind::RequestRejected,
                        format!("{e:#}"),
                    );
                    RequestResult {
                        id,
                        ok: false,
                        message: format!("{e:#}"),
                    }
                }
            };
            // Saved before answering, so whoever reads the answer sees the new state.
            if let Err(e) = self.save_if_unsaved() {
                eprintln!("zsf: cannot save the state: {e:#}");
            }
            if let Err(e) = self.feature.answer_request(&path, &answer) {
                eprintln!("zsf: cannot answer request {}: {e:#}", answer.id);
            }
        }
    }

    /// Applies one action; returns what to tell the person.
    pub fn apply(&mut self, action: &Action) -> Result<String> {
        match action {
            Action::Start => Ok(self.apply_start()),
            Action::Stop => Ok(self.apply_stop()),
            Action::Unpause => self.apply_unpause(),
            Action::SetMaxParallel { max_parallel } => self.apply_max_parallel(*max_parallel),
            Action::Approve { slice } => self.apply_approve(slice),
            Action::Retry {
                slice,
                notes,
                fresh,
            } => self.apply_retry(slice, notes.as_deref(), *fresh),
            Action::Accept { slice } => self.apply_accept(slice),
            Action::Skip { slice } => self.apply_skip(slice),
            Action::Edit { slice, edit } => self.edit(slice, edit),
        }
    }

    fn apply_start(&mut self) -> String {
        self.state.active = true;
        self.emit(None, EventKind::Started, "");
        format!("{} started", self.feature.config.name)
    }

    fn apply_stop(&mut self) -> String {
        self.state.active = false;
        self.emit(None, EventKind::Stopped, "");
        format!(
            "{} stopped; running Zeroshot runs continue and are picked up by the next start",
            self.feature.config.name
        )
    }

    fn apply_unpause(&mut self) -> Result<String> {
        let name = self.feature.config.name.clone();
        if self.state.paused.is_none() {
            bail!("{name} is not paused");
        }
        self.state.paused = None;
        self.state.consecutive_infra = 0;
        self.emit(None, EventKind::Unpaused, "");
        Ok(format!("{name} unpaused"))
    }

    fn apply_max_parallel(&mut self, max_parallel: u32) -> Result<String> {
        if max_parallel == 0 {
            bail!("max parallel must be at least 1");
        }
        self.feature.config.limits.max_parallel = max_parallel;
        self.feature.save_config()?;
        self.emit(
            None,
            EventKind::Edited,
            format!("max parallel {max_parallel}"),
        );
        Ok(format!("up to {max_parallel} slices at once"))
    }

    /// Approves a checkpoint waiting for review: the gate runs and it merges (its dependents start
    /// after that). A checkpoint an older zsf merged before its review is only released.
    fn apply_approve(&mut self, slice: &str) -> Result<String> {
        let plan = self.plan_slice(slice)?;
        let s = self.slice(slice);
        if !plan.checkpoint {
            bail!("{slice} is not a checkpoint");
        }
        if s.approved {
            return Ok(format!("{slice} was already approved"));
        }
        if s.waiting_for_review() {
            s.approved = true;
            self.emit(Some(slice), EventKind::Approved, "checking and merging");
            self.dispatch_integrate(slice, false);
            return Ok(format!("{slice} approved: checking and merging"));
        }
        if !s.merged_unapproved(plan.checkpoint) {
            bail!(
                "{slice} is {}, not waiting for review",
                s.status.as_str().replace('_', " ")
            );
        }
        s.approved = true;
        self.emit(Some(slice), EventKind::Approved, "");
        Ok(format!("{slice} approved"))
    }

    /// Gives a stopped slice a new attempt: now when a slice may start, otherwise queued as a
    /// pending slice that the scheduler launches later (after unpause, start, or when a slot
    /// frees up). On a slice waiting for review it requests changes: the reviewed attempt ends
    /// as `changes_requested` and the new one works on in the same worktree.
    fn apply_retry(&mut self, slice: &str, notes: Option<&str>, fresh: bool) -> Result<String> {
        self.plan_slice(slice)?;
        let s = self.slice(slice);
        let review = s.waiting_for_review();
        if !s.is_stopped() && !review {
            bail!(
                "{slice} is {}; only a stopped slice or one waiting for review can be retried",
                s.status.as_str().replace('_', " ")
            );
        }
        if let Some(notes) = notes.map(str::trim).filter(|n| !n.is_empty()) {
            s.notes.push(notes.to_owned());
        }
        if review {
            let why = notes.map(|n| format!("changes requested: {}", n.trim()));
            close_attempt(s, AttemptResult::ChangesRequested, why);
        }
        if fresh {
            s.carry = None;
        }
        s.infra_failures = 0;
        s.conflict_reruns = 0;
        s.gate_reruns = 0;
        // The new attempt changes the work: a checkpoint waits for review again.
        s.approved = false;
        s.retry = None;
        self.state.consecutive_infra = 0;
        let mut detail = match (notes, fresh) {
            (Some(n), true) => format!("fresh, with notes: {n}"),
            (Some(n), false) => format!("with notes: {n}"),
            (None, true) => "fresh".to_owned(),
            (None, false) => String::new(),
        };
        if review {
            detail = match detail.is_empty() {
                true => "changes requested".to_owned(),
                false => format!("changes requested, {detail}"),
            };
        }
        self.emit(Some(slice), EventKind::Retried, detail);
        if let Some(when) = self.start_blocker() {
            let s = self.slice(slice);
            s.status = SliceStatus::Pending;
            s.retry = Some(QueuedRetry { fresh });
            s.reason = None;
            self.unsaved = true;
            return Ok(format!("{slice} retries {when}"));
        }
        self.dispatch_launch(slice, AttemptKind::ManualRetry, fresh, false);
        let n = self
            .state
            .slices
            .get(slice)
            .map(|s| s.attempts.len())
            .unwrap_or(0);
        Ok(format!("{slice}: attempt {n} started"))
    }

    fn apply_accept(&mut self, slice: &str) -> Result<String> {
        self.plan_slice(slice)?;
        let s = self.slice(slice);
        if !s.is_stopped() || s.worktree_on_disk().is_none() {
            bail!(
                "{slice} is {}; accept takes a stopped slice whose worktree you finished",
                s.status.as_str()
            );
        }
        self.dispatch_integrate(slice, true);
        Ok(format!("merging {slice} without the gate"))
    }

    fn apply_skip(&mut self, slice: &str) -> Result<String> {
        self.plan_slice(slice)?;
        let s = self.slice(slice);
        if s.job.is_some() {
            bail!(
                "{slice} is {}; try again when that is done",
                s.status.as_str()
            );
        }
        if s.status == SliceStatus::Merged {
            bail!("{slice} is already merged");
        }
        let mut detail = String::new();
        if s.status == SliceStatus::Running
            && let Some(run) = s.last_attempt().and_then(|a| a.run_id.clone())
            && let Err(e) = self.zeroshot.force_stop(&run)
        {
            detail = format!("could not stop run {run}: {e:#}");
        }
        let s = self.slice(slice);
        if let Some(a) = s.last_attempt_mut() {
            // Unlike `close_attempt`, keeps the result and reason the attempt already had.
            a.result.get_or_insert(AttemptResult::Skipped);
            a.ended_ms.get_or_insert(now_ms());
        }
        s.retry = None;
        s.status = SliceStatus::Skipped;
        s.reason = Some("skipped".to_owned());
        self.emit(Some(slice), EventKind::Skipped, detail);
        Ok(format!("{slice} skipped"))
    }

    fn edit(&mut self, id: &str, edit: &SliceEdit) -> Result<String> {
        self.plan_slice(id)?;
        let s = self.state.slices.get(id).cloned().unwrap_or_default();
        if !s.is_editable() {
            bail!(
                "{id} is {}; only a slice that has not started can be edited",
                s.status.as_str()
            );
        }
        let mut plan = self.plan.clone();
        let mut changes = Vec::new();
        {
            let p = plan.get_mut(id).expect("slice");
            if let Some(title) = edit.title.as_deref().map(str::trim) {
                if title.is_empty() {
                    bail!("the title cannot be empty");
                }
                p.title = title.to_owned();
                changes.push(format!("title {title:?}"));
            }
            if let Some(spec) = &edit.spec {
                if !spec.is_absolute() || !spec.is_file() {
                    bail!(
                        "the spec {} is not a file (give an absolute path)",
                        spec.display()
                    );
                }
                p.spec = spec.clone();
                changes.push("spec".to_owned());
            }
            if let Some(deps) = &edit.deps {
                let mut deps = deps.clone();
                deps.dedup();
                p.deps = deps;
                changes.push(if p.deps.is_empty() {
                    "no dependencies".to_owned()
                } else {
                    format!("after {}", p.deps.join(", "))
                });
            }
            if let Some(checkpoint) = edit.checkpoint {
                p.checkpoint = checkpoint;
                changes.push(format!("checkpoint {checkpoint}"));
            }
        }
        if let Some((target, after)) = edit
            .before
            .as_ref()
            .map(|t| (t, false))
            .or(edit.after.as_ref().map(|t| (t, true)))
        {
            if target == id {
                bail!("cannot move {id} relative to itself");
            }
            let from = plan.slices.iter().position(|p| p.id == id).expect("slice");
            let moved = plan.slices.remove(from);
            let to = plan
                .slices
                .iter()
                .position(|p| &p.id == target)
                .ok_or_else(|| anyhow!("no slice {target} in the plan"))?;
            plan.slices.insert(if after { to + 1 } else { to }, moved);
            changes.push(format!(
                "{} {target}",
                if after { "after" } else { "before" }
            ));
        }
        if changes.is_empty() {
            bail!("nothing to change");
        }
        plan.validate(&self.feature.config.leases)?;
        self.feature.save_plan(&plan)?;
        self.plan = plan;
        let detail = changes.join(", ");
        self.emit(Some(id), EventKind::Edited, detail.clone());
        Ok(format!("{id}: {detail}"))
    }
}

/// Closes the slice's last attempt with `result` and `reason` (its end time is kept when set).
fn close_attempt(s: &mut SliceState, result: AttemptResult, reason: Option<String>) {
    if let Some(a) = s.last_attempt_mut() {
        a.result = Some(result);
        a.reason = reason;
        a.ended_ms.get_or_insert(now_ms());
    }
}

/// The slice is failed with `reason` and has no job: it waits for a person.
fn mark_failed(s: &mut SliceState, reason: String) {
    s.status = SliceStatus::Failed;
    s.reason = Some(reason);
    s.job = None;
}

/// Attempt `n`, started now and not launched yet.
fn new_attempt(
    n: u32,
    kind: AttemptKind,
    fresh: bool,
    files: AttemptFiles,
    launch_key: String,
) -> Attempt {
    Attempt {
        n,
        kind,
        fresh,
        task_file: files.task,
        log_file: Some(files.log),
        launch_key: Some(launch_key),
        started_ms: now_ms(),
        run_id: None,
        earlier_runs: Vec::new(),
        ended_ms: None,
        resumed_ms: None,
        result: None,
        reason: None,
        resume_tried: false,
        backup: None,
    }
}

/// The first 10 characters of a commit id, for events and reasons.
fn short_commit(commit: &str) -> &str {
    &commit[..commit.len().min(10)]
}
