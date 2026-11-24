//! Global authorization gate for coordinator admin routes (issue #1010).
//!
//! Every `/api/admin/*` request must carry a Stellar signed-payload challenge
//! from an address in the server-side `ADMIN_KEYS` allowlist. The UI gate is
//! cosmetic only; this middleware is the enforcement point.

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::api::admin::{
    authenticate_admin_request, consume_admin_nonce, header_string, nonce_from_headers,
    AdminAuthGuardState,
};

/// Reject unauthenticated admin HTTP requests before any handler runs.
pub async fn require_admin_auth(
    State(guard): State<AdminAuthGuardState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if !request.uri().path().starts_with("/api/admin") {
        return Ok(next.run(request).await);
    }

    let headers: HeaderMap = request.headers().clone();

    // The action is supplied by the caller and re-checked by each handler with
    // its own expected value, so a signature minted for one operation cannot
    // be replayed against another.
    let action = header_string(&headers, "x-admin-action")?;
    let auth = authenticate_admin_request(&guard, &headers, &action).await?;
    let nonce = nonce_from_headers(&headers)?;
    consume_admin_nonce(&guard, &auth.address, nonce).await?;

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::admin::AdminConfig;
    use axum::{routing::get, Router};
    use tower::ServiceExt;

    fn admin_router() -> Router {
        Router::new()
            .route("/api/admin/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                AdminAuthGuardState::new(AdminConfig::empty()),
                require_admin_auth,
            ))
    }

    #[tokio::test]
    async fn rejects_unauthenticated_admin_request() {
        let response = admin_router()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(
            response.status() == StatusCode::UNAUTHORIZED
                || response.status() == StatusCode::FORBIDDEN,
            "unauthenticated admin request should return 401/403, got {}",
            response.status()
        );
    }

    #[tokio::test]
    async fn public_routes_are_not_gated() {
        let app = Router::new()
            .route("/api/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                AdminAuthGuardState::new(AdminConfig::empty()),
                require_admin_auth,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
