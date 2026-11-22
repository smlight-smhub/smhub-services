//! Turning the daemon's errors into the API's error shape.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use smhub_model::Error;

/// The API's error type: wraps the daemon's `Error`.
#[derive(Debug)]
pub struct ApiError(pub Error);

pub type ApiResult<T> = Result<T, ApiError>;

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let e = self.0;
        let status = StatusCode::from_u16(e.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        if status.is_server_error() {
            tracing::warn!("{e}");
        }
        let body = json!({ "error": { "code": e.code(), "message": e.message(), "field": e.field() } });
        let mut r = (status, Json(body)).into_response();
        r.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
        r
    }
}

/// Shorthand for a 422 on a field.
pub fn invalid(field: &str, msg: &str) -> ApiError {
    ApiError(Error::invalid(field, msg))
}
