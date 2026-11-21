import { describe, expect, it } from "vitest";
import {
  nextTurnThreshold,
  turnStartAnnouncement,
  turnThresholdAnnouncement,
} from "@/lib/turn-countdown";

describe("nextTurnThreshold", () => {
  it("says nothing while plenty of time remains", () => {
    expect(nextTurnThreshold(30, new Set())).toBeNull();
    expect(nextTurnThreshold(11, new Set())).toBeNull();
  });

  it("announces 10 seconds once the clock reaches it", () => {
    expect(nextTurnThreshold(10, new Set())).toBe(10);
  });

  it("does not repeat a threshold already spoken", () => {
    expect(nextTurnThreshold(9, new Set([10]))).toBe(5);
    expect(nextTurnThreshold(9, new Set([10, 5]))).toBeNull();
  });

  it("speaks only the lowest pending threshold when the clock skips several", () => {
    expect(nextTurnThreshold(3, new Set())).toBe(5);
  });

  it("reports time up at zero", () => {
    expect(nextTurnThreshold(0, new Set([10, 5]))).toBe(0);
  });
});

describe("announcement text", () => {
  it("announces the start of a turn with its duration", () => {
    expect(turnStartAnnouncement(30)).toBe("It's your turn. 30 seconds to act.");
  });

  it("says what happens at zero depending on auto-fold", () => {
    expect(turnThresholdAnnouncement(0, true)).toBe("Time is up. Folding automatically.");
    expect(turnThresholdAnnouncement(0, false)).toBe("Time is up.");
  });

  it("counts down at the other thresholds", () => {
    expect(turnThresholdAnnouncement(5, true)).toBe("5 seconds left to act.");
    expect(turnThresholdAnnouncement(10, false)).toBe("10 seconds left to act.");
  });
});
