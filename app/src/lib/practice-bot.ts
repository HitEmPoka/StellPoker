/**
 * Heuristic opponents for practice mode (#174).
 *
 * The bots are deliberately rule-based rather than solver-driven: practice
 * mode exists so a new player can learn the interface and the flow of a hand
 * without a wallet, so what matters is that opponents are *legible* — a
 * calling station should feel like a calling station — not that they are
 * strong.
 *
 * Difficulty is expressed with the two axes poker players actually use to
 * describe each other: how many hands someone plays (tight ↔ loose) and how
 * hard they push the ones they play (passive ↔ aggressive).
 *
 *   easy   — loose-passive "calling station": plays almost anything, calls
 *            down light, almost never raises, never bluffs.
 *   medium — tight-passive "rock": folds weak hands, calls with the rest,
 *            raises only with a genuinely strong holding.
 *   hard   — tight-aggressive: folds weak hands, bets and raises its strong
 *            ones for value, and bluffs at a low, fixed frequency.
 *
 * Every decision is drawn from a seeded RNG owned by the caller, so a whole
 * practice session replays identically from the same seed — which is what
 * makes the engine testable.
 */

import { bestHandRank } from "./hand-rank";

export type Difficulty = "easy" | "medium" | "hard";

export interface BotProfile {
  id: Difficulty;
  /** Shown in the difficulty picker. */
  label: string;
  /** One-line description of the playing style. */
  description: string;
  /** Hand strength (0–1) below which the bot folds to a bet it can't check. */
  foldBelow: number;
  /** Hand strength at or above which the bot bets/raises for value. */
  raiseAbove: number;
  /** Probability of firing a bluff with a weak hand when checked to. */
  bluffFrequency: number;
  /**
   * How much slack the bot gives pot odds. Above 1 it calls looser than the
   * odds justify (a station), below 1 it needs a better price than break-even.
   */
  callLooseness: number;
  /** Value bet as a fraction of the pot. */
  betSizing: number;
}

export const BOT_PROFILES: Record<Difficulty, BotProfile> = {
  easy: {
    id: "easy",
    label: "EASY",
    description: "Loose-passive. Calls almost anything, rarely raises.",
    foldBelow: 0.08,
    raiseAbove: 0.9,
    bluffFrequency: 0,
    callLooseness: 2.2,
    betSizing: 0.4,
  },
  medium: {
    id: "medium",
    label: "MEDIUM",
    description: "Tight-passive. Folds junk, calls with the rest.",
    foldBelow: 0.34,
    raiseAbove: 0.74,
    bluffFrequency: 0.05,
    callLooseness: 1.15,
    betSizing: 0.55,
  },
  hard: {
    id: "hard",
    label: "HARD",
    description: "Tight-aggressive. Bets for value and bluffs sometimes.",
    foldBelow: 0.42,
    raiseAbove: 0.62,
    bluffFrequency: 0.22,
    callLooseness: 0.95,
    betSizing: 0.7,
  },
};

export const DIFFICULTIES: Difficulty[] = ["easy", "medium", "hard"];

// ── Hand strength ────────────────────────────────────────────────────────────

function rankOf(card: number): number {
  return (card % 13) + 2; // 2..14, ace high
}

function suitOf(card: number): number {
  return Math.floor(card / 13);
}

/**
 * Pre-flop strength on a 0–1 scale, from the features that drive every
 * starting-hand chart: pair, high cards, suitedness, connectedness.
 *
 * AA lands at 1.0 and 72o near 0, which is all the resolution a heuristic bot
 * needs to decide whether it is playing the hand.
 */
export function preflopStrength(hole: readonly [number, number]): number {
  const [a, b] = [rankOf(hole[0]), rankOf(hole[1])].sort((x, y) => y - x);
  const suited = suitOf(hole[0]) === suitOf(hole[1]);
  const gap = a - b;

  if (a === b) {
    // Pairs: 22 ≈ 0.50, AA = 1.0.
    return 0.5 + ((a - 2) / 12) * 0.5;
  }

  // Base on the high card, with the kicker contributing less.
  let score = ((a - 2) / 12) * 0.45 + ((b - 2) / 12) * 0.2;
  if (suited) score += 0.08;
  if (gap === 1) score += 0.07;
  else if (gap === 2) score += 0.04;
  else if (gap >= 5) score -= 0.05;

  return Math.max(0, Math.min(0.95, score));
}

/**
 * Floor score for each made-hand category.
 *
 * Deliberately not a straight line from 1 to 9: the jump from high card to a
 * pair is the single biggest step in how playable a hand is, while the top
 * categories are all "just bet it" and barely need separating. Spacing them
 * this way is what lets one set of thresholds distinguish a bot that calls
 * with any pair from one that needs two pair to raise.
 */
const CATEGORY_SCORE: Record<number, number> = {
  1: 0.1, // high card
  2: 0.38, // pair
  3: 0.58, // two pair
  4: 0.72, // trips
  5: 0.82, // straight
  6: 0.88, // flush
  7: 0.93, // full house
  8: 0.97, // quads
  9: 1.0, // straight flush
};

/** How much the top tiebreak rank can lift a hand within its category. */
const KICKER_WEIGHT = 0.14;

/**
 * Post-flop strength on the same 0–1 scale, from the made-hand category with
 * the top-card rank separating hands inside a category — so top pair beats
 * bottom pair without needing a second evaluation.
 *
 * This is absolute hand strength, not equity against a range: enough for a
 * practice opponent, and it never needs a simulation to compute.
 */
export function postflopStrength(
  hole: readonly [number, number],
  board: readonly number[]
): number {
  const rank = bestHandRank([...hole, ...board]);
  if (!rank) {
    // Fewer than five cards known — fall back to the pre-flop read.
    return preflopStrength(hole);
  }

  const base = CATEGORY_SCORE[rank.category] ?? 0.1;
  const kicker = ((rank.tiebreak[0] ?? 2) - 2) / 12;
  return Math.max(0, Math.min(1, base + kicker * KICKER_WEIGHT));
}

/** Strength for whatever street the hand is on. */
export function handStrength(
  hole: readonly [number, number],
  board: readonly number[]
): number {
  return board.length === 0
    ? preflopStrength(hole)
    : postflopStrength(hole, board);
}

// ── Decision ─────────────────────────────────────────────────────────────────

/** Everything a bot is allowed to know when it acts. */
export interface BotView {
  hole: readonly [number, number];
  board: readonly number[];
  /** Highest bet anyone has made this street. */
  currentBet: number;
  /** What this bot has already put in this street. */
  myBet: number;
  /** This bot's remaining chips. */
  myStack: number;
  pot: number;
  /** Smallest legal raise increment (usually the big blind). */
  minRaise: number;
  /** Opponents still live in the hand, excluding this bot. */
  opponentsLive: number;
}

export type BotAction = "fold" | "check" | "call" | "bet" | "raise" | "allin";

export interface BotDecision {
  action: BotAction;
  /** Target total bet for this street (bet/raise only). */
  amount?: number;
  /** Short line for the dealer feed, e.g. "Bot 1 raises to 60". */
  reason: string;
}

/** Returns a float in [0, 1). Supplied by the engine's seeded RNG. */
export type Rng = () => number;

/**
 * Chooses an action for one bot.
 *
 * The shape is the same at every difficulty — strength in, action out — and
 * the profile supplies the thresholds. Facing a bet, the bot compares its
 * strength against the pot odds it is being offered, scaled by how loose the
 * profile is; unopposed, it bets its strong hands and (for aggressive
 * profiles) occasionally its weak ones.
 */
export function decideBotAction(
  view: BotView,
  profile: BotProfile,
  rng: Rng
): BotDecision {
  const toCall = Math.max(0, Math.min(view.currentBet - view.myBet, view.myStack));
  const strength = handStrength(view.hole, view.board);

  // Multiway pots need a stronger hand to continue: every extra live opponent
  // is another chance someone has us beaten.
  const crowding = 1 + Math.max(0, view.opponentsLive - 1) * 0.08;
  const effective = strength / crowding;

  if (toCall === 0) {
    // Nobody has bet — we can check for free or take the initiative.
    const wantsValue = effective >= profile.raiseAbove;
    const wantsBluff =
      !wantsValue && effective < profile.foldBelow && rng() < profile.bluffFrequency;

    if ((wantsValue || wantsBluff) && view.myStack > 0) {
      const target = sizeBet(view, profile, wantsBluff ? 0.5 : 1);
      if (target >= view.myStack + view.myBet) {
        return { action: "allin", reason: "shoves all in" };
      }
      return {
        action: view.currentBet === 0 ? "bet" : "raise",
        amount: target,
        reason: wantsBluff ? "fires a bluff" : "bets for value",
      };
    }
    return { action: "check", reason: "checks" };
  }

  // Facing a bet. The price we are getting, as a fraction of the pot we'd be
  // playing for: this is the break-even equity a call needs.
  const potOdds = toCall / (view.pot + toCall);
  const callThreshold = Math.min(
    0.95,
    Math.max(profile.foldBelow, potOdds / profile.callLooseness)
  );

  if (effective >= profile.raiseAbove && view.myStack > toCall) {
    const target = sizeBet(view, profile, 1);
    if (target >= view.myStack + view.myBet) {
      return { action: "allin", reason: "shoves all in" };
    }
    return { action: "raise", amount: target, reason: "raises for value" };
  }

  if (effective >= callThreshold) {
    if (toCall >= view.myStack) {
      return { action: "allin", reason: "calls all in" };
    }
    return { action: "call", reason: "calls" };
  }

  return { action: "fold", reason: "folds" };
}

/**
 * Target *total* street bet for a value bet or raise, clamped to a legal
 * raise and to the stack.
 */
function sizeBet(view: BotView, profile: BotProfile, scale: number): number {
  const potPortion = Math.round(view.pot * profile.betSizing * scale);
  const minLegal = view.currentBet + view.minRaise;
  const target = Math.max(minLegal, view.currentBet + potPortion);
  return Math.min(target, view.myBet + view.myStack);
}
