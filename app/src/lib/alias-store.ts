/**
 * Client-side alias storage, keyed by Stellar address. Aliases are a purely
 * local display preference (no on-chain or coordinator round-trip), so they
 * persist per-browser via localStorage and survive across hands/sessions
 * until the player changes or clears them.
 */

const STORAGE_PREFIX = "stellpoker:alias:";
const MAX_ALIAS_LENGTH = 16;

function storageKey(address: string): string {
  return `${STORAGE_PREFIX}${address}`;
}

/** Loads Alias.
 * @param address - address.
 * @returns string | null.
 * @remarks Reads/writes browser storage.
 */
export function getAlias(address: string): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(storageKey(address));
  } catch {
    return null;
  }
}

/** Persists Alias.
 * @param address - address.
 * @param alias - alias.
 * @remarks Reads/writes browser storage.
 */
export function setAlias(address: string, alias: string): void {
  if (typeof window === "undefined") return;
  const trimmed = alias.trim().slice(0, MAX_ALIAS_LENGTH);
  try {
    if (trimmed.length === 0) {
      window.localStorage.removeItem(storageKey(address));
    } else {
      window.localStorage.setItem(storageKey(address), trimmed);
    }
  } catch {
    // Storage unavailable (private browsing, quota) — alias just won't persist.
  }
}
