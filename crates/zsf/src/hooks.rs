//! Hook commands: `bash -c <hook>` with the `ZSF_*` environment.
//!
//! # The hook environment
//!
//! Every hook (and the launch command, and `zeroshot resume`) gets the feature's `env` and
//! these variables. A variable that does not apply is set to the empty string.
//!
//! | Variable           | Value                                         | Empty                                 |
//! |--------------------|-----------------------------------------------|---------------------------------------|
//! | `ZSF_FEATURE`      | the feature's name                            |                                       |
//! | `ZSF_SLICE`        | the slice id                                  |                                       |
//! | `ZSF_ATTEMPT`      | the attempt number (1, 2, …)                  |                                       |
//! | `ZSF_WORKTREE`     | the slice's worktree                          | in `after_merge` (the worktree is gone; it runs in the repository) |
//! | `ZSF_BRANCH`       | the feature branch                            |                                       |
//! | `ZSF_SLICE_BRANCH` | the slice's working branch                    |                                       |
//! | `ZSF_CONTEXT`      | the feature's context directory               | when the feature has none             |
//! | `ZSF_RUN_ID`       | the attempt's Zeroshot run                    | in `prepare` and the launch (no run yet) |
//! | `ZSF_TASK_FILE`    | the attempt's task file                       | except in `prepare` and the launch    |
//! | `ZSF_LAUNCH_KEY`   | the attempt's launch key (`--submission-key`) | except in `prepare` and the launch    |
//! | `ZSF_STATE_DIR`    | the feature's state directory                 |                                       |

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Result;

/// The `ZSF_*` variables of one slice attempt (see the table in the module doc).
#[derive(Clone, Debug, Default)]
pub struct HookEnv {
    pub feature: String,
    pub slice: String,
    pub attempt: u32,
    pub worktree: Option<PathBuf>,
    /// The feature branch.
    pub branch: String,
    /// The slice's working branch.
    pub slice_branch: String,
    pub context: Option<PathBuf>,
    pub run_id: Option<String>,
    pub task_file: Option<PathBuf>,
    /// The attempt's launch key, for a launch hook's `zeroshot run --submission-key`.
    pub launch_key: Option<String>,
    pub state_dir: PathBuf,
    /// The feature's `env` (already `~/`-expanded).
    pub extra: Vec<(String, String)>,
}

impl HookEnv {
    /// The feature's `env`, then the `ZSF_*` variables (`None` becomes the empty string).
    pub fn vars(&self) -> Vec<(String, String)> {
        let path = |p: &Option<PathBuf>| {
            p.as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let text = |t: &Option<String>| t.clone().unwrap_or_default();
        let zsf = [
            ("ZSF_FEATURE", self.feature.clone()),
            ("ZSF_SLICE", self.slice.clone()),
            ("ZSF_ATTEMPT", self.attempt.to_string()),
            ("ZSF_WORKTREE", path(&self.worktree)),
            ("ZSF_BRANCH", self.branch.clone()),
            ("ZSF_SLICE_BRANCH", self.slice_branch.clone()),
            ("ZSF_CONTEXT", path(&self.context)),
            ("ZSF_RUN_ID", text(&self.run_id)),
            ("ZSF_TASK_FILE", path(&self.task_file)),
            ("ZSF_LAUNCH_KEY", text(&self.launch_key)),
            (
                "ZSF_STATE_DIR",
                self.state_dir.to_string_lossy().into_owned(),
            ),
        ];
        let mut vars = self.extra.clone();
        vars.extend(zsf.map(|(name, value)| (name.to_owned(), value)));
        vars
    }
}

pub struct HookOutcome {
    pub ok: bool,
    /// Combined stdout and stderr.
    pub output: String,
}

impl HookOutcome {
    /// The last `max` characters of the output, for reasons and events.
    pub fn tail(&self, max: usize) -> String {
        tail(&self.output, max)
    }
}

/// The last `max` characters of `text` (trimmed), with `…` in front when it was cut.
pub fn tail(text: &str, max: usize) -> String {
    let text = text.trim();
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    format!("…{}", text.chars().skip(count - max).collect::<String>())
}

/// Runs a hook to completion in `cwd`; stdout and stderr also go to `log` when given.
pub fn run(hook: &str, cwd: &Path, env: &HookEnv, log: Option<&Path>) -> Result<HookOutcome> {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(format!("exec 2>&1\n{hook}"))
        .current_dir(cwd)
        .envs(env.vars())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = command
        .output()
        .map_err(|e| anyhow::anyhow!("cannot run bash: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if let Some(log) = log {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
        {
            let _ = writeln!(file, "$ {hook}\n{text}");
        }
    }
    Ok(HookOutcome {
        ok: output.status.success(),
        output: text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vars_are_the_feature_env_then_every_zsf_variable() {
        let env = HookEnv {
            feature: "calc".to_owned(),
            slice: "02".to_owned(),
            attempt: 3,
            run_id: Some("r1".to_owned()),
            state_dir: PathBuf::from("/state"),
            extra: vec![("A".to_owned(), "b".to_owned())],
            ..HookEnv::default()
        };
        let vars = env.vars();
        let names: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            [
                "A",
                "ZSF_FEATURE",
                "ZSF_SLICE",
                "ZSF_ATTEMPT",
                "ZSF_WORKTREE",
                "ZSF_BRANCH",
                "ZSF_SLICE_BRANCH",
                "ZSF_CONTEXT",
                "ZSF_RUN_ID",
                "ZSF_TASK_FILE",
                "ZSF_LAUNCH_KEY",
                "ZSF_STATE_DIR",
            ]
        );
        let value = |name: &str| vars.iter().find(|(k, _)| k == name).unwrap().1.clone();
        assert_eq!(value("ZSF_ATTEMPT"), "3");
        assert_eq!(value("ZSF_RUN_ID"), "r1");
        assert_eq!(value("ZSF_WORKTREE"), "");
        assert_eq!(value("ZSF_LAUNCH_KEY"), "");
        assert_eq!(value("ZSF_STATE_DIR"), "/state");
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("  short  ", 10), "short");
        assert_eq!(tail("abcdef", 3), "…def");
    }
}
