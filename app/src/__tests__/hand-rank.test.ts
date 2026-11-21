import { describe, expect, it } from "vitest";
import { bestHandRank, compareHandRank, type HandRank } from "@/lib/hand-rank";
function spade(rankIndex: number): number {
  return 3 * 13 + rankIndex;
}
function heart(rankIndex: number): number {
  return 2 * 13 + rankIndex;
}
function diamond(rankIndex: number): number {
  return 1 * 13 + rankIndex;
}
function club(rankIndex: number): number {
  return 0 * 13 + rankIndex;
}

// rankIndex: 0="2" ... 12="A"
const A = 12;
const K = 11;
const Q = 10;
const J = 9;
const TEN = 8;

describe("bestHandRank", () => {
  it("returns null for fewer than 5 cards", () => {
    expect(bestHandRank([spade(A), heart(K)])).toBeNull();
    expect(bestHandRank([])).toBeNull();
  });

  it("classifies a royal straight flush", () => {
    const hand = bestHandRank([spade(A), spade(K), spade(Q), spade(J), spade(TEN)]);
    expect(hand).toEqual({ category: 9, name: "Straight Flush", tiebreak: [14] });
  });

  it("classifies a wheel straight (A-2-3-4-5) as a 5-high straight", () => {
    const hand = bestHandRank([spade(A), heart(0), diamond(1), club(2), heart(3)]);
    expect(hand?.category).toBe(5);
    expect(hand?.name).toBe("Straight");
    expect(hand?.tiebreak).toEqual([5]);
  });

  it("classifies four of a kind with kicker", () => {
    const hand = bestHandRank([heart(A), diamond(A), club(A), spade(A), heart(K)]);
    expect(hand).toEqual({ category: 8, name: "Four of a Kind", tiebreak: [14, 13] });
  });

  it("classifies a full house", () => {
    const hand = bestHandRank([heart(K), diamond(K), club(K), heart(Q), diamond(Q)]);
    expect(hand).toEqual({ category: 7, name: "Full House", tiebreak: [13, 12] });
  });

  it("classifies a flush", () => {
    const hand = bestHandRank([heart(A), heart(K), heart(Q), heart(J), heart(6)]);
    expect(hand?.category).toBe(6);
    expect(hand?.name).toBe("Flush");
    expect(hand?.tiebreak).toEqual([14, 13, 12, 11, 8]);
  });

  it("classifies a straight", () => {
    const hand = bestHandRank([heart(A), diamond(K), club(Q), spade(J), heart(TEN)]);
    expect(hand).toEqual({ category: 5, name: "Straight", tiebreak: [14] });
  });

  it("classifies three of a kind with kickers", () => {
    const hand = bestHandRank([heart(Q), diamond(Q), club(Q), spade(K), diamond(5)]);
    expect(hand).toEqual({
      category: 4,
      name: "Three of a Kind",
      tiebreak: [12, 13, 7],
    });
  });

  it("classifies two pair", () => {
    const hand = bestHandRank([heart(K), diamond(K), heart(Q), diamond(Q), spade(6)]);
    expect(hand).toEqual({
      category: 3,
      name: "Two Pair",
      tiebreak: [13, 12, 8],
    });
  });

  it("classifies a pair", () => {
    const hand = bestHandRank([heart(A), diamond(A), spade(K), heart(Q), diamond(6)]);
    expect(hand).toEqual({
      category: 2,
      name: "Pair",
      tiebreak: [14, 13, 12, 8],
    });
  });

  it("classifies high card", () => {
    const hand = bestHandRank([heart(A), diamond(K), club(Q), spade(J), heart(7)]);
    expect(hand).toEqual({
      category: 1,
      name: "High Card",
      tiebreak: [14, 13, 12, 11, 9],
    });
  });

  it("picks the best 5-card combination out of 7 cards", () => {
    // 7 cards containing a flush; bestHandRank must return the flush, not a
    // lower 5-card subset.
    const hand = bestHandRank([
      heart(A), heart(K), heart(Q), heart(J), heart(9), // flush hearts
      spade(2), spade(3),
    ]);
    expect(hand?.category).toBe(6);
  });
});

describe("compareHandRank", () => {
  it("ranks higher category above lower category", () => {
    const straightFlush: HandRank = { category: 9, name: "Straight Flush", tiebreak: [14] };
    const fourOfAKind: HandRank = { category: 8, name: "Four of a Kind", tiebreak: [14, 13] };
    expect(compareHandRank(straightFlush, fourOfAKind)).toBeGreaterThan(0);
    expect(compareHandRank(fourOfAKind, straightFlush)).toBeLessThan(0);
  });

  it("breaks ties by descending tiebreak values", () => {
    const highAce: HandRank = { category: 1, name: "High Card", tiebreak: [14, 13, 12, 11, 10] };
    const highKing: HandRank = { category: 1, name: "High Card", tiebreak: [13, 12, 11, 10, 9] };
    expect(compareHandRank(highAce, highKing)).toBeGreaterThan(0);
    expect(compareHandRank(highKing, highAce)).toBeLessThan(0);
  });

  it("returns 0 for identical hands", () => {
    const a: HandRank = { category: 5, name: "Straight", tiebreak: [14] };
    const b: HandRank = { category: 5, name: "Straight", tiebreak: [14] };
    expect(compareHandRank(a, b)).toBe(0);
  });
});
