//! What a person does to a feature or a slice: checked against the actions zsf offers now
//! (`zsf::actions`), then queued to the feature's daemon, the one writer.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zsf::model::{Action, Plan, SliceEdit, SlicePlan, State};
use zsf::store::Feature;
use zsf::{FeatureAction, FeatureFacts, SliceAction};

use super::project::worktree_changes;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionBody {
    pub action: String,
    pub notes: Option<String>,
    #[serde(default)]
    pub fresh: bool,
    pub max_parallel: Option<u32>,
    pub title: Option<String>,
    pub deps: Option<Vec<String>>,
    /// New spec text (edit): saved as `edits/<id>-<ms>.md` in the feature directory
    /// ([`edited_spec_path`]).
    pub spec: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ActOutcome {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug)]
pub enum ActError {
    NotFound(String),
    Refused(String),
    Bad(String),
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for ActError {
    fn from(e: anyhow::Error) -> Self {
        ActError::Failed(e)
    }
}

/// How long a queued request waits for the daemon's answer.
const WAIT: Duration = Duration::from_secs(15);
/// How long `stop` waits for the daemon to exit on its own before stopping it.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// How often `stop` looks whether the daemon has exited.
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const STOPPED: &str = "Stopped launching; running slices keep running";
/// `retry` without notes on a slice waiting for review (the UI's Request changes).
const REQUEST_CHANGES_NEEDS_NOTES: &str =
    "Request changes needs notes: say what to change (`notes`).";
/// Slices a feature may run at once (`max_parallel`).
const PARALLEL_LIMITS: std::ops::RangeInclusive<u32> = 1..=64;

/// `Can't approve 03: it is merged now.` (`verb`: the action's name, or how a sentence says it.)
fn refusal(verb: &str, subject: &str, why: &str) -> String {
    format!("Can't {verb} {subject}: {why}.")
}

fn feature_verb(action: FeatureAction) -> &'static str {
    match action {
        FeatureAction::MaxParallel => "change the parallel limit of",
        other => other.as_str(),
    }
}

/// `Unknown action `x`; use one of: start, stop, unpause, max_parallel.`
fn unknown_action(action: &str, known: &[&str]) -> ActError {
    ActError::Bad(format!(
        "Unknown action `{action}`; use one of: {}.",
        known.join(", ")
    ))
}

/// Applies one action to a feature (`slice: None`) or a slice, if zsf would accept it now.
pub fn act(
    root: &Path,
    zsf_bin: Option<&Path>,
    name: &str,
    slice: Option<&str>,
    body: ActionBody,
) -> Result<ActOutcome, ActError> {
    if !super::has_feature(root, name) {
        return Err(ActError::NotFound(format!("No feature `{name}`.")));
    }
    // A name outside the action lists is refused before anything is read.
    match slice {
        None => {
            let Some(action) = FeatureAction::parse(&body.action) else {
                return Err(unknown_action(
                    &body.action,
                    &FeatureAction::ALL.map(FeatureAction::as_str),
                ));
            };
            let (feature, plan, state) = open_readable(root, name)?;
            act_on_feature(root, zsf_bin, name, &feature, &plan, &state, action, body)
        }
        Some(id) => {
            let Some(action) = SliceAction::parse(&body.action) else {
                return Err(unknown_action(
                    &body.action,
                    &SliceAction::ALL.map(SliceAction::as_str),
                ));
            };
            let (feature, plan, state) = open_readable(root, name)?;
            act_on_slice(name, &feature, &plan, &state, id, action, body)
        }
    }
}

/// The feature with its plan and state; nothing can change while either cannot be read.
fn open_readable(root: &Path, name: &str) -> Result<(Feature, Plan, State), ActError> {
    let feature = Feature::open(root, name)?;
    let plan = feature.load_plan().map_err(|e| {
        ActError::Refused(format!(
            "Nothing can change while the plan cannot be read: {e:#}"
        ))
    })?;
    let state = feature.load_state().map_err(|e| {
        ActError::Refused(format!(
            "Nothing can change while the state cannot be read: {e:#}"
        ))
    })?;
    Ok((feature, plan, state))
}

/// Queues `action` to the daemon and waits for its answer (a refusal is zsf's own message).
fn submit(feature: &Feature, action: Action) -> Result<ActOutcome, ActError> {
    let result = zsf::daemon::submit(feature, action, WAIT)?;
    if result.ok {
        Ok(ActOutcome {
            ok: true,
            message: result.message,
        })
    } else {
        Err(ActError::Refused(result.message))
    }
}

#[allow(clippy::too_many_arguments)]
fn act_on_feature(
    root: &Path,
    zsf_bin: Option<&Path>,
    name: &str,
    feature: &Feature,
    plan: &Plan,
    state: &State,
    action: FeatureAction,
    body: ActionBody,
) -> Result<ActOutcome, ActError> {
    let all_finished = state.all_finished(plan);
    let facts = FeatureFacts {
        all_finished,
        daemon_alive: feature.daemon_pid().is_some(),
        can_spawn: zsf_bin.is_some(),
    };
    if !zsf::actions::feature_actions(state, facts).contains(&action) {
        let why = if all_finished {
            "done"
        } else {
            "in another state now"
        };
        return Err(ActError::Refused(refusal(feature_verb(action), name, why)));
    }
    match action {
        FeatureAction::Start => {
            let bin = zsf_bin.ok_or_else(|| {
                ActError::Refused("No `zsf` program found to run the daemon.".to_owned())
            })?;
            feature.queue_request(Action::Start)?;
            zsf::daemon::spawn_with(feature, root, true, bin)?;
            Ok(ActOutcome {
                ok: true,
                message: "Started".to_owned(),
            })
        }
        FeatureAction::Stop if feature.daemon_pid().is_some() => {
            stop_daemon(feature)?;
            Ok(ActOutcome {
                ok: true,
                message: STOPPED.to_owned(),
            })
        }
        FeatureAction::Stop => submit(feature, Action::Stop),
        FeatureAction::Unpause => submit(feature, Action::Unpause),
        FeatureAction::MaxParallel => match body.max_parallel {
            Some(n) if PARALLEL_LIMITS.contains(&n) => {
                submit(feature, Action::SetMaxParallel { max_parallel: n })
            }
            _ => Err(ActError::Bad("Run 1 to 64 slices at once.".to_owned())),
        },
    }
}

/// Asks the running daemon to stop and gives it [`STOP_GRACE`] to exit; stops it if it has not.
fn stop_daemon(feature: &Feature) -> anyhow::Result<()> {
    feature.queue_request(Action::Stop)?;
    let deadline = Instant::now() + STOP_GRACE;
    while feature.daemon_pid().is_some() && Instant::now() < deadline {
        std::thread::sleep(POLL_INTERVAL);
    }
    if feature.daemon_pid().is_some() {
        zsf::daemon::stop(feature)?;
    }
    Ok(())
}

fn act_on_slice(
    name: &str,
    feature: &Feature,
    plan: &Plan,
    state: &State,
    id: &str,
    action: SliceAction,
    body: ActionBody,
) -> Result<ActOutcome, ActError> {
    let Some(p) = plan.get(id) else {
        return Err(ActError::NotFound(format!("No slice `{id}` in {name}.")));
    };
    let s = state.slices.get(id).cloned().unwrap_or_default();
    let changed = worktree_changes(&feature.config, &s).is_some_and(|c| !c.files.is_empty());
    if !zsf::actions::slice_actions(p, &s, changed).contains(&action) {
        let why = if action == SliceAction::Accept && s.is_stopped() {
            "its worktree has no changes to merge".to_owned()
        } else {
            format!(
                "it is {} now",
                s.display_status(p.checkpoint).replace('_', " ")
            )
        };
        return Err(ActError::Refused(refusal(action.as_str(), id, &why)));
    }
    let slice = id.to_owned();
    match action {
        SliceAction::Approve => submit(feature, Action::Approve { slice }),
        SliceAction::Accept => submit(feature, Action::Accept { slice }),
        SliceAction::Skip => submit(feature, Action::Skip { slice }),
        SliceAction::Retry => {
            let notes = body
                .notes
                .map(|n| n.trim().to_owned())
                .filter(|n| !n.is_empty());
            // On a slice waiting for review, retry is "request changes": the notes are the review.
            if s.waiting_for_review() && notes.is_none() {
                return Err(ActError::Bad(REQUEST_CHANGES_NEEDS_NOTES.to_owned()));
            }
            submit(
                feature,
                Action::Retry {
                    slice,
                    notes,
                    fresh: body.fresh,
                },
            )
        }
        SliceAction::Edit => {
            let edit = slice_edit(feature, plan, p, body)?;
            submit(feature, Action::Edit { slice, edit })
        }
    }
}

/// Only what changed; a new spec text is written to its own file (zsf takes a spec path).
fn slice_edit(
    feature: &Feature,
    plan: &Plan,
    p: &SlicePlan,
    body: ActionBody,
) -> Result<SliceEdit, ActError> {
    let mut edit = SliceEdit::default();
    if let Some(title) = body.title.map(|t| t.trim().to_owned()) {
        if title.is_empty() {
            return Err(ActError::Bad("the title cannot be empty".to_owned()));
        }
        if title != p.title {
            edit.title = Some(title);
        }
    }
    if let Some(deps) = body.deps {
        let deps = normalise_deps(deps);
        if let Some(unknown) = deps.iter().find(|d| plan.get(d).is_none()) {
            return Err(ActError::Bad(format!(
                "{unknown} is not a slice of this feature"
            )));
        }
        if deps.contains(&p.id) {
            return Err(ActError::Bad(format!("{} can't come after itself", p.id)));
        }
        let mut changed = plan.clone();
        if let Some(q) = changed.get_mut(&p.id) {
            q.deps = deps.clone();
        }
        if let Some(path) = dependency_loop(&changed, &p.id) {
            return Err(ActError::Bad(format!(
                "That would make a loop: {}",
                path.join(" → ")
            )));
        }
        if deps != p.deps {
            edit.deps = Some(deps);
        }
    }
    if let Some(spec) = body.spec {
        let current = std::fs::read_to_string(feature.spec_path(&p.spec)).unwrap_or_default();
        if spec.trim_end() != current.trim_end() {
            let path = edited_spec_path(feature, &p.id);
            std::fs::create_dir_all(path.parent().unwrap_or(&feature.dir))
                .map_err(anyhow::Error::from)?;
            zsf::store::write_atomic(&path, format!("{}\n", spec.trim_end()).as_bytes())?;
            edit.spec = Some(path);
        }
    }
    if edit == SliceEdit::default() {
        return Err(ActError::Bad("nothing to change".to_owned()));
    }
    Ok(edit)
}

/// Dependency ids trimmed, blanks dropped, each kept once (first occurrence, order kept).
fn normalise_deps(deps: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    deps.into_iter()
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty())
        .filter(|d| seen.insert(d.clone()))
        .collect()
}

/// Where an edited spec text is saved: `edits/<slice id>-<ms>.md` in the feature directory.
fn edited_spec_path(feature: &Feature, slice: &str) -> PathBuf {
    feature
        .dir
        .join("edits")
        .join(format!("{slice}-{}.md", crate::clock::now_ms()))
}

/// A dependency path from `id` back to itself (`06 → 05 → 06`), if the plan has one.
pub(super) fn dependency_loop(plan: &Plan, id: &str) -> Option<Vec<String>> {
    fn walk(
        plan: &Plan,
        target: &str,
        at: &str,
        path: &mut Vec<String>,
        seen: &mut BTreeSet<String>,
    ) -> bool {
        for dep in plan.get(at).map(|p| p.deps.clone()).unwrap_or_default() {
            path.push(dep.clone());
            if dep == target || (seen.insert(dep.clone()) && walk(plan, target, &dep, path, seen)) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = vec![id.to_owned()];
    walk(plan, id, id, &mut path, &mut BTreeSet::new()).then_some(path)
}
