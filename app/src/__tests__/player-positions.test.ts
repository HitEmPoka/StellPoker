import { describe, it, expect, vi } from "vitest";
import {
  MAX_POSITIONS_BATCH,
  chunkTableIds,
  fetchPlayerPositions,
  parsePlayerPosition,
  seatedPositions,
  type PlayerPosition,
} from "@/lib/player-positions";

/** A position as `scValToNative` decodes the contract's `PlayerPosition`. */
function native(tableId: number, overrides: Record<string, unknown> = {}) {
  return {
    table_id: tableId,
    exists: true,
    seated: true,
    seat_index: 1,
    stack: BigInt(500),
    committed: BigInt(10),
    total_buy_in: BigInt(600),
    phase: ["Preflop"],
    hand_number: 3,
    ...overrides,
  };
}

describe("chunkTableIds", () => {
  it("returns no batches for no tables", () => {
    expect(chunkTableIds([])).toEqual([]);
  });

  it("keeps up to the contract limit in one batch", () => {
    const ids = Array.from({ length: MAX_POSITIONS_BATCH }, (_, i) => i);
    expect(chunkTableIds(ids)).toEqual([ids]);
  });

  it("splits past the limit so no call is BatchTooLarge", () => {
    const ids = Array.from({ length: MAX_POSITIONS_BATCH * 2 + 1 }, (_, i) => i);
    const batches = chunkTableIds(ids);
    expect(batches.map((b) => b.length)).toEqual([MAX_POSITIONS_BATCH, MAX_POSITIONS_BATCH, 1]);
    expect(batches.flat()).toEqual(ids);
  });

  it("drops duplicate ids, keeping first-seen order", () => {
    expect(chunkTableIds([4, 2, 4, 9, 2])).toEqual([[4, 2, 9]]);
  });

  it.each([-1, 1.5, NaN, 2 ** 32])("rejects the invalid table id %s", (bad) => {
    expect(() => chunkTableIds([1, bad])).toThrow(/Invalid table id/);
  });

  it("accepts the largest u32 id", () => {
    expect(chunkTableIds([0xffffffff])).toEqual([[0xffffffff]]);
  });

  it("rejects a non-positive batch size", () => {
    expect(() => chunkTableIds([1], 0)).toThrow(/positive integer/);
  });
});

describe("parsePlayerPosition", () => {
  it("maps the contract struct to camelCase with bigint chip amounts", () => {
    expect(parsePlayerPosition(native(7))).toEqual({
      tableId: 7,
      exists: true,
      seated: true,
      seatIndex: 1,
      stack: BigInt(500),
      committed: BigInt(10),
      totalBuyIn: BigInt(600),
      phase: "Preflop",
      handNumber: 3,
    });
  });

  it("keeps i128 amounts exact beyond 2^53", () => {
    const big = BigInt("170141183460469231731687303715884105727");
    expect(parsePlayerPosition(native(1, { stack: big })).stack).toBe(big);
  });

  it("accepts a bare-string phase as well as the decoded enum array", () => {
    expect(parsePlayerPosition(native(1, { phase: "Waiting" })).phase).toBe("Waiting");
  });

  it("rejects an unrecognised phase and a non-object entry", () => {
    expect(() => parsePlayerPosition(native(1, { phase: 5 }))).toThrow(/phase/);
    expect(() => parsePlayerPosition(null)).toThrow(/not an object/);
    expect(() => parsePlayerPosition("x")).toThrow(/not an object/);
  });
});

describe("fetchPlayerPositions", () => {
  it("makes one call per 20 tables and returns positions in id order", async () => {
    const ids = Array.from({ length: 45 }, (_, i) => i + 100);
    const read = vi.fn(async (batch: number[]) => batch.map((id) => native(id)));

    const positions = await fetchPlayerPositions(ids, read);

    expect(read).toHaveBeenCalledTimes(3);
    expect(read.mock.calls.map(([b]) => b.length)).toEqual([20, 20, 5]);
    expect(positions.map((p) => p.tableId)).toEqual(ids);
  });

  it("does not call the contract for an empty list", async () => {
    const read = vi.fn();
    expect(await fetchPlayerPositions([], read)).toEqual([]);
    expect(read).not.toHaveBeenCalled();
  });

  it("asks once for a table listed twice", async () => {
    const read = vi.fn(async (batch: number[]) => batch.map((id) => native(id)));
    const positions = await fetchPlayerPositions([3, 3, 3], read);
    expect(read).toHaveBeenCalledWith([3]);
    expect(positions).toHaveLength(1);
  });

  it("fails rather than misalign when the response is the wrong length", async () => {
    await expect(fetchPlayerPositions([1, 2], async () => [native(1)])).rejects.toThrow(
      /Expected 2 positions, got 1/
    );
    await expect(fetchPlayerPositions([1], async () => "nope")).rejects.toThrow(
      /non-list response/
    );
  });

  it("fails when an entry answers for a different table", async () => {
    await expect(
      fetchPlayerPositions([1, 2], async () => [native(1), native(9)])
    ).rejects.toThrow(/expected 2/);
  });

  it("propagates a contract error such as BatchTooLarge", async () => {
    await expect(
      fetchPlayerPositions([1], async () => {
        throw new Error("Simulation failed for get_player_positions: Error(Contract, #119)");
      })
    ).rejects.toThrow(/#119/);
  });

  it("surfaces an invalid id before any network call", async () => {
    const read = vi.fn();
    await expect(fetchPlayerPositions([1, -5], read)).rejects.toThrow(/Invalid table id/);
    expect(read).not.toHaveBeenCalled();
  });
});

describe("seatedPositions", () => {
  it("keeps only tables that exist and where the wallet holds a seat", () => {
    const all: PlayerPosition[] = [
      parsePlayerPosition(native(1)),
      parsePlayerPosition(native(2, { seated: false })),
      parsePlayerPosition(native(3, { exists: false, seated: false })),
      parsePlayerPosition(native(4)),
    ];
    expect(seatedPositions(all).map((p) => p.tableId)).toEqual([1, 4]);
  });
});
