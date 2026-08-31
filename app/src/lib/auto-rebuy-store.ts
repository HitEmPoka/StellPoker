/**
 * Client-side auto-rebuy preference storage, keyed by table + Stellar
 * address (Issue #164). Purely local — no on-chain or coordinator
 * round-trip — so it persists per-browser via localStorage, per table,
 * per player, the same way alias-store.ts persists display aliases.
 */
import type { AutoRebuyPreference } from "./auto-rebuy";

const STORAGE_PREFIX = "stellpoker:auto-rebuy:";

function storageKey(tableId: number, address: string): string {
  return `${STORAGE_PREFIX}${tableId}:${address}`;
}

const DEFAULT_PREFERENCE: AutoRebuyPreference = { mode: "never" };

export function getAutoRebuyPreference(tableId: number, address: string): AutoRebuyPreference {
  if (typeof window === "undefined") return DEFAULT_PREFERENCE;
  try {
    const raw = window.localStorage.getItem(storageKey(tableId, address));
    if (!raw) return DEFAULT_PREFERENCE;
    const parsed = JSON.parse(raw) as Partial<AutoRebuyPreference>;
    if (parsed.mode !== "always_max" && parsed.mode !== "below_threshold" && parsed.mode !== "never") {
      return DEFAULT_PREFERENCE;
    }
    return { mode: parsed.mode, thresholdBB: parsed.thresholdBB };
  } catch {
    return DEFAULT_PREFERENCE;
  }
}

export function setAutoRebuyPreference(
  tableId: number,
  address: string,
  preference: AutoRebuyPreference
): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(storageKey(tableId, address), JSON.stringify(preference));
  } catch {
    // Storage unavailable (private browsing, quota) — preference just won't persist.
  }
}
