/**
 * Local Texas Hold'em engine for practice mode (#174).
 *
 * Everything here runs in the browser: the deck is shuffled locally, the bots
 * decide locally, and nothing is signed, submitted, or persisted on chain.
 * That is the point of the issue — a player should be able to learn the table
 * without a wallet, without XLM, and without the MPC committee being up.
 *
 * The engine is a pure state machine: every exported transition takes a state
 * and returns a new one, so the React layer only has to hold the latest state
 * and re-render. Randomness comes from a seeded generator carried *inside* the
 * state, which keeps transitions pure and makes a session reproducible from
 * its seed.
 */

import { bestHandRank } from "./hand-rank";
import {
  BOT_PROFILES,
  decideBotAction,
  type BotAction,
  type Difficulty,
} from "./practice-bot";

// ── Types ────────────────────────────────────────────────────────────────────

export type PracticePhase =
  | "waiting"
  | "preflop"
  | "flop"
  | "turn"
  | "river"
  | "settlement";

export interface PracticeSeat {
  id: string;
  name: string;
  isBot: boolean;
  stack: number;
  /** Chips put in during the current betting round. */
  betThisRound: number;
  /** Chips put in across the whole hand — drives side-pot splitting. */
  committed: number;
  folded: boolean;
  allIn: boolean;
  cards: [number, number] | null;
  /** Whether this seat has acted at least once in the current round. */
  hasActed: boolean;
}

export interface PracticeLogEntry {
  handNumber: number;
  street: PracticePhase;
  text: string;
}

export interface PracticePayout {
  seatId: string;
  amount: number;
  /** Name of the winning hand, when it went to showdown. */
  handName?: string;
}

export interface PracticeConfig {
  botCount: number;
  difficulty: Difficulty;
  startingStack: number;
  smallBlind: number;
  bigBlind: number;
}

export interface PracticeState {
  config: PracticeConfig;
  seats: PracticeSeat[];
  phase: PracticePhase;
  board: number[];
  /** Chips already gathered from completed betting rounds. */
  pot: number;
  /** Highest total bet on the current street. */
  currentBet: number;
  /** Smallest legal raise increment on the current street. */
  minRaise: number;
  /** Index of the seat to act, or -1 when nobody is. */
  toAct: number;
  dealerSeat: number;
  handNumber: number;
  deck: number[];
  log: PracticeLogEntry[];
  payouts: PracticePayout[];
  /** True once the human is out of chips and cannot start another hand. */
  busted: boolean;
  rngState: number;
}

export const HUMAN_SEAT_ID = "you";

export const DEFAULT_PRACTICE_CONFIG: PracticeConfig = {
  botCount: 1,
  difficulty: "medium",
  startingStack: 1000,
  smallBlind: 5,
  bigBlind: 10,
};

/** Bots allowed at a practice table, matching the 6-max real tables. */
export const MAX_BOTS = 5;

// ── Seeded RNG ───────────────────────────────────────────────────────────────

/**
 * mulberry32 — small, fast, and good enough for shuffling a practice deck.
 * The state is threaded through `PracticeState` so transitions stay pure.
 */
function nextRandom(state: number): [number, number] {
  let t = (state + 0x6d2b79f5) | 0;
  t = Math.imul(t ^ (t >>> 15), 1 | t);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  const value = ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  return [value, t | 0];
}

/**
 * Wraps the state's RNG as a plain `() => number` for the duration of one
 * transition, then hands back the advanced seed.
 */
function rngCursor(seed: number): { next: () => number; seed: () => number } {
  let current = seed;
  return {
    next: () => {
      const [value, advanced] = nextRandom(current);
      current = advanced;
      return value;
    },
    seed: () => current,
  };
}

// ── Setup ────────────────────────────────────────────────────────────────────

function freshDeck(): number[] {
  return Array.from({ length: 52 }, (_, i) => i);
}

/** Fisher–Yates, drawing from the supplied generator. */
function shuffle(deck: number[], rng: () => number): number[] {
  const out = [...deck];
  for (let i = out.length - 1; i > 0; i -= 1) {
    const j = Math.floor(rng() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

function botName(index: number, difficulty: Difficulty): string {
  return `${BOT_PROFILES[difficulty].label} BOT ${index + 1}`;
}

function makeSeat(
  id: string,
  name: string,
  isBot: boolean,
  stack: number
): PracticeSeat {
  return {
    id,
    name,
    isBot,
    stack,
    betThisRound: 0,
    committed: 0,
    folded: false,
    allIn: false,
    cards: null,
    hasActed: false,
  };
}

/**
 * Builds a table that has not been dealt yet. The human always sits in seat 0
 * so the UI can render "you" at the bottom without searching.
 */
export function createPracticeGame(
  config: Partial<PracticeConfig> = {},
  seed = 1
): PracticeState {
  const merged: PracticeConfig = { ...DEFAULT_PRACTICE_CONFIG, ...config };
  const botCount = Math.max(1, Math.min(MAX_BOTS, merged.botCount));
  const resolved: PracticeConfig = { ...merged, botCount };

  const seats = [
    makeSeat(HUMAN_SEAT_ID, "YOU", false, resolved.startingStack),
    ...Array.from({ length: botCount }, (_, i) =>
      makeSeat(`bot-${i}`, botName(i, resolved.difficulty), true, resolved.startingStack)
    ),
  ];

  return {
    config: resolved,
    seats,
    phase: "waiting",
    board: [],
    pot: 0,
    currentBet: 0,
    minRaise: resolved.bigBlind,
    toAct: -1,
    dealerSeat: 0,
    handNumber: 0,
    deck: freshDeck(),
    log: [],
    payouts: [],
    busted: false,
    // Keep the seed away from 0, where mulberry32 starts out lightly biased.
    rngState: (seed | 0) + 0x9e3779b9,
  };
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function liveSeats(state: PracticeState): PracticeSeat[] {
  return state.seats.filter((s) => !s.folded);
}

/** Seats that can still put chips in — not folded and not already all in. */
function actionableSeats(state: PracticeState): PracticeSeat[] {
  return state.seats.filter((s) => !s.folded && !s.allIn && s.stack > 0);
}

function nextSeatIndex(state: PracticeState, from: number): number {
  const count = state.seats.length;
  for (let step = 1; step <= count; step += 1) {
    const index = (from + step) % count;
    const seat = state.seats[index];
    if (!seat.folded && !seat.allIn && seat.stack > 0) return index;
  }
  return -1;
}

function log(
  state: PracticeState,
  text: string
): PracticeLogEntry[] {
  const entry: PracticeLogEntry = {
    handNumber: state.handNumber,
    street: state.phase,
    text,
  };
  // Keep the feed bounded; nothing reads further back than the visible list.
  return [...state.log, entry].slice(-60);
}

/** Moves `amount` (capped by the stack) from a seat into the pot. */
function postChips(seat: PracticeSeat, amount: number): PracticeSeat {
  const paid = Math.max(0, Math.min(amount, seat.stack));
  return {
    ...seat,
    stack: seat.stack - paid,
    betThisRound: seat.betThisRound + paid,
    committed: seat.committed + paid,
    allIn: seat.stack - paid <= 0,
  };
}

// ── Dealing ──────────────────────────────────────────────────────────────────

/**
 * Deals a new hand: rotates the button, posts blinds, deals hole cards, and
 * runs bot action up to the human's first decision.
 *
 * Seats that busted out sit the hand out rather than being removed, so seat
 * numbering stays stable across a session.
 */
export function startHand(state: PracticeState): PracticeState {
  const human = state.seats.find((s) => s.id === HUMAN_SEAT_ID);
  if (!human || human.stack <= 0) {
    return { ...state, busted: true };
  }

  const cursor = rngCursor(state.rngState);
  const deck = shuffle(freshDeck(), cursor.next);

  // Anyone without chips sits out; the rest are dealt in.
  const playing = state.seats.map((seat) => seat.stack > 0);
  const playingCount = playing.filter(Boolean).length;
  if (playingCount < 2) {
    return {
      ...state,
      busted: human.stack <= 0,
      log: log(state, "Not enough funded seats to deal — reset to play on."),
    };
  }

  let cursorCard = 0;
  const seats = state.seats.map((seat, index) => {
    const base = makeSeat(seat.id, seat.name, seat.isBot, seat.stack);
    if (!playing[index]) {
      return { ...base, folded: true };
    }
    const cards: [number, number] = [deck[cursorCard++], deck[cursorCard++]];
    return { ...base, cards };
  });

  const dealerSeat = nextFundedSeat(seats, state.dealerSeat);

  const dealt: PracticeState = {
    ...state,
    seats,
    phase: "preflop",
    board: [],
    pot: 0,
    currentBet: 0,
    minRaise: state.config.bigBlind,
    dealerSeat,
    handNumber: state.handNumber + 1,
    deck: deck.slice(cursorCard),
    payouts: [],
    busted: false,
    rngState: cursor.seed(),
  };

  return advance(postBlinds(dealt));
}

function nextFundedSeat(seats: PracticeSeat[], from: number): number {
  for (let step = 1; step <= seats.length; step += 1) {
    const index = (from + step) % seats.length;
    if (seats[index].stack > 0) return index;
  }
  return from;
}

/**
 * Posts the small and big blind and sets first action.
 *
 * Heads-up follows the standard rule — the button posts the small blind and
 * acts first pre-flop — which differs from the multiway table, where the
 * blinds sit to the button's immediate left.
 */
function postBlinds(state: PracticeState): PracticeState {
  const { smallBlind, bigBlind } = state.config;
  const headsUp = state.seats.filter((s) => !s.folded).length === 2;

  const sbIndex = headsUp
    ? state.dealerSeat
    : nextFundedSeat(state.seats, state.dealerSeat);
  const bbIndex = nextFundedSeat(state.seats, sbIndex);

  const seats = state.seats.map((seat, index) => {
    if (index === sbIndex) return postChips(seat, smallBlind);
    if (index === bbIndex) return postChips(seat, bigBlind);
    return seat;
  });

  const withBlinds: PracticeState = {
    ...state,
    seats,
    currentBet: Math.max(...seats.map((s) => s.betThisRound)),
    minRaise: bigBlind,
  };

  const first = nextSeatIndex(withBlinds, bbIndex);

  return {
    ...withBlinds,
    toAct: first,
    log: log(
      withBlinds,
      `Hand #${withBlinds.handNumber} — blinds ${smallBlind}/${bigBlind}.`
    ),
  };
}

// ── Legal actions ────────────────────────────────────────────────────────────

export interface LegalActions {
  canAct: boolean;
  canCheck: boolean;
  canCall: boolean;
  callAmount: number;
  canRaise: boolean;
  /** Smallest legal *total* street bet for a raise. */
  minRaiseTo: number;
  /** Largest legal total street bet — the seat's whole stack. */
  maxRaiseTo: number;
}

/** What the seat to act is allowed to do right now. */
export function legalActions(state: PracticeState): LegalActions {
  const seat = state.seats[state.toAct];
  const idle: LegalActions = {
    canAct: false,
    canCheck: false,
    canCall: false,
    callAmount: 0,
    canRaise: false,
    minRaiseTo: 0,
    maxRaiseTo: 0,
  };
  if (!seat || seat.folded || seat.allIn || state.phase === "waiting") {
    return idle;
  }

  const callAmount = Math.min(state.currentBet - seat.betThisRound, seat.stack);
  const maxRaiseTo = seat.betThisRound + seat.stack;
  const minRaiseTo = Math.min(state.currentBet + state.minRaise, maxRaiseTo);

  return {
    canAct: true,
    canCheck: callAmount <= 0,
    canCall: callAmount > 0,
    callAmount: Math.max(0, callAmount),
    // A seat can always move in; it can only make a *sized* raise if it has
    // more chips than the call costs.
    canRaise: seat.stack > Math.max(0, callAmount),
    minRaiseTo,
    maxRaiseTo,
  };
}

// ── Applying an action ───────────────────────────────────────────────────────

/**
 * Applies `action` for whichever seat is to act, then runs the hand forward
 * — through bot decisions and street changes — until it is the human's turn
 * again or the hand is over.
 *
 * An illegal action is coerced to the closest legal one rather than throwing,
 * so a stale click from the UI can never wedge the table.
 */
export function applyAction(
  state: PracticeState,
  action: BotAction,
  amount?: number
): PracticeState {
  if (state.phase === "waiting" || state.phase === "settlement") return state;
  return advance(applyActionInternal(state, action, amount));
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function replaceSeat(
  state: PracticeState,
  index: number,
  seat: PracticeSeat
): PracticeState {
  const seats = [...state.seats];
  seats[index] = seat;
  return { ...state, seats };
}

/**
 * Records an aggressive action.
 *
 * A raise that clears the previous bet by at least a full raise re-opens the
 * betting: everyone still in owes a decision again. A short all-in that does
 * not clear it raises the price without re-opening action, which is the rule
 * this branch is careful about.
 */
function raiseTo(
  state: PracticeState,
  index: number,
  moved: PracticeSeat
): PracticeState {
  const raiseSize = moved.betThisRound - state.currentBet;
  const isFullRaise = raiseSize >= state.minRaise;
  const newBet = Math.max(state.currentBet, moved.betThisRound);

  const seats = state.seats.map((s, i) => {
    if (i === index) return { ...moved, hasActed: true };
    // Reopen action for everyone else only on a full-sized raise.
    if (isFullRaise && !s.folded && !s.allIn) return { ...s, hasActed: false };
    return s;
  });

  return {
    ...state,
    seats,
    currentBet: newBet,
    minRaise: isFullRaise ? Math.max(state.minRaise, raiseSize) : state.minRaise,
  };
}

// ── Round / street progression ───────────────────────────────────────────────

/** True when nobody left to act still owes a decision or chips. */
function isRoundComplete(state: PracticeState): boolean {
  const actionable = actionableSeats(state);
  if (actionable.length === 0) return true;
  // One player left with chips and everyone else all in: no betting to do.
  if (actionable.length === 1 && liveSeats(state).length > 1) {
    const only = actionable[0];
    if (only.hasActed && only.betThisRound >= state.currentBet) return true;
  }
  return actionable.every(
    (seat) => seat.hasActed && seat.betThisRound >= state.currentBet
  );
}

const STREET_ORDER: PracticePhase[] = ["preflop", "flop", "turn", "river"];

/** Sweeps the street's bets into the pot and resets for the next one. */
function collectBets(state: PracticeState): PracticeState {
  const collected = state.seats.reduce((sum, s) => sum + s.betThisRound, 0);
  return {
    ...state,
    pot: state.pot + collected,
    seats: state.seats.map((s) => ({ ...s, betThisRound: 0, hasActed: false })),
    currentBet: 0,
    minRaise: state.config.bigBlind,
  };
}

function dealBoard(state: PracticeState, count: number): PracticeState {
  return {
    ...state,
    board: [...state.board, ...state.deck.slice(0, count)],
    deck: state.deck.slice(count),
  };
}

/**
 * Drives the hand forward: resolves finished betting rounds into new streets,
 * lets bots act, and stops as soon as the human has a decision to make (or the
 * hand ends).
 */
function advance(state: PracticeState): PracticeState {
  let current = state;

  // Bounded rather than `while (true)`: a full hand is a few dozen steps, and
  // a cap means a logic slip degrades to a stuck table instead of a hung tab.
  for (let guard = 0; guard < 400; guard += 1) {
    if (current.phase === "settlement" || current.phase === "waiting") {
      return current;
    }

    // Everyone but one folded — that player takes the pot uncontested.
    if (liveSeats(current).length <= 1) {
      return settle(current);
    }

    if (!isRoundComplete(current)) {
      const seat = current.seats[current.toAct];
      if (!seat || seat.folded || seat.allIn || seat.stack <= 0) {
        const nextIndex = nextSeatIndex(current, current.toAct);
        if (nextIndex === -1) {
          current = advanceStreet(collectBets(current));
          continue;
        }
        current = { ...current, toAct: nextIndex };
        continue;
      }

      if (!seat.isBot) {
        // The human's turn — hand control back to the UI.
        return current;
      }

      current = runBotTurn(current, current.toAct);
      continue;
    }

    current = advanceStreet(collectBets(current));
  }

  return current;
}

/** Moves to the next street, dealing the board cards it brings with it. */
function advanceStreet(state: PracticeState): PracticeState {
  const streetIndex = STREET_ORDER.indexOf(state.phase);
  if (streetIndex === -1 || streetIndex === STREET_ORDER.length - 1) {
    return settle(state);
  }

  const nextPhase = STREET_ORDER[streetIndex + 1];
  let next = { ...state, phase: nextPhase };
  next = dealBoard(next, nextPhase === "flop" ? 3 : 1);

  const label = nextPhase.toUpperCase();
  next = { ...next, log: log(next, `--- ${label} --- pot ${next.pot}`) };

  // Post-flop, action starts to the button's left regardless of table size.
  const first = nextSeatIndex(next, next.dealerSeat);
  if (first === -1) {
    // Everyone is all in: run the rest of the board out with no betting.
    return advanceStreet(next);
  }
  return { ...next, toAct: first };
}

function runBotTurn(state: PracticeState, index: number): PracticeState {
  const seat = state.seats[index];
  if (!seat.cards) {
    return applyAction({ ...state, toAct: index }, "fold");
  }

  const cursor = rngCursor(state.rngState);
  const decision = decideBotAction(
    {
      hole: seat.cards,
      board: state.board,
      currentBet: state.currentBet,
      myBet: seat.betThisRound,
      myStack: seat.stack,
      pot: state.pot + state.seats.reduce((sum, s) => sum + s.betThisRound, 0),
      minRaise: state.minRaise,
      opponentsLive: liveSeats(state).length - 1,
    },
    BOT_PROFILES[state.config.difficulty],
    cursor.next
  );

  const withSeed = { ...state, toAct: index, rngState: cursor.seed() };
  return applyActionInternal(withSeed, decision.action, decision.amount);
}

/**
 * Records one seat's action and passes the turn on, without re-entering
 * `advance` — the driving loop stays in one place rather than recursing once
 * per bot.
 *
 * Passing the turn belongs here rather than in the loop: whoever acted is
 * done, so the seat after them is next regardless of why they acted.
 */
function applyActionInternal(
  state: PracticeState,
  action: BotAction,
  amount?: number
): PracticeState {
  const index = state.toAct;
  const seat = state.seats[index];
  const legal = legalActions(state);
  if (!seat || !legal.canAct) return state;

  let next: PracticeState;
  let description: string;

  if (action === "fold") {
    next = replaceSeat(state, index, { ...seat, folded: true, hasActed: true });
    description = `${seat.name} folds.`;
  } else if (action === "check" && legal.canCheck) {
    next = replaceSeat(state, index, { ...seat, hasActed: true });
    description = `${seat.name} checks.`;
  } else if (action === "call" || (action === "check" && !legal.canCheck)) {
    // A "check" facing a bet is treated as a call rather than rejected, so a
    // stale click from the UI can't wedge the table.
    const paid = legal.callAmount;
    next = replaceSeat(state, index, { ...postChips(seat, paid), hasActed: true });
    description = paid > 0 ? `${seat.name} calls ${paid}.` : `${seat.name} checks.`;
  } else {
    const target =
      action === "allin"
        ? legal.maxRaiseTo
        : clamp(
            Math.floor(amount ?? legal.minRaiseTo),
            legal.minRaiseTo,
            legal.maxRaiseTo
          );
    const moved = postChips(seat, target - seat.betThisRound);
    next = raiseTo(state, index, moved);
    description =
      action === "allin"
        ? `${seat.name} moves all in for ${moved.betThisRound}.`
        : state.currentBet === 0
          ? `${seat.name} bets ${moved.betThisRound}.`
          : `${seat.name} raises to ${moved.betThisRound}.`;
  }

  return {
    ...next,
    toAct: nextSeatIndex(next, index),
    log: log(next, description),
  };
}

// ── Settlement ───────────────────────────────────────────────────────────────

/**
 * Ends the hand and pays it out.
 *
 * Pots are split by contribution level so short all-ins are handled properly:
 * each distinct `committed` amount defines a layer that only the players who
 * reached it can win, and each layer goes to the best hand among them.
 */
function settle(state: PracticeState): PracticeState {
  const swept = collectBets(state);
  const live = swept.seats.filter((s) => !s.folded);

  // Uncontested — one player left, no cards need showing.
  if (live.length === 1) {
    const winner = live[0];
    const seats = swept.seats.map((s) =>
      s.id === winner.id ? { ...s, stack: s.stack + swept.pot } : s
    );
    const settled: PracticeState = {
      ...swept,
      seats,
      phase: "settlement",
      toAct: -1,
      pot: 0,
      payouts: [{ seatId: winner.id, amount: swept.pot }],
    };
    return {
      ...settled,
      busted: isHumanBusted(settled),
      log: log(settled, `${winner.name} wins ${swept.pot} uncontested.`),
    };
  }

  const ranked = live.map((seat) => ({
    seat,
    rank: seat.cards ? bestHandRank([...seat.cards, ...swept.board]) : null,
  }));

  // Layer boundaries: every distinct amount a live player put in.
  const levels = [
    ...new Set(swept.seats.filter((s) => s.committed > 0).map((s) => s.committed)),
  ].sort((a, b) => a - b);

  const payouts = new Map<string, number>();
  const winningHands = new Map<string, string>();
  let previousLevel = 0;

  for (const level of levels) {
    if (level <= previousLevel) continue;

    // Everyone who reached into this layer contributes to it — folded
    // players' chips included, which is exactly why they are in the pot.
    const layer = swept.seats.reduce((sum, s) => {
      const capped = Math.min(s.committed, level);
      return sum + Math.max(0, capped - previousLevel);
    }, 0);
    previousLevel = level;
    if (layer <= 0) continue;

    const contenders = ranked.filter(
      (r) => r.seat.committed >= level && r.rank !== null
    );
    if (contenders.length === 0) continue;

    const best = contenders.reduce((top, cur) =>
      compareRanks(cur.rank!, top.rank!) > 0 ? cur : top
    );
    const winners = contenders.filter(
      (c) => compareRanks(c.rank!, best.rank!) === 0
    );

    const share = Math.floor(layer / winners.length);
    // Odd chips go to the first winner clockwise from the button, as they do
    // at a real table.
    let remainder = layer - share * winners.length;
    for (const winner of winners) {
      const extra = remainder > 0 ? 1 : 0;
      remainder -= extra;
      payouts.set(winner.seat.id, (payouts.get(winner.seat.id) ?? 0) + share + extra);
      if (winner.rank) winningHands.set(winner.seat.id, winner.rank.name);
    }
  }

  const seats = swept.seats.map((s) => ({
    ...s,
    stack: s.stack + (payouts.get(s.id) ?? 0),
  }));

  const settled: PracticeState = {
    ...swept,
    seats,
    phase: "settlement",
    toAct: -1,
    pot: 0,
    payouts: [...payouts.entries()].map(([seatId, amount]) => ({
      seatId,
      amount,
      handName: winningHands.get(seatId),
    })),
  };

  const summary = settled.payouts
    .map((p) => {
      const seat = settled.seats.find((s) => s.id === p.seatId);
      return `${seat?.name ?? p.seatId} wins ${p.amount}${p.handName ? ` with ${p.handName}` : ""}`;
    })
    .join(", ");

  return {
    ...settled,
    busted: isHumanBusted(settled),
    log: log(settled, summary ? `${summary}.` : "Hand complete."),
  };
}

function isHumanBusted(state: PracticeState): boolean {
  const human = state.seats.find((s) => s.id === HUMAN_SEAT_ID);
  return !!human && human.stack <= 0;
}

function compareRanks(
  a: NonNullable<ReturnType<typeof bestHandRank>>,
  b: NonNullable<ReturnType<typeof bestHandRank>>
): number {
  if (a.category !== b.category) return a.category - b.category;
  const len = Math.max(a.tiebreak.length, b.tiebreak.length);
  for (let i = 0; i < len; i += 1) {
    const av = a.tiebreak[i] ?? 0;
    const bv = b.tiebreak[i] ?? 0;
    if (av !== bv) return av - bv;
  }
  return 0;
}

// ── Session controls ─────────────────────────────────────────────────────────

/**
 * Rebuilds the table with new settings, keeping the current RNG position so a
 * mid-session difficulty change doesn't replay the same cards.
 */
export function reconfigure(
  state: PracticeState,
  config: Partial<PracticeConfig>
): PracticeState {
  return createPracticeGame({ ...state.config, ...config }, state.rngState);
}

/** True when the human seat is the one being asked to act. */
export function isHumanTurn(state: PracticeState): boolean {
  return state.seats[state.toAct]?.id === HUMAN_SEAT_ID;
}

export function humanSeat(state: PracticeState): PracticeSeat | undefined {
  return state.seats.find((s) => s.id === HUMAN_SEAT_ID);
}

/** Total chips on the table right now, including uncollected street bets. */
export function displayPot(state: PracticeState): number {
  return state.pot + state.seats.reduce((sum, s) => sum + s.betThisRound, 0);
}
