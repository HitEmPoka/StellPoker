//! Graceful drain protocol for blue-green deployments (Issue #340).
//!
//! A coordinator restart drops every entry in `state.tables` and aborts any
//! in-flight MPC proof, which loses in-flight hands. The drain protocol gives
//! operators a defined cutover window:
//!
//! 1. `POST /api/admin/drain` flips the instance into *draining* mode. New
//!    game-mutating requests (create/join/deal/reveal/showdown/player actions)
//!    are refused with `503 draining`; read traffic, WebSockets and admin
//!    traffic keep working so in-flight hands can finish.
//! 2. The load balancer/readiness probe observes `GET /api/readyz` returning
//!    `503` and stops routing *new* sessions to the instance.
//! 3. The operator (or the SIGTERM handler, which enters drain automatically)
//!    waits until `active_mpc_sessions` reaches zero — or the grace deadline
//!    elapses — then swaps traffic / stops the process.
//! 4. `DELETE /api/admin/drain` aborts a drain (rollback switch).
//!
//! Sessions that cannot complete inside the grace window are handed off: the
//! incoming instance rebuilds their table state from chain via
//! `ensure_session_exists` (see `docs/blue-green-runbook.md`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::State,
    http::{Method, Request},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Default grace window (seconds) before a draining instance is allowed to
/// stop. Matches the ALB `deregistration_delay` used in `infrastructure/`.
pub const DEFAULT_DRAIN_GRACE_SECS: u64 = 30;

/// Immutable-ish status snapshot of the drain state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrainInfo {
    pub draining: bool,
    /// Epoch seconds when drain began.
    pub started_at: Option<u64>,
    pub reason: Option<String>,
    pub grace_secs: u64,
    /// Epoch seconds when the grace window expires.
    pub deadline: Option<u64>,
}

#[derive(Debug)]
struct DrainInner {
    draining: AtomicBool,
    grace_secs: AtomicU64,
    started_at: Mutex<Option<Instant>>,
    reason: Mutex<Option<String>>,
}

/// Shared drain state. Cheap to clone; all clones observe the same flag.
#[derive(Clone, Debug)]
pub struct DrainState {
    inner: Arc<DrainInner>,
}

impl Default for DrainState {
    fn default() -> Self {
        Self::new()
    }
}

impl DrainState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DrainInner {
                draining: AtomicBool::new(false),
                grace_secs: AtomicU64::new(DEFAULT_DRAIN_GRACE_SECS),
                started_at: Mutex::new(None),
                reason: Mutex::new(None),
            }),
        }
    }

    /// Read the grace window from `DRAIN_TIMEOUT_SECS`, falling back to
    /// [`DEFAULT_DRAIN_GRACE_SECS`].
    pub fn grace_secs_from_env() -> u64 {
        std::env::var("DRAIN_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_DRAIN_GRACE_SECS)
    }

    pub fn is_draining(&self) -> bool {
        self.inner.draining.load(Ordering::SeqCst)
    }

    pub fn grace_secs(&self) -> u64 {
        self.inner.grace_secs.load(Ordering::SeqCst)
    }

    /// Enter draining mode. Idempotent — a second call keeps the original
    /// start time so the deadline is stable.
    pub async fn begin(&self, reason: impl Into<String>, grace_secs: u64) {
        if self
            .inner
            .draining
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.inner.grace_secs.store(grace_secs, Ordering::SeqCst);
            *self.inner.started_at.lock().await = Some(Instant::now());
            *self.inner.reason.lock().await = Some(reason.into());
            tracing::warn!(
                "Coordinator entered drain mode (grace={}s)",
                self.grace_secs()
            );
        }
    }

    /// Abort draining (rollback switch). Returns `true` if drain was active.
    pub async fn cancel(&self) -> bool {
        let was_draining = self.inner.draining.swap(false, Ordering::SeqCst);
        *self.inner.started_at.lock().await = None;
        *self.inner.reason.lock().await = None;
        if was_draining {
            tracing::warn!("Coordinator drain mode cancelled");
        }
        was_draining
    }

    /// Whether the grace window for an active drain has expired.
    pub async fn grace_expired(&self) -> bool {
        if !self.is_draining() {
            return false;
        }
        let started = *self.inner.started_at.lock().await;
        match started {
            Some(start) => start.elapsed() >= Duration::from_secs(self.grace_secs()),
            None => false,
        }
    }

    pub async fn info(&self) -> DrainInfo {
        let started = *self.inner.started_at.lock().await;
        let reason = self.inner.reason.lock().await.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let (started_epoch, deadline) = match started {
            Some(start) => {
                let elapsed = start.elapsed().as_secs();
                let started_at = now.saturating_sub(elapsed);
                (Some(started_at), Some(started_at + self.grace_secs()))
            }
            None => (None, None),
        };
        DrainInfo {
            draining: self.is_draining(),
            started_at: started_epoch,
            reason,
            grace_secs: self.grace_secs(),
            deadline,
        }
    }

    /// Wait until the supplied active-session counter reaches zero or the
    /// grace window elapses. Returns the number of sessions still running.
    pub async fn wait_for_sessions(
        &self,
        active_sessions: Arc<std::sync::atomic::AtomicUsize>,
    ) -> usize {
        let deadline = Instant::now() + Duration::from_secs(self.grace_secs());
        loop {
            let active = active_sessions.load(Ordering::SeqCst);
            if active == 0 {
                tracing::info!("Drain complete: all in-flight sessions finished");
                return 0;
            }
            if Instant::now() >= deadline {
                tracing::warn!(
                    "Drain grace window ({}s) elapsed with {} session(s) still active; \
                     relying on session handoff / rehydration",
                    self.grace_secs(),
                    active
                );
                return active;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

/// Decide whether a request must be refused while draining.
///
/// Read-only traffic, health/readiness/metrics probes, admin endpoints and
/// WebSocket upgrades are always allowed so in-flight hands can finish and the
/// orchestrator can still observe the instance. Game-mutating requests are
/// refused so the incoming (green) instance owns new work.
pub fn should_block(path: &str, method: &Method, draining: bool) -> bool {
    if !draining {
        return false;
    }
    if path.starts_with("/api/admin") {
        return false;
    }
    if path == "/api/health" || path == "/api/readyz" || path == "/metrics" {
        return false;
    }
    // Reads, preflight and WebSocket upgrades are not new game work.
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return false;
    }
    true
}

/// Axum middleware enforcing [`should_block`]. Takes the drain state directly
/// (not the full `AppState`) so it can be unit-tested in isolation.
pub async fn drain_middleware(
    State(drain): State<DrainState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if should_block(req.uri().path(), req.method(), drain.is_draining()) {
        let error = crate::errors::ApiError::with_message(
            "draining",
            "Coordinator is draining for a blue-green cutover; retry against the incoming instance.",
        );
        return error.into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Router};
    use tower::ServiceExt;

    #[test]
    fn disabled_drain_allows_everything() {
        assert!(!should_block(
            "/api/table/1/request-deal",
            &Method::POST,
            false
        ));
    }

    #[test]
    fn drain_blocks_mutations_but_allows_reads_and_admin() {
        let m = Method::POST;
        assert!(should_block("/api/table/1/request-deal", &m, true));
        assert!(should_block("/api/tables/create", &m, true));
        assert!(should_block("/api/table/1/request-reveal/flop", &m, true));
        assert!(should_block("/api/table/1/request-showdown", &m, true));
        assert!(should_block("/api/table/1/player-action", &m, true));

        for path in [
            "/api/table/1/state",
            "/api/health",
            "/api/readyz",
            "/metrics",
            "/api/admin/drain",
        ] {
            assert!(
                !should_block(path, &Method::GET, true),
                "{path} must stay readable while draining"
            );
        }

        // Admin mutations (abort drain, maintenance toggle) stay available.
        assert!(!should_block("/api/admin/drain", &Method::DELETE, true));
        assert!(!should_block("/api/admin/maintenance", &m, true));
    }

    #[tokio::test]
    async fn drain_state_transitions_and_deadline() {
        let drain = DrainState::new();
        assert!(!drain.is_draining());
        assert!(!drain.grace_expired().await);

        drain.begin("test", 1).await;
        assert!(drain.is_draining());
        let info = drain.info().await;
        assert!(info.draining);
        assert_eq!(info.reason.as_deref(), Some("test"));
        assert!(info.deadline.is_some());

        // Grace window expiry is time-based; with 0-ish grace it is immediate.
        let short = DrainState::new();
        short.begin("short", 0).await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(short.grace_expired().await);

        assert!(drain.cancel().await);
        assert!(!drain.is_draining());
        assert!(!drain.grace_expired().await);
        assert!(!drain.cancel().await);
    }

    #[tokio::test]
    async fn wait_for_sessions_returns_immediately_when_idle() {
        let drain = DrainState::new();
        drain.begin("test", 5).await;
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        assert_eq!(drain.wait_for_sessions(counter).await, 0);
    }

    /// End-to-end middleware test: a real Axum router layered with
    /// `drain_middleware` must return a `draining` error body for a mutating
    /// request while draining, and pass it through otherwise.
    #[tokio::test]
    async fn middleware_refuses_mutations_while_draining() {
        async fn ok_handler() -> &'static str {
            "ok"
        }

        let drain = DrainState::new();
        let app = Router::new()
            .route("/api/table/:id/request-deal", post(ok_handler))
            .route("/api/table/:id/state", axum::routing::get(ok_handler))
            .route_layer(axum::middleware::from_fn_with_state(
                drain.clone(),
                drain_middleware,
            ));

        // Not draining: request succeeds.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/table/1/request-deal")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);

        // Draining: mutating request is refused with a stable code.
        drain.begin("test", 30).await;
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/table/1/request-deal")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "draining");
        assert_eq!(json["retryable"], true);

        // Draining: read request still succeeds so hands can be observed.
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/table/1/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
    }
}
