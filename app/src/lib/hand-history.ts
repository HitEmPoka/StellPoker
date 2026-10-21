/**
 * Client-side hand history capture/persistence for the current browser
 * session. Each table keeps its own localStorage entry so the viewer panel
 * can show completed hands (street-by-street pot/board progression, the
 * viewing player's own hole cards when known, final pot, winner, and the
 * settlement proof tx) even after a hand has ended.
 */

import { bestHandRank } from "./hand-rank";
import type { TimelineEvent } from "./hand-timeline";

/** Street shape. */
export type Street = "preflop" | "flop" | "turn" | "river";

/** Street Snapshot shape. */
export interface StreetSnapshot {
  street: Street;
  pot: number;
  boardCards: number[];
}

/** A player dealt into the hand. */
export interface HandPlayer {
  address: string;
  seat: number;
}

/** Chips going into the pot during a street, as the table observed it. */
export interface HandAction {
  street: Street;
  /** Chips added to the pot. */
  amount: number;
  /** Pot after the chips went in. */
  pot: number;
}

/** Hand History Entry shape. */
export interface HandHistoryEntry {
  tableId: number;
  handNumber: number;
  timestamp: number;
  streets: StreetSnapshot[];
  finalPot: number;
  boardCards: number[];
  holeCards?: [number, number];
  handRankName?: string;
  winnerAddress?: string | null;
  txHash?: string;
  /** Address of the viewing player, whose hole cards `holeCards` are (#158). */
  heroAddress?: string;
  /** Everyone seated for the hand (#158). */
  players?: HandPlayer[];
  /** Betting actions in the order they were seen (#158). */
  actions?: HandAction[];
}

// ── Replayer frames ───────────────────────────────────────────────────────────

/** A single step shown during replay. */
export interface ReplayFrame {
  /** Human-readable label for this moment in the hand. */
  label: string;
  /** Street this frame belongs to. */
  street: Street | "settlement";
  /** Board cards visible at this point (grows as streets are revealed). */
  boardCards: number[];
  /** Pot size at this point. */
  pot: number;
  /** Hole cards (only shown when known). */
  holeCards?: [number, number];
  /** Best hand name at this frame (requires ≥5 cards). */
  handRankName?: string;
  /** Whether this is the final settlement frame. */
  isSettlement?: boolean;
  /** Winner address if known (settlement frame). */
  winnerAddress?: string | null;
}

/**
 * Build a sequence of replay frames from a completed hand history entry.
 * Each street snapshot becomes one frame, with an extra settlement frame at
 * the end showing the final board and winner.
 */
export function buildReplayFrames(entry: HandHistoryEntry): ReplayFrame[] {
  const frames: ReplayFrame[] = [];

  const STREET_LABELS: Record<Street, string> = {
    preflop: "PRE-FLOP",
    flop: "FLOP",
    turn: "TURN",
    river: "RIVER",
  };

  // One frame per street snapshot captured during the hand
  for (const snap of entry.streets) {
    const allCards = entry.holeCards
      ? [...entry.holeCards, ...snap.boardCards]
      : snap.boardCards;
    const rankName =
      allCards.length >= 5
        ? bestHandRank(allCards)?.name
        : undefined;

    frames.push({
      label: STREET_LABELS[snap.street],
      street: snap.street,
      boardCards: snap.boardCards,
      pot: snap.pot,
      holeCards: entry.holeCards,
      handRankName: rankName,
    });
  }

  // Final settlement frame
  frames.push({
    label: "SHOWDOWN",
    street: "settlement",
    boardCards: entry.boardCards,
    pot: entry.finalPot,
    holeCards: entry.holeCards,
    handRankName: entry.handRankName,
    isSettlement: true,
    winnerAddress: entry.winnerAddress,
  });

  return frames;
}

const STORAGE_PREFIX = "stellpoker:hand-history:";
const MAX_ENTRIES_PER_TABLE = 50;

function storageKey(tableId: number): string {
  return `${STORAGE_PREFIX}${tableId}`;
}

/** Loads Hand History.
 * @param tableId - table Id.
 * @returns HandHistoryEntry[].
 * @remarks Reads/writes browser storage.
 */
export function loadHandHistory(tableId: number): HandHistoryEntry[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(storageKey(tableId));
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as HandHistoryEntry[]) : [];
  } catch {
    return [];
  }
}

/** Persists Hand History Entry.
 * @param entry - entry.
 * @remarks Reads/writes browser storage.
 */
export function saveHandHistoryEntry(entry: HandHistoryEntry): void {
  if (typeof window === "undefined") return;
  try {
    const existing = loadHandHistory(entry.tableId);
    const next = [entry, ...existing].slice(0, MAX_ENTRIES_PER_TABLE);
    window.localStorage.setItem(storageKey(entry.tableId), JSON.stringify(next));
  } catch {
    // Storage unavailable (private browsing, quota) — history just won't persist.
  }
}

/** Computes Hand Rank Name.
 * @param holeCards - hole Cards.
 * @param boardCards - board Cards.
 * @returns string | undefined.
 */
export function buildHandRankName(
  holeCards: [number, number] | undefined,
  boardCards: number[]
): string | undefined {
  if (!holeCards) return undefined;
  return bestHandRank([...holeCards, ...boardCards])?.name;
}

/**
 * The betting actions of one hand, taken from its live timeline (#176): each
 * time more chips went into the pot on a street. The timeline's `actor` is
 * whoever's turn it was when the pot change was seen — by then the turn has
 * usually passed to the next player — so it is deliberately not carried over.
 */
export function actionsFromTimeline(
  handNumber: number,
  events: readonly TimelineEvent[]
): HandAction[] {
  const prefix = `${handNumber}:`;
  const actions: HandAction[] = [];
  for (const event of events) {
    if (event.kind !== "action" || !event.id.startsWith(prefix)) continue;
    const street = event.street;
    if (street !== "preflop" && street !== "flop" && street !== "turn" && street !== "river") {
      continue;
    }
    actions.push({ street, amount: event.amount ?? 0, pot: event.pot });
  }
  return actions;
}
