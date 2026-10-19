import { describe, expect, it } from "vitest";
import {
  CHAT_MAX_ALIAS_LENGTH,
  CHAT_MAX_MESSAGE_LENGTH,
  createChatRateLimiter,
  parseIncomingChatFrame,
  sanitizeChatAlias,
  sanitizeChatText,
} from "@/lib/chat-sanitize";

describe("sanitizeChatText", () => {
  it.each([
    ["<script>alert(1)</script>hi", "hi"],
    ["<img src=x onerror=alert(1)>", ""],
    ['<a href="javascript:alert(1)">click</a>', "click"],
    ["<b>bold</b> move", "bold move"],
    ["<svg><script>alert(1)</script></svg>gg", "gg"],
    ["&lt;script&gt;alert(1)&lt;/script&gt;", "scriptalert(1)/script"],
    ["<style>body{display:none}</style>ok", "ok"],
  ])("strips markup from %s", (input, expected) => {
    expect(sanitizeChatText(input)).toBe(expected);
  });

  it("keeps ordinary text and punctuation", () => {
    // Angle brackets are dropped even when they are not part of a tag.
    expect(sanitizeChatText("nice hand! all-in? 2 > 1 & 1 < 2")).toBe(
      "nice hand! all-in? 2 1 & 1 2",
    );
  });

  it("removes control, zero-width and bidi-override characters", () => {
    expect(sanitizeChatText("gg‮evil​\u0007")).toBe("ggevil");
  });

  it("collapses whitespace to a single line", () => {
    expect(sanitizeChatText("  hello \n\n  world\t ")).toBe("hello world");
  });

  it("truncates by characters without splitting emoji", () => {
    const long = "🎉".repeat(CHAT_MAX_MESSAGE_LENGTH + 10);
    const out = sanitizeChatText(long);
    expect(Array.from(out)).toHaveLength(CHAT_MAX_MESSAGE_LENGTH);
    expect(out).toBe("🎉".repeat(CHAT_MAX_MESSAGE_LENGTH));
  });

  it("returns an empty string for non-string input", () => {
    expect(sanitizeChatText(42)).toBe("");
    expect(sanitizeChatText({ toString: () => "<b>x</b>" })).toBe("");
  });
});

describe("sanitizeChatAlias", () => {
  it("limits aliases and falls back to the seat label", () => {
    expect(Array.from(sanitizeChatAlias("x".repeat(50), 1))).toHaveLength(
      CHAT_MAX_ALIAS_LENGTH,
    );
    expect(sanitizeChatAlias("<img src=x onerror=alert(1)>", 3)).toBe("Seat 3");
  });
});

describe("parseIncomingChatFrame", () => {
  it("sanitizes a text frame", () => {
    expect(
      parseIncomingChatFrame(
        JSON.stringify({ seat_index: 2, alias: "<i>Ann</i>", text: "<b>hi</b>" }),
      ),
    ).toEqual({ seatIndex: 2, alias: "Ann", text: "hi" });
  });

  it("accepts allowlisted emotes only", () => {
    expect(parseIncomingChatFrame({ seat_index: 1, alias: "Bo", emote: "🎉" })).toEqual({
      seatIndex: 1,
      alias: "Bo",
      emote: "🎉",
    });
    expect(
      parseIncomingChatFrame({ seat_index: 1, alias: "Bo", emote: "<img src=x>" }),
    ).toBeNull();
  });

  it.each([
    "not json",
    JSON.stringify(null),
    JSON.stringify({ alias: "a", text: "hi" }),
    JSON.stringify({ seat_index: -1, text: "hi" }),
    JSON.stringify({ seat_index: 1.5, text: "hi" }),
    JSON.stringify({ seat_index: 99, text: "hi" }),
    JSON.stringify({ seat_index: "0", text: "hi" }),
    JSON.stringify({ seat_index: 0, text: "<script>x</script>" }),
  ])("rejects %s", (raw) => {
    expect(parseIncomingChatFrame(raw)).toBeNull();
  });
});

describe("createChatRateLimiter", () => {
  it("allows maxMessages per window, then recovers", () => {
    let t = 0;
    const limiter = createChatRateLimiter({ maxMessages: 2, windowMs: 1000 }, () => t);
    expect(limiter.tryConsume()).toBe(true);
    expect(limiter.tryConsume()).toBe(true);
    expect(limiter.tryConsume()).toBe(false);
    t = 999;
    expect(limiter.tryConsume()).toBe(false);
    t = 1000;
    expect(limiter.tryConsume()).toBe(true);
  });

  it("rejects rapid duplicate messages without consuming the whole window", () => {
    let t = 0;
    const limiter = createChatRateLimiter({ maxMessages: 5, windowMs: 10_000 }, () => t);
    expect(limiter.tryConsume("hello")).toBe(true);
    t = 100;
    expect(limiter.tryConsume("hello")).toBe(false);
    t = 600;
    expect(limiter.tryConsume("different")).toBe(true);
  });
});
