/**
 * Live hand timeline (#176).
 *
 * A player who drops for thirty seconds — a tunnel, a locked screen, a tab
 * suspend — comes back to a board that has moved on with no record of how it
 * got there. This records each moment of the *current* hand as it happens and
 * lets the player step back through them without leaving the live table.
 *
 * It is deliberately separate from `hand-history.ts`, which archives hands
 * once they are over. This is the in-flight view: it resets each hand, and it
 * persists to localStorage so it survives a reload mid-hand, which is exactly
 * the disconnect case the issue describes.
 */

export type TimelineStreet =
  | "waiting"
  | "preflop"
  | "flop"
  | "turn"
  | "river"
  | "showdown"
  | "settlement";

export type TimelineKind = "deal" | "street" | "action" | "settlement";

export interface TimelineEvent {
  /** Stable identity, so a re-render or a repeated poll can't duplicate it. */
  id: string;
  kind: TimelineKind;
  /** Short label for the marker, e.g. "FLOP" or "RAISE 60". */
  label: string;
  /** Wall-clock ms when the moment was first observed. */
  timestamp: number;
  street: TimelineStreet;
  /** Pot as it stood at this moment. */
  pot: number;
  /** Board cards visible at this moment. */
  boardCards: number[];
  /** Address of the player who acted, for action events. */
  actor?: string;
  /** Chips involved, for action events. */
  amount?: number;
}

/** The table state as it stood at one point on the timeline. */
export interface TimelineSnapshot {
  index: number;
  label: string;
  street: TimelineStreet;
  pot: number;
  boardCards: number[];
  timestamp: number;
  actor?: string;
  amount?: number;
  /** True when this is the most recent event — i.e. the live state. */
  isLive: boolean;
}

const STORAGE_PREFIX = "stellpoker:hand-timeline:";

/**
 * A hand has at most a few dozen moments. The cap only exists so a table left
 * open through a stuck phase can't grow the entry without bound.
 */
const MAX_EVENTS = 120;

interface StoredTimeline {
  handNumber: number;
  events: TimelineEvent[];
}

function storageKey(tableId: number): string {
  return `${STORAGE_PREFIX}${tableId}`;
}

// ── Event construction ───────────────────────────────────────────────────────

/**
 * Builds an event id from what the moment *is* rather than when it was seen.
 *
 * The table re-syncs from several sources at once (chain events, a WebSocket
 * push, and an interval poll), so the same moment is observed repeatedly. A
 * content-derived id makes appending idempotent.
 */
export function eventId(
  handNumber: number,
  kind: TimelineKind,
  discriminator: string
): string {
  return `${handNumber}:${kind}:${discriminator}`;
}

/**
 * Adds an event unless one with the same id is already recorded.
 *
 * Returns the original array when nothing changed, so a React state setter can
 * bail out of a re-render by identity.
 */
export function appendEvent(
  events: readonly TimelineEvent[],
  event: TimelineEvent
): TimelineEvent[] {
  if (events.some((existing) => existing.id === event.id)) {
    return events as TimelineEvent[];
  }
  return [...events, event].slice(-MAX_EVENTS);
}

// ── Deriving events from table state ─────────────────────────────────────────

/** The slice of live table state the timeline watches. */
export interface TimelineObservation {
  handNumber: number;
  phase: TimelineStreet;
  pot: number;
  boardCards: number[];
  /** Address whose turn it is, used to attribute a street's action. */
  turnAddress?: string;
}

const STREET_LABELS: Partial<Record<TimelineStreet, string>> = {
  preflop: "DEAL",
  flop: "FLOP",
  turn: "TURN",
  river: "RIVER",
  showdown: "SHOWDOWN",
  settlement: "PAYOUT",
};

/**
 * Turns an observation of the table into the event it represents, or `null`
 * when this moment isn't one worth a marker.
 *
 * Street changes and pot movements are what a returning player needs to
 * reconstruct: "the flop came, then the pot went from 40 to 160". Both are
 * visible in the state the table already polls, so the timeline needs no extra
 * endpoint.
 */
export function observeEvent(
  observation: TimelineObservation,
  previous: TimelineEvent | undefined,
  now: number
): TimelineEvent | null {
  const { handNumber, phase, pot, boardCards } = observation;

  if (phase === "waiting") return null;

  // A new street always gets a marker, whether or not the pot moved.
  if (!previous || previous.street !== phase) {
    return {
      id: eventId(handNumber, phase === "preflop" ? "deal" : "street", phase),
      kind: phase === "preflop" ? "deal" : phase === "settlement" ? "settlement" : "street",
      label: STREET_LABELS[phase] ?? phase.toUpperCase(),
      timestamp: now,
      street: phase,
      pot,
      boardCards: [...boardCards],
    };
  }

  // Same street, more chips in the middle: someone bet, called, or raised.
  if (pot > previous.pot) {
    const amount = pot - previous.pot;
    return {
      id: eventId(handNumber, "action", `${phase}:${pot}`),
      kind: "action",
      label: `+${amount.toLocaleString()}`,
      timestamp: now,
      street: phase,
      pot,
      boardCards: [...boardCards],
      actor: observation.turnAddress,
      amount,
    };
  }

  return null;
}

// ── Reading the timeline ─────────────────────────────────────────────────────

/** The state of the table as of `index`, or `null` for an empty timeline. */
export function snapshotAt(
  events: readonly TimelineEvent[],
  index: number
): TimelineSnapshot | null {
  if (events.length === 0) return null;
  const clamped = Math.max(0, Math.min(index, events.length - 1));
  const event = events[clamped];

  return {
    index: clamped,
    label: event.label,
    street: event.street,
    pot: event.pot,
    boardCards: event.boardCards,
    timestamp: event.timestamp,
    actor: event.actor,
    amount: event.amount,
    isLive: clamped === events.length - 1,
  };
}

/**
 * Elapsed time from the start of the hand, as `m:ss` — more useful on a
 * timeline than a wall clock, since what a returning player wants to know is
 * how long ago something happened relative to the hand.
 */
export function formatElapsed(
  events: readonly TimelineEvent[],
  index: number
): string {
  const origin = events[0]?.timestamp;
  const at = events[index]?.timestamp;
  if (origin === undefined || at === undefined) return "0:00";

  const seconds = Math.max(0, Math.round((at - origin) / 1000));
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

/** Wall-clock `HH:MM:SS` for the marker's tooltip. */
export function formatClock(timestamp: number): string {
  return new Date(timestamp).toLocaleTimeString();
}

// ── Persistence ──────────────────────────────────────────────────────────────

/**
 * Reads the stored timeline for a table, but only if it belongs to the hand
 * being played now — a timeline from a finished hand is stale by definition.
 */
export function loadTimeline(
  tableId: number,
  handNumber: number
): TimelineEvent[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(storageKey(tableId));
    if (!raw) return [];
    const parsed = JSON.parse(raw) as StoredTimeline;
    if (!parsed || parsed.handNumber !== handNumber) return [];
    return Array.isArray(parsed.events) ? parsed.events : [];
  } catch {
    return [];
  }
}

/** Persists the current hand's timeline so a reload mid-hand keeps it. */
export function saveTimeline(
  tableId: number,
  handNumber: number,
  events: readonly TimelineEvent[]
): void {
  if (typeof window === "undefined") return;
  try {
    const payload: StoredTimeline = { handNumber, events: [...events] };
    window.localStorage.setItem(storageKey(tableId), JSON.stringify(payload));
  } catch {
    // Storage unavailable (private browsing, quota) — the timeline just won't
    // survive a reload, which degrades to the pre-#176 behaviour.
  }
}
