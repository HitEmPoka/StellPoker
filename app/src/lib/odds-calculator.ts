import { bestHandRank, type HandRank } from "./hand-rank";

/** Same 0-51 card encoding as cards.ts / hand-rank.ts: value = suit * 13 + rankIndex. */
export type CardValue = number;

export interface OddsCalculatorInput {
  /** The player's two hole cards. */
  holeCards: [CardValue, CardValue];
  /** 0-5 known community cards (flop/turn/river as revealed so far). */
  boardCards: CardValue[];
  /** Number of opponents, each modeled as a random unknown two-card hand. */
  numOpponents: number;
  /** Monte Carlo trial count. Higher = more accurate, slower. */
  iterations?: number;
}

export interface OddsResult {
  /** Fraction of trials the hero's hand was strictly best (0-1). */
  win: number;
  /** Fraction of trials the hero tied for best (0-1). */
  tie: number;
  /** Fraction of trials the hero lost (0-1). */
  loss: number;
  iterations: number;
}

const FULL_DECK: CardValue[] = Array.from({ length: 52 }, (_, i) => i);
const DEFAULT_ITERATIONS = 3000;

function compareHandRank(a: HandRank, b: HandRank): number {
  if (a.category !== b.category) return a.category - b.category;
  const len = Math.max(a.tiebreak.length, b.tiebreak.length);
  for (let i = 0; i < len; i++) {
    const av = a.tiebreak[i] ?? 0;
    const bv = b.tiebreak[i] ?? 0;
    if (av !== bv) return av - bv;
  }
  return 0;
}

/** Fisher-Yates shuffle. Not cryptographically secure — fine for an
 * odds-estimation tool, unlike real card dealing (which happens via the
 * on-chain MPC nodes, not this client-side utility). */
function shuffle<T>(items: T[]): T[] {
  const arr = [...items];
  for (let i = arr.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [arr[i], arr[j]] = [arr[j], arr[i]];
  }
  return arr;
}

export class OddsCalculatorError extends Error {}

function validateInput(input: OddsCalculatorInput): void {
  const { holeCards, boardCards, numOpponents } = input;
  if (holeCards.length !== 2) {
    throw new OddsCalculatorError("Exactly two hole cards are required");
  }
  if (boardCards.length > 5) {
    throw new OddsCalculatorError("At most five board cards are allowed");
  }
  if (numOpponents < 1 || numOpponents > 8) {
    throw new OddsCalculatorError("Number of opponents must be between 1 and 8");
  }
  const allKnown = [...holeCards, ...boardCards];
  const uniqueKnown = new Set(allKnown);
  if (uniqueKnown.size !== allKnown.length) {
    throw new OddsCalculatorError("Duplicate cards are not allowed");
  }
  for (const c of allKnown) {
    if (!Number.isInteger(c) || c < 0 || c > 51) {
      throw new OddsCalculatorError(`Invalid card value: ${c}`);
    }
  }
}

/**
 * Estimates win/tie/loss probability for a hand via Monte Carlo simulation
 * against `numOpponents` random unknown hands, given whatever board cards
 * are already known (Issue #163).
 */
export function calculateOdds(input: OddsCalculatorInput): OddsResult {
  validateInput(input);
  const iterations = input.iterations ?? DEFAULT_ITERATIONS;
  const { holeCards, boardCards, numOpponents } = input;

  const known = new Set<CardValue>([...holeCards, ...boardCards]);
  const remainingDeck = FULL_DECK.filter((c) => !known.has(c));

  const boardSlotsNeeded = 5 - boardCards.length;
  const cardsNeededPerTrial = boardSlotsNeeded + numOpponents * 2;

  if (cardsNeededPerTrial > remainingDeck.length) {
    throw new OddsCalculatorError(
      "Not enough remaining cards in the deck for this many opponents"
    );
  }

  let wins = 0;
  let ties = 0;
  let losses = 0;

  for (let trial = 0; trial < iterations; trial++) {
    const drawn = shuffle(remainingDeck).slice(0, cardsNeededPerTrial);
    const fullBoard = [...boardCards, ...drawn.slice(0, boardSlotsNeeded)];

    const heroRank = bestHandRank([...holeCards, ...fullBoard]);
    if (!heroRank) continue; // shouldn't happen once board has 3+ cards

    let bestOpponentRank: HandRank | null = null;
    for (let opp = 0; opp < numOpponents; opp++) {
      const start = boardSlotsNeeded + opp * 2;
      const oppHole = drawn.slice(start, start + 2);
      const oppRank = bestHandRank([...oppHole, ...fullBoard]);
      if (oppRank && (!bestOpponentRank || compareHandRank(oppRank, bestOpponentRank) > 0)) {
        bestOpponentRank = oppRank;
      }
    }

    if (!bestOpponentRank) continue;
    const cmp = compareHandRank(heroRank, bestOpponentRank);
    if (cmp > 0) wins++;
    else if (cmp === 0) ties++;
    else losses++;
  }

  const total = wins + ties + losses;
  if (total === 0) {
    return { win: 0, tie: 0, loss: 0, iterations: 0 };
  }
  return {
    win: wins / total,
    tie: ties / total,
    loss: losses / total,
    iterations: total,
  };
}
