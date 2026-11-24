//! A feature's and a slice's files, read on demand: tasks, artifacts, events, and the git
//! reads (squash commit, worktree diff, the whole feature's changes).

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};
use zsf::model::Event;
use zsf::store::Feature;

use super::git::{
    DiffOutput, LOG_FORMAT, git_command, git_out, parse_log, parse_numstat, worktree_against_branch,
};
use super::line_diff::line_diff;
use super::runs::{Roots, locate};
use super::view::{CommitView, EventView, FeatureChanges, TaskView, WorktreeDiff};
use super::{open_feature, open_slice};

/// The most a view returns of one text (longer ones are cut, see `cap`).
pub(super) const MAX_SPEC_BYTES: usize = 200_000;
const MAX_TASK_BYTES: usize = 400_000;
const MAX_ARTIFACT_BYTES: usize = 1_000_000;
const MAX_DIFF_BYTES: usize = 600_000;
/// The most artifact names a slice lists.
const MAX_ARTIFACTS: usize = 500;

pub(super) fn cap(mut text: String, max: usize) -> String {
    if text.len() > max {
        let cut = crate::text::truncate_utf8(&text, max).len();
        text.truncate(cut);
        text.push_str("\n… [truncated]");
    }
    text
}

// ---------------------------------------------------------------- tasks

/// The task file zsf wrote for attempt `n` of a slice: `<slice>-a<n>.md` (zsf's layout).
fn task_file_name(slice: &str, n: u32) -> String {
    format!("{slice}-a{n}.md")
}

/// Where the task of attempt `n` of a slice is, in the feature's tasks directory.
pub(super) fn task_file(feature: &Feature, slice: &str, n: u32) -> PathBuf {
    feature.tasks_dir().join(task_file_name(slice, n))
}

/// The exact task attempt `n` was given, and how it differs from the attempt before.
pub fn task(root: &Path, name: &str, slice: &str, n: u32) -> Result<Option<TaskView>> {
    let Some(feature) = open_slice(root, name, slice)? else {
        return Ok(None);
    };
    let Ok(text) = std::fs::read_to_string(task_file(&feature, slice, n)) else {
        return Ok(None);
    };
    let previous = latest_earlier_task(&feature, slice, n);
    let diff = previous.as_ref().map(|(m, old)| {
        line_diff(
            old,
            &text,
            &task_file_name(slice, *m),
            &task_file_name(slice, n),
        )
    });
    Ok(Some(TaskView {
        attempt: n,
        text: cap(text, MAX_TASK_BYTES),
        previous_attempt: previous.map(|(m, _)| m),
        diff,
    }))
}

/// The task of the latest attempt before `n` that has one (an attempt may have left none):
/// its number and text.
fn latest_earlier_task(feature: &Feature, slice: &str, n: u32) -> Option<(u32, String)> {
    (1..n).rev().find_map(|m| {
        std::fs::read_to_string(task_file(feature, slice, m))
            .ok()
            .map(|text| (m, text))
    })
}

// ---------------------------------------------------------------- artifacts

/// Files saved under `artifacts/<slice>/`, relative, sorted (at most 500).
pub fn artifacts(feature: &Feature, slice: &str) -> Vec<String> {
    let base = feature.artifacts_dir().join(slice);
    let mut out = Vec::new();
    let mut stack = vec![base.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(&base) {
                out.push(rel.to_string_lossy().into_owned());
            }
            if out.len() >= MAX_ARTIFACTS {
                break;
            }
        }
    }
    out.sort();
    out
}

/// A relative path with only plain components (no `..`, no root).
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    (!path.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_))))
        .then(|| p.to_path_buf())
}

pub fn artifact(root: &Path, name: &str, slice: &str, file: &str) -> Result<Option<String>> {
    let Some(feature) = open_slice(root, name, slice)? else {
        return Ok(None);
    };
    let Some(rel) = safe_relative(file) else {
        return Ok(None);
    };
    let path = feature.artifacts_dir().join(slice).join(rel);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(cap(
            String::from_utf8_lossy(&bytes).into_owned(),
            MAX_ARTIFACT_BYTES,
        ))),
        Err(_) => Ok(None),
    }
}

// ---------------------------------------------------------------- the pull request

/// The file a run that opened a pull request records its link in (the first line that is a
/// pull or merge request URL), inside one of the feature's declared artifact directories
/// (`artifacts = [".zeroshot-pr"]`) or declared as an artifact itself.
const PR_FILE: &str = "PR.md";
/// A PR file is read only this far.
const MAX_PR_FILE_BYTES: u64 = 64 * 1024;

/// The pull request a slice's run delivered, if it recorded one: `PR.md` among the feature's
/// declared artifacts, in the slice's worktree (the run that waits for review) first, else in
/// what zsf saved under `artifacts/<slice>/`.
pub(super) fn pr_url(feature: &Feature, slice: &str, worktree: Option<&Path>) -> Option<String> {
    let saved = feature.artifacts_dir().join(slice);
    let bases = worktree.into_iter().chain(std::iter::once(saved.as_path()));
    bases
        .flat_map(|base| pr_candidates(&feature.config.artifacts, base))
        .find_map(|path| read_pr_url(&path))
}

/// Where a PR file can be under `base`: each declared artifact that is a `PR.md`, else the
/// `PR.md` inside it (a declared directory).
fn pr_candidates(declared: &[String], base: &Path) -> Vec<PathBuf> {
    declared
        .iter()
        .filter_map(|rel| safe_relative(rel.trim_end_matches('/')))
        .map(|rel| {
            if rel.file_name().is_some_and(|f| f == PR_FILE) {
                base.join(rel)
            } else {
                base.join(rel).join(PR_FILE)
            }
        })
        .collect()
}

/// The first `http(s)://…/pull/<n>` (or GitLab's `/merge_requests/<n>`) in the file.
fn read_pr_url(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_PR_FILE_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    text.split(|c: char| {
        c.is_whitespace() || matches!(c, '<' | '>' | '(' | ')' | '[' | ']' | '"' | '\'')
    })
    .find(|word| is_pr_url(word))
    .map(str::to_owned)
}

pub(super) fn is_pr_url(word: &str) -> bool {
    let Some(rest) = word
        .strip_prefix("https://")
        .or_else(|| word.strip_prefix("http://"))
    else {
        return false;
    };
    let mut parts = rest.split('/').skip(1).collect::<Vec<_>>();
    if parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    let n = parts.len();
    n >= 2
        && matches!(parts[n - 2], "pull" | "pulls" | "merge_requests")
        && !parts[n - 1].is_empty()
        && parts[n - 1].chars().all(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------- events

/// The feature's events, oldest first (optionally one slice's), at most the last `limit`.
pub fn events(
    root: &Path,
    roots: &Roots,
    name: &str,
    slice: Option<&str>,
    limit: usize,
) -> Result<Option<Vec<EventView>>> {
    let Some(feature) = open_feature(root, name)? else {
        return Ok(None);
    };
    let all: Vec<Event> = feature
        .events()
        .into_iter()
        .filter(|e| slice.is_none_or(|s| e.slice.as_deref() == Some(s)))
        .collect();
    let skip = all.len().saturating_sub(limit);
    // Each run is looked up once, however many events name it.
    let mut targets: HashMap<String, Option<String>> = HashMap::new();
    let mut target_of = |run: &String| {
        targets
            .entry(run.clone())
            .or_insert_with(|| locate(roots, run).map(|(target, _)| target))
            .clone()
    };
    let views = all
        .into_iter()
        .skip(skip)
        .map(|event| {
            let run_target = event.run_id.as_ref().and_then(&mut target_of);
            EventView { event, run_target }
        })
        .collect();
    Ok(Some(views))
}

// ---------------------------------------------------------------- git reads

/// The merged slice's squash commit, read from the feature repository.
pub fn commit(root: &Path, name: &str, slice: &str) -> Result<Option<CommitView>> {
    let Some(feature) = open_slice(root, name, slice)? else {
        return Ok(None);
    };
    let state = feature.load_state()?;
    let Some(commit) = state.slices.get(slice).and_then(|s| s.commit.clone()) else {
        return Ok(None);
    };
    if !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(None);
    }
    let repo = feature.resolve(&feature.config.repo);
    let message = git_out(&repo, &["show", "-s", "--format=%B", &commit], None)?
        .trim()
        .to_owned();
    let out = git_command(
        &repo,
        &[
            "show",
            "--format=",
            "--patch",
            "--no-color",
            "--no-ext-diff",
            &commit,
        ],
        None,
    )?;
    if !out.status.success() {
        return Err(anyhow!(
            "git show {commit}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let diff = String::from_utf8_lossy(&out.stdout).trim_end().to_owned();
    Ok(Some(CommitView {
        commit,
        message,
        diff: cap(diff, MAX_DIFF_BYTES),
    }))
}

/// A slice's worktree against the commit it forked from (what Accept or Approve would merge).
pub fn worktree_diff(root: &Path, name: &str, slice: &str) -> Result<Option<WorktreeDiff>> {
    let Some(feature) = open_slice(root, name, slice)? else {
        return Ok(None);
    };
    let state = feature.load_state()?;
    let Some(worktree) = state
        .slices
        .get(slice)
        .and_then(|s| s.worktree.clone())
        .filter(|w| w.is_dir())
    else {
        return Ok(None);
    };
    let (fork, diff) =
        worktree_against_branch(&worktree, &feature.config.branch, DiffOutput::Patch)?;
    Ok(Some(WorktreeDiff {
        fork,
        diff: cap(diff, MAX_DIFF_BYTES),
    }))
}

/// What the feature produced: its branch against the base (`base...branch`), commits oldest
/// first with their slice; the patch only when asked for.
pub fn changes(root: &Path, name: &str, with_diff: bool) -> Result<Option<FeatureChanges>> {
    let Some(feature) = open_feature(root, name)? else {
        return Ok(None);
    };
    let repo = feature.resolve(&feature.config.repo);
    let (base, branch) = (feature.config.base.clone(), feature.config.branch.clone());
    let head = git_out(&repo, &["rev-parse", &format!("refs/heads/{branch}")], None)?
        .trim()
        .to_owned();
    let range = format!("{base}..refs/heads/{branch}");
    let commits = parse_log(&git_out(
        &repo,
        &["log", "--reverse", LOG_FORMAT, &range],
        None,
    )?);
    let three_dot = format!("{base}...refs/heads/{branch}");
    let stat = parse_numstat(&git_out(
        &repo,
        &["diff", "--numstat", "--no-renames", &three_dot],
        None,
    )?);
    let diff = if with_diff {
        Some(cap(
            git_out(
                &repo,
                &["diff", "--no-color", "--no-ext-diff", &three_dot],
                None,
            )?,
            MAX_DIFF_BYTES,
        ))
    } else {
        None
    };
    Ok(Some(FeatureChanges {
        base,
        branch,
        head,
        commits,
        stat,
        diff,
    }))
}
