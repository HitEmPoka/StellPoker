/**
 * Offline-aware lobby & table caching (Issue #618).
 *
 * Persists public table listings and table state in client-side storage
 * so spectators, disconnected players, or users in intermittent network
 * conditions can view the public state in read-only mode with stale indicators
 * and clear reconnection cues.
 */

import type { OpenTableInfo, TableLobbyResponse } from "./api";

const LOBBY_CACHE_KEY = "stellpoker:lobby-cache";
const TABLE_CACHE_PREFIX = "stellpoker:table-cache:";
export const DEFAULT_STALE_THRESHOLD_MS = 30_000; // 30 seconds

export interface CachedLobbyData {
  tables: OpenTableInfo[];
  tableLobbies: Record<number, TableLobbyResponse>;
  cachedAt: number;
}

export interface CachedTableData {
  tableId: number;
  state: unknown;
  cachedAt: number;
}

/**
 * Save open tables and table lobbies to local storage.
 */
export function saveLobbyCache(
  tables: OpenTableInfo[],
  tableLobbies: Record<number, TableLobbyResponse> = {},
  timestamp = Date.now()
): void {
  if (typeof window === "undefined") return;
  try {
    const payload: CachedLobbyData = {
      tables,
      tableLobbies,
      cachedAt: timestamp,
    };
    window.localStorage.setItem(LOBBY_CACHE_KEY, JSON.stringify(payload));
  } catch {
    // Ignore storage quota or disabled storage
  }
}

/**
 * Load cached lobby data.
 */
export function loadLobbyCache(): CachedLobbyData | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(LOBBY_CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || !Array.isArray(parsed.tables)) {
      return null;
    }
    return {
      tables: parsed.tables,
      tableLobbies: parsed.tableLobbies || {},
      cachedAt: typeof parsed.cachedAt === "number" ? parsed.cachedAt : 0,
    };
  } catch {
    return null;
  }
}

/**
 * Clear cached lobby data.
 */
export function clearLobbyCache(): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.removeItem(LOBBY_CACHE_KEY);
  } catch {
    // Ignore
  }
}

/**
 * Save public table state for offline/disconnected viewing.
 */
export function saveTableCache(
  tableId: number,
  state: unknown,
  timestamp = Date.now()
): void {
  if (typeof window === "undefined") return;
  try {
    const payload: CachedTableData = {
      tableId,
      state,
      cachedAt: timestamp,
    };
    window.localStorage.setItem(
      `${TABLE_CACHE_PREFIX}${tableId}`,
      JSON.stringify(payload)
    );
  } catch {
    // Ignore
  }
}

/**
 * Load cached public table state.
 */
export function loadTableCache(tableId: number): CachedTableData | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(`${TABLE_CACHE_PREFIX}${tableId}`);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return null;
    return {
      tableId,
      state: parsed.state,
      cachedAt: typeof parsed.cachedAt === "number" ? parsed.cachedAt : 0,
    };
  } catch {
    return null;
  }
}

/**
 * Check whether cached data has aged past the stale threshold.
 */
export function isCacheStale(
  cachedAt: number,
  maxAgeMs = DEFAULT_STALE_THRESHOLD_MS,
  now = Date.now()
): boolean {
  if (!cachedAt || cachedAt <= 0) return true;
  return now - cachedAt > maxAgeMs;
}

/**
 * Format timestamp into human-readable relative string and localized time.
 */
export function formatLastUpdated(cachedAt: number, now = Date.now()): string {
  if (!cachedAt || cachedAt <= 0) return "Never";
  const diffSec = Math.max(0, Math.floor((now - cachedAt) / 1000));
  if (diffSec < 5) return "Just now";
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24) return `${diffHours}h ago`;
  return new Date(cachedAt).toLocaleTimeString();
}

/**
 * Check if the browser currently reports being offline.
 */
export function isBrowserOffline(): boolean {
  if (typeof navigator !== "undefined" && typeof navigator.onLine === "boolean") {
    return !navigator.onLine;
  }
  return false;
}
