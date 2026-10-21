/**
 * A wallet's chips across every table it is seated at, fetched with the
 * contract's bounded batch read `get_player_positions` instead of one
 * `get_table` per table (#560).
 *
 * This module is the pure part — chunking, response parsing and result
 * assembly — so it can be tested without an RPC. `getPlayerPositions` in
 * `onchain.ts` supplies the RPC call.
 */

/** Table ids the contract accepts per call (`MAX_POSITIONS_BATCH`). More is a
 * `BatchTooLarge` error, so requests are split to this size. */
export const MAX_POSITIONS_BATCH = 20;

/** One wallet's position at one table. */
export interface PlayerPosition {
  tableId: number;
  /** The table exists on-chain. */
  exists: boolean;
  /** The wallet holds a seat at it. */
  seated: boolean;
  seatIndex: number;
  /** Chips behind, in the token's smallest unit. */
  stack: bigint;
  /** Chips in the pot this hand. */
  committed: bigint;
  totalBuyIn: bigint;
  /** Game phase, e.g. `Waiting`, `Preflop`, `Settlement`. */
  phase: string;
  handNumber: number;
}

/** Reads one batch of at most `MAX_POSITIONS_BATCH` ids and returns the raw
 * contract response, one entry per id, in order. */
export type ReadPositionsBatch = (tableIds: number[]) => Promise<unknown>;

/** Removes duplicates (keeping first occurrence) and rejects anything that is
 * not a valid `u32` table id, then splits into batches the contract accepts. */
export function chunkTableIds(
  tableIds: readonly number[],
  size: number = MAX_POSITIONS_BATCH
): number[][] {
  if (!Number.isInteger(size) || size < 1) {
    throw new Error("Batch size must be a positive integer");
  }
  const unique: number[] = [];
  const seen = new Set<number>();
  for (const id of tableIds) {
    if (!Number.isInteger(id) || id < 0 || id > 0xffffffff) {
      throw new Error(`Invalid table id: ${id}`);
    }
    if (!seen.has(id)) {
      seen.add(id);
      unique.push(id);
    }
  }
  const batches: number[][] = [];
  for (let i = 0; i < unique.length; i += size) {
    batches.push(unique.slice(i, i + size));
  }
  return batches;
}

/** A Soroban unit-enum value decodes as `["Variant"]`; accept a bare string too. */
function variantName(native: unknown): string {
  if (typeof native === "string") return native;
  if (Array.isArray(native) && typeof native[0] === "string") return native[0];
  throw new Error("Unrecognised game phase in position response");
}

/** Converts one decoded `PlayerPosition` struct from the contract. */
export function parsePlayerPosition(native: unknown): PlayerPosition {
  if (typeof native !== "object" || native === null) {
    throw new Error("Position response entry is not an object");
  }
  const p = native as Record<string, unknown>;
  return {
    tableId: Number(p.table_id),
    exists: Boolean(p.exists),
    seated: Boolean(p.seated),
    seatIndex: Number(p.seat_index),
    stack: BigInt(p.stack as bigint | number | string),
    committed: BigInt(p.committed as bigint | number | string),
    totalBuyIn: BigInt(p.total_buy_in as bigint | number | string),
    phase: variantName(p.phase),
    handNumber: Number(p.hand_number),
  };
}

/**
 * Fetches positions for `tableIds` in as few calls as the contract allows and
 * returns them in the order the ids were first given (duplicates collapsed).
 * A batch that answers with the wrong number of entries is an error rather
 * than a silently misaligned dashboard.
 */
export async function fetchPlayerPositions(
  tableIds: readonly number[],
  readBatch: ReadPositionsBatch
): Promise<PlayerPosition[]> {
  const batches = chunkTableIds(tableIds);
  const results = await Promise.all(
    batches.map(async (ids) => {
      const native = await readBatch(ids);
      if (!Array.isArray(native) || native.length !== ids.length) {
        throw new Error(
          `Expected ${ids.length} positions, got ${
            Array.isArray(native) ? native.length : "a non-list response"
          }`
        );
      }
      const parsed = native.map(parsePlayerPosition);
      parsed.forEach((position, i) => {
        if (position.tableId !== ids[i]) {
          throw new Error(
            `Position ${i} is for table ${position.tableId}, expected ${ids[i]}`
          );
        }
      });
      return parsed;
    })
  );
  return results.flat();
}

/** Only the tables where the wallet actually holds a seat. */
export function seatedPositions(positions: readonly PlayerPosition[]): PlayerPosition[] {
  return positions.filter((p) => p.exists && p.seated);
}
