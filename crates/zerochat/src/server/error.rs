//! The API's error answer: an HTTP status and `{code, message}`, used by every handler.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use crate::actions::CliError;
use crate::actions::cli::code;
use crate::pb_exec::PbCommandError;

#[derive(Debug)]
pub struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) code: String,
    pub(crate) message: String,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.to_owned(),
            message: message.into(),
        }
    }
    pub(crate) fn bad_request(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
    pub(crate) fn not_found(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message)
    }
    pub(crate) fn unavailable(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }
    pub(crate) fn internal(error: anyhow::Error) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("{error:#}"),
        )
    }
    pub(crate) fn from_cli(error: anyhow::Error) -> Self {
        match error.downcast_ref::<CliError>() {
            Some(cli) => {
                let status = match (cli.kind.as_deref(), cli.code.as_str()) {
                    (_, code::ZEROSHOT_NOT_FOUND) => StatusCode::SERVICE_UNAVAILABLE,
                    (_, code::TIMEOUT) => StatusCode::GATEWAY_TIMEOUT,
                    (Some("invalid_request"), code) if code.starts_with("replay.") => {
                        StatusCode::UNPROCESSABLE_ENTITY
                    }
                    (Some("invalid_request"), _) => StatusCode::BAD_REQUEST,
                    (Some("run_not_found"), _) => StatusCode::NOT_FOUND,
                    (Some("submission_conflict"), _) => StatusCode::CONFLICT,
                    _ => StatusCode::BAD_GATEWAY,
                };
                Self::new(status, &cli.code, cli.message.clone())
            }
            None => Self::internal(error),
        }
    }
    /// A `pb_exec` failure: either request validation (caught before any process ran) or pb's
    /// own refusal/error text. `pb run --detach`'s "a batch is already running" message is the
    /// one case worth a distinct status so the UI can tell a conflict from a bad request.
    pub(crate) fn from_pb(error: anyhow::Error) -> Self {
        match error.downcast_ref::<PbCommandError>() {
            Some(pb) => {
                let status = if pb.message.contains("already running") {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::BAD_REQUEST
                };
                Self::new(status, "pb_failed", pb.message.clone())
            }
            None => Self::internal(error),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"code": self.code, "message": self.message})),
        )
            .into_response()
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;
