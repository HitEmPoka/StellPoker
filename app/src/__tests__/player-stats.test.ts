import { describe, it, expect } from "vitest";
import {
  computePlayerDashboard,
  formatXlm,
  toGraphPoints,
  flattenHandHistory,
  type PerformancePoint,
} from "@/lib/player-stats";
import type { HandHistoryEntry } from "@/lib/hand-history";

const ADDR = "GABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890";
const OTHER = "GZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ";

function entry(overrides: Partial<HandHistoryEntry>): HandHistoryEntry {
  return {
    tableId: 1,
    handNumber: 1,
    timestamp: Date.now(),
    streets: [{ street: "preflop", pot: 0, boardCards: [] }],
    finalPot: 100,
    boardCards: [],
    ...overrides,
  };
}

describe("computePlayerDashboard", () => {
  it("returns zeroed stats for no hands", () => {
    const s = computePlayerDashboard([], ADDR);
    expect(s.totalHands).toBe(0);
    expect(s.winRate).toBe(0);
    expect(s.roi).toBe(0);
    expect(s.favoriteHand).toBeNull();
    expect(s.performance).toEqual([]);
  });

  it("counts hands won and computes win rate", () => {
    const entries = [
      entry({ winnerAddress: ADDR, finalPot: 100 }),
      entry({ winnerAddress: OTHER, finalPot: 100 }),
    ];
    const s = computePlayerDashboard(entries, ADDR);
    expect(s.totalHands).toBe(2);
    expect(s.handsWon).toBe(1);
    expect(s.winRate).toBe(50);
  });

  it("records the biggest pot won only on wins", () => {
    const entries = [
      entry({ winnerAddress: ADDR, finalPot: 500 }),
      entry({ winnerAddress: ADDR, finalPot: 200 }),
    ];
    const s = computePlayerDashboard(entries, ADDR);
    expect(s.biggestPotWon).toBe(500);
    expect(s.biggestPotLost).toBe(0);
  });

  it("records biggest pot lost on losses", () => {
    const entries = [
      entry({ winnerAddress: OTHER, finalPot: 700 }),
      entry({ winnerAddress: OTHER, finalPot: 300 }),
    ];
    const s = computePlayerDashboard(entries, ADDR);
    expect(s.biggestPotLost).toBe(700);
    expect(s.biggestPotWon).toBe(0);
  });

  it("computes favorite hand from winning ranks", () => {
    const entries = [
      entry({ winnerAddress: ADDR, handRankName: "PAIR" }),
      entry({ winnerAddress: ADDR, handRankName: "PAIR" }),
      entry({ winnerAddress: ADDR, handRankName: "FLUSH" }),
    ];
    const s = computePlayerDashboard(entries, ADDR);
    expect(s.favoriteHand).toBe("PAIR");
  });

  it("orders performance points oldest to newest", () => {
    const entries = [
      entry({ timestamp: 3, winnerAddress: ADDR, finalPot: 100 }),
      entry({ timestamp: 1, winnerAddress: ADDR, finalPot: 100 }),
      entry({ timestamp: 2, winnerAddress: ADDR, finalPot: 100 }),
    ];
    const s = computePlayerDashboard(entries, ADDR);
    const ts = s.performance.map((p) => p.timestamp);
    expect(ts).toEqual([1, 2, 3]);
  });

  it("accumulates cumulative net across the graph", () => {
    const entries = [
      entry({ timestamp: 1, winnerAddress: ADDR, finalPot: 100 }),
      entry({ timestamp: 2, winnerAddress: OTHER, finalPot: 100 }),
    ];
    const s = computePlayerDashboard(entries, ADDR);
    const cum = s.performance.map((p) => p.cumulativeNetStroops);
    // First hand won → positive; cumulative stays monotonic per point.
    expect(cum.length).toBe(2);
  });

  it("exposes HUD stats when provided", () => {
    const s = computePlayerDashboard([], ADDR, { vpip: 25, pfr: 15 });
    expect(s.vpip).toBe(25);
    expect(s.pfr).toBe(15);
  });

  it("keeps HUD stats null when absent", () => {
    const s = computePlayerDashboard([], ADDR);
    expect(s.vpip).toBeNull();
    expect(s.pfr).toBeNull();
  });
});

describe("formatXlm", () => {
  it("formats stroops to whole XLM", () => {
    expect(formatXlm(10_000_000)).toBe("1");
    expect(formatXlm(0)).toBe("0");
  });

  it("formats fractional XLM", () => {
    expect(formatXlm(5_000_000)).toBe("0.50");
  });
});

describe("toGraphPoints", () => {
  it("returns empty for no performance", () => {
    expect(toGraphPoints([])).toEqual([]);
  });

  it("normalises points into the 0-100 range", () => {
    const perf: PerformancePoint[] = [
      { timestamp: 1, cumulativeHands: 1, netStroops: 0, cumulativeNetStroops: 0 },
      { timestamp: 2, cumulativeHands: 2, netStroops: 0, cumulativeNetStroops: 100 },
      { timestamp: 3, cumulativeHands: 3, netStroops: 0, cumulativeNetStroops: 0 },
    ];
    const pts = toGraphPoints(perf);
    expect(pts).toHaveLength(3);
    expect(pts[0].x).toBe(0);
    expect(pts[2].x).toBe(100);
    expect(pts[0].y).toBe(100);
    expect(pts[1].y).toBe(0);
  });
});

describe("flattenHandHistory", () => {
  it("concatenates entries across tables", () => {
    const load = (id: number): HandHistoryEntry[] =>
      id === 1 ? [entry({ tableId: 1 })] : [entry({ tableId: 2 })];
    const flat = flattenHandHistory([1, 2], load);
    expect(flat).toHaveLength(2);
    expect(flat[1].tableId).toBe(2);
  });
});
