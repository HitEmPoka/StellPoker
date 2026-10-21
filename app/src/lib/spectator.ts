/**
 * Spectator mode (Issue #171): anonymous, wallet-less viewing of a live table.
 *
 * Spectators only ever see public information — community cards, pot,
 * stacks, bets and actions. Hole cards are served exclusively by the
 * authenticated `/cards` endpoint, and `parseSpectatorView` additionally
 * drops any per-player card data so a spectator view can never render them.
 */

import type { GamePhase } from "./game-state";

/** Spectator Player shape. */
export interface SpectatorPlayer {
  address: string;
  seat: number;
  stack: number;
  betThisRound: number;
  folded: boolean;
  allIn: boolean;
}

/** Spectator View shape. */
export interface SpectatorView {
  phase: GamePhase;
  rawPhase: string;
  players: SpectatorPlayer[];
  boardCards: number[];
  pot: number;
  currentTurn: number;
  dealerSeat: number;
  handNumber: number;
}

/** Spectator Action Kind shape. */
export type SpectatorActionKind =
  | "new_hand"
  | "street"
  | "fold"
  | "all_in"
  | "bet"
  | "check";

/** Spectator Action shape. */
export interface SpectatorAction {
  kind: SpectatorActionKind;
  handNumber: number;
  /** Seat address for player actions; undefined for table events. */
  address?: string;
  amount?: number;
  street?: GamePhase;
}

const PHASE_MAP: Record<string, GamePhase> = {
  Waiting: "waiting",
  Dealing: "dealing",
  Preflop: "preflop",
  Flop: "flop",
  Turn: "turn",
  River: "river",
  Showdown: "showdown",
  Settlement: "settlement",
  DealingFlop: "preflop",
  DealingTurn: "flop",
  DealingRiver: "turn",
};

function toNumber(value: unknown, fallback: number): number {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return fallback;
}

/** Maps the raw on-chain `get_table` JSON to a hole-card-free view. */
export function parseSpectatorView(
  parsed: Record<string, unknown> | null
): SpectatorView | null {
  if (!parsed) return null;
  const rawPhase = typeof parsed.phase === "string" ? parsed.phase : "Waiting";
  const rawPlayers = Array.isArray(parsed.players)
    ? (parsed.players as Array<Record<string, unknown>>)
    : [];

  return {
    phase: PHASE_MAP[rawPhase] ?? "waiting",
    rawPhase,
    players: rawPlayers.map((raw, index) => ({
      address: typeof raw.address === "string" ? raw.address : `seat-${index}`,
      seat: toNumber(raw.seat_index, index),
      stack: toNumber(raw.stack, 0),
      betThisRound: toNumber(raw.bet_this_round, 0),
      folded: Boolean(raw.folded),
      allIn: Boolean(raw.all_in),
    })),
    boardCards: Array.isArray(parsed.board_cards)
      ? parsed.board_cards.map((v) => toNumber(v, -1)).filter((v) => v >= 0)
      : [],
    pot: toNumber(parsed.pot, 0),
    currentTurn: toNumber(parsed.current_turn, 0),
    dealerSeat: toNumber(parsed.dealer_seat, 0),
    handNumber: toNumber(parsed.hand_number, 0),
  };
}

const BETTING_STREETS: GamePhase[] = ["preflop", "flop", "turn", "river"];

/**
 * Infers the public actions that happened between two consecutive
 * snapshots, for the spectator action feed.
 */
export function diffSpectatorActions(
  prev: SpectatorView | null,
  next: SpectatorView
): SpectatorAction[] {
  if (!prev) return [];
  const handNumber = next.handNumber;

  if (next.handNumber !== prev.handNumber) {
    return [{ kind: "new_hand", handNumber }];
  }

  const actions: SpectatorAction[] = [];
  const sameStreet = next.phase === prev.phase;

  for (const player of next.players) {
    const before = prev.players.find((p) => p.address === player.address);
    if (!before) continue;
    const base = { handNumber, address: player.address };
    if (player.folded && !before.folded) {
      actions.push({ ...base, kind: "fold" });
    } else if (player.allIn && !before.allIn) {
      actions.push({ ...base, kind: "all_in", amount: player.betThisRound });
    } else if (sameStreet && player.betThisRound > before.betThisRound) {
      actions.push({ ...base, kind: "bet", amount: player.betThisRound });
    } else if (
      sameStreet &&
      BETTING_STREETS.includes(next.phase) &&
      prev.currentTurn === before.seat &&
      next.currentTurn !== before.seat &&
      player.betThisRound === before.betThisRound
    ) {
      // Turn passed without chips moving: a check.
      actions.push({ ...base, kind: "check" });
    }
  }

  if (!sameStreet && BETTING_STREETS.includes(next.phase)) {
    actions.push({ kind: "street", handNumber, street: next.phase });
  }

  return actions;
}

/** Short Seat Address.
 * @param address - address.
 * @returns string.
 */
export function shortSeatAddress(address: string): string {
  return address.length > 12 ? `${address.slice(0, 4)}…${address.slice(-4)}` : address;
}

/** Describe Spectator Action.
 * @param action - action.
 * @returns string.
 */
export function describeSpectatorAction(action: SpectatorAction): string {
  const who = action.address ? shortSeatAddress(action.address) : "";
  switch (action.kind) {
    case "new_hand":
      return `HAND #${action.handNumber} STARTED`;
    case "street":
      return `— ${(action.street ?? "").toUpperCase()} —`;
    case "fold":
      return `${who} FOLDS`;
    case "all_in":
      return `${who} IS ALL-IN`;
    case "bet":
      return `${who} BETS ${action.amount?.toLocaleString() ?? 0}`;
    case "check":
      return `${who} CHECKS`;
  }
}

/** Computes Spectator Count.
 * @param count - count.
 * @returns string.
 */
export function formatSpectatorCount(count: number): string {
  return `${count} WATCHING`;
}

/** URL a spectator opens to watch `tableId` without a wallet. */
export function spectateHref(tableId: number): string {
  return `/table/${tableId}?spectate=1`;
}
