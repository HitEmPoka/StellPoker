/**
 * Table chat hardening (Issue #140).
 *
 * Every chat field that came from another player is untrusted. It is run
 * through DOMPurify with no allowed tags, reduced to plain text and rendered
 * only as a React text node — never via `dangerouslySetInnerHTML`. The
 * coordinator applies the same rules server-side (`services/coordinator/src/chat.rs`),
 * so the limits below must stay in sync with it.
 */
import DOMPurify from "dompurify";

/** Maximum characters in one chat message (matches `MAX_CHAT_TEXT_CHARS`). */
export const CHAT_MAX_MESSAGE_LENGTH = 128;
/** Maximum characters in a sender alias (matches `MAX_ALIAS_CHARS`). */
export const CHAT_MAX_ALIAS_LENGTH = 24;
/** Highest seat index a chat frame may claim (matches `MAX_SEAT_INDEX`). */
export const CHAT_MAX_SEAT_INDEX = 9;
/** The only emotes the server relays (matches `CHAT_EMOTES`). */
export const CHAT_EMOTES = ["😃", "😢", "😠", "😎", "🤔", "🎉"] as const;
/** Messages a client may send per window (the server enforces the same). */
export const CHAT_RATE_LIMIT = { maxMessages: 5, windowMs: 10_000 } as const;
export const CHAT_MIN_INTERVAL_MS = 500;
export const CHAT_DUPLICATE_WINDOW_MS = 2_000;

export type ChatEmote = (typeof CHAT_EMOTES)[number];

/**
 * Control, zero-width and bidi-override characters. They are invisible but
 * can hide or reorder text (e.g. spoofing another player's alias), so they
 * are removed rather than rendered.
 */
const INVISIBLE_OR_CONTROL =
  /[\u0000-\u0008\u000B-\u001F\u007F-\u009F​-‏‪-‮⁠-⁤⁦-⁩﻿]/g;

/** Strip every tag with DOMPurify, keeping only the text content. */
function stripMarkup(raw: string): string {
  if (typeof window !== "undefined" && DOMPurify.isSupported) {
    const fragment = DOMPurify.sanitize(raw, {
      ALLOWED_TAGS: [],
      ALLOWED_ATTR: [],
      KEEP_CONTENT: true,
      RETURN_DOM_FRAGMENT: true,
    });
    return fragment.textContent ?? "";
  }
  // No DOM (server render): drop anything tag-shaped.
  return raw.replace(/<[^>]*>?/g, "");
}

/**
 * Reduce untrusted input to safe, single-line plain text of at most
 * `maxLength` characters (code points, so emoji are never split).
 */
export function sanitizeChatText(
  raw: unknown,
  maxLength: number = CHAT_MAX_MESSAGE_LENGTH,
): string {
  if (typeof raw !== "string") return "";
  const text = stripMarkup(raw)
    .replace(INVISIBLE_OR_CONTROL, "")
    // Entities decoded by the parser (e.g. `&lt;`) must not come back as markup.
    .replace(/[<>]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return Array.from(text).slice(0, maxLength).join("");
}

export function sanitizeChatAlias(raw: unknown, seatIndex: number): string {
  return sanitizeChatText(raw, CHAT_MAX_ALIAS_LENGTH) || `Seat ${seatIndex}`;
}

export function isChatEmote(value: unknown): value is ChatEmote {
  return typeof value === "string" && (CHAT_EMOTES as readonly string[]).includes(value);
}

export interface ChatFrame {
  seatIndex: number;
  alias: string;
  text?: string;
  emote?: ChatEmote;
}

/**
 * Validate and sanitize a frame received on the chat WebSocket. Returns
 * `null` for anything malformed: bad JSON, an out-of-range seat, an unknown
 * emote, or a frame with nothing left to show after sanitizing.
 */
export function parseIncomingChatFrame(raw: unknown): ChatFrame | null {
  let data: unknown = raw;
  if (typeof raw === "string") {
    try {
      data = JSON.parse(raw);
    } catch {
      return null;
    }
  }
  if (typeof data !== "object" || data === null) return null;
  const record = data as Record<string, unknown>;

  const seatIndex = record.seat_index;
  if (
    typeof seatIndex !== "number" ||
    !Number.isInteger(seatIndex) ||
    seatIndex < 0 ||
    seatIndex > CHAT_MAX_SEAT_INDEX
  ) {
    return null;
  }

  const frame: ChatFrame = {
    seatIndex,
    alias: sanitizeChatAlias(record.alias, seatIndex),
  };
  const text = sanitizeChatText(record.text);
  if (text) frame.text = text;
  if (record.emote !== undefined) {
    if (!isChatEmote(record.emote)) return null;
    frame.emote = record.emote;
  }
  return frame.text || frame.emote ? frame : null;
}

/**
 * Sliding-window limiter for outgoing chat. `tryConsume` returns `false`
 * once `maxMessages` have been sent within the last `windowMs`.
 */
export function createChatRateLimiter(
  { maxMessages, windowMs }: { maxMessages: number; windowMs: number } = CHAT_RATE_LIMIT,
  now: () => number = Date.now,
) {
  const sent: number[] = [];
  let lastMessage = "";
  let lastMessageAt = -Infinity;
  return {
    tryConsume(message?: string): boolean {
      const t = now();
      while (sent.length > 0 && t - sent[0] >= windowMs) sent.shift();
      if (sent.length >= maxMessages) return false;
      if (message && message === lastMessage && t - lastMessageAt < CHAT_DUPLICATE_WINDOW_MS) return false;
      if (message && sent.length > 0 && t - sent[sent.length - 1] < CHAT_MIN_INTERVAL_MS) return false;
      sent.push(t);
      if (message) { lastMessage = message; lastMessageAt = t; }
      return true;
    },
  };
}
