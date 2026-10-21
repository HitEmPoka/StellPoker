/**
 * Turn-timer preferences. Stored per-browser (localStorage), per table.
 * Analogous pattern to auto-rebuy store in `auto-rebuy-store.ts`.
 */
export type AutoActionOnTimeout = "fold" | "check_if_possible" | "never";

export interface TurnTimerPreference {
  /** Seconds allotted per turn before auto-action fires. Default 30s. */
  durationSeconds: number;
  /** What to do when the timer hits 0. Default: "fold" (safe, tournament-like). */
  autoAction: AutoActionOnTimeout;
  /** If false, show countdown visuals but do NOT auto-execute (just a warning). */
  enabled: boolean;
}

const STORAGE_PREFIX = "stellpoker:turn-timer:";

export const DEFAULT_TURN_TIMER: TurnTimerPreference = {
  durationSeconds: 30,
  autoAction: "fold",
  enabled: true,
};

function storageKey(tableId: number, address: string): string {
  return `${STORAGE_PREFIX}${tableId}:${address}`;
}

function clampDuration(seconds: number): number {
  if (!Number.isFinite(seconds)) return DEFAULT_TURN_TIMER.durationSeconds;
  return Math.max(5, Math.min(180, Math.floor(seconds)));
}

export function getTurnTimerPreference(
  tableId: number,
  address: string
): TurnTimerPreference {
  if (typeof window === "undefined") return { ...DEFAULT_TURN_TIMER };
  try {
    const raw = window.localStorage.getItem(storageKey(tableId, address));
    if (!raw) return { ...DEFAULT_TURN_TIMER };
    const parsed = JSON.parse(raw) as Partial<TurnTimerPreference>;
    const autoAction: AutoActionOnTimeout =
      parsed.autoAction === "fold" ||
      parsed.autoAction === "check_if_possible" ||
      parsed.autoAction === "never"
        ? parsed.autoAction
        : DEFAULT_TURN_TIMER.autoAction;
    return {
      durationSeconds: clampDuration(parsed.durationSeconds ?? DEFAULT_TURN_TIMER.durationSeconds),
      autoAction,
      enabled: parsed.enabled ?? DEFAULT_TURN_TIMER.enabled,
    };
  } catch {
    return { ...DEFAULT_TURN_TIMER };
  }
}

export function setTurnTimerPreference(
  tableId: number,
  address: string,
  pref: TurnTimerPreference
): void {
  if (typeof window === "undefined") return;
  try {
    const normalized: TurnTimerPreference = {
      durationSeconds: clampDuration(pref.durationSeconds),
      autoAction: pref.autoAction,
      enabled: !!pref.enabled,
    };
    window.localStorage.setItem(
      storageKey(tableId, address),
      JSON.stringify(normalized)
    );
  } catch {
    // Storage unavailable (private browsing, quota) — preference won't persist.
  }
}
