//! The `zeroshot` CLI client. Everything that talks to a controller goes through it, since it
//! already speaks every target protocol: zerochat builds the arguments, runs the program with
//! `ZEROSHOT_ERROR_FORMAT=json`, and reads its JSON answer or its JSON error.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Result, anyhow};
use serde_json::Value;
use tokio::process::Command;

/// How long each CLI call may take before zerochat gives up on it (`CliError` code
/// [`code::TIMEOUT`]). They differ on purpose: a steer or a checkpoint page is a quick request
/// to the controller, a stop waits for the run's agents to end, and a resume restarts them.
pub mod timeout {
    use std::time::Duration;

    /// `zeroshot steer --help`, read to detect what this Zeroshot can do.
    pub const CAPABILITIES: Duration = Duration::from_secs(10);
    /// `zeroshot steer`: the controller only records the message.
    pub const STEER: Duration = Duration::from_secs(20);
    /// One page of `zeroshot checkpoints`.
    pub const CHECKPOINTS_PAGE: Duration = Duration::from_secs(20);
    /// `zeroshot force-stop`: waits for the run's agents to stop.
    pub const STOP: Duration = Duration::from_secs(60);
    /// `zeroshot resume` (also what a replay runs): restores and restarts the run.
    pub const RESUME: Duration = Duration::from_secs(120);
}

/// The `CliError::code`s zerochat gives itself when the CLI did not answer with its own.
pub mod code {
    /// The program could not be started.
    pub const ZEROSHOT_NOT_FOUND: &str = "zeroshot_not_found";
    /// No answer within the call's timeout.
    pub const TIMEOUT: &str = "timeout";
    /// Succeeded, but printed no JSON.
    pub const BAD_OUTPUT: &str = "bad_output";
    /// Failed without a code of its own.
    pub const FAILED: &str = "failed";
}

#[derive(Clone, Debug)]
pub struct Cli {
    pub program: PathBuf,
}

/// A CLI failure with the CLI's own message, shown to the user verbatim.
#[derive(Debug)]
pub struct CliError {
    pub command: String,
    /// The CLI's error code, or one of [`code`].
    pub code: String,
    pub message: String,
    /// `zeroshot.error/v1` kind: `invalid_request`, `run_not_found`, `submission_conflict`,
    /// `protocol` or `target` (absent for non-JSON failures).
    pub kind: Option<String>,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}` failed: {}", self.command, self.message)
    }
}

impl std::error::Error for CliError {}

impl Cli {
    /// Runs `zeroshot <args>` and returns the JSON it printed last (on stdout, else stderr).
    pub async fn json(&self, args: &[String], timeout: Duration) -> Result<Value> {
        let shown = shown_command(args);
        let failure = |code: &str, message: String, kind: Option<String>| {
            anyhow!(CliError {
                command: shown.clone(),
                code: code.to_owned(),
                message,
                kind
            })
        };
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .env("ZEROSHOT_ERROR_FORMAT", "json")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = command.spawn().map_err(|error| {
            failure(
                code::ZEROSHOT_NOT_FOUND,
                format!("cannot start {}: {error}", self.program.display()),
                None,
            )
        })?;
        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(output) => output?,
            Err(_) => {
                return Err(failure(
                    code::TIMEOUT,
                    format!("no answer within {}s", timeout.as_secs()),
                    None,
                ));
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let parsed = last_json(&stdout).or_else(|| last_json(&stderr));
        if output.status.success() {
            return parsed.ok_or_else(|| {
                failure(
                    code::BAD_OUTPUT,
                    format!(
                        "unexpected output: {}",
                        stdout.chars().take(500).collect::<String>()
                    ),
                    None,
                )
            });
        }
        let (kind, code, message) = match &parsed {
            Some(v) => error_fields(v),
            None => (
                None,
                code::FAILED.to_owned(),
                first_line(&stderr)
                    .or_else(|| first_line(&stdout))
                    .unwrap_or_else(|| format!("exited with {}", output.status)),
            ),
        };
        Err(failure(&code, message, kind))
    }

    /// What this Zeroshot can do beyond the commands every version has, detected from
    /// `zeroshot steer --help` (feature detection, not version numbers; see
    /// [`steer_capabilities`]). The help is the local program's: a named target is driven through
    /// that same program, so it is the program that has to know the flags.
    pub async fn capabilities(&self, _target: Option<&str>) -> Vec<String> {
        let mut command = Command::new(&self.program);
        command
            .args(["steer", "--help"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let Ok(child) = command.spawn() else {
            return Vec::new();
        };
        match tokio::time::timeout(timeout::CAPABILITIES, child.wait_with_output()).await {
            Ok(Ok(output)) if output.status.success() => {
                steer_capabilities(&String::from_utf8_lossy(&output.stdout))
            }
            _ => Vec::new(),
        }
    }

    pub async fn version(&self) -> Option<String> {
        let output = Command::new(&self.program)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .await
            .ok()?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

/// Capabilities read from `zeroshot steer --help`:
/// - `steer`: run-wide steering (`zeroshot steer RUN --mode once --text-file F`, Zeroshot 16.1);
/// - `steer_node`: steering one agent node (`--node NAME`, upstream PR
///   the-open-engine/zeroshot#1667, not in 16.1).
pub fn steer_capabilities(help: &str) -> Vec<String> {
    let flag = |name: &str| help.lines().any(|l| l.trim_start().starts_with(name));
    let mut caps = Vec::new();
    if flag("--text-file") && flag("--mode") {
        caps.push("steer".to_owned());
        if flag("--node <") {
            caps.push("steer_node".to_owned());
        }
    }
    caps
}

/// `zeroshot steer --mode=once [--node=NAME] --text-file=F [--target=T] -- RUN`. `once`: each
/// recipient takes the message into its conversation once (a correction, not a standing rule).
/// Without `node` every agent conversation of the run receives it.
pub fn steer_args(
    run_id: &str,
    node: Option<&str>,
    text_file: &Path,
    target: Option<&str>,
) -> Vec<String> {
    let mut args = vec!["steer".to_owned(), "--mode=once".to_owned()];
    if let Some(node) = node {
        args.push(format!("--node={node}"));
    }
    args.push(format!("--text-file={}", text_file.display()));
    push_target(&mut args, target);
    args.push("--".to_owned());
    args.push(run_id.to_owned());
    args
}

/// `zeroshot resume [--from-checkpoint=ID [--discard-workspace-changes]] [--target=T] -- RUN`.
/// Zeroshot 16.1 restores a local checkpoint only when told to throw away the workspace's later
/// changes (the UI has the person confirm that rollback first); a named target's workspace is
/// not on this machine, so the flag never goes with `--target`.
pub fn resume_args(run_id: &str, checkpoint: Option<&str>, target: Option<&str>) -> Vec<String> {
    let mut args = vec!["resume".to_owned()];
    if let Some(checkpoint) = checkpoint {
        args.push(format!("--from-checkpoint={checkpoint}"));
        if target.is_none() {
            args.push("--discard-workspace-changes".to_owned());
        }
    }
    push_target(&mut args, target);
    args.push("--".to_owned());
    args.push(run_id.to_owned());
    args
}

/// A steer's receipt (`{"runId", "steer": {"id", "receivedExecutions", …}}`): the steer id and
/// how many agent conversations had received it when the command returned (usually 0: delivery
/// happens at each agent's next tool call or prompt and is recorded as `steer_receipt`).
pub fn steer_receipt(value: &Value) -> Option<(String, u64)> {
    let steer = value.get("steer")?;
    Some((
        steer.get("id")?.as_str()?.to_owned(),
        steer
            .get("receivedExecutions")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    ))
}

pub fn push_target(args: &mut Vec<String>, target: Option<&str>) {
    if let Some(target) = target {
        args.push(format!("--target={target}"));
    }
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
}

/// The command as shown in error messages: long arguments (file paths) elided.
fn shown_command(args: &[String]) -> String {
    format!(
        "zeroshot {}",
        args.iter()
            .map(|a| if a.len() > 60 { "…" } else { a.as_str() })
            .collect::<Vec<_>>()
            .join(" ")
    )
}

/// `(kind, code, message)` of a failed command's JSON. Zeroshot prints its error in one of two
/// shapes, and either may carry each field:
/// - nested (`zeroshot.error/v1`): `{"kind": …, "error": {"kind", "code", "message"}}`;
/// - flat: `{"kind", "code", "message"}`.
///
/// `kind` is read top level first, `code` and `message` nested first. Without a code it is
/// [`code::FAILED`]; without a message, the whole JSON.
fn error_fields(v: &Value) -> (Option<String>, String, String) {
    let field = |nested: &str, flat: &str| {
        v.pointer(nested)
            .or(v.get(flat))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let kind = v
        .get("kind")
        .or_else(|| v.pointer("/error/kind"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let code = field("/error/code", "code").unwrap_or_else(|| code::FAILED.to_owned());
    let message = field("/error/message", "message").unwrap_or_else(|| v.to_string());
    (kind, code, message)
}

fn last_json(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        return Some(v);
    }
    text.lines().rev().find_map(|line| {
        serde_json::from_str::<Value>(line.trim())
            .ok()
            .filter(Value::is_object)
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_cli_json_from_last_line() {
        assert_eq!(
            last_json("progress\n{\"runId\":\"x\"}\n").unwrap()["runId"],
            "x"
        );
    }

    #[test]
    fn error_fields_read_both_envelope_shapes() {
        let nested = json!({"kind": "zeroshot.error/v1", "error": {"kind": "run_not_found", "code": "run.not_found", "message": "no such run"}});
        assert_eq!(
            error_fields(&nested),
            (
                Some("zeroshot.error/v1".into()),
                "run.not_found".into(),
                "no such run".into()
            )
        );
        let nested_only =
            json!({"error": {"kind": "target", "code": "target.unavailable", "message": "down"}});
        assert_eq!(
            error_fields(&nested_only),
            (
                Some("target".into()),
                "target.unavailable".into(),
                "down".into()
            )
        );
        let flat = json!({"kind": "invalid_request", "code": "graph.unknown_field", "message": "bad graph"});
        assert_eq!(
            error_fields(&flat),
            (
                Some("invalid_request".into()),
                "graph.unknown_field".into(),
                "bad graph".into()
            )
        );
        let bare = json!({"status": "nope"});
        assert_eq!(
            error_fields(&bare),
            (None, "failed".into(), bare.to_string())
        );
    }

    /// `zeroshot steer --help` of stock Zeroshot 16.1.0 (npm @the-open-engine-company/zeroshot).
    const STEER_HELP_16_1: &str = "Send direction to agents or publish guidance to authorized graph networks

Usage: zeroshot steer [OPTIONS] <RUN_ID> <TEXT|--text-file <FILE>|--text-stdin>

Arguments:
  <RUN_ID>  Public run ID
  [TEXT]    Direction, as given. Do not put secrets in a steer

Options:
      --target <NAME>     Use this named target. If omitted, use the local controller
      --text-file <FILE>  Read UTF-8 direction from this file
      --text-stdin        Read UTF-8 direction from standard input
      --network <NAME>    Broadcast to this authorized graph-local network; repeat for several networks
      --mode <MODE>       Once per recipient node instance, or standing across fresh conversations. Defaults to once for network messages and standing for human direction [possible values: once, standing]
  -h, --help              Print help
";

    /// The same help of a build of upstream PR the-open-engine/zeroshot#1667 (`--node`).
    const STEER_HELP_PR_1667: &str = "Send direction to agents or publish guidance to authorized graph networks

Usage: zeroshot steer [OPTIONS] <RUN_ID> <TEXT|--text-file <FILE>|--text-stdin>

Arguments:
  <RUN_ID>  Public run ID
  [TEXT]    Direction, as given. Do not put secrets in a steer

Options:
      --target <NAME>     Use this named target. If omitted, use the local controller
      --text-file <FILE>  Read UTF-8 direction from this file
      --text-stdin        Read UTF-8 direction from standard input
      --node <NAME>       Direct human guidance to this exact agent node name, including future instances
      --network <NAME>    Broadcast to this authorized graph-local network; repeat for several networks
      --mode <MODE>       Once per recipient node instance, or standing across fresh conversations. Defaults to once for network messages and standing for human direction [possible values: once, standing]
  -h, --help              Print help
";

    #[test]
    fn steer_help_detects_run_wide_and_node_steering() {
        assert_eq!(steer_capabilities(STEER_HELP_16_1), ["steer"]);
        assert_eq!(
            steer_capabilities(STEER_HELP_PR_1667),
            ["steer", "steer_node"]
        );
        // Zeroshot without `steer` (11.x, the old fork) prints an error, not this help.
        assert!(steer_capabilities("error: unrecognized subcommand 'steer'").is_empty());
        assert!(steer_capabilities("").is_empty());
        // A mention of --node in prose is not the flag.
        assert_eq!(
            steer_capabilities(
                &STEER_HELP_16_1.replace("Print help", "Print help (see --node in later versions)")
            ),
            ["steer"]
        );
    }

    #[test]
    fn steer_args_send_once_from_a_file_run_wide_or_to_one_node() {
        let file = Path::new("/tmp/zerochat-steer/message.txt");
        assert_eq!(
            steer_args("01a-run", None, file, None),
            [
                "steer",
                "--mode=once",
                "--text-file=/tmp/zerochat-steer/message.txt",
                "--",
                "01a-run"
            ]
        );
        assert_eq!(
            steer_args("01a-run", Some("worker"), file, None),
            [
                "steer",
                "--mode=once",
                "--node=worker",
                "--text-file=/tmp/zerochat-steer/message.txt",
                "--",
                "01a-run"
            ]
        );
        assert_eq!(
            steer_args("01a-run", Some("worker"), file, Some("lab")),
            [
                "steer",
                "--mode=once",
                "--node=worker",
                "--text-file=/tmp/zerochat-steer/message.txt",
                "--target=lab",
                "--",
                "01a-run"
            ]
        );
    }

    #[test]
    fn resume_discards_workspace_changes_only_for_a_local_checkpoint() {
        assert_eq!(resume_args("r", None, None), ["resume", "--", "r"]);
        assert_eq!(
            resume_args("r", Some("c1"), None),
            [
                "resume",
                "--from-checkpoint=c1",
                "--discard-workspace-changes",
                "--",
                "r"
            ]
        );
        assert_eq!(
            resume_args("r", Some("c1"), Some("lab")),
            ["resume", "--from-checkpoint=c1", "--target=lab", "--", "r"]
        );
        assert_eq!(
            resume_args("r", None, Some("lab")),
            ["resume", "--target=lab", "--", "r"]
        );
    }

    #[test]
    fn parses_a_steer_receipt() {
        // Verified live against 16.1 (zeroshot-official-steering.md §3).
        let receipt = json!({"runId": "r", "steer": {"id": "01a1f8c9-45ce", "mode": "once", "createdAt": 1, "textBytes": 56, "excerpt": "…", "receivedExecutions": 0, "executions": []}});
        assert_eq!(
            steer_receipt(&receipt),
            Some(("01a1f8c9-45ce".to_owned(), 0))
        );
        assert_eq!(steer_receipt(&json!({"status": "delivered"})), None);
    }
}
