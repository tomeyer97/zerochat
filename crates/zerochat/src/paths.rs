//! Where things live on disk: Zeroshot's state and config directories, zerochat's own config
//! directory, and the layout of a run inside a Zeroshot state directory.
//!
//! ```text
//! <Zeroshot state dir>/runs/<run id>/
//!     runs.sqlite3                  the run's ledger (source::read_local)
//!     workspace-recovery.json  controller.ready.json  checkpoints/
//! <pb_root>/                        pb (ProgramBench runner): the benchmarking tool's own directory
//!     pb.sqlite                     its database (pb_db)
//!     runs/<pb run id>/agent/zeroshot-state/
//!                                   the Zeroshot state dir of one pb run
//!     runs/<pb run id>/agent/calls/<call>/live.json
//!                                   the usage of one model call so far
//!     runs/<pb run id>/harness/replay.json
//!                                   what a replay run started from
//!     batches/<batch id>/pb.log     what a batch's detached runner prints
//! ```

use std::path::{Path, PathBuf};

/// The ledger's file name inside a run directory.
pub const LEDGER_FILE: &str = "runs.sqlite3";

/// `$HOME`, else `/`.
pub fn home() -> PathBuf {
    non_empty_env("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Zeroshot's local state directory: `$ZEROSHOT_STATE_DIR`, else `$XDG_STATE_HOME/zeroshot`,
/// else `~/.local/state/zeroshot`. One rule for zerochat and zsf: zsf's own resolution, with no
/// feature environment on top.
pub fn zeroshot_state_dir() -> PathBuf {
    zsf::store::zeroshot_state_root(&Default::default())
}

/// Zeroshot's config directory (its `targets.json`): `$ZEROSHOT_CONFIG_DIR`, else
/// `$XDG_CONFIG_HOME/zeroshot`, else `~/.config/zeroshot`.
pub fn zeroshot_config_dir() -> PathBuf {
    if let Some(dir) = non_empty_env("ZEROSHOT_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = non_empty_env("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("zeroshot");
    }
    home().join(".config/zeroshot")
}

/// zerochat's own config directory (its `config.toml`): `$XDG_CONFIG_HOME/zerochat`, else
/// `~/.config/zerochat`.
pub fn zerochat_config_dir() -> PathBuf {
    if let Some(dir) = non_empty_env("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("zerochat");
    }
    home().join(".config/zerochat")
}

/// `<state_root>/runs/<run_id>`: everything Zeroshot keeps for one run.
pub fn run_dir(state_root: &Path, run_id: &str) -> PathBuf {
    state_root.join("runs").join(run_id)
}

/// `<state_root>/runs/<run_id>/runs.sqlite3`: the run's ledger.
pub fn ledger_path(state_root: &Path, run_id: &str) -> PathBuf {
    run_dir(state_root, run_id).join(LEDGER_FILE)
}

/// `<pb_root>/runs/<pb_run_id>`: everything pb keeps for one of its runs.
pub fn pb_run_dir(pb_root: &Path, pb_run_id: &str) -> PathBuf {
    pb_root.join("runs").join(pb_run_id)
}

/// `<pb_root>/runs/<pb_run_id>/agent/zeroshot-state`: the Zeroshot state directory pb gives
/// the agent of one of its runs.
pub fn pb_run_state_root(pb_root: &Path, pb_run_id: &str) -> PathBuf {
    pb_run_dir(pb_root, pb_run_id)
        .join("agent")
        .join("zeroshot-state")
}

/// `<pb_root>/runs/<pb_run_id>/agent/calls`: one directory per model call of the run's agent,
/// each with the usage so far (`live.json`).
pub fn pb_run_calls_dir(pb_root: &Path, pb_run_id: &str) -> PathBuf {
    pb_run_dir(pb_root, pb_run_id).join("agent").join("calls")
}

/// `<pb_root>/runs/<pb_run_id>/harness/replay.json`: what a replay run started from.
pub fn pb_replay_json(pb_root: &Path, pb_run_id: &str) -> PathBuf {
    pb_run_dir(pb_root, pb_run_id)
        .join("harness")
        .join("replay.json")
}

/// `<pb_root>/batches/<batch_id>`: a batch's own directory, created before its runner starts.
pub fn pb_batch_dir(pb_root: &Path, batch_id: &str) -> PathBuf {
    pb_root.join("batches").join(batch_id)
}

/// `<pb_root>/batches/<batch_id>/pb.log`: what the batch's detached runner prints.
pub fn pb_batch_log(pb_root: &Path, batch_id: &str) -> PathBuf {
    pb_batch_dir(pb_root, batch_id).join("pb.log")
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}
