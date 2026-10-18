//! Table chat hardening (Issue #140).
//!
//! Every frame a client sends on `/api/table/:table_id/chat/ws` is untrusted.
//! Before it is relayed to the table it is:
//!
//! - size-capped ([`MAX_CHAT_FRAME_BYTES`]) and parsed into a fixed schema —
//!   unknown fields are dropped instead of being echoed to every player;
//! - reduced to plain text: tags and angle brackets removed, control,
//!   zero-width and bidi-override characters stripped, whitespace collapsed,
//!   and truncated by *characters* ([`MAX_CHAT_TEXT_CHARS`]), never splitting a
//!   UTF-8 sequence;
//! - checked against the emote allowlist ([`CHAT_EMOTES`]) and seat range;
//! - rate limited per connection ([`ChatRateLimiter`]).
//!
//! The web client applies the same rules with DOMPurify
//! (`app/src/lib/chat-sanitize.ts`); keep the limits in sync.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Maximum characters in one chat message.
pub const MAX_CHAT_TEXT_CHARS: usize = 128;
/// Maximum characters in a sender alias.
pub const MAX_ALIAS_CHARS: usize = 24;
/// Largest raw frame accepted before parsing.
pub const MAX_CHAT_FRAME_BYTES: usize = 2048;
/// Highest seat index a frame may claim.
pub const MAX_SEAT_INDEX: u32 = 9;
/// The only emotes relayed to the table.
pub const CHAT_EMOTES: [&str; 6] = ["😃", "😢", "😠", "😎", "🤔", "🎉"];
/// Frames (messages or emotes) a connection may send per window.
pub const CHAT_RATE_LIMIT_MESSAGES: usize = 5;
/// Sliding window for [`CHAT_RATE_LIMIT_MESSAGES`].
pub const CHAT_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);
/// Minimum spacing between frames from one connection.
pub const CHAT_MIN_INTERVAL: Duration = Duration::from_millis(500);
/// Identical frames are ignored briefly to stop accidental resend loops.
pub const CHAT_DUPLICATE_WINDOW: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct IncomingChatFrame {
    seat_index: u32,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    emote: Option<String>,
}

/// The only shape relayed to other players.
#[derive(Debug, PartialEq, Serialize)]
pub struct OutgoingChatFrame {
    pub seat_index: u32,
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emote: Option<String>,
}

/// Why a frame was dropped instead of relayed.
#[derive(Debug, PartialEq, Eq)]
pub enum ChatRejection {
    TooLarge,
    Malformed,
    InvalidSeat,
    UnknownEmote,
    Empty,
}

impl std::fmt::Display for ChatRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Self::TooLarge => "frame too large",
            Self::Malformed => "malformed frame",
            Self::InvalidSeat => "seat index out of range",
            Self::UnknownEmote => "emote not in allowlist",
            Self::Empty => "nothing to relay after sanitizing",
        };
        f.write_str(reason)
    }
}

/// Invisible characters that can hide or reorder text (e.g. spoofing another
/// player's alias with a right-to-left override).
fn is_invisible_or_control(c: char) -> bool {
    c.is_control() && c != '\n' && c != '\t'
        || matches!(
            c,
            '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// Drops `<...>` tags, keeping the text between them. A `<` that does not
/// start a tag (e.g. `1 < 2`) is dropped on its own; any `>` is dropped too,
/// so no angle bracket ever reaches a client.
fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let starts_tag = chars
                    .peek()
                    .is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!' | '?'));
                if starts_tag {
                    for skipped in chars.by_ref() {
                        if skipped == '>' {
                            break;
                        }
                    }
                }
            }
            '>' => {}
            other => out.push(other),
        }
    }
    out
}

/// Reduce untrusted input to single-line plain text of at most `max_chars`
/// characters.
pub fn sanitize_chat_text(input: &str, max_chars: usize) -> String {
    let stripped = strip_tags(input);
    let visible: String = stripped
        .chars()
        .filter(|c| !is_invisible_or_control(*c))
        .collect();
    visible
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

/// Validate and sanitize one raw frame, returning the JSON to broadcast.
pub fn process_incoming_frame(raw: &str) -> Result<String, ChatRejection> {
    if raw.len() > MAX_CHAT_FRAME_BYTES {
        return Err(ChatRejection::TooLarge);
    }
    let frame: IncomingChatFrame =
        serde_json::from_str(raw).map_err(|_| ChatRejection::Malformed)?;
    if frame.seat_index > MAX_SEAT_INDEX {
        return Err(ChatRejection::InvalidSeat);
    }

    let text = frame
        .text
        .map(|t| sanitize_chat_text(&t, MAX_CHAT_TEXT_CHARS))
        .filter(|t| !t.is_empty());
    let emote = match frame.emote {
        Some(e) if CHAT_EMOTES.contains(&e.as_str()) => Some(e),
        Some(_) => return Err(ChatRejection::UnknownEmote),
        None => None,
    };
    if text.is_none() && emote.is_none() {
        return Err(ChatRejection::Empty);
    }

    let alias = frame
        .alias
        .map(|a| sanitize_chat_text(&a, MAX_ALIAS_CHARS))
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| format!("Seat {}", frame.seat_index));

    let out = OutgoingChatFrame {
        seat_index: frame.seat_index,
        alias,
        text,
        emote,
    };
    serde_json::to_string(&out).map_err(|_| ChatRejection::Malformed)
}

/// Per-connection sliding-window rate limiter.
pub struct ChatRateLimiter {
    max_messages: usize,
    window: Duration,
    sent: VecDeque<Instant>,
    last_payload: Option<(String, Instant)>,
}

impl Default for ChatRateLimiter {
    fn default() -> Self {
        Self::new(CHAT_RATE_LIMIT_MESSAGES, CHAT_RATE_LIMIT_WINDOW)
    }
}

impl ChatRateLimiter {
    pub fn new(max_messages: usize, window: Duration) -> Self {
        Self {
            max_messages,
            window,
            sent: VecDeque::with_capacity(max_messages),
            last_payload: None,
        }
    }

    /// Records a frame at `now`; returns `false` if it exceeds the limit.
    pub fn allow(&mut self, now: Instant) -> bool {
        while self
            .sent
            .front()
            .is_some_and(|t| now.duration_since(*t) >= self.window)
        {
            self.sent.pop_front();
        }
        if self.sent.len() >= self.max_messages {
            return false;
        }
        self.sent.push_back(now);
        true
    }

    pub fn allow_payload(&mut self, now: Instant, payload: &str) -> bool {
        if let Some(previous) = self.sent.back() {
            if now.duration_since(*previous) < CHAT_MIN_INTERVAL { return false; }
        }
        if let Some((previous, at)) = &self.last_payload {
            if previous == payload && now.duration_since(*at) < CHAT_DUPLICATE_WINDOW {
                return false;
            }
        }
        if !self.allow(now) { return false; }
        self.last_payload = Some((payload.to_owned(), now));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay(raw: &str) -> serde_json::Value {
        serde_json::from_str(&process_incoming_frame(raw).expect("frame relayed")).unwrap()
    }

    #[test]
    fn strips_markup_and_script_payloads() {
        for (input, expected) in [
            ("<script>alert(1)</script>hi", "alert(1)hi"),
            ("<img src=x onerror=alert(1)>", ""),
            ("<a href=\"javascript:alert(1)\">click</a>", "click"),
            ("<b>bold</b> move", "bold move"),
            ("2 > 1 & 1 < 2", "2 1 & 1 2"),
        ] {
            assert_eq!(
                sanitize_chat_text(input, MAX_CHAT_TEXT_CHARS),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn output_never_contains_angle_brackets() {
        let out = sanitize_chat_text("<<script>>x<</script>>", MAX_CHAT_TEXT_CHARS);
        assert!(!out.contains('<') && !out.contains('>'), "got {out:?}");
    }

    #[test]
    fn removes_invisible_and_bidi_characters() {
        assert_eq!(
            sanitize_chat_text("gg\u{202E}evil\u{200B}\u{7}", MAX_CHAT_TEXT_CHARS),
            "ggevil"
        );
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(
            sanitize_chat_text("  hello \n\n world\t", MAX_CHAT_TEXT_CHARS),
            "hello world"
        );
    }

    #[test]
    fn truncates_on_char_boundaries() {
        // The old byte-slicing sanitizer panicked when a multi-byte char
        // straddled byte 128.
        let input = format!("{}🎉🎉", "a".repeat(MAX_CHAT_TEXT_CHARS - 1));
        let out = sanitize_chat_text(&input, MAX_CHAT_TEXT_CHARS);
        assert_eq!(out.chars().count(), MAX_CHAT_TEXT_CHARS);
        assert!(out.ends_with('🎉'));
    }

    #[test]
    fn relays_only_known_fields() {
        let out = relay(
            r#"{"seat_index":2,"alias":"<i>Ann</i>","text":"<b>hi</b>","html":"<script>x</script>"}"#,
        );
        assert_eq!(
            out,
            serde_json::json!({"seat_index": 2, "alias": "Ann", "text": "hi"})
        );
    }

    #[test]
    fn defaults_empty_alias_to_seat_label() {
        let out = relay(r#"{"seat_index":3,"alias":"<img src=x>","emote":"🎉"}"#);
        assert_eq!(out["alias"], "Seat 3");
        assert_eq!(out["emote"], "🎉");
    }

    #[test]
    fn rejects_invalid_frames() {
        let too_big = format!(
            r#"{{"seat_index":0,"text":"{}"}}"#,
            "a".repeat(MAX_CHAT_FRAME_BYTES)
        );
        for (raw, reason) in [
            ("not json", ChatRejection::Malformed),
            (r#"{"text":"hi"}"#, ChatRejection::Malformed),
            (r#"{"seat_index":-1,"text":"hi"}"#, ChatRejection::Malformed),
            (
                r#"{"seat_index":"0","text":"hi"}"#,
                ChatRejection::Malformed,
            ),
            (
                r#"{"seat_index":10,"text":"hi"}"#,
                ChatRejection::InvalidSeat,
            ),
            (
                r#"{"seat_index":0,"emote":"<img src=x>"}"#,
                ChatRejection::UnknownEmote,
            ),
            (
                r#"{"seat_index":0,"text":"<script></script>"}"#,
                ChatRejection::Empty,
            ),
            (r#"{"seat_index":0}"#, ChatRejection::Empty),
            (too_big.as_str(), ChatRejection::TooLarge),
        ] {
            assert_eq!(process_incoming_frame(raw), Err(reason), "{raw}");
        }
    }

    #[test]
    fn rate_limiter_uses_a_sliding_window() {
        let start = Instant::now();
        let mut limiter = ChatRateLimiter::new(2, Duration::from_secs(10));
        assert!(limiter.allow(start));
        assert!(limiter.allow(start + Duration::from_secs(1)));
        assert!(!limiter.allow(start + Duration::from_secs(2)));
        assert!(!limiter.allow(start + Duration::from_millis(9_999)));
        assert!(limiter.allow(start + Duration::from_secs(10)));
    }
}
