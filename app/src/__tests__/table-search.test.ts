import { describe, it, expect } from "vitest";
import {
  isAddressPrefix,
  isFullAddress,
  searchSeats,
  matchTable,
  shortenAddress,
  describeSeatMatch,
  type SearchableSeat,
} from "@/lib/table-search";

/** Valid 56-character Stellar public keys (G + 55 base32 chars). */
const ALICE = "GA" + "A".repeat(54);
const BOB = "GB" + "B".repeat(54);
const CAROL = "GC" + "C".repeat(54);

const SEATS: SearchableSeat[] = [
  { seat_index: 0, chain_address: "CONTRACT_SEAT_0", wallet_address: ALICE },
  { seat_index: 1, chain_address: "CONTRACT_SEAT_1", wallet_address: BOB },
  { seat_index: 2, chain_address: CAROL, wallet_address: null },
];

const ALIASES: Record<string, string> = {
  [ALICE]: "Alice",
  [BOB]: "Bobby",
};

const resolveAlias = (address: string) => ALIASES[address] ?? null;
const noAliases = () => null;

describe("address prefix detection", () => {
  it("accepts a bare G and any leading slice of a key", () => {
    expect(isAddressPrefix("G")).toBe(true);
    expect(isAddressPrefix("GA")).toBe(true);
    expect(isAddressPrefix(ALICE.slice(0, 10))).toBe(true);
    expect(isAddressPrefix(ALICE)).toBe(true);
  });

  it("is case-insensitive about the query", () => {
    expect(isAddressPrefix("gabc")).toBe(true);
  });

  it("rejects things that cannot start a Stellar key", () => {
    // Keys start with G, use base32 (no 0/1/8/9), and are at most 56 chars.
    expect(isAddressPrefix("Alice")).toBe(false);
    expect(isAddressPrefix("GA0")).toBe(false);
    expect(isAddressPrefix("GA1")).toBe(false);
    expect(isAddressPrefix("12")).toBe(false);
    expect(isAddressPrefix(ALICE + "A")).toBe(false);
  });

  it("only calls a complete 56-character key a full address", () => {
    expect(isFullAddress(ALICE)).toBe(true);
    expect(isFullAddress(ALICE.slice(0, 20))).toBe(false);
  });
});

describe("searchSeats", () => {
  it("finds nothing for an empty query", () => {
    expect(searchSeats("", SEATS, resolveAlias)).toEqual([]);
    expect(searchSeats("   ", SEATS, resolveAlias)).toEqual([]);
  });

  it("matches an alias as a case-insensitive substring", () => {
    const matches = searchSeats("bob", SEATS, resolveAlias);
    expect(matches).toHaveLength(1);
    expect(matches[0]).toMatchObject({
      seatIndex: 1,
      alias: "Bobby",
      kind: "alias",
    });
  });

  it("matches a public key prefix", () => {
    const matches = searchSeats(BOB.slice(0, 8), SEATS, noAliases);
    expect(matches.map((m) => m.seatIndex)).toEqual([1]);
    expect(matches[0].kind).toBe("address-prefix");
  });

  it("reports a complete key as a full-address match", () => {
    const matches = searchSeats(ALICE, SEATS, noAliases);
    expect(matches[0].kind).toBe("address");
  });

  it("matches a prefix regardless of the case typed", () => {
    expect(searchSeats(BOB.slice(0, 8).toLowerCase(), SEATS, noAliases)).toHaveLength(1);
  });

  it("matches on a prefix, never on a fragment from the middle of a key", () => {
    // "AAAA" appears inside Alice's key but is not how it starts.
    expect(searchSeats("AAAA", SEATS, noAliases)).toEqual([]);
  });

  it("searches the chain address too, for seats with no wallet mapping", () => {
    const matches = searchSeats(CAROL.slice(0, 6), SEATS, noAliases);
    expect(matches.map((m) => m.seatIndex)).toEqual([2]);
    expect(matches[0].address).toBe(CAROL);
  });

  it("prefers the alias reason when both an alias and a prefix could match", () => {
    const matches = searchSeats("GA", SEATS, () => "GAMER");
    expect(matches.every((m) => m.kind === "alias")).toBe(true);
  });

  it("returns matches in seat order", () => {
    const matches = searchSeats("G", SEATS, noAliases);
    expect(matches.map((m) => m.seatIndex)).toEqual([0, 1, 2]);
  });

  it("carries the alias along on an address match, when one is known", () => {
    const matches = searchSeats(BOB.slice(0, 8), SEATS, resolveAlias);
    expect(matches[0].alias).toBe("Bobby");
    expect(matches[0].kind).toBe("address-prefix");
  });
});

describe("matchTable", () => {
  it("keeps every table while the query is empty", () => {
    const result = matchTable("", 7, SEATS, resolveAlias);
    expect(result.matched).toBe(true);
    expect(result.seats).toEqual([]);
  });

  it("matches on the table number", () => {
    const result = matchTable("7", 7, [], noAliases);
    expect(result.matched).toBe(true);
    expect(result.matchedTableId).toBe(true);
  });

  it("tolerates a leading # on a table number", () => {
    expect(matchTable("#12", 12, [], noAliases).matched).toBe(true);
  });

  it("matches a table number by substring, so 1 finds 21", () => {
    expect(matchTable("1", 21, [], noAliases).matched).toBe(true);
  });

  it("matches a table through one of its seats", () => {
    const result = matchTable("alice", 3, SEATS, resolveAlias);
    expect(result.matched).toBe(true);
    expect(result.matchedTableId).toBe(false);
    expect(result.seats.map((s) => s.seatIndex)).toEqual([0]);
  });

  it("rejects a table whose seats and number both miss", () => {
    expect(matchTable("zzz", 3, SEATS, resolveAlias).matched).toBe(false);
  });

  it("still matches on seats when the lobby detail is missing", () => {
    // Seat detail can fail to load for an individual table; the number should
    // keep working rather than the row vanishing.
    expect(matchTable("3", 3, [], noAliases).matched).toBe(true);
    expect(matchTable("alice", 3, [], resolveAlias).matched).toBe(false);
  });
});

describe("result labels", () => {
  it("shortens a long key and leaves a short one alone", () => {
    expect(shortenAddress(ALICE)).toBe("GAAAAA…AAAA");
    expect(shortenAddress("SEAT_1")).toBe("SEAT_1");
  });

  it("describes an alias match by name and a key match by short key", () => {
    expect(
      describeSeatMatch({ seatIndex: 0, address: ALICE, alias: "Alice", kind: "alias" })
    ).toBe("seat 1: Alice");
    expect(
      describeSeatMatch({ seatIndex: 2, address: CAROL, kind: "address-prefix" })
    ).toBe("seat 3: GCCCCC…CCCC");
  });
});
