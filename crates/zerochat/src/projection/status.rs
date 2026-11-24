//! A run's status, the one word the run list and the run page lead with, decided from what
//! Zeroshot recorded (terminal record, phase) and what this machine sees (the controller).

use serde_json::Value;

/// Zeroshot's terminal reasons the status depends on.
pub(super) mod reason {
    /// A person force-stopped the run.
    pub const FORCE_STOPPED: &str = "force_stopped";
    /// The controller died; `zeroshot resume` closes the run with this when it starts the
    /// successor.
    pub const RUNTIME_LOST: &str = "runtime_lost";
}

/// Zeroshot's run phases the status depends on.
pub(super) mod phase {
    pub const RUNNING: &str = "running";
    pub const FINISHED: &str = "finished";
    /// Zeroshot 16.1 suspended the run at a checkpoint; it continues only as a successor.
    pub const SUSPENDED: &str = "suspended";
}

const STOPPED: &str = "stopped";
const STALLED: &str = "stalled";
const UNKNOWN: &str = "unknown";

/// What the run page calls the run's state.
#[derive(Debug, PartialEq)]
pub(super) enum RunStatus {
    /// Force-stopped by a person.
    Stopped,
    /// The status Zeroshot's terminal record gives (`succeeded`, `failed`, …).
    Terminal(String),
    /// Zeroshot says it is running, but its controller is gone.
    Stalled,
    /// Zeroshot recorded no phase.
    Unknown,
    /// Still going: the phase Zeroshot recorded (`running`, `admitted`, `suspended`, …).
    Phase(String),
}

impl RunStatus {
    pub(super) fn as_str(&self) -> &str {
        match self {
            RunStatus::Stopped => STOPPED,
            RunStatus::Stalled => STALLED,
            RunStatus::Unknown => UNKNOWN,
            RunStatus::Terminal(s) | RunStatus::Phase(s) => s,
        }
    }

    /// Reads as stalled (Zeroshot's own words included): its executions are no longer running.
    pub(super) fn is_stalled(&self) -> bool {
        self.as_str() == STALLED
    }

    /// Reads as stopped: a failed execution was cut off by the stop, not failing on its own.
    pub(super) fn is_stopped(&self) -> bool {
        self.as_str() == STOPPED
    }
}

/// The run's status: a force stop wins, then Zeroshot's terminal status, then a running run
/// whose controller this machine saw die, then the phase.
pub(super) fn run_status(terminal: &Value, phase: &str, controller_dead: bool) -> RunStatus {
    if terminal["reason"] == reason::FORCE_STOPPED {
        RunStatus::Stopped
    } else if let Some(status) = terminal["status"].as_str() {
        RunStatus::Terminal(status.to_owned())
    } else if phase == phase::RUNNING && controller_dead {
        RunStatus::Stalled
    } else if phase.is_empty() {
        RunStatus::Unknown
    } else {
        RunStatus::Phase(phase.to_owned())
    }
}
