/**
 * Per-player chip stack trend for the seat sparklines (#157).
 *
 * Each seated player's stack is recorded once per hand, between hands when no
 * chips are in the middle, so the sparkline next to a player's name shows how
 * their stack moved over the last few hands rather than jittering with every
 * bet. Like `hand-history.ts`, it is kept in localStorage per table so the
 * trend survives a reload.
 */

/** How many hands of history each sparkline shows. */
export const STACK_TREND_HANDS = 10;

/** Stack Point shape. */
export interface StackPoint {
  handNumber: number;
  stack: number;
}

/** Recorded stack points per player address, oldest first. */
export type StackTrends = Record<string, StackPoint[]>;

const STORAGE_PREFIX = "stellpoker:stack-trend:";

function storageKey(tableId: number): string {
  return `${STORAGE_PREFIX}${tableId}`;
}

/**
 * Records each player's stack as of `handNumber`, keeping the last `limit`
 * hands per player. A hand already on record is overwritten rather than
 * duplicated, since the table re-syncs the same settled state many times.
 *
 * Returns the original object when nothing changed, so a React state setter
 * can bail out of a re-render by identity.
 */
export function recordStacks(
  trends: StackTrends,
  handNumber: number,
  players: ReadonlyArray<{ address: string; stack: number }>,
  limit: number = STACK_TREND_HANDS
): StackTrends {
  let next: StackTrends | null = null;

  for (const { address, stack } of players) {
    const points = trends[address] ?? [];
    const last = points[points.length - 1];
    if (last && last.handNumber === handNumber && last.stack === stack) continue;

    const kept = last && last.handNumber === handNumber ? points.slice(0, -1) : points;
    next = next ?? { ...trends };
    next[address] = [...kept, { handNumber, stack }].slice(-limit);
  }

  return next ?? trends;
}

/**
 * Maps a series of stacks onto a `width` × `height` box as sparkline
 * vertices, oldest on the left. A flat series sits on the vertical middle
 * rather than hugging an edge.
 */
export function sparklinePoints(
  values: readonly number[],
  width: number,
  height: number,
  padding = 1
): Array<{ x: number; y: number }> {
  if (values.length === 0) return [];

  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min;
  const innerWidth = width - padding * 2;
  const innerHeight = height - padding * 2;
  const step = values.length > 1 ? innerWidth / (values.length - 1) : 0;

  return values.map((value, index) => ({
    x: padding + index * step,
    y:
      range === 0
        ? height / 2
        : padding + innerHeight - ((value - min) / range) * innerHeight,
  }));
}

/** Loads Stack Trends.
 * @param tableId - table Id.
 * @returns StackTrends.
 * @remarks Reads/writes browser storage.
 */
export function loadStackTrends(tableId: number): StackTrends {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(storageKey(tableId));
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as StackTrends)
      : {};
  } catch {
    return {};
  }
}

/** Persists Stack Trends.
 * @param tableId - table Id.
 * @param trends - trends.
 * @remarks Reads/writes browser storage.
 */
export function saveStackTrends(tableId: number, trends: StackTrends): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(storageKey(tableId), JSON.stringify(trends));
  } catch {
    // Storage unavailable (private browsing, quota) — the trend just won't
    // survive a reload.
  }
}
