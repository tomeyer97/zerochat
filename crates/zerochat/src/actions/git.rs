//! Read-only git views of a local run's workspace: changed files, diffs, files and trees at a
//! commit. Git runs with `--no-optional-locks` (no index writes), never prompts, and every
//! revision and path is checked before it reaches its argv.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::process::{ChildStderr, ChildStdout, Command};

use crate::validate;
use crate::workspace::{self, Entry, ReadError};

/// What a preview reads at most from one file.
pub const PREVIEW_LIMIT: u64 = 2 << 20;

// The most output kept from one git command, by what it prints.

/// Lists of changed or untracked paths.
const LIST_CAP: usize = 4 << 20;
/// A diff, or a file at a commit.
const DIFF_CAP: usize = 2 << 20;
/// Every path of a tree.
const TREE_CAP: usize = 8 << 20;
/// A few commit hashes (or nothing, for `cat-file -e`).
const HASHES_CAP: usize = 4 << 10;
/// Git's error text.
const STDERR_CAP: u64 = 64 << 10;

/// Untracked files counted at most (each one is read to count its lines).
const MAX_UNTRACKED: usize = 2000;
/// Paths of a tree listed at most.
const MAX_TREE_PATHS: usize = 20_000;

/// How long git may take to print its output, and then to exit.
const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// A revision or path refused before it reached git (`server` answers it as a bad request).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidInput {
    Path,
    Revision,
}

impl std::fmt::Display for InvalidInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            InvalidInput::Path => "invalid path",
            InvalidInput::Revision => "invalid revision",
        })
    }
}

impl std::error::Error for InvalidInput {}

fn safe_rev(rev: &str) -> Result<&str, InvalidInput> {
    if !validate::GIT_REVISION.accepts(rev) {
        return Err(InvalidInput::Revision);
    }
    Ok(rev)
}

fn safe_path(path: &str) -> Result<&str, InvalidInput> {
    workspace::safe_relative(path).map_err(|_| InvalidInput::Path)
}

/// What git printed, up to the command's cap.
struct Output {
    raw: String,
    /// The cap it was cut at, when there was more.
    cut_at: Option<usize>,
}

impl Output {
    /// The output for a person to read: a cut one ends with a line saying so.
    fn text(self) -> String {
        match self.cut_at {
            Some(cap) => format!(
                "{}\n… [diff truncated: more than {} KiB]\n",
                self.raw,
                cap >> 10
            ),
            None => self.raw,
        }
    }
}

/// Runs `git -C workspace <args>` and keeps at most `cap` bytes of what it prints (more is not
/// an error: the output says it was cut). A failing git is an error with git's own message.
async fn git(workspace: &Path, args: &[&str], cap: usize) -> Result<Output> {
    let verb = args.first().copied().unwrap_or("");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["-c", "core.quotepath=off", "--no-optional-locks"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let (mut stdout, mut stderr) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let (out, err) =
        match tokio::time::timeout(GIT_TIMEOUT, read_capped(&mut stdout, &mut stderr, cap)).await {
            Ok(result) => result?,
            Err(_) => bail!("git {verb}: no answer within {}s", GIT_TIMEOUT.as_secs()),
        };
    let cut = out.len() > cap;
    if cut {
        // There is more: stop git instead of waiting for it to print the rest.
        let _ = child.kill().await;
    } else {
        let status = tokio::time::timeout(GIT_TIMEOUT, child.wait())
            .await
            .map_err(|_| anyhow!("git timed out"))??;
        if !status.success() {
            bail!("git {verb}: {}", String::from_utf8_lossy(&err).trim());
        }
    }
    let raw = String::from_utf8_lossy(&out[..out.len().min(cap)]).into_owned();
    Ok(Output {
        raw,
        cut_at: cut.then_some(cap),
    })
}

/// Stdout up to one byte past `cap` (so a longer output is known to be cut) and stderr up to
/// [`STDERR_CAP`], read together: never more than that in memory.
async fn read_capped(
    stdout: &mut ChildStdout,
    stderr: &mut ChildStderr,
    cap: usize,
) -> std::io::Result<(Vec<u8>, Vec<u8>)> {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let (mut stdout, mut stderr) = (stdout.take(cap as u64 + 1), stderr.take(STDERR_CAP));
    let (read_out, read_err) =
        tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
    read_out?;
    read_err?;
    Ok((out, err))
}

/// One file of a run's changes, as the Files panel lists it.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ChangedFile {
    pub path: String,
    /// git's status letter (`A`, `M`, `D`, `R`, `C`, `T`).
    pub status: String,
    /// Lines added; None for a binary file (or an untracked one that could not be read).
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary: bool,
    /// Not in git yet (only when the range ends at the working tree).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub untracked: bool,
}

/// Changed files between two commits, or between `from` and the working tree when `to` is None
/// (tracked changes plus untracked files, without touching the index).
pub async fn changed_files(
    workspace: &Path,
    from: &str,
    to: Option<&str>,
) -> Result<Vec<ChangedFile>> {
    let mut range = vec![safe_rev(from)?];
    if let Some(to) = to {
        range.push(safe_rev(to)?);
    }
    let diff_of_range =
        |format| [["diff", format, "-z", "--find-renames"].as_slice(), &range].concat();
    let numstat = git(workspace, &diff_of_range("--numstat"), LIST_CAP)
        .await?
        .text();
    let names = git(workspace, &diff_of_range("--name-status"), LIST_CAP)
        .await?
        .text();
    let mut files = parse_changes(&numstat, &names);
    if to.is_none() {
        let untracked = git(
            workspace,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            LIST_CAP,
        )
        .await?
        .text();
        let paths: Vec<String> = untracked
            .split('\0')
            .filter(|p| !p.is_empty())
            .take(MAX_UNTRACKED)
            .map(str::to_owned)
            .collect();
        for (path, lines) in workspace::line_counts(workspace, paths, PREVIEW_LIMIT).await {
            files.push(ChangedFile {
                path,
                status: "A".to_owned(),
                additions: lines,
                deletions: Some(0),
                binary: lines.is_none(),
                untracked: true,
            });
        }
    }
    Ok(files)
}

/// Joins `git diff --numstat -z` (line counts) with `git diff --name-status -z` (status letters).
fn parse_changes(numstat: &str, names: &str) -> Vec<ChangedFile> {
    let statuses = parse_name_status(names);
    let mut files = Vec::new();
    let mut fields = numstat.split('\0').filter(|p| !p.is_empty());
    while let Some(entry) = fields.next() {
        let mut cols = entry.splitn(3, '\t');
        let (add, del) = (cols.next().unwrap_or("-"), cols.next().unwrap_or("-"));
        let path = match cols.next().unwrap_or("") {
            // A rename or copy: the counts come without a path, then the old and the new path.
            "" => {
                fields.next();
                fields.next().unwrap_or("")
            }
            path => path,
        };
        let status = statuses.get(path).cloned().unwrap_or_else(|| "M".into());
        files.push(ChangedFile {
            path: path.to_owned(),
            status,
            additions: add.parse().ok(),
            deletions: del.parse().ok(),
            binary: add == "-",
            untracked: false,
        });
    }
    files
}

/// The status letter of each path of `git diff --name-status -z` (the new path of a rename).
fn parse_name_status(names: &str) -> HashMap<String, String> {
    let mut statuses = HashMap::new();
    let mut fields = names.split('\0').filter(|p| !p.is_empty());
    while let Some(code) = fields.next() {
        // A rename or copy (`R100`, `C75`) names the old path, then the new one.
        if code.starts_with(['R', 'C']) {
            fields.next();
        }
        if let Some(path) = fields.next() {
            statuses.insert(
                path.to_owned(),
                code.chars().next().unwrap_or('M').to_string(),
            );
        }
    }
    statuses
}

/// The diff of a range (to the working tree when `to` is None), of one path or all of them.
pub async fn diff(
    workspace: &Path,
    from: &str,
    to: Option<&str>,
    path: Option<&str>,
) -> Result<String> {
    let mut args = vec!["diff", "--no-color", "--find-renames", safe_rev(from)?];
    if let Some(to) = to {
        args.push(safe_rev(to)?);
    }
    let path = path.map(safe_path).transpose()?;
    if let Some(path) = path {
        args.extend(["--", path]);
    }
    let text = git(workspace, &args, DIFF_CAP).await?.text();
    // git shows nothing for an untracked file: show it as an addition.
    if text.is_empty()
        && to.is_none()
        && let Some(path) = path
    {
        match workspace::read(workspace, path, PREVIEW_LIMIT, true).await {
            Ok(entry) => return Ok(added_file_diff(path, &entry)),
            Err(ReadError::NotFound) => {}
            Err(error) => return Err(anyhow!(error)),
        }
    }
    Ok(text)
}

/// The unified diff that adds `entry` at `path`. A symlink shows as its target text (as git
/// shows one), never as the contents of whatever it points to.
fn added_file_diff(path: &str, entry: &Entry) -> String {
    match entry {
        Entry::Symlink(target) => format!(
            "new file mode 120000\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+{target}\n\\ No newline at end of file\n"
        ),
        Entry::File(bytes) => {
            let content = String::from_utf8_lossy(bytes);
            let body: String = content.lines().map(|l| format!("+{l}\n")).collect();
            format!(
                "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n{body}",
                content.lines().count()
            )
        }
    }
}

/// A file at a commit, or in the working tree when `commit` is None.
pub async fn file(workspace: &Path, commit: Option<&str>, path: &str) -> Result<String> {
    let path = safe_path(path)?;
    match commit {
        Some(commit) => Ok(git(
            workspace,
            &["show", &format!("{}:{}", safe_rev(commit)?, path)],
            DIFF_CAP,
        )
        .await?
        .text()),
        None => match workspace::read(workspace, path, PREVIEW_LIMIT, false).await? {
            Entry::File(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Entry::Symlink(target) => Ok(target),
        },
    }
}

/// Every path at a commit, or in the working tree (tracked and untracked) when `commit` is None.
pub async fn tree(workspace: &Path, commit: Option<&str>) -> Result<Vec<String>> {
    let output = match commit {
        Some(commit) => {
            git(
                workspace,
                &["ls-tree", "-r", "--name-only", "-z", safe_rev(commit)?],
                TREE_CAP,
            )
            .await?
        }
        None => {
            git(
                workspace,
                &[
                    "ls-files",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                ],
                TREE_CAP,
            )
            .await?
        }
    };
    Ok(output
        .text()
        .split('\0')
        .filter(|p| !p.is_empty())
        .take(MAX_TREE_PATHS)
        .map(str::to_owned)
        .collect())
}

pub async fn head(workspace: &Path) -> Option<String> {
    git(workspace, &["rev-parse", "HEAD"], HASHES_CAP)
        .await
        .ok()
        .map(|o| o.raw.trim().to_owned())
}

/// The commit that was checked out when a run started (created at `created_ms`): the newest
/// commit on any ref made before that moment, else the repository's first commit.
pub async fn commit_at(workspace: &Path, created_ms: u64) -> Option<String> {
    let before = format!("--before={}", created_ms / 1000 + 1);
    let newest = git(workspace, &["rev-list", "-1", &before, "--all"], HASHES_CAP)
        .await
        .ok()
        .map(|o| o.raw.trim().to_owned())
        .filter(|s| !s.is_empty());
    if newest.is_some() {
        return newest;
    }
    let roots = git(
        workspace,
        &["rev-list", "--max-parents=0", "HEAD"],
        HASHES_CAP,
    )
    .await
    .ok()?
    .raw;
    roots
        .lines()
        .last()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

pub async fn rev_exists(workspace: &Path, rev: &str) -> bool {
    match safe_rev(rev) {
        Ok(rev) => git(
            workspace,
            &["cat-file", "-e", &format!("{rev}^{{commit}}")],
            HASHES_CAP,
        )
        .await
        .is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_option_like_revisions_and_escaping_paths() {
        assert!(safe_rev("--output=/tmp/x").is_err());
        assert!(safe_rev("HEAD~1").is_ok());
        assert!(safe_path("../etc/passwd").is_err());
        assert!(safe_path("/etc/passwd").is_err());
        assert!(safe_path("src/main.rs").is_ok());
    }

    #[test]
    fn invalid_input_reads_as_before() {
        assert_eq!(anyhow!(InvalidInput::Path).to_string(), "invalid path");
        assert_eq!(
            anyhow!(InvalidInput::Revision).to_string(),
            "invalid revision"
        );
    }

    #[test]
    fn parses_changes_with_renames_and_binaries() {
        let names = "M\0src/a.rs\0R100\0old.rs\0new.rs\0A\0logo.png\0";
        let numstat = concat!(
            "3\t1\tsrc/a.rs\0",
            "0\t0\t\0old.rs\0new.rs\0",
            "-\t-\tlogo.png\0"
        );
        let file = |path: &str, status: &str, additions, deletions, binary| ChangedFile {
            path: path.into(),
            status: status.into(),
            additions,
            deletions,
            binary,
            untracked: false,
        };
        assert_eq!(
            parse_changes(numstat, names),
            [
                file("src/a.rs", "M", Some(3), Some(1), false),
                file("new.rs", "R", Some(0), Some(0), false),
                file("logo.png", "A", None, None, true)
            ]
        );
    }

    #[test]
    fn an_added_file_diff_shows_every_line_or_the_link_target() {
        assert_eq!(
            added_file_diff("a.txt", &Entry::File(b"one\ntwo\n".to_vec())),
            "--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n"
        );
        assert_eq!(
            added_file_diff("l", &Entry::Symlink("../x".into())),
            "new file mode 120000\n--- /dev/null\n+++ b/l\n@@ -0,0 +1 @@\n+../x\n\\ No newline at end of file\n"
        );
    }

    #[test]
    fn a_cut_output_says_so() {
        assert_eq!(
            Output {
                raw: "abc".into(),
                cut_at: None
            }
            .text(),
            "abc"
        );
        assert_eq!(
            Output {
                raw: "abc".into(),
                cut_at: Some(2 << 20)
            }
            .text(),
            "abc\n… [diff truncated: more than 2048 KiB]\n"
        );
    }
}
