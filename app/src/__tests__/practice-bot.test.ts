import { describe, it, expect } from "vitest";
import {
  BOT_PROFILES,
  DIFFICULTIES,
  decideBotAction,
  handStrength,
  preflopStrength,
  postflopStrength,
  type BotView,
} from "@/lib/practice-bot";

/** Card encoding matches `cards.ts`: value = suit * 13 + rankIndex. */
const CLUBS = 0;
const DIAMONDS = 13;
const HEARTS = 26;
const SPADES = 39;
/** rankIndex 0 = deuce … 12 = ace. */
const card = (suitBase: number, rankIndex: number) => suitBase + rankIndex;

const ACE_CLUBS = card(CLUBS, 12);
const ACE_SPADES = card(SPADES, 12);
const KING_CLUBS = card(CLUBS, 11);
const SEVEN_CLUBS = card(CLUBS, 5);
const TWO_DIAMONDS = card(DIAMONDS, 0);

function view(overrides: Partial<BotView> = {}): BotView {
  return {
    hole: [ACE_CLUBS, ACE_SPADES],
    board: [],
    currentBet: 0,
    myBet: 0,
    myStack: 1000,
    pot: 30,
    minRaise: 10,
    opponentsLive: 1,
    ...overrides,
  };
}

/** Fixed generator so a "random" branch is pinned in a test. */
const always = (value: number) => () => value;

describe("hand strength", () => {
  it("rates pocket aces at the top of the pre-flop scale", () => {
    expect(preflopStrength([ACE_CLUBS, ACE_SPADES])).toBeCloseTo(1, 5);
  });

  it("rates a low offsuit disconnected hand near the bottom", () => {
    const junk = preflopStrength([SEVEN_CLUBS, TWO_DIAMONDS]);
    expect(junk).toBeLessThan(0.3);
    expect(junk).toBeGreaterThanOrEqual(0);
  });

  it("prefers suited and connected over offsuit and gapped", () => {
    const suitedConnector = preflopStrength([card(HEARTS, 8), card(HEARTS, 7)]);
    const offsuitGapper = preflopStrength([card(HEARTS, 8), card(SPADES, 2)]);
    expect(suitedConnector).toBeGreaterThan(offsuitGapper);
  });

  it("ranks every pair above the same high card unpaired", () => {
    const pair = preflopStrength([card(CLUBS, 3), card(SPADES, 3)]);
    const unpaired = preflopStrength([card(CLUBS, 3), card(SPADES, 1)]);
    expect(pair).toBeGreaterThan(unpaired);
  });

  it("keeps every pre-flop score inside the 0–1 scale", () => {
    for (let a = 0; a < 52; a += 7) {
      for (let b = a + 1; b < 52; b += 5) {
        const score = preflopStrength([a, b]);
        expect(score).toBeGreaterThanOrEqual(0);
        expect(score).toBeLessThanOrEqual(1);
      }
    }
  });

  it("rates a made flush above a bare pair post-flop", () => {
    const flushBoard = [card(CLUBS, 2), card(CLUBS, 4), card(CLUBS, 9)];
    const flush = postflopStrength([ACE_CLUBS, KING_CLUBS], flushBoard);
    const pair = postflopStrength(
      [ACE_SPADES, card(DIAMONDS, 3)],
      [card(SPADES, 1), card(HEARTS, 6), card(DIAMONDS, 9)]
    );
    expect(flush).toBeGreaterThan(pair);
  });

  it("falls back to the pre-flop read when fewer than five cards are known", () => {
    expect(handStrength([ACE_CLUBS, ACE_SPADES], [])).toBeCloseTo(
      preflopStrength([ACE_CLUBS, ACE_SPADES]),
      5
    );
  });
});

describe("bot decisions", () => {
  it("checks a weak hand when it costs nothing and it never bluffs", () => {
    const decision = decideBotAction(
      view({ hole: [SEVEN_CLUBS, TWO_DIAMONDS] }),
      BOT_PROFILES.easy,
      always(0.99)
    );
    expect(decision.action).toBe("check");
  });

  it("bets a monster when checked to", () => {
    const decision = decideBotAction(view(), BOT_PROFILES.hard, always(0.99));
    expect(["bet", "allin"]).toContain(decision.action);
  });

  it("sizes a value bet from the pot and never below the minimum raise", () => {
    const decision = decideBotAction(
      view({ pot: 100, minRaise: 10 }),
      BOT_PROFILES.hard,
      always(0.99)
    );
    expect(decision.action).toBe("bet");
    expect(decision.amount).toBeGreaterThanOrEqual(10);
  });

  it("never bets more than it has", () => {
    for (const difficulty of DIFFICULTIES) {
      const decision = decideBotAction(
        view({ myStack: 25, pot: 500 }),
        BOT_PROFILES[difficulty],
        always(0.01)
      );
      if (decision.amount !== undefined) {
        expect(decision.amount).toBeLessThanOrEqual(25);
      }
    }
  });

  it("raises a monster rather than just calling", () => {
    const decision = decideBotAction(
      view({ currentBet: 40, myBet: 0 }),
      BOT_PROFILES.hard,
      always(0.99)
    );
    expect(["raise", "allin"]).toContain(decision.action);
  });

  it("folds junk to a large bet at every difficulty", () => {
    const facingAShove = view({
      hole: [SEVEN_CLUBS, TWO_DIAMONDS],
      board: [ACE_SPADES, KING_CLUBS, card(DIAMONDS, 9)],
      currentBet: 400,
      myBet: 0,
      pot: 40,
    });
    for (const difficulty of DIFFICULTIES) {
      const decision = decideBotAction(
        facingAShove,
        BOT_PROFILES[difficulty],
        always(0.99)
      );
      expect(decision.action).toBe("fold");
    }
  });

  it("calls a small bet with ace-high on easy where hard folds it", () => {
    // No pair, just an ace, priced cheaply: a calling station takes it, a
    // tight-aggressive bot lets it go. This is the tight/loose axis showing up.
    const marginal = view({
      hole: [ACE_SPADES, card(DIAMONDS, 1)],
      board: [card(CLUBS, 8), card(DIAMONDS, 4), card(HEARTS, 6)],
      currentBet: 20,
      myBet: 0,
      pot: 100,
    });
    expect(decideBotAction(marginal, BOT_PROFILES.easy, always(0.99)).action).toBe(
      "call"
    );
    expect(decideBotAction(marginal, BOT_PROFILES.hard, always(0.99)).action).toBe(
      "fold"
    );
  });

  it("does not fold top pair on hard, where it folded ace-high", () => {
    const topPair = view({
      hole: [ACE_SPADES, card(DIAMONDS, 3)],
      board: [ACE_CLUBS, card(DIAMONDS, 10), card(SPADES, 4)],
      currentBet: 20,
      myBet: 0,
      pot: 100,
    });
    expect(
      decideBotAction(topPair, BOT_PROFILES.hard, always(0.99)).action
    ).not.toBe("fold");
  });

  it("bluffs only when the profile allows it and the roll comes in", () => {
    const bluffSpot = view({
      hole: [SEVEN_CLUBS, TWO_DIAMONDS],
      board: [ACE_SPADES, KING_CLUBS, card(DIAMONDS, 9)],
    });

    const rolled = decideBotAction(bluffSpot, BOT_PROFILES.hard, always(0));
    expect(["bet", "allin"]).toContain(rolled.action);

    const missed = decideBotAction(bluffSpot, BOT_PROFILES.hard, always(0.99));
    expect(missed.action).toBe("check");

    // The calling station has a zero bluff frequency, so no roll makes it bet.
    expect(decideBotAction(bluffSpot, BOT_PROFILES.easy, always(0)).action).toBe(
      "check"
    );
  });

  it("moves all in rather than betting more than its stack", () => {
    const decision = decideBotAction(
      view({ myStack: 30, pot: 400, minRaise: 10 }),
      BOT_PROFILES.hard,
      always(0.99)
    );
    expect(decision.action).toBe("allin");
  });

  it("tightens up as more opponents stay in the hand", () => {
    const spot = (opponentsLive: number) =>
      view({
        hole: [card(HEARTS, 9), card(SPADES, 9)],
        board: [card(CLUBS, 2), card(DIAMONDS, 5), card(SPADES, 11)],
        currentBet: 80,
        myBet: 0,
        pot: 80,
        opponentsLive,
      });
    const short = decideBotAction(spot(1), BOT_PROFILES.hard, always(0.99));
    const crowded = decideBotAction(spot(5), BOT_PROFILES.hard, always(0.99));
    const order = { fold: 0, check: 1, call: 2, bet: 3, raise: 4, allin: 5 };
    expect(order[crowded.action]).toBeLessThanOrEqual(order[short.action]);
  });

  it("always returns an action carrying a human-readable reason", () => {
    for (const difficulty of DIFFICULTIES) {
      const decision = decideBotAction(view(), BOT_PROFILES[difficulty], always(0.5));
      expect(decision.reason.length).toBeGreaterThan(0);
    }
  });
});

describe("difficulty profiles", () => {
  it("exposes one profile per difficulty with a label and description", () => {
    for (const difficulty of DIFFICULTIES) {
      const profile = BOT_PROFILES[difficulty];
      expect(profile.id).toBe(difficulty);
      expect(profile.label).toBeTruthy();
      expect(profile.description).toBeTruthy();
    }
  });

  it("gets tighter and more aggressive as difficulty rises", () => {
    expect(BOT_PROFILES.easy.foldBelow).toBeLessThan(BOT_PROFILES.medium.foldBelow);
    expect(BOT_PROFILES.medium.foldBelow).toBeLessThan(BOT_PROFILES.hard.foldBelow);
    expect(BOT_PROFILES.easy.bluffFrequency).toBeLessThan(
      BOT_PROFILES.hard.bluffFrequency
    );
    expect(BOT_PROFILES.easy.callLooseness).toBeGreaterThan(
      BOT_PROFILES.hard.callLooseness
    );
  });
});
