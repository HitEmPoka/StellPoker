import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  STACK_TREND_HANDS,
  loadStackTrends,
  recordStacks,
  saveStackTrends,
  sparklinePoints,
  type StackTrends,
} from "@/lib/stack-trend";

const ALICE = "GALICE";
const BOB = "GBOB";

describe("recordStacks (Issue #157)", () => {
  it("starts a trend for each seated player", () => {
    const trends = recordStacks({}, 1, [
      { address: ALICE, stack: 100 },
      { address: BOB, stack: 80 },
    ]);
    expect(trends).toEqual({
      [ALICE]: [{ handNumber: 1, stack: 100 }],
      [BOB]: [{ handNumber: 1, stack: 80 }],
    });
  });

  it("adds one point per hand", () => {
    let trends: StackTrends = {};
    trends = recordStacks(trends, 1, [{ address: ALICE, stack: 100 }]);
    trends = recordStacks(trends, 2, [{ address: ALICE, stack: 140 }]);
    expect(trends[ALICE].map((p) => p.stack)).toEqual([100, 140]);
  });

  it("overwrites the current hand instead of duplicating it", () => {
    let trends = recordStacks({}, 3, [{ address: ALICE, stack: 90 }]);
    trends = recordStacks(trends, 3, [{ address: ALICE, stack: 150 }]);
    expect(trends[ALICE]).toEqual([{ handNumber: 3, stack: 150 }]);
  });

  it("returns the same object when nothing changed", () => {
    const trends = recordStacks({}, 1, [{ address: ALICE, stack: 100 }]);
    expect(recordStacks(trends, 1, [{ address: ALICE, stack: 100 }])).toBe(trends);
  });

  it("keeps only the most recent hands", () => {
    let trends: StackTrends = {};
    for (let hand = 1; hand <= STACK_TREND_HANDS + 3; hand++) {
      trends = recordStacks(trends, hand, [{ address: ALICE, stack: hand * 10 }]);
    }
    expect(trends[ALICE]).toHaveLength(STACK_TREND_HANDS);
    expect(trends[ALICE][0].handNumber).toBe(4);
    expect(trends[ALICE][STACK_TREND_HANDS - 1].handNumber).toBe(STACK_TREND_HANDS + 3);
  });

  it("leaves players missing from an update untouched", () => {
    const before = recordStacks({}, 1, [{ address: BOB, stack: 80 }]);
    const after = recordStacks(before, 2, [{ address: ALICE, stack: 100 }]);
    expect(after[BOB]).toBe(before[BOB]);
  });
});

describe("sparklinePoints (Issue #157)", () => {
  it("spreads points across the width, oldest on the left", () => {
    const points = sparklinePoints([10, 20, 30], 40, 12, 0);
    expect(points.map((p) => p.x)).toEqual([0, 20, 40]);
  });

  it("puts the highest stack at the top and the lowest at the bottom", () => {
    const [low, high] = sparklinePoints([10, 30], 40, 12, 1);
    expect(high.y).toBe(1);
    expect(low.y).toBe(11);
  });

  it("draws a flat series along the middle", () => {
    expect(sparklinePoints([50, 50, 50], 40, 12).every((p) => p.y === 6)).toBe(true);
  });

  it("returns nothing for an empty series", () => {
    expect(sparklinePoints([], 40, 12)).toEqual([]);
  });
});

describe("stack trend storage (Issue #157)", () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => { store.set(key, value); },
      removeItem: (key: string) => { store.delete(key); },
      clear: () => store.clear(),
      length: 0,
      key: () => null,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("round-trips a table's trends, per table", () => {
    const trends = recordStacks({}, 1, [{ address: ALICE, stack: 100 }]);
    saveStackTrends(701, trends);
    expect(loadStackTrends(701)).toEqual(trends);
    expect(loadStackTrends(702)).toEqual({});
  });

  it("ignores corrupted storage", () => {
    window.localStorage.setItem("stellpoker:stack-trend:703", "{not json");
    expect(loadStackTrends(703)).toEqual({});
  });
});
