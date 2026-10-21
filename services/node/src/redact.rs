//! Structured log redaction (Issue #509).
//!
//! Guarantees that card values, MPC secret shares, and commitment salts never
//! reach the log stream, independently of the `REQUEST_LOG_FORMAT` in use
//! (`json` or pretty). The same policy is written up in
//! `docs/unified-logging-schema.md` and enforced by the grep-based check in
//! `.github/workflows/ci.yml` → `scripts/check_log_redaction.py`.
//!
//! Two behaviours compose with the existing `tracing_subscriber::fmt`
//! setup:
//!
//! 1. `RedactingMakeWriter` wraps the configured sink and rewrites every line
//!    before it is written. JSON lines are parsed and walked (sensitive keys
//!    at any nesting depth are replaced), everything else goes through a
//!    regex that redacts `key=value` / `key="..."` / `key=[...]` tokens.
//! 2. `is_sensitive_key` is the single source of truth used by both paths —
//!    keep it in sync with the script-level forbidden list.

use regex::Regex;
use std::io::{self, Write};
use std::sync::OnceLock;
use tracing_subscriber::fmt::MakeWriter;

/// Field keys whose values must never reach a log stream.
///
/// Card values, commitment salts, MPC secret shares, session/committee
/// secrets, and API credentials. Matching is boundary-aware (see
/// `is_sensitive_key`) so public fields such as `deck_root`,
/// `hand_commitments` or `board_indices` are unaffected.
pub const SENSITIVE_FIELD_KEYS: &[&str] = &[
    // Card values — never log hole cards or any resolved card.
    "hole_cards",
    "hole_card",
    "hole_card1",
    "hole_card2",
    "card1",
    "card2",
    "cards",
    "card",
    "player_card_positions",
    // Commitment salts — reveal the committed value when read with the hand.
    "salts",
    "salt1",
    "salt2",
    "salt",
    // MPC secret shares / share bundles.
    "shares",
    "share",
    "share_bundle",
    "share_set_id",
    "share_ids",
    // Credentials and session secrets.
    "secret",
    "secret_key",
    "private_key",
    "committee_secret",
    "ciphertext",
    "api_key",
    "authorization",
    "password",
];

/// True when `key` names a field whose value must be redacted.
///
/// Boundary-aware substring matching on `_`-separated tokens, so `deck_root`
/// is NOT flagged by `secret`, `dealt_indices` is NOT flagged by `card`, but
/// `player_card_positions` IS flagged by `card`.
pub fn is_sensitive_key(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() {
        return false;
    }
    SENSITIVE_FIELD_KEYS.iter().any(|needle| {
        let needle = *needle;
        let needle = needle.trim().to_ascii_lowercase();
        key == needle
            || key.starts_with(&format!("{needle}_"))
            || key.ends_with(&format!("_{needle}"))
            || key.contains(&format!("_{needle}_"))
    })
}

fn sensitive_key_alt() -> String {
    SENSITIVE_FIELD_KEYS
        .iter()
        .map(|k| regex::escape(k))
        .collect::<Vec<_>>()
        .join("|")
}

fn sensitive_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let keys = sensitive_key_alt();
        // `key="..."` (Debug), `key=[...]` (Vec/array Debug) or `key=value`.
        // The value alternatives deliberately stop at whitespace or a closing
        // bracket so surrounding log text (timestamps, other fields) survives.
        Regex::new(&format!(
            r#"(?i)(\b(?:{keys})\b\s*=)(?:"[^"]*"|\[[^\]\n]*\]|[^=\s][^,\s]*)"#
        ))
        .expect("invalid redaction regex")
    })
}

/// JSON-colon variant: `"hole_cards":"..."` / `"hole_cards":[...]`. Used when a
/// line starts like JSON but fails to parse (malformed/truncated), so it can
/// still be scrubbed before it reaches the sink.
fn sensitive_jsonish_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let keys = sensitive_key_alt();
        Regex::new(&format!(
            r#"(?i)("(?:{keys})"\s*:)(?:"[^"]*"|\[[^\]\n]*\]|[^,\]\s][^,\]]*)"#
        ))
        .expect("invalid json-ish redaction regex")
    })
}

fn redact_pretty(line: &str) -> String {
    let pretty = sensitive_pattern();
    let jsonish = sensitive_jsonish_pattern();
    let jsonish_first = jsonish.replace_all(line, |caps: &regex::Captures| {
        // Keep the key/colon prefix, force a quoted `[REDACTED]` value so the
        // result stays valid JSON in the common case.
        format!("{}\"[REDACTED]\"", &caps[1])
    });
    pretty
        .replace_all(&jsonish_first, |caps: &regex::Captures| {
            // caps[1] = `key=`; the value is the rest of group 0.
            format!("{}[REDACTED]", &caps[1])
        })
        .into_owned()
}

fn redact_json_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if is_sensitive_key(&k) {
                    out.insert(k, serde_json::Value::String("[REDACTED]".to_string()));
                } else {
                    out.insert(k, redact_json_value(v));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(redact_json_value).collect())
        }
        other => other,
    }
}

/// Redact a single log line.
///
/// Structured JSON lines are parsed and walked; any other format goes through
/// the pretty redaction regex. Unparseable JSON falls back to the regex path
/// so a malformed line can never leak a value.
pub fn redact_line(line: &str) -> String {
    let trimmed = line.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            match serde_json::to_string(&redact_json_value(value)) {
                Ok(redacted) => return redacted,
                Err(_) => {}
            }
        }
    }
    redact_pretty(line)
}

/// A `Write` wrapper that redacts every line before handing it to the
/// underlying sink.
pub struct RedactingWriter<W: Write> {
    inner: W,
}

impl<W: Write> RedactingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.inner.write_all(redact_line(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// A `MakeWriter` that wraps the configured logging sink with redaction.
#[derive(Clone, Copy, Debug, Default)]
pub struct RedactingMakeWriter<W> {
    inner: W,
}

impl<W> RedactingMakeWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<'a, W: MakeWriter<'a>> MakeWriter<'a> for RedactingMakeWriter<W> {
    type Writer = RedactingWriter<W::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter::new(self.inner.make_writer())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_matching_keeps_public_fields() {
        assert!(is_sensitive_key("hole_cards"));
        assert!(is_sensitive_key("player_card_positions"));
        assert!(is_sensitive_key("salts"));
        assert!(is_sensitive_key("secret_key"));
        assert!(is_sensitive_key("committee_secret"));
        // Public commitments / on-chain state must keep flowing.
        assert!(!is_sensitive_key("deck_root"));
        assert!(!is_sensitive_key("hand_commitments"));
        assert!(!is_sensitive_key("board_indices"));
        assert!(!is_sensitive_key("dealt_indices"));
        assert!(!is_sensitive_key("session_id"));
    }

    #[test]
    fn redacts_pretty_debug_quoted() {
        let line =
            r#"INFO request completed: request_id=a method=GET hole_cards="[1, 2]" status=200"#;
        let out = redact_line(line);
        assert!(out.contains("hole_cards=[REDACTED]"), "got: {out}");
        assert!(!out.contains("[1, 2]"), "got: {out}");
        assert!(out.contains("method=GET"));
    }

    #[test]
    fn redacts_pretty_bracket_values() {
        let line = r#"INFO deal: table_id=1 salts=[ab12cd, 34ef56] shares=[[...]]"#;
        let out = redact_line(line);
        assert!(out.contains("salts=[REDACTED]"), "got: {out}");
        assert!(out.contains("shares=[REDACTED]"), "got: {out}");
        assert!(!out.contains("ab12cd"), "got: {out}");
        assert!(!out.contains("34ef56"), "got: {out}");
    }

    #[test]
    fn redacts_json_nested_fields() {
        let line = r#"{"level":"INFO","fields":{"message":"deal ok","hole_cards":"[3, 45]","salts":["abc"],"deck_root":"0x123","nested":{"salt":"vvv"}},"target":"api"}"#;
        let out = redact_line(line);
        assert!(out.contains("[REDACTED]"), "got: {out}");
        assert!(!out.contains("\"[3, 45]\""), "got: {out}");
        assert!(!out.contains("abc"), "got: {out}");
        assert!(!out.contains("vvv"), "got: {out}");
        // Public fields survive inside the same JSON object.
        assert!(out.contains("0x123"), "got: {out}");
        // Structure is preserved and re-serialisable.
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed["fields"]["hole_cards"],
            serde_json::Value::String("[REDACTED]".to_string())
        );
    }

    #[test]
    fn sensitive_values_cannot_leak_through_json_values() {
        let line = r#"{"fields":{"cards":{"card1":"Ah","card2":"Kd"}}}"#;
        let out = redact_line(line);
        assert!(!out.contains("Ah"), "got: {out}");
        assert!(!out.contains("Kd"), "got: {out}");
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn writer_wraps() {
        let mut buf = Vec::new();
        let line = "INFO hole_cards=\"[9, 9]\" salts=xyz";
        RedactingWriter::new(&mut buf)
            .write_all(line.as_bytes())
            .unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(!out.contains("[9, 9]"), "got: {out}");
        assert!(!out.contains("xyz"), "got: {out}");
    }

    #[test]
    fn unparseable_json_falls_back_to_pretty() {
        // A truncated/weird JSON-ish line must still have sensitive tokens removed.
        let line = r#"{"fields":{"hole_cards":"[1, 2" garbage"#;
        let out = redact_line(line);
        assert!(!out.contains("[1, 2"), "got: {out}");
        assert!(out.contains("[REDACTED]"), "got: {out}");
    }
}
