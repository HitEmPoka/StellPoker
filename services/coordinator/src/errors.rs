//! Stable, machine-readable API error codes.
//!
//! The catalogue lives in the repository-root `errors.json` (a single source of
//! truth shared with the OpenAPI specs and the Next.js frontend). This module
//! embeds that file and turns it into an [`ApiError`] type that serialises to
//!
//! ```json
//! { "code": "table_not_found", "message": "...", "retryable": false }
//! ```
//!
//! Handlers return `Result<T, ApiError>`; the `From<StatusCode>` implementation
//! lets the large body of existing `Err(StatusCode::X)` / `.map_err(|_|
//! StatusCode::X)?` call sites keep working while gaining a stable code and a
//! JSON body. Use [`ApiError::new`] / [`ApiError::with_message`] with an
//! explicit code where the status alone is ambiguous (e.g. `draining`,
//! `mpc_node_unavailable`).

use std::sync::OnceLock;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Embedded copy of the repo-root `errors.json` catalogue.
const ERRORS_JSON: &str = include_str!("../../../errors.json");

/// One entry from `errors.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDef {
    pub code: String,
    pub http_status: u16,
    pub retryable: bool,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct ErrorCatalogue {
    #[allow(dead_code)]
    version: u32,
    errors: Vec<ErrorDef>,
}

/// Wire format returned to clients for every error response.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ErrorBody {
    /// Stable machine-readable error code (see `errors.json`).
    pub code: String,
    /// Human-readable description; not intended for programmatic matching.
    pub message: String,
    /// Whether the client may safely retry the same request.
    pub retryable: bool,
}

fn catalogue() -> &'static Vec<ErrorDef> {
    static CATALOGUE: OnceLock<Vec<ErrorDef>> = OnceLock::new();
    CATALOGUE.get_or_init(|| {
        let parsed: ErrorCatalogue =
            serde_json::from_str(ERRORS_JSON).expect("errors.json must be valid JSON");
        parsed.errors
    })
}

/// Look up a definition by code.
pub fn definition(code: &str) -> Option<&'static ErrorDef> {
    catalogue().iter().find(|e| e.code == code)
}

/// All error definitions, in catalogue order.
pub fn definitions() -> &'static [ErrorDef] {
    catalogue().as_slice()
}

/// Map an HTTP status to the default error code for that status.
///
/// Used when a handler only knows the status. Codes returned here are always
/// present in `errors.json` (enforced by the unit tests below).
pub fn default_code_for_status(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 => "bad_request",
        401 => "unauthorized",
        403 => "forbidden",
        404 => "not_found",
        405 => "bad_request",
        409 => "conflict",
        413 => "bad_request",
        422 => "bad_request",
        429 => "rate_limited",
        500 => "internal_error",
        502 => "upstream_unavailable",
        503 => "service_unavailable",
        504 => "upstream_unavailable",
        s if (400..500).contains(&s) => "bad_request",
        _ => "internal_error",
    }
}

/// A coordinator API error: a stable code, an HTTP status, a retryability flag
/// and a human-readable message.
#[derive(Debug, Clone)]
pub struct ApiError {
    code: &'static str,
    message: String,
}

impl ApiError {
    /// Build an error from a code in the catalogue, using its default message.
    pub fn new(code: &'static str) -> Self {
        let message = definition(code)
            .map(|d| d.description.clone())
            .unwrap_or_else(|| "An unexpected error occurred.".to_string());
        Self { code, message }
    }

    /// Build an error from a code with a custom (client-safe) message.
    pub fn with_message(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Build an error from an HTTP status, picking the default code for it.
    pub fn from_status(status: StatusCode) -> Self {
        Self::new(default_code_for_status(status))
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn http_status(&self) -> StatusCode {
        definition(self.code)
            .map(|d| StatusCode::from_u16(d.http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn retryable(&self) -> bool {
        definition(self.code).map(|d| d.retryable).unwrap_or(false)
    }

    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            code: self.code.to_string(),
            message: self.message.clone(),
            retryable: self.retryable(),
        }
    }
}

impl From<StatusCode> for ApiError {
    fn from(status: StatusCode) -> Self {
        Self::from_status(status)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.http_status();
        let retryable = self.retryable();
        let body = json!(ErrorBody {
            code: self.code.to_string(),
            message: self.message,
            retryable,
        });
        (status, axum::Json(body)).into_response()
    }
}

/// Outermost response middleware that gives *every* error response a stable
/// machine-readable code.
///
/// Most coordinator handlers predate `ApiError` and still return
/// `Result<_, StatusCode>`, producing a bare status with an empty body. Rather
/// than rewriting every call site, this layer rewrites any client/server error
/// whose body is not already JSON into the standard envelope:
///
/// ```json
/// { "code": "not_found", "message": "...", "retryable": false }
/// ```
///
/// Responses that already carry `application/json` (e.g. `ApiError`) are left
/// untouched, as are successful responses. The original HTTP status is always
/// preserved.
pub async fn error_response_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let response = next.run(req).await;
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return response;
    }

    let already_json = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false);
    if already_json {
        return response;
    }

    let error = ApiError::from_status(status);
    let body = json!(error.body());
    let mut coded = (status, axum::Json(body)).into_response();
    coded.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );
    coded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_parses_and_codes_are_unique() {
        let defs = definitions();
        assert!(!defs.is_empty(), "errors.json must define at least one code");
        let mut seen = std::collections::HashSet::new();
        for d in defs {
            assert!(seen.insert(d.code.clone()), "duplicate code: {}", d.code);
            assert!(
                (400..=599).contains(&d.http_status),
                "invalid status for {}",
                d.code
            );
            assert!(!d.description.trim().is_empty(), "empty description: {}", d.code);
        }
    }

    #[test]
    fn status_defaults_exist_in_catalogue() {
        for status in [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
            StatusCode::CONFLICT,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
        ] {
            let code = default_code_for_status(status);
            assert!(
                definition(code).is_some(),
                "default code {code} for status {status} missing from errors.json"
            );
        }
    }

    #[test]
    fn api_error_uses_catalogue_status_and_flags() {
        let err = ApiError::new("draining");
        assert_eq!(err.http_status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(err.retryable());

        let err = ApiError::new("mpc_node_unavailable");
        assert_eq!(err.http_status(), StatusCode::CONFLICT);
        assert!(err.retryable());

        let err = ApiError::from_status(StatusCode::BAD_GATEWAY);
        assert_eq!(err.code(), "upstream_unavailable");
    }

    #[test]
    fn custom_message_overrides_default() {
        let err = ApiError::with_message("bad_request", "table_id must be > 0");
        assert_eq!(err.message(), "table_id must be > 0");
        assert_eq!(err.http_status(), StatusCode::BAD_REQUEST);
    }
}
