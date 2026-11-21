import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  saveLobbyCache,
  loadLobbyCache,
  clearLobbyCache,
  saveTableCache,
  loadTableCache,
  isCacheStale,
  formatLastUpdated,
  DEFAULT_STALE_THRESHOLD_MS,
} from "../lib/lobby-cache";
import type { OpenTableInfo, TableLobbyResponse } from "../lib/api";

describe("Offline-Aware Lobby Cache (Issue #618)", () => {
  let memoryStore: Record<string, string> = {};

  beforeEach(() => {
    memoryStore = {};
    vi.spyOn(localStorage, "getItem").mockImplementation((key: string) => memoryStore[key] ?? null);
    vi.spyOn(localStorage, "setItem").mockImplementation((key: string, val: string) => {
      memoryStore[key] = String(val);
    });
    vi.spyOn(localStorage, "removeItem").mockImplementation((key: string) => {
      delete memoryStore[key];
    });
    vi.spyOn(localStorage, "clear").mockImplementation(() => {
      memoryStore = {};
    });
  });

  const sampleTables: OpenTableInfo[] = [
    {
      table_id: 101,
      max_players: 6,
      open_wallet_slots: 3,
      spectators: 2,
    },
    {
      table_id: 102,
      max_players: 2,
      open_wallet_slots: 1,
      spectators: 0,
    },
  ];

  const sampleLobbies: Record<number, TableLobbyResponse> = {
    101: {
      table_id: 101,
      phase: "Preflop",
      hand_number: 14,
      seats: [],
      dealer_seat: 0,
    },
  };

  it("saves and loads lobby tables from local cache", () => {
    const now = 1_700_000_000_000;
    saveLobbyCache(sampleTables, sampleLobbies, now);

    const loaded = loadLobbyCache();
    expect(loaded).not.toBeNull();
    expect(loaded?.tables).toHaveLength(2);
    expect(loaded?.tables[0].table_id).toBe(101);
    expect(loaded?.tableLobbies[101]?.hand_number).toBe(14);
    expect(loaded?.cachedAt).toBe(now);
  });

  it("clears cached lobby data", () => {
    saveLobbyCache(sampleTables, sampleLobbies);
    expect(loadLobbyCache()).not.toBeNull();

    clearLobbyCache();
    expect(loadLobbyCache()).toBeNull();
  });

  it("saves and loads table state cache", () => {
    const tableId = 42;
    const tableState = { handNumber: 5, pot: 250, phase: "flop" };
    const now = 1_700_000_000_000;

    saveTableCache(tableId, tableState, now);

    const loaded = loadTableCache(tableId);
    expect(loaded).not.toBeNull();
    expect(loaded?.tableId).toBe(tableId);
    expect(loaded?.state).toEqual(tableState);
    expect(loaded?.cachedAt).toBe(now);
  });

  it("identifies stale cache based on threshold", () => {
    const now = 1_700_000_000_000;
    const freshTimestamp = now - 5_000; // 5 seconds ago
    const staleTimestamp = now - (DEFAULT_STALE_THRESHOLD_MS + 10_000); // 40 seconds ago

    expect(isCacheStale(freshTimestamp, DEFAULT_STALE_THRESHOLD_MS, now)).toBe(false);
    expect(isCacheStale(staleTimestamp, DEFAULT_STALE_THRESHOLD_MS, now)).toBe(true);
    expect(isCacheStale(0, DEFAULT_STALE_THRESHOLD_MS, now)).toBe(true);
  });

  it("formats last-updated timestamp human-readably", () => {
    const now = 1_700_000_000_000;
    expect(formatLastUpdated(now - 2_000, now)).toBe("Just now");
    expect(formatLastUpdated(now - 30_000, now)).toBe("30s ago");
    expect(formatLastUpdated(now - 120_000, now)).toBe("2m ago");
    expect(formatLastUpdated(now - 7_200_000, now)).toBe("2h ago");
    expect(formatLastUpdated(0, now)).toBe("Never");
  });
});
