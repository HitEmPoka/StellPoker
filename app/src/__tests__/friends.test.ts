import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  loadFriends,
  addFriend,
  removeFriend,
  setFriendAlias,
  setFriendInvited,
  markFriendPresence,
  computeOnlineAddresses,
  tablesOccupiedBy,
  displayName,
  shortAddr,
} from "@/lib/friends";
import type { OpenTable } from "@/lib/open-tables";

const ADDR_A = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA55";
const ADDR_B = "GBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB55";

function setupStorage() {
  const store = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => { store.set(key, value); },
    removeItem: (key: string) => { store.delete(key); },
    clear: () => store.clear(),
    length: 0,
    key: () => null,
  });
  return store;
}

describe("friends store", () => {
  beforeEach(setupStorage);
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("adds a friend and persists it", () => {
    const next = addFriend(ADDR_A, "Alice");
    expect(next).toHaveLength(1);
    expect(next[0].address).toBe(ADDR_A);
    expect(next[0].alias).toBe("Alice");
    expect(loadFriends()).toHaveLength(1);
  });

  it("does not duplicate a friend", () => {
    addFriend(ADDR_A, "Alice");
    addFriend(ADDR_A, "Alice2");
    expect(loadFriends()).toHaveLength(1);
  });

  it("removes a friend", () => {
    addFriend(ADDR_A, "Alice");
    addFriend(ADDR_B, "Bob");
    const next = removeFriend(ADDR_A);
    expect(next).toHaveLength(1);
    expect(next[0].address).toBe(ADDR_B);
  });

  it("updates a friend alias", () => {
    addFriend(ADDR_A, "Alice");
    const next = setFriendAlias(ADDR_A, "Ace");
    expect(next[0].alias).toBe("Ace");
  });

  it("marks a friend invited", () => {
    addFriend(ADDR_A);
    const next = setFriendInvited(ADDR_A, true);
    expect(next[0].invited).toBe(true);
  });

  it("marks friend presence", () => {
    addFriend(ADDR_A);
    const next = markFriendPresence(ADDR_A, true);
    expect(next[0].online).toBe(true);
  });

  it("trims aliases", () => {
    addFriend(ADDR_A, "  Some Very Long Alias That Should Be Trimmed  ");
    expect(loadFriends()[0].alias?.length).toBeLessThanOrEqual(16);
  });
});

describe("computeOnlineAddresses", () => {
  it("collects distinct seated addresses across open tables", () => {
    const tables: OpenTable[] = [
      { tableId: 1, lastVisited: 1 },
      { tableId: 2, lastVisited: 2 },
    ];
    const seatedAt = (id: number): string[] =>
      id === 1 ? [ADDR_A] : [ADDR_A, ADDR_B];
    const online = computeOnlineAddresses(tables, seatedAt);
    expect(online.has(ADDR_A)).toBe(true);
    expect(online.has(ADDR_B)).toBe(true);
  });
});

describe("tablesOccupiedBy", () => {
  it("maps each friend to the tables they occupy", () => {
    const tables: OpenTable[] = [
      { tableId: 1, lastVisited: 1 },
      { tableId: 5, lastVisited: 2 },
    ];
    const seatedAt = (id: number): string[] => (id === 1 ? [ADDR_A] : [ADDR_A, ADDR_B]);
    const occupied = tablesOccupiedBy(tables, seatedAt);
    expect(occupied[ADDR_A]).toEqual([1, 5]);
    expect(occupied[ADDR_B]).toEqual([5]);
  });
});

describe("displayName / shortAddr", () => {
  it("uses alias when present", () => {
    expect(displayName({ address: ADDR_A, alias: "Alice", online: false, invited: false })).toBe("Alice");
  });

  it("falls back to short address", () => {
    expect(displayName({ address: ADDR_A, alias: null, online: false, invited: false })).toBe(shortAddr(ADDR_A));
  });

  it("truncates long addresses", () => {
    const s = shortAddr(ADDR_A);
    expect(s).toContain("…");
    expect(s.length).toBeLessThan(ADDR_A.length);
  });
});
