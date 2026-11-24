//! Zeroshot CLI failures as HTTP statuses.

use anyhow::anyhow;
use axum::http::StatusCode;

use crate::actions::CliError;
use crate::server::error::ApiError;

#[test]
fn invalid_requests_are_client_errors() {
    let error = |kind: Option<&str>, code: &str| {
        ApiError::from_cli(anyhow!(CliError {
            command: "zeroshot x".into(),
            code: code.into(),
            message: "m".into(),
            kind: kind.map(str::to_owned)
        }))
    };
    assert_eq!(
        error(Some("invalid_request"), "graph.unknown_field").status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        error(Some("invalid_request"), "replay.completed_node_changed").status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        error(Some("run_not_found"), "run.not_found").status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        error(Some("target"), "target.unavailable").status,
        StatusCode::BAD_GATEWAY
    );
}
