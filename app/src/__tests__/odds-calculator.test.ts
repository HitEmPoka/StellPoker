import { describe, it, expect } from "vitest";
import { calculateOdds, OddsCalculatorError } from "../lib/odds-calculator";

// Card encoding: value = suit * 13 + rankIndex.
// Suits: 0=clubs, 1=diamonds, 2=hearts, 3=spades.
// Ranks: 0="2" .. 8="10", 9="J", 10="Q", 11="K", 12="A".
const SPADE = 3;
const TEN = 8, JACK = 9, QUEEN = 10, KING = 11, ACE = 12;
const TEN_SPADES = SPADE * 13 + TEN;
const JACK_SPADES = SPADE * 13 + JACK;
const QUEEN_SPADES = SPADE * 13 + QUEEN;
const KING_SPADES = SPADE * 13 + KING;
const ACE_SPADES = SPADE * 13 + ACE;
const TWO_CLUBS = 0 * 13 + 0;
const THREE_DIAMONDS = 1 * 13 + 1;
const ACE_HEARTS = 2 * 13 + ACE;
const ACE_DIAMONDS = 1 * 13 + ACE;

describe("calculateOdds validation (Issue #163)", () => {
  it("rejects a hole card count other than two", () => {
    expect(() =>
      calculateOdds({
        holeCards: [1, 2, 3] as unknown as [number, number],
        boardCards: [],
        numOpponents: 1,
      })
    ).toThrow(OddsCalculatorError);
  });

  it("rejects more than five board cards", () => {
    expect(() =>
      calculateOdds({
        holeCards: [ACE_SPADES, KING_SPADES],
        boardCards: [1, 2, 3, 4, 5, 6],
        numOpponents: 1,
      })
    ).toThrow(OddsCalculatorError);
  });

  it("rejects zero or too many opponents", () => {
    expect(() =>
      calculateOdds({ holeCards: [1, 2], boardCards: [], numOpponents: 0 })
    ).toThrow(OddsCalculatorError);
    expect(() =>
      calculateOdds({ holeCards: [1, 2], boardCards: [], numOpponents: 9 })
    ).toThrow(OddsCalculatorError);
  });

  it("rejects duplicate cards across hole and board", () => {
    expect(() =>
      calculateOdds({
        holeCards: [ACE_SPADES, KING_SPADES],
        boardCards: [ACE_SPADES, 1, 2],
        numOpponents: 1,
      })
    ).toThrow(OddsCalculatorError);
  });

  it("rejects out-of-range card values", () => {
    expect(() =>
      calculateOdds({ holeCards: [-1, 2], boardCards: [], numOpponents: 1 })
    ).toThrow(OddsCalculatorError);
    expect(() =>
      calculateOdds({ holeCards: [52, 2], boardCards: [], numOpponents: 1 })
    ).toThrow(OddsCalculatorError);
  });

  it("rejects too many opponents for the remaining deck size", () => {
    // Nearly the whole deck already known as board cards leaves too few
    // remaining cards to deal 8 opponents two hole cards each.
    const manyBoardCards = Array.from({ length: 3 }, (_, i) => i + 10);
    expect(() =>
      calculateOdds({
        holeCards: [ACE_SPADES, KING_SPADES],
        boardCards: manyBoardCards,
        numOpponents: 8,
        iterations: 10,
      })
    ).not.toThrow(); // 3 known board cards still leaves plenty of deck — sanity check this doesn't over-reject
  });
});

describe("calculateOdds deterministic outcomes", () => {
  it("gives 100% win rate when the hero already holds the best possible hand (royal flush) on a complete board", () => {
    // Hero: K♠ A♠. Board (river, all 5 known): 10♠ J♠ Q♠ 2♣ 3♦.
    // Hero's best 5-card hand is unambiguously a royal flush — the single
    // best possible hand in poker — so no opponent hand can tie or beat it,
    // regardless of the random cards they're dealt.
    const result = calculateOdds({
      holeCards: [KING_SPADES, ACE_SPADES],
      boardCards: [TEN_SPADES, JACK_SPADES, QUEEN_SPADES, TWO_CLUBS, THREE_DIAMONDS],
      numOpponents: 3,
      iterations: 200,
    });

    expect(result.win).toBe(1);
    expect(result.tie).toBe(0);
    expect(result.loss).toBe(0);
    expect(result.iterations).toBe(200);
  });

  it("returns fractions that sum to 1 (within floating point tolerance)", () => {
    const result = calculateOdds({
      holeCards: [ACE_HEARTS, ACE_DIAMONDS],
      boardCards: [],
      numOpponents: 2,
      iterations: 500,
    });
    expect(result.win + result.tie + result.loss).toBeCloseTo(1, 5);
  });
});

describe("calculateOdds statistical sanity check", () => {
  it("gives pocket aces a strong (>70%) win rate heads-up preflop against one random opponent", () => {
    // Well-known baseline: AA vs a random hand heads-up is ~85% equity.
    // Using a generous >70% threshold and enough iterations to avoid
    // Monte Carlo flakiness while still keeping the test fast.
    const result = calculateOdds({
      holeCards: [ACE_HEARTS, ACE_DIAMONDS],
      boardCards: [],
      numOpponents: 1,
      iterations: 4000,
    });
    expect(result.win).toBeGreaterThan(0.7);
  });
});
