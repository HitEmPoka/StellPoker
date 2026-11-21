//! CSP violation report ingestion and storage (issue #136).
//!
//! The Next.js frontend emits `report-uri` / `report-to` directives; reports
//! reach the coordinator through the same-origin collector route
//! (`POST /api/csp/report` on the app), or directly from tooling. Two body
//! shapes are accepted:
//!
//! * legacy report-uri: `{"csp-report": {"violated-directive": ...}}`
//! * Reporting API: `[{"type": "csp", "body": {...}}]`
//!
//! Every body is size-capped and structurally validated before it enters the
//! bounded in-memory store; Prometheus counters feed the provisioned Grafana
//! dashboard (`load-testing/grafana/dashboards/security-csp-dashboard.json`).

use axum::{body::Bytes, extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::{AppState, PrometheusMetrics};

/// Hard cap on a single report body, in bytes.
pub const MAX_REPORT_BYTES: usize = 16 * 1024;

/// Violations retained in the in-memory ring buffer.
const MAX_RECENT: usize = 200;

/// How many of the most recent violations `GET /api/csp/reports` returns.
const SUMMARY_RECENT_LIMIT: usize = 50;

/// Per-field character cap applied to stored reports.
const MAX_FIELD_CHARS: usize = 256;

/// Directives allowed as metric labels; anything else collapses to `other`
/// so hostile report bodies cannot inflate Prometheus cardinality.
const KNOWN_DIRECTIVES: &[&str] = &[
    "default-src",
    "script-src",
    "script-src-elem",
    "script-src-attr",
    "style-src",
    "style-src-elem",
    "style-src-attr",
    "img-src",
    "font-src",
    "connect-src",
    "media-src",
    "object-src",
    "frame-src",
    "frame-ancestors",
    "worker-src",
    "manifest-src",
    "base-uri",
    "form-action",
    "sandbox",
    "upgrade-insecure-requests",
    "block-all-mixed-content",
    "plugin-types",
    "reflected-xss",
    "trusted-types",
    "require-trusted-types-for",
    "navigate-to",
];

/// A CSP violation report after validation and capping.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CspViolation {
    /// Unix timestamp (seconds) at which the coordinator received the report.
    pub received_at: u64,
    pub document_uri: Option<String>,
    pub violated_directive: Option<String>,
    pub effective_directive: Option<String>,
    pub blocked_uri: Option<String>,
    pub source_file: Option<String>,
    /// `enforce`, `report` or `unknown`.
    pub disposition: String,
    pub line_number: Option<i64>,
    pub column_number: Option<i64>,
    pub original_policy: Option<String>,
}

/// Validation failures reported by [`parse_report`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CspReportError {
    /// Body exceeded [`MAX_REPORT_BYTES`].
    TooLarge,
    /// Body was not valid JSON or not a recognised CSP report shape.
    Malformed,
    /// Well-formed report that carried no violation directive.
    NoViolation,
}

impl CspReportError {
    /// Low-cardinality reason label for `coordinator_csp_reports_rejected_total`.
    fn reason(self) -> &'static str {
        match self {
            CspReportError::TooLarge => "too_large",
            CspReportError::Malformed => "malformed",
            CspReportError::NoViolation => "no_violation",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawViolation {
    #[serde(alias = "violated_directive")]
    violated_directive: Option<String>,
    #[serde(alias = "effective_directive")]
    effective_directive: Option<String>,
    #[serde(alias = "document_uri")]
    document_uri: Option<String>,
    #[serde(alias = "blocked_uri")]
    blocked_uri: Option<String>,
    #[serde(alias = "source_file")]
    source_file: Option<String>,
    disposition: Option<String>,
    #[serde(alias = "line_number")]
    line_number: Option<i64>,
    #[serde(alias = "column_number")]
    column_number: Option<i64>,
    #[serde(alias = "original_policy")]
    original_policy: Option<String>,
}

/// Validate a raw CSP report body and return its violations.
///
/// Returns [`CspReportError::TooLarge`] when the body exceeds
/// [`MAX_REPORT_BYTES`], [`CspReportError::Malformed`] for unparseable or
/// unrecognised shapes, and [`CspReportError::NoViolation`] when the report
/// carries neither a `violated-directive` nor an `effective-directive`.
pub fn parse_report(body: &[u8]) -> Result<Vec<CspViolation>, CspReportError> {
    if body.len() > MAX_REPORT_BYTES {
        return Err(CspReportError::TooLarge);
    }
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| CspReportError::Malformed)?;
    let now = unix_now();

    let violations = match value {
        serde_json::Value::Array(entries) => {
            let mut violations = Vec::new();
            for entry in entries {
                let entry = entry.as_object().ok_or(CspReportError::Malformed)?;
                if let Some(kind) = entry.get("type").and_then(|t| t.as_str()) {
                    if kind != "csp" {
                        continue;
                    }
                }
                let raw = entry
                    .get("body")
                    .cloned()
                    .ok_or(CspReportError::Malformed)?;
                let raw: RawViolation =
                    serde_json::from_value(raw).map_err(|_| CspReportError::Malformed)?;
                if let Some(violation) = sanitize(raw, now) {
                    violations.push(violation);
                }
            }
            if violations.is_empty() {
                return Err(CspReportError::NoViolation);
            }
            violations
        }
        serde_json::Value::Object(map) => {
            let raw = map
                .get("csp-report")
                .cloned()
                .ok_or(CspReportError::Malformed)?;
            let raw: RawViolation =
                serde_json::from_value(raw).map_err(|_| CspReportError::Malformed)?;
            let Some(violation) = sanitize(raw, now) else {
                return Err(CspReportError::NoViolation);
            };
            vec![violation]
        }
        _ => return Err(CspReportError::Malformed),
    };
    Ok(violations)
}

/// Validate, store and count one raw CSP report body.
///
/// Successful ingestion returns the number of violations recorded and
/// increments `coordinator_csp_reports_total` plus one
/// `coordinator_csp_violations_total{effective_directive, disposition}`
/// sample per violation. Failures are counted on
/// `coordinator_csp_reports_rejected_total{reason}` before being returned.
pub fn ingest(
    store: &CspReportStore,
    metrics: &PrometheusMetrics,
    body: &[u8],
) -> Result<usize, CspReportError> {
    let violations = match parse_report(body) {
        Ok(violations) => violations,
        Err(error) => {
            metrics
                .csp_reports_rejected_total
                .with_label_values(&[error.reason()])
                .inc();
            return Err(error);
        }
    };
    for violation in &violations {
        metrics
            .csp_violations_total
            .with_label_values(&[&directive_label(violation), &violation.disposition])
            .inc();
        store.record(violation.clone());
    }
    metrics.csp_reports_total.inc();
    Ok(violations.len())
}

/// Bounded in-memory store of validated CSP violations.
#[derive(Clone, Default)]
pub struct CspReportStore(Arc<Mutex<CspReportInner>>);

#[derive(Default)]
struct CspReportInner {
    total: u64,
    by_directive: HashMap<String, u64>,
    recent: VecDeque<CspViolation>,
}

impl CspReportStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a violation, evicting the oldest entry once the buffer is full.
    pub fn record(&self, violation: CspViolation) {
        let mut inner = self.0.lock().expect("CSP report store lock poisoned");
        inner.total += 1;
        *inner
            .by_directive
            .entry(directive_label(&violation))
            .or_insert(0) += 1;
        if inner.recent.len() >= MAX_RECENT {
            inner.recent.pop_front();
        }
        inner.recent.push_back(violation);
    }

    /// Snapshot for `GET /api/csp/reports`: totals by sanitized directive
    /// plus the most recent violations, newest first.
    pub fn summary(&self) -> CspReportSummary {
        let inner = self.0.lock().expect("CSP report store lock poisoned");
        CspReportSummary {
            total: inner.total,
            by_directive: inner
                .by_directive
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            recent: inner
                .recent
                .iter()
                .rev()
                .take(SUMMARY_RECENT_LIMIT)
                .cloned()
                .collect(),
        }
    }
}

/// JSON body of `GET /api/csp/reports`.
#[derive(Debug, Serialize)]
pub struct CspReportSummary {
    /// Violations received since process start.
    pub total: u64,
    /// Violation counts keyed by sanitized directive label.
    pub by_directive: BTreeMap<String, u64>,
    /// Most recent violations, newest first (up to 50).
    pub recent: Vec<CspViolation>,
}

/// POST /api/csp/report
///
/// Ingests a CSP violation report from the frontend collector (or any other
/// reporting agent). Bodies are size-capped and validated before they enter
/// the store. Returns `204` when recorded, `413` for oversized bodies and
/// `400` for malformed reports — browsers fire-and-forget these requests, so
/// the status line is the only feedback channel.
pub async fn report_csp(State(state): State<AppState>, body: Bytes) -> StatusCode {
    match ingest(&state.csp_reports, &state.metrics.prometheus, &body) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(CspReportError::TooLarge) => StatusCode::PAYLOAD_TOO_LARGE,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

/// GET /api/csp/reports
///
/// Snapshot of everything ingested so far: totals by sanitized directive
/// label plus the most recent violations, newest first.
pub async fn list_csp_reports(State(state): State<AppState>) -> Json<CspReportSummary> {
    Json(state.csp_reports.summary())
}

/// Bounded, whitelist-only label derived from a violation's directive.
fn directive_label(violation: &CspViolation) -> String {
    let source = violation
        .effective_directive
        .as_deref()
        .or(violation.violated_directive.as_deref())
        .unwrap_or_default();
    let token = source
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if KNOWN_DIRECTIVES.contains(&token.as_str()) {
        token
    } else if token.is_empty() {
        "unknown".to_string()
    } else {
        "other".to_string()
    }
}

fn disposition_label(value: Option<&str>) -> String {
    match value {
        Some(v) if v.eq_ignore_ascii_case("enforce") => "enforce".to_string(),
        Some(v) if v.eq_ignore_ascii_case("report") => "report".to_string(),
        _ => "unknown".to_string(),
    }
}

fn cap(value: Option<String>) -> Option<String> {
    let value = value.filter(|v| !v.is_empty())?;
    if value.chars().count() > MAX_FIELD_CHARS {
        Some(value.chars().take(MAX_FIELD_CHARS).collect())
    } else {
        Some(value)
    }
}

fn sanitize(raw: RawViolation, received_at: u64) -> Option<CspViolation> {
    let violated_directive = cap(raw.violated_directive);
    let effective_directive = cap(raw.effective_directive);
    if violated_directive.is_none() && effective_directive.is_none() {
        return None;
    }
    Some(CspViolation {
        received_at,
        document_uri: cap(raw.document_uri),
        violated_directive,
        effective_directive,
        blocked_uri: cap(raw.blocked_uri),
        source_file: cap(raw.source_file),
        disposition: disposition_label(raw.disposition.as_deref()),
        line_number: raw.line_number.filter(|n| *n >= 0),
        column_number: raw.column_number.filter(|n| *n >= 0),
        original_policy: cap(raw.original_policy),
    })
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> PrometheusMetrics {
        PrometheusMetrics::new()
    }

    fn legacy_body(directive: &str, disposition: &str) -> Vec<u8> {
        serde_json::json!({
            "csp-report": {
                "document-uri": "https://app.example.com/play",
                "violated-directive": directive,
                "effective-directive": directive,
                "blocked-uri": "https://evil.example.com/x.js",
                "source-file": "https://app.example.com/bundle.js",
                "disposition": disposition,
                "line-number": 42,
                "column-number": 7,
                "original-policy": "default-src 'self'"
            }
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn parses_legacy_report_uri_body() {
        let violations = parse_report(&legacy_body("script-src", "report")).expect("valid");
        assert_eq!(violations.len(), 1);
        let violation = &violations[0];
        assert_eq!(violation.effective_directive.as_deref(), Some("script-src"));
        assert_eq!(violation.violated_directive.as_deref(), Some("script-src"));
        assert_eq!(violation.blocked_uri.as_deref(), Some("https://evil.example.com/x.js"));
        assert_eq!(violation.disposition, "report");
        assert_eq!(violation.line_number, Some(42));
        assert_eq!(violation.column_number, Some(7));
        assert!(violation.received_at > 0);
    }

    #[test]
    fn parses_snake_case_field_aliases() {
        let body = br#"{"csp-report":{"violated_directive":"img-src","line_number":3}}"#;
        let violations = parse_report(body).expect("valid");
        assert_eq!(violations[0].violated_directive.as_deref(), Some("img-src"));
        assert_eq!(violations[0].line_number, Some(3));
    }

    #[test]
    fn parses_reporting_api_array_and_skips_other_types() {
        let body = serde_json::json!([
            {"type": "network", "url": "https://app.example.com/", "body": {"foo": "bar"}},
            {"type": "csp", "age": 4, "body": {
                "violated-directive": "style-src",
                "disposition": "enforce"
            }}
        ])
        .to_string()
        .into_bytes();
        let violations = parse_report(&body).expect("valid");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].violated_directive.as_deref(), Some("style-src"));
        assert_eq!(violations[0].disposition, "enforce");
    }

    #[test]
    fn rejects_garbage_and_unrecognised_shapes() {
        assert_eq!(parse_report(b"not json"), Err(CspReportError::Malformed));
        assert_eq!(parse_report(b"123"), Err(CspReportError::Malformed));
        assert_eq!(parse_report(br#"{"foo": 1}"#), Err(CspReportError::Malformed));
        assert_eq!(
            parse_report(br#"{"csp-report": {"blocked-uri": "x"}}"#),
            Err(CspReportError::NoViolation)
        );
        assert_eq!(
            parse_report(br#"[{"type": "network"}]"#),
            Err(CspReportError::NoViolation)
        );
        assert_eq!(
            parse_report(br#"[{"type": "csp"}]"#),
            Err(CspReportError::Malformed)
        );
    }

    #[test]
    fn rejects_bodies_over_the_size_cap() {
        let mut body = legacy_body("script-src", "report");
        body.resize(MAX_REPORT_BYTES + 1, b' ');
        assert_eq!(parse_report(&body), Err(CspReportError::TooLarge));
    }

    #[test]
    fn caps_long_fields_and_drops_negative_line_numbers() {
        let body = serde_json::json!({
            "csp-report": {
                "violated-directive": "script-src",
                "blocked-uri": "a".repeat(MAX_FIELD_CHARS + 100),
                "line-number": -4
            }
        })
        .to_string()
        .into_bytes();
        let violations = parse_report(&body).expect("valid");
        let violation = &violations[0];
        assert_eq!(
            violation.blocked_uri.as_deref().map(|s| s.chars().count()),
            Some(MAX_FIELD_CHARS)
        );
        assert_eq!(violation.line_number, None);
    }

    #[test]
    fn normalizes_disposition_labels() {
        let enforce = parse_report(&legacy_body("script-src", "ENFORCE")).expect("valid");
        assert_eq!(enforce[0].disposition, "enforce");
        let bogus = parse_report(&legacy_body("script-src", "sideways")).expect("valid");
        assert_eq!(bogus[0].disposition, "unknown");
    }

    #[test]
    fn ingest_counts_and_stores() {
        let store = CspReportStore::new();
        let metrics = metrics();
        let recorded = ingest(&store, &metrics, &legacy_body("script-src", "report"))
            .expect("valid");
        assert_eq!(recorded, 1);
        assert_eq!(metrics.csp_reports_total.get(), 1);
        assert_eq!(
            metrics
                .csp_violations_total
                .with_label_values(&["script-src", "report"])
                .get(),
            1
        );
        assert_eq!(store.summary().total, 1);
    }

    #[test]
    fn ingest_counts_rejections_without_touching_the_store() {
        let store = CspReportStore::new();
        let metrics = metrics();
        assert_eq!(
            ingest(&store, &metrics, b"nope"),
            Err(CspReportError::Malformed)
        );
        assert_eq!(
            metrics
                .csp_reports_rejected_total
                .with_label_values(&["malformed"])
                .get(),
            1
        );
        assert_eq!(metrics.csp_reports_total.get(), 0);
        assert_eq!(store.summary().total, 0);
    }

    #[test]
    fn summary_bounds_recent_entries_and_sorts_directives() {
        let store = CspReportStore::new();
        let metrics = metrics();
        for _ in 0..MAX_RECENT + 25 {
            ingest(&store, &metrics, &legacy_body("style-src", "report")).expect("valid");
        }
        ingest(&store, &metrics, &legacy_body("img-src", "enforce")).expect("valid");

        let summary = store.summary();
        assert_eq!(summary.total, MAX_RECENT as u64 + 26);
        assert_eq!(summary.recent.len(), SUMMARY_RECENT_LIMIT);
        assert_eq!(
            summary.recent[0].effective_directive.as_deref(),
            Some("img-src")
        );
        assert_eq!(
            summary.by_directive.get("style-src"),
            Some(&(MAX_RECENT as u64 + 25))
        );
        assert_eq!(summary.by_directive.get("img-src"), Some(&1));
        let keys: Vec<&str> = summary.by_directive.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["img-src", "style-src"]);
    }

    #[test]
    fn directive_labels_are_whitelisted() {
        let store = CspReportStore::new();
        let metrics = metrics();

        let sneaky = serde_json::json!({
            "csp-report": {"violated-directive": "made-up-src 'self' https://x.example.com"}
        })
        .to_string()
        .into_bytes();
        ingest(&store, &metrics, &sneaky).expect("valid");

        let summary = store.summary();
        assert!(summary.by_directive.contains_key("other"));
        assert_eq!(summary.by_directive.len(), 1);

        let bare = serde_json::json!({
            "csp-report": {"violated-directive": "img-src 'self' data:"}
        })
        .to_string()
        .into_bytes();
        ingest(&store, &metrics, &bare).expect("valid");
        let summary = store.summary();
        assert!(summary.by_directive.contains_key("img-src"));
    }
}
