//! `GET /api/setup`: can this machine work with zerochat (`discovery::Setup`)? The same check the
//! startup banner and `zerochat doctor` print. A second zerochat also reads it to recognise a
//! running one on a busy port: `GET /api/setup?probe` answers only `{app: "zerochat", version}`,
//! at once, without running any program.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;

use crate::discovery::{self, PbCheck, Setup, ZeroshotFacts, ZsfCheck};
use crate::features;
use crate::server::state::AppState;

/// How long `zeroshot --version` and `zsf --version` may take before the check goes on without.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
pub(crate) struct SetupQuery {
    probe: Option<String>,
}

pub(crate) async fn setup(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SetupQuery>,
) -> Response {
    if q.probe.is_some() {
        return Json(json!({"app": discovery::APP_NAME, "version": env!("CARGO_PKG_VERSION")}))
            .into_response();
    }
    Json(check(&state).await).into_response()
}

/// Gathers the facts (the zeroshot program, the state directory, zsf, pb) and checks them.
pub async fn check(state: &AppState) -> Setup {
    let config = state.config();
    let program = &state.cli.program;
    let found = program.is_file();
    let (version_output, capabilities) = if found {
        let version = tokio::time::timeout(VERSION_TIMEOUT, state.version())
            .await
            .ok()
            .flatten();
        (
            version,
            state.capabilities(&discovery::local_target()).await,
        )
    } else {
        (None, Vec::new())
    };
    let zeroshot = discovery::zeroshot_check(ZeroshotFacts {
        program: program.display().to_string(),
        found,
        version_output,
        capabilities,
    });
    let state_root = state.state_root.clone();
    let runs = tokio::task::spawn_blocking(move || discovery::runs_check(&state_root))
        .await
        .expect("the runs check does not panic");
    let zsf_program = features::zsf_bin(&config);
    let zsf_version = match &zsf_program {
        Some(bin) => state.zsf_version(bin, VERSION_TIMEOUT).await,
        None => None,
    };
    let zsf = ZsfCheck {
        found: zsf_program.is_some(),
        program: zsf_program.map(|p| p.display().to_string()),
        version: zsf_version,
        features: features::names(&features::zsf_root(&config)).len(),
    };
    let pb = PbCheck {
        configured: config.pb_root.is_some(),
        root: config.pb_root.map(|p| p.display().to_string()),
    };
    Setup::new(zeroshot, runs, zsf, pb)
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::actions::Cli;
    use crate::server::HostAllowlist;

    /// A fake `zeroshot` that prints `version` for `--version` and, for `steer --help`, the
    /// options of 16.1 (plus `--node` when `node`).
    pub(crate) fn fake_zeroshot(dir: &Path, version: &str, node: bool) -> PathBuf {
        let node_line = if node {
            "      --node <NAME>       Steer one agent node\n"
        } else {
            ""
        };
        // `zeroshot steer` exists from 16.1 on: an older fake answers it like an unknown subcommand.
        let has_steer = crate::discovery::parse_version(version)
            .is_some_and(|v| v >= crate::discovery::ZEROSHOT_MINIMUM);
        let steer = if has_steer { "0" } else { "1" };
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{version}'; exit 0; fi\nif [ \"$1\" = steer ] && [ {steer} = 0 ]; then cat <<'EOF'\nOptions:\n      --text-file <FILE>  Read UTF-8 direction from this file\n      --mode <MODE>       once or standing\n{node_line}EOF\nexit 0; fi\nexit 2\n"
        );
        let path = dir.join(format!("zeroshot-{version}-{node}"));
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    async fn check_with(program: PathBuf) -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(
            dir.path().join("state"),
            dir.path().join("zs-config"),
            dir.path().join("zc-config"),
            Cli { program },
            HostAllowlist::any(),
        );
        check(&state).await
    }

    #[tokio::test]
    async fn checks_a_missing_an_old_and_a_current_zeroshot() {
        let dir = tempfile::tempdir().unwrap();
        let missing = check_with(dir.path().join("no-zeroshot")).await;
        assert!(!missing.ok && !missing.zeroshot.found);
        assert_eq!(
            missing.problems[0].fix.as_deref(),
            Some(discovery::ZEROSHOT_INSTALL)
        );

        let old = check_with(fake_zeroshot(dir.path(), "11.0.0", false)).await;
        assert!(!old.ok && old.zeroshot.found && !old.zeroshot.meets_minimum);
        assert_eq!(old.zeroshot.version.as_deref(), Some("11.0.0"));

        let stock = check_with(fake_zeroshot(dir.path(), "16.1.0", false)).await;
        assert!(stock.ok, "{:?}", stock.problems);
        assert!(stock.zeroshot.stop_resume && stock.zeroshot.steer && !stock.zeroshot.steer_node);

        let node = check_with(fake_zeroshot(dir.path(), "16.1.0", true)).await;
        assert!(node.ok && node.zeroshot.steer && node.zeroshot.steer_node);
        assert!(
            !node.runs.exists && node.runs.count == 0,
            "a fresh machine has no state dir yet, and that is fine"
        );
        assert_eq!(node.app, "zerochat");
    }
}
