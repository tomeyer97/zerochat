//! Where a local run's files are, and which commit its changes are compared with.
//!
//! A run's workspace is normally still on disk. A zsf slice's worktree is removed once the slice
//! is merged; its run is then read from the feature's repository, which still holds the commits
//! Zeroshot snapshotted the workspace to.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::actions;
use crate::source::{self, Event, RunData};

/// Where a local run's files can be read.
pub struct Workspace {
    pub path: PathBuf,
    /// The run's own workspace is gone (a zsf slice worktree removed after merging); `path` is
    /// the repository it belonged to, which still holds the run's snapshot commits.
    pub removed: bool,
    /// The run's last workspace snapshot: "now" for a removed workspace.
    last_snapshot: Option<String>,
}

/// A removed workspace of which Zeroshot recorded no snapshot: there is no "now" to show.
#[derive(Debug, PartialEq, Eq)]
pub struct NoSnapshot;

impl Workspace {
    /// `to` for a range ending "now": the working tree (None), or the last snapshot of a removed one.
    pub fn now(&self, to: Option<String>) -> Result<Option<String>, NoSnapshot> {
        match (to, self.removed) {
            (Some(to), _) => Ok(Some(to)),
            (None, false) => Ok(None),
            (None, true) => self.last_snapshot.clone().map(Some).ok_or(NoSnapshot),
        }
    }
}

/// The run's workspace, if it is on this machine (or was, in a zsf feature's repository).
pub fn workspace_of(data: &RunData) -> Option<Workspace> {
    workspace_in(data, &zsf::store::state_root())
}

/// [`workspace_of`] with zsf's state root given.
pub fn workspace_in(data: &RunData, zsf_root: &Path) -> Option<Workspace> {
    let path = PathBuf::from(data.local.as_ref()?.workspace.clone()?);
    if path.is_dir() {
        return Some(Workspace {
            path,
            removed: false,
            last_snapshot: None,
        });
    }
    let repo = zsf_repository(zsf_root, &path)?;
    Some(Workspace {
        path: repo,
        removed: true,
        last_snapshot: last_snapshot(&data.events),
    })
}

/// The repository of the zsf feature whose slice worktree `workspace` was.
fn zsf_repository(root: &Path, workspace: &Path) -> Option<PathBuf> {
    zsf::store::list_features(root)
        .into_iter()
        .filter_map(|name| zsf::store::Feature::open(root, &name).ok())
        .find_map(|feature| {
            let repo = feature.resolve(&feature.config.repo);
            (workspace.starts_with(feature.worktrees_dir()) && repo.is_dir()).then_some(repo)
        })
}

/// How the commit a run's changes are compared with was chosen (`base` in the changes answer).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseKind {
    /// Given by the request: one execution's own snapshot commits.
    Execution,
    /// The revision the run was admitted with.
    Source,
    /// The workspace snapshot Zeroshot took when the run's first node started.
    Snapshot,
    /// Guessed: the newest commit made before the run was created.
    CheckedOut,
}

/// What a run's changes are compared with, and how that commit was chosen.
pub async fn run_base(data: &RunData, workspace: &Path) -> Option<(String, BaseKind)> {
    if let Some(revision) = data
        .admitted
        .pointer("/source/revision")
        .and_then(Value::as_str)
    {
        return Some((revision.to_owned(), BaseKind::Source));
    }
    if let Some(commit) = first_snapshot(&data.events) {
        return Some((commit, BaseKind::Snapshot));
    }
    let created = source::uuid_v7_millis(&data.run_id)?;
    actions::commit_at(workspace, created)
        .await
        .map(|c| (c, BaseKind::CheckedOut))
}

// The older patched Zeroshot snapshotted the workspace to a commit: `node_started` carries the
// commit a node starts from (`/workspace/commit`), `node_completed` the base its changes are
// counted from (`/workspace/base`) and the commit it ended with (`/workspace/commit`), either
// at the top level or under `/completion`.

/// The run's base: the first snapshot a node started from (or counted its changes from).
fn first_snapshot(events: &[Event]) -> Option<String> {
    events.iter().find_map(|e| {
        let body = &e.body;
        let pointer = match body["kind"].as_str() {
            Some("node_started") => "/workspace/commit",
            Some("node_completed") => "/workspace/base",
            _ => return None,
        };
        body.pointer(pointer)
            .or_else(|| body.pointer(&format!("/completion{pointer}")))
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

/// The commit the last completed node left the workspace at.
fn last_snapshot(events: &[Event]) -> Option<String> {
    events.iter().rev().find_map(|e| {
        (e.body["kind"] == "node_completed")
            .then(|| {
                e.body
                    .pointer("/workspace/commit")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn events(bodies: Vec<Value>) -> Vec<Event> {
        bodies
            .into_iter()
            .zip(1..)
            .map(|(body, seq)| Event { seq, body })
            .collect()
    }

    #[test]
    fn the_first_snapshot_is_the_base_and_the_last_completion_is_now() {
        let run = events(vec![
            json!({"kind": "node_started", "completion": {"workspace": {"commit": "s1"}}}),
            json!({"kind": "node_completed", "workspace": {"base": "s1", "commit": "c1"}}),
            json!({"kind": "node_completed", "workspace": {"base": "c1", "commit": "c2"}}),
            json!({"kind": "terminal"}),
        ]);
        assert_eq!(first_snapshot(&run).as_deref(), Some("s1"));
        assert_eq!(last_snapshot(&run).as_deref(), Some("c2"));
        assert_eq!(
            first_snapshot(&events(vec![json!({"kind": "terminal"})])),
            None
        );
    }

    #[test]
    fn base_kinds_keep_their_wire_names() {
        let names: Vec<Value> = [
            BaseKind::Execution,
            BaseKind::Source,
            BaseKind::Snapshot,
            BaseKind::CheckedOut,
        ]
        .iter()
        .map(|k| serde_json::to_value(k).unwrap())
        .collect();
        assert_eq!(
            names,
            [
                json!("execution"),
                json!("source"),
                json!("snapshot"),
                json!("checked_out")
            ]
        );
    }
}
