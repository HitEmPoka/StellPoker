//! W3C trace-context propagation for MPC node handlers (Issue #672).
//!
//! The coordinator already forwards a `traceparent` header on every node call
//! (Issue #255), but the node used to drop it, so node logs could not be joined
//! with the coordinator trace. This module closes that gap:
//!
//! - [`propagate`] is an axum middleware that reads `traceparent` /
//!   `tracestate`, derives a child span id for this node hop, and runs the
//!   handler inside a `mpc.request` tracing span carrying `trace_id`,
//!   `span_id` and `parent_span_id`. Requests without a valid header start a
//!   new root trace so node-originated work is still correlatable.
//! - The active context is stored in a task-local so outbound peer calls
//!   (share dispatch) can forward it with [`inject`] without threading it
//!   through every function signature.
//! - The node's `traceparent` is echoed on the response so callers can link
//!   the hop even when no OTel collector is running.
//!
//! Work spawned onto a new tokio task (proof generation) does not inherit the
//! task-local; capture [`current`] before spawning and re-enter it with
//! [`scope`].

use axum::{body::Body, extract::Request, http::HeaderMap, middleware::Next, response::Response};
use tracing::Instrument;

pub const TRACEPARENT: &str = "traceparent";
pub const TRACESTATE: &str = "tracestate";

/// Maximum `tracestate` length we forward; the W3C spec caps it at 32
/// list-members and recommends propagating at least 512 bytes.
const MAX_TRACESTATE_LEN: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    /// 32 lowercase hex chars, shared by every hop of the trace.
    pub trace_id: String,
    /// 16 lowercase hex chars identifying this node's hop.
    pub span_id: String,
    /// Span id of the caller, when the request carried a valid `traceparent`.
    pub parent_span_id: Option<String>,
    pub sampled: bool,
    pub tracestate: Option<String>,
}

impl TraceContext {
    /// Start a new trace (no upstream caller).
    pub fn new_root() -> Self {
        Self {
            trace_id: random_hex_id(16),
            span_id: random_hex_id(8),
            parent_span_id: None,
            sampled: true,
            tracestate: None,
        }
    }

    /// Continue the trace described by the incoming headers, or start a new
    /// one when `traceparent` is missing or malformed.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let parsed = headers
            .get(TRACEPARENT)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_traceparent);

        match parsed {
            Some((trace_id, parent_span_id, sampled)) => Self {
                trace_id,
                span_id: random_hex_id(8),
                parent_span_id: Some(parent_span_id),
                sampled,
                // tracestate is only meaningful alongside a valid traceparent.
                tracestate: headers
                    .get(TRACESTATE)
                    .and_then(|v| v.to_str().ok())
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && s.len() <= MAX_TRACESTATE_LEN)
                    .map(str::to_string),
            },
            None => Self::new_root(),
        }
    }

    /// Render this hop as a `traceparent` header value.
    pub fn traceparent(&self) -> String {
        format!(
            "00-{}-{}-{}",
            self.trace_id,
            self.span_id,
            if self.sampled { "01" } else { "00" }
        )
    }
}

/// Parse a W3C `traceparent` header into `(trace_id, parent_span_id, sampled)`.
///
/// Rejects unknown-version formats we cannot interpret, uppercase hex, and the
/// all-zero ids the spec declares invalid.
pub fn parse_traceparent(value: &str) -> Option<(String, String, bool)> {
    let parts: Vec<&str> = value.trim().split('-').collect();
    if parts.len() < 4 {
        return None;
    }
    let (version, trace_id, span_id, flags) = (parts[0], parts[1], parts[2], parts[3]);

    if !is_lower_hex(version, 2) || version == "ff" {
        return None;
    }
    // Version 00 has exactly four fields; future versions may append more.
    if version == "00" && parts.len() != 4 {
        return None;
    }
    if !is_lower_hex(trace_id, 32) || trace_id.bytes().all(|b| b == b'0') {
        return None;
    }
    if !is_lower_hex(span_id, 16) || span_id.bytes().all(|b| b == b'0') {
        return None;
    }
    if !is_lower_hex(flags, 2) {
        return None;
    }
    let flags = u8::from_str_radix(flags, 16).ok()?;

    Some((trace_id.to_string(), span_id.to_string(), flags & 0x01 == 1))
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn random_hex_id(bytes: usize) -> String {
    loop {
        let raw: Vec<u8> = (0..bytes).map(|_| rand::random::<u8>()).collect();
        if raw.iter().any(|b| *b != 0) {
            return hex::encode(raw);
        }
    }
}

tokio::task_local! {
    static CURRENT: TraceContext;
}

/// The trace context of the request currently being handled, if any.
pub fn current() -> Option<TraceContext> {
    CURRENT.try_with(|ctx| ctx.clone()).ok()
}

/// Run `fut` with `ctx` as the active trace context, inside a span that
/// carries its ids so every log line emitted by `fut` is correlatable.
pub async fn scope<F: std::future::Future>(
    ctx: TraceContext,
    span_name: &'static str,
    fut: F,
) -> F::Output {
    let span = tracing::info_span!(
        "mpc.trace",
        otel.name = span_name,
        trace_id = %ctx.trace_id,
        span_id = %ctx.span_id,
        parent_span_id = ctx.parent_span_id.as_deref().unwrap_or("-"),
    );
    CURRENT.scope(ctx, fut.instrument(span)).await
}

/// Attach the trace context to an outbound peer request.
pub fn inject(req: reqwest::RequestBuilder, ctx: Option<&TraceContext>) -> reqwest::RequestBuilder {
    let Some(ctx) = ctx else {
        return req;
    };
    let req = req.header(TRACEPARENT, ctx.traceparent());
    match &ctx.tracestate {
        Some(state) => req.header(TRACESTATE, state.as_str()),
        None => req,
    }
}

/// Axum middleware: extract the incoming trace context, run the handler inside
/// a correlated span, and echo this hop's `traceparent` on the response.
pub async fn propagate(request: Request<Body>, next: Next) -> Response {
    let ctx = TraceContext::from_headers(request.headers());
    let traceparent = ctx.traceparent();

    let span = tracing::info_span!(
        "mpc.request",
        trace_id = %ctx.trace_id,
        span_id = %ctx.span_id,
        parent_span_id = ctx.parent_span_id.as_deref().unwrap_or("-"),
        http.method = %request.method(),
        http.target = %request.uri().path(),
        http.status_code = tracing::field::Empty,
    );

    let mut response = CURRENT
        .scope(ctx, next.run(request).instrument(span.clone()))
        .await;
    span.record("http.status_code", response.status().as_u16());

    if let Ok(value) = traceparent.parse() {
        response.headers_mut().insert(TRACEPARENT, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, routing::get, Router};
    use tower::ServiceExt;

    const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
    const PARENT: &str = "00f067aa0ba902b7";

    fn header(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(TRACEPARENT, v.parse().unwrap());
        h
    }

    #[test]
    fn parses_valid_traceparent() {
        let (t, p, sampled) = parse_traceparent(&format!("00-{TRACE}-{PARENT}-01")).unwrap();
        assert_eq!(t, TRACE);
        assert_eq!(p, PARENT);
        assert!(sampled);

        let (_, _, sampled) = parse_traceparent(&format!("00-{TRACE}-{PARENT}-00")).unwrap();
        assert!(!sampled);
    }

    #[test]
    fn rejects_malformed_traceparent() {
        for bad in [
            "",
            "garbage",
            &format!("ff-{TRACE}-{PARENT}-01"),
            &format!("00-{}-{PARENT}-01", "0".repeat(32)),
            &format!("00-{TRACE}-{}-01", "0".repeat(16)),
            &format!("00-{}-{PARENT}-01", TRACE.to_uppercase()),
            &format!("00-{TRACE}-{PARENT}-01-extra"),
            &format!("00-{TRACE}-{PARENT}-zz"),
            &format!("00-{}-{PARENT}-01", &TRACE[..31]),
        ] {
            assert!(parse_traceparent(bad).is_none(), "accepted {bad:?}");
        }
    }

    #[test]
    fn future_versions_with_extra_fields_are_accepted() {
        assert!(parse_traceparent(&format!("01-{TRACE}-{PARENT}-01-extra")).is_some());
    }

    #[test]
    fn child_context_keeps_trace_id_and_new_span() {
        let mut h = header(&format!("00-{TRACE}-{PARENT}-01"));
        h.insert(TRACESTATE, "vendor=abc".parse().unwrap());
        let ctx = TraceContext::from_headers(&h);
        assert_eq!(ctx.trace_id, TRACE);
        assert_eq!(ctx.parent_span_id.as_deref(), Some(PARENT));
        assert_ne!(ctx.span_id, PARENT);
        assert_eq!(ctx.tracestate.as_deref(), Some("vendor=abc"));
        assert!(parse_traceparent(&ctx.traceparent()).is_some());
    }

    #[test]
    fn missing_header_starts_new_root() {
        let ctx = TraceContext::from_headers(&HeaderMap::new());
        assert!(ctx.parent_span_id.is_none());
        assert!(ctx.tracestate.is_none());
        let (t, s, sampled) = parse_traceparent(&ctx.traceparent()).unwrap();
        assert_eq!(t, ctx.trace_id);
        assert_eq!(s, ctx.span_id);
        assert!(sampled);
    }

    #[test]
    fn inject_sets_outbound_headers() {
        let mut ctx = TraceContext::new_root();
        ctx.tracestate = Some("vendor=abc".into());
        let req = inject(reqwest::Client::new().post("http://peer/x"), Some(&ctx))
            .build()
            .unwrap();
        assert_eq!(req.headers()[TRACEPARENT], ctx.traceparent().as_str());
        assert_eq!(req.headers()[TRACESTATE], "vendor=abc");

        let req = inject(reqwest::Client::new().post("http://peer/x"), None)
            .build()
            .unwrap();
        assert!(req.headers().get(TRACEPARENT).is_none());
    }

    #[tokio::test]
    async fn scope_exposes_current_context() {
        assert!(current().is_none());
        let ctx = TraceContext::new_root();
        let seen = scope(ctx.clone(), "test", async { current() }).await;
        assert_eq!(seen, Some(ctx));
    }

    #[tokio::test]
    async fn middleware_continues_trace_and_echoes_header() {
        let app = Router::new()
            .route(
                "/probe",
                get(|| async { current().map(|c| c.trace_id).unwrap_or_default() }),
            )
            .layer(axum::middleware::from_fn(propagate));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/probe")
                    .header(TRACEPARENT, format!("00-{TRACE}-{PARENT}-01"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let echoed = resp.headers()[TRACEPARENT].to_str().unwrap().to_string();
        let (t, s, _) = parse_traceparent(&echoed).unwrap();
        assert_eq!(t, TRACE);
        assert_ne!(s, PARENT);

        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], TRACE.as_bytes());
    }
}
