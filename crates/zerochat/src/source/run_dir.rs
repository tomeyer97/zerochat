//! The files Zeroshot keeps for one run, by name:
//!
//! ```text
//! <state dir>/runs/<run id>/
//!     runs.sqlite3              the ledger: the run row and its events (SQLite)
//!     runs.sqlite3-wal          the ledger's write-ahead log, while Zeroshot writes
//!     workspace-recovery.json   the run's workspace and lineage (resumed from / as, replays)
//!     controller.ready.json     the controller process (`pid`) once it is up
//!     checkpoints/index.json    the run's checkpoints, when it has any
//! ```

use std::path::{Path, PathBuf};

use crate::paths;

/// One run's directory in a Zeroshot state directory.
pub struct RunDir(PathBuf);

impl RunDir {
    /// `<state_root>/runs/<run_id>`.
    pub fn new(state_root: &Path, run_id: &str) -> Self {
        RunDir(paths::run_dir(state_root, run_id))
    }

    /// A run directory already found on disk.
    pub fn at(path: PathBuf) -> Self {
        RunDir(path)
    }

    pub fn into_path(self) -> PathBuf {
        self.0
    }

    pub fn ledger(&self) -> PathBuf {
        self.0.join(paths::LEDGER_FILE)
    }

    pub fn ledger_wal(&self) -> PathBuf {
        self.0.join(format!("{}-wal", paths::LEDGER_FILE))
    }

    pub fn recovery(&self) -> PathBuf {
        self.0.join("workspace-recovery.json")
    }

    pub fn controller_ready(&self) -> PathBuf {
        self.0.join("controller.ready.json")
    }

    pub fn checkpoint_index(&self) -> PathBuf {
        self.0.join("checkpoints/index.json")
    }
}
