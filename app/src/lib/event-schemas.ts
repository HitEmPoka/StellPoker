import { xdr, scValToNative } from "@stellar/stellar-sdk";

/**
 * Typed decoding for poker-table contract events.
 *
 * Each event has a JSON schema in `docs/events/<name>.schema.json`. This module
 * turns raw event topics and data into the canonical JSON form those schemas
 * describe: u32 as numbers, i128 as decimal strings, addresses as strkeys,
 * 32-byte values as lowercase hex and unit enum variants as their name.
 */

/** Decimal string holding a signed 128-bit integer. */
export type I128 = string;
/** Stellar strkey (G... account or C... contract). */
export type StrKey = string;
/** Lowercase hex encoding of 32 bytes. */
export type Hex32 = string;

export type GamePhase =
  | "Waiting"
  | "WaitingForPlayers"
  | "Dealing"
  | "Preflop"
  | "DealingFlop"
  | "Flop"
  | "DealingTurn"
  | "Turn"
  | "DealingRiver"
  | "River"
  | "Showdown"
  | "Settlement"
  | "Dispute"
  | "AwaitingRunItTwice"
  | "ShowdownRun1"
  | "ShowdownRun2"
  | "RitSettlement";

export type ActionType = "fold" | "check" | "call" | "bet" | "raise" | "all_in";

/** Topics and data for each event, keyed by event name. */
export interface PokerTableEventMap {
  table_created: { topics: ["table_created", number]; data: StrKey };
  player_joined: { topics: ["player_joined", number]; data: [StrKey, number] };
  player_left: { topics: ["player_left", number]; data: [StrKey, I128] };
  hand_started: { topics: ["hand_started", number]; data: number };
  deal_committed: { topics: ["deal_committed", number]; data: [number, Hex32[]] };
  player_action: {
    topics: ["player_action", number, ActionType];
    data: [StrKey, I128];
  };
  board_revealed: { topics: ["board_revealed", number]; data: [number[], number[]] };
  phase_change: { topics: ["phase_change", number]; data: GamePhase };
  hand_settled: {
    topics: ["hand_settled", number];
    data: [StrKey, I128, [number, I128][]];
  };
  fold_win: { topics: ["fold_win", number]; data: [StrKey, I128] };
  rake_collected: {
    topics: ["rake_collected", number];
    /** Five fields after a showdown, three after a fold win or run it twice. */
    data: [number, I128, I128, I128, I128] | [number, I128, I128];
  };
}

export type PokerTableEventName = keyof PokerTableEventMap;

export type DecodedPokerTableEvent = {
  [K in PokerTableEventName]: { name: K } & PokerTableEventMap[K];
}[PokerTableEventName];

/** Topic count and allowed data lengths (for tuple data) per event. */
const SHAPES: Record<PokerTableEventName, { topics: number; data?: number[] }> = {
  table_created: { topics: 2 },
  player_joined: { topics: 2, data: [2] },
  player_left: { topics: 2, data: [2] },
  hand_started: { topics: 2 },
  deal_committed: { topics: 2, data: [2] },
  player_action: { topics: 3, data: [2] },
  board_revealed: { topics: 2, data: [2] },
  phase_change: { topics: 2 },
  hand_settled: { topics: 2, data: [3] },
  fold_win: { topics: 2, data: [2] },
  rake_collected: { topics: 2, data: [5, 3] },
};

export const POKER_TABLE_EVENT_NAMES = Object.keys(SHAPES) as PokerTableEventName[];

function isEventName(name: unknown): name is PokerTableEventName {
  return typeof name === "string" && name in SHAPES;
}

function toScVal(value: xdr.ScVal | string): xdr.ScVal {
  return typeof value === "string" ? xdr.ScVal.fromXDR(value, "base64") : value;
}

/** Convert `scValToNative` output to the canonical JSON form. */
export function toCanonical(native: unknown): unknown {
  if (typeof native === "bigint") return native.toString();
  if (ArrayBuffer.isView(native)) {
    const bytes = new Uint8Array(native.buffer, native.byteOffset, native.byteLength);
    return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  }
  if (Array.isArray(native)) return native.map(toCanonical);
  return native;
}

/**
 * Decode a poker-table event. Topics and data may be `xdr.ScVal` objects or
 * base64 XDR strings, as returned by RPC `getEvents`.
 *
 * Returns `null` for events without a schema and throws when a known event
 * has the wrong number of topics or data fields.
 */
export function decodePokerTableEvent(
  rawTopics: (xdr.ScVal | string)[],
  rawData: xdr.ScVal | string
): DecodedPokerTableEvent | null {
  const topics = rawTopics.map((t) => toCanonical(scValToNative(toScVal(t))));
  const name = topics[0];
  if (!isEventName(name)) return null;

  const shape = SHAPES[name];
  if (topics.length !== shape.topics) {
    throw new Error(`${name}: expected ${shape.topics} topics, got ${topics.length}`);
  }

  let data = toCanonical(scValToNative(toScVal(rawData)));
  // A unit enum variant arrives as a one-element vector holding its name.
  if (name === "phase_change" && Array.isArray(data)) {
    data = data[0];
  }
  if (shape.data) {
    const length = Array.isArray(data) ? data.length : -1;
    if (!shape.data.includes(length)) {
      throw new Error(`${name}: unexpected data shape ${JSON.stringify(data)}`);
    }
  }

  return { name, topics, data } as DecodedPokerTableEvent;
}
