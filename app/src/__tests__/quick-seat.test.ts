import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  loadSeatPreference,
  pickQuickSeatTable,
  readTableStakes,
  saveSeatPreference,
  type QuickSeatCandidate,
  type SeatPreference,
  type TableStakes,
} from "@/lib/quick-seat";

const TOKEN = "CTOKEN";
const STAKES: TableStakes = { buyIn: BigInt("1000000000"), token: TOKEN };
const PREF: SeatPreference = { maxPlayers: 6, buyIn: "1000000000", token: TOKEN, seatIndex: 2 };

function table(
  tableId: number,
  opts: { maxPlayers?: number; joinedWallets?: number; stakes?: TableStakes | null } = {}
): QuickSeatCandidate {
  return {
    tableId,
    maxPlayers: opts.maxPlayers ?? 6,
    joinedWallets: opts.joinedWallets ?? 1,
    stakes: opts.stakes === undefined ? STAKES : opts.stakes,
  };
}

describe("pickQuickSeatTable (Issue #159)", () => {
  it("returns null when no table is open", () => {
    expect(pickQuickSeatTable(PREF, [])).toBeNull();
  });

  it("only considers tables of the preferred size", () => {
    expect(pickQuickSeatTable(PREF, [table(1, { maxPlayers: 2 })])).toBeNull();
  });

  it("never picks a table at different stakes", () => {
    const pricier = table(1, { stakes: { buyIn: BigInt("5000000000"), token: TOKEN } });
    const otherToken = table(2, { stakes: { buyIn: BigInt("1000000000"), token: "COTHER" } });
    const unreadable = table(3, { stakes: null });
    expect(pickQuickSeatTable(PREF, [pricier, otherToken, unreadable])).toBeNull();
  });

  it("skips full tables", () => {
    expect(pickQuickSeatTable(PREF, [table(1, { joinedWallets: 6 })])).toBeNull();
  });

  it("prefers the table where the preferred seat is the next one free", () => {
    const best = pickQuickSeatTable(PREF, [
      table(1, { joinedWallets: 4 }),
      table(2, { joinedWallets: 2 }),
    ]);
    expect(best?.tableId).toBe(2);
  });

  it("then prefers the fullest table, then the lowest table id", () => {
    expect(
      pickQuickSeatTable(PREF, [table(1, { joinedWallets: 1 }), table(2, { joinedWallets: 4 })])
        ?.tableId
    ).toBe(2);
    expect(
      pickQuickSeatTable(PREF, [table(5, { joinedWallets: 3 }), table(3, { joinedWallets: 3 })])
        ?.tableId
    ).toBe(3);
  });
});

describe("readTableStakes (Issue #159)", () => {
  it("reads the buy-in and token from the table config", () => {
    expect(readTableStakes({ config: { min_buy_in: "1000000000", token: TOKEN } })).toEqual(
      STAKES
    );
  });

  it("accepts a numeric buy-in", () => {
    expect(readTableStakes({ config: { min_buy_in: 500 } })).toEqual({
      buyIn: BigInt(500),
      token: null,
    });
  });

  it("returns null without a readable buy-in", () => {
    expect(readTableStakes(null)).toBeNull();
    expect(readTableStakes({})).toBeNull();
    expect(readTableStakes({ config: { min_buy_in: "lots" } })).toBeNull();
  });
});

describe("seat preference storage (Issue #159)", () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => { store.set(key, value); },
      removeItem: (key: string) => { store.delete(key); },
      clear: () => store.clear(),
      length: 0,
      key: () => null,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("round-trips a preference, per wallet", () => {
    saveSeatPreference("GROUNDTRIP", PREF);
    expect(loadSeatPreference("GROUNDTRIP")).toEqual(PREF);
    expect(loadSeatPreference("GNOBODY")).toBeNull();
  });

  it("rejects a malformed stored preference", () => {
    window.localStorage.setItem(
      "stellpoker:seat-preference:GBROKEN",
      JSON.stringify({ maxPlayers: 6, buyIn: "lots", seatIndex: 0 })
    );
    expect(loadSeatPreference("GBROKEN")).toBeNull();

    window.localStorage.setItem("stellpoker:seat-preference:GJUNK", "{not json");
    expect(loadSeatPreference("GJUNK")).toBeNull();
  });
});
