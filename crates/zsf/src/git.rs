//! Git through the `git` CLI (synchronous; called only from job threads or short commands).

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Result, bail};

/// Output of a finished command.
pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    run_input(dir, args, None)
}

pub fn run_input(dir: &Path, args: &[&str], input: Option<&str>) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| anyhow::anyhow!("cannot run git: {e}"))?;
    if let Some(input) = input {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    Ok(Output {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

/// Runs git and returns trimmed stdout, failing with git's message.
pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = run(dir, args)?;
    if !out.ok {
        bail!(
            "git {}: {}",
            args.join(" "),
            if out.stderr.is_empty() {
                &out.stdout
            } else {
                &out.stderr
            }
        );
    }
    Ok(out.stdout.trim().to_owned())
}

pub fn rev_parse(dir: &Path, rev: &str) -> Result<String> {
    git(
        dir,
        &["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")],
    )
}

/// One commit of [`commits`].
pub struct Commit {
    pub sha: String,
    /// The full message (subject, body and trailers).
    pub message: String,
}

/// `git log <args>` (a range, options, paths) as commits with their messages, in log order.
pub fn commits(dir: &Path, args: &[&str]) -> Result<Vec<Commit>> {
    // Each commit is `<sha>\n<message>` followed by a NUL, which no message contains.
    let mut command = vec!["log", "--format=%H%n%B%x00"];
    command.extend_from_slice(args);
    let out = git(dir, &command)?;
    Ok(out
        .split('\0')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| {
            let (sha, message) = c.split_once('\n').unwrap_or((c, ""));
            Commit {
                sha: sha.to_owned(),
                message: message.to_owned(),
            }
        })
        .collect())
}

pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok_and(|o| o.ok)
}

pub fn is_ancestor(dir: &Path, ancestor: &str, of: &str) -> bool {
    run(dir, &["merge-base", "--is-ancestor", ancestor, of]).is_ok_and(|o| o.ok)
}

pub fn is_dirty(dir: &Path) -> Result<bool> {
    Ok(!git(dir, &["status", "--porcelain"])?.is_empty())
}

/// Adds `pattern` to the repository's shared `info/exclude` once.
pub fn exclude(worktree: &Path, pattern: &str) -> Result<()> {
    let path = git(
        worktree,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/exclude",
        ],
    )?;
    let path = Path::new(&path);
    let current = std::fs::read_to_string(path).unwrap_or_default();
    if current.lines().any(|l| l.trim() == pattern) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(pattern);
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}
