//! The git reads behind the features views: zerochat only reads git, zsf is the one writer.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Result, anyhow};

use super::view::{ChangeStat, FeatureCommit, FileStat};

/// zerochat's one git wrapper (it only reads): runs git in `dir` without optional locks, so a
/// read never blocks zsf's writes; `index` replaces the repository's index file.
pub(super) fn git_command(
    dir: &Path,
    args: &[&str],
    index: Option<&Path>,
) -> Result<std::process::Output> {
    let mut command = std::process::Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0");
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    Ok(command.output()?)
}

/// git's stdout, or an error with git's message.
pub(super) fn git_out(dir: &Path, args: &[&str], index: Option<&Path>) -> Result<String> {
    let out = git_command(dir, args, index)?;
    if !out.status.success() {
        return Err(anyhow!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The commit `rev` names, if it names one.
pub(super) fn rev_parse(repo: &Path, rev: &str) -> Option<String> {
    let commit = format!("{rev}^{{commit}}");
    git_out(repo, &["rev-parse", "--verify", "-q", &commit], None)
        .ok()
        .map(|out| out.trim().to_owned())
}

/// What [`worktree_against_branch`] asks git for.
#[derive(Clone, Copy, Debug)]
pub(super) enum DiffOutput {
    /// `--numstat`: additions and deletions per file (see [`parse_numstat`]).
    Numstat,
    /// The unified patch.
    Patch,
}

/// Numbers the temporary index files, so two reads at once never share one.
static NEXT_INDEX_FILE: AtomicU64 = AtomicU64::new(0);

/// A stopped slice's worktree (committed and uncommitted work, new files too) against the commit
/// it forked from on the feature branch, without touching the worktree's own index: the worktree
/// is staged into a temporary index file and diffed from there. Returns the fork commit and
/// git's output.
pub(super) fn worktree_against_branch(
    worktree: &Path,
    branch: &str,
    output: DiffOutput,
) -> Result<(String, String)> {
    let branch_ref = format!("refs/heads/{branch}");
    let fork = git_out(worktree, &["merge-base", "HEAD", &branch_ref], None)?
        .trim()
        .to_owned();
    let n = NEXT_INDEX_FILE.fetch_add(1, Ordering::Relaxed);
    let index_path =
        std::env::temp_dir().join(format!("zerochat-index-{}-{n}", std::process::id()));
    git_out(worktree, &["read-tree", "HEAD"], Some(&index_path))?;
    git_out(worktree, &["add", "-A"], Some(&index_path))?;
    let args: Vec<&str> = match output {
        DiffOutput::Numstat => vec!["diff", "--cached", "--numstat", "--no-renames", &fork],
        DiffOutput::Patch => vec!["diff", "--cached", "--no-color", "--no-ext-diff", &fork],
    };
    let out = git_out(worktree, &args, Some(&index_path));
    std::fs::remove_file(&index_path).ok();
    Ok((fork, out?))
}

/// `git diff --numstat` output: one `additions<TAB>deletions<TAB>path` line per file (`-` for a
/// binary file counts as 0).
pub(super) fn parse_numstat(text: &str) -> ChangeStat {
    let mut stat = ChangeStat::default();
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(added), Some(deleted), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let (additions, deletions) = (added.parse().unwrap_or(0), deleted.parse().unwrap_or(0));
        stat.additions += additions;
        stat.deletions += deletions;
        stat.files.push(FileStat {
            path: path.to_owned(),
            additions,
            deletions,
        });
    }
    stat
}

/// Separates the fields of one commit in [`LOG_FORMAT`] (git's `%x1f`, ASCII unit separator).
const FIELD_SEP: char = '\u{1f}';
/// Ends each commit in [`LOG_FORMAT`] (git's `%x1e`, ASCII record separator).
const RECORD_SEP: char = '\u{1e}';
/// `git log --format` for [`parse_log`]: sha, subject and whole message, split by the separators
/// above (they cannot appear in a commit message).
pub(super) const LOG_FORMAT: &str = "--format=%H%x1f%s%x1f%B%x1e";
/// The trailer zsf writes into each slice's squash commit message (`Slice: 03`, zsf's
/// `jobs.rs`); it ties a commit on the feature branch back to its slice.
const SLICE_TRAILER: &str = "Slice: ";

/// Commits from `git log` run with [`LOG_FORMAT`], in git's order, each with the slice its
/// [`SLICE_TRAILER`] names (the last one, if several).
pub(super) fn parse_log(text: &str) -> Vec<FeatureCommit> {
    text.split(RECORD_SEP)
        .filter_map(|record| {
            let mut fields = record.trim_start_matches('\n').splitn(3, FIELD_SEP);
            let sha = fields.next()?.trim().to_owned();
            if sha.is_empty() {
                return None;
            }
            let subject = fields.next().unwrap_or("").to_owned();
            let message = fields.next().unwrap_or("");
            let slice = message.lines().rev().find_map(|line| {
                line.trim()
                    .strip_prefix(SLICE_TRAILER)
                    .map(|id| id.trim().to_owned())
            });
            Some(FeatureCommit {
                sha,
                subject,
                slice,
            })
        })
        .collect()
}
