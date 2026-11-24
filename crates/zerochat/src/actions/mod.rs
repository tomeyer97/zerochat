//! zerochat's two ways of reaching outside itself:
//! - [`cli`]: the `zeroshot` CLI. Everything that talks to a controller (steer, stop, resume,
//!   checkpoints) goes through it, since it already speaks every target protocol;
//! - [`git`]: read-only git views of a local run's workspace (changed files, diffs, files and
//!   trees at a commit), with no index writes.
//!
//! The handlers that perform the actions are in `server::api::run_actions`.

pub mod cli;
pub mod git;

pub use cli::{Cli, CliError, push_target, resume_args, steer_args, steer_receipt};
pub use git::{changed_files, commit_at, diff, file, head, rev_exists, tree};
