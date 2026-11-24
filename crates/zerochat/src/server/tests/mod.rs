//! The server's tests, one file per concern. Shared here: the app state they run against.

mod benchmarks_cache;
mod errors;
mod features;
mod overview;
mod run_actions;
mod sources;
mod workspace;

use std::path::PathBuf;
use std::sync::Arc;

use crate::actions::Cli;
use crate::discovery::{Target, TargetKind};

use super::boundary::HostAllowlist;
use super::state::AppState;

pub(super) fn test_state(zerochat_config_root: PathBuf) -> AppState {
    AppState::new(
        PathBuf::from("/nonexistent/zeroshot-state"),
        PathBuf::from("/nonexistent/zeroshot-config"),
        zerochat_config_root,
        Cli {
            program: PathBuf::from("/nonexistent/zeroshot"),
        },
        HostAllowlist::default(),
    )
}

pub(super) fn observe_target(actions_allowed: bool) -> Target {
    Target {
        id: "pb".into(),
        name: "pb".into(),
        kind: TargetKind::Observe,
        origin: None,
        cli_name: None,
        roots: Vec::new(),
        actions_allowed,
    }
}

/// A state whose config names `dir` as pb's root (with one pb run in it): the `pb` observe
/// source exists.
pub(super) fn pb_state(dir: &std::path::Path) -> Arc<AppState> {
    std::fs::create_dir_all(dir.join("runs/x/agent/zeroshot-state")).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        format!("pb_root = \"{}\"\n", dir.display()),
    )
    .unwrap();
    Arc::new(test_state(dir.to_path_buf()))
}

pub(super) fn zsf_state(dir: &std::path::Path) -> Arc<AppState> {
    crate::features::fixture::build(&dir.join("zsf"));
    std::fs::write(
        dir.join("config.toml"),
        format!(
            "zsf_state_dir = \"{}\"\nzsf_bin = \"{}\"\n",
            dir.join("zsf").display(),
            dir.join("no-zsf").display()
        ),
    )
    .unwrap();
    Arc::new(test_state(dir.to_path_buf()))
}
