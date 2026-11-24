//! A run's workspace, the directory its agents worked in:
//! - [`location`]: where a local run's files are (its workspace, or the repository of a removed
//!   zsf worktree) and which commit its changes are compared with;
//! - [`safe_read`]: reading a file in it without leaving it (the workspace may be hostile).
//!
//! The git views of a workspace (diffs, trees) are `actions::git`.

pub mod location;
pub mod safe_read;

pub use location::{BaseKind, NoSnapshot, Workspace, run_base, workspace_of};
pub use safe_read::{Entry, ReadError, line_counts, read, safe_relative};
