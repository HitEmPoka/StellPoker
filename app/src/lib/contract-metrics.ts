/**
 * Contract-level metrics for the admin dashboard (Issue #563).
 *
 * `contracts/poker-table` keeps running counters for hands played, total rake
 * and active seats, and exposes them through two O(1) views:
 * `get_contract_metrics` and `get_table_metrics`. This module turns the native
 * values Soroban returns for those views into typed values, and formats them
 * for display. It has no network dependency so it can be tested directly; the
 * calls themselves live in `onchain.ts`.
 */

/** Aggregate counters across every table on the contract. */
export interface ContractMetrics {
  tablesCreated: number;
  /** Settlements archived across all tables. */
  handsPlayed: bigint;
  /** Lifetime rake, including the jackpot share. Not reduced by withdrawals. */
  totalRake: bigint;
  /** Players currently seated across all tables (queued players excluded). */
  activeSeats: number;
}

/** Aggregate counters for a single table. */
export interface TableMetrics {
  handsPlayed: bigint;
  totalRake: bigint;
  activeSeats: number;
}

function asRecord(native: unknown, view: string): Record<string, unknown> {
  if (typeof native !== "object" || native === null) {
    throw new Error(`${view} returned an unexpected value`);
  }
  return native as Record<string, unknown>;
}

function asBigInt(value: unknown, field: string): bigint {
  if (typeof value === "bigint") return value;
  if (typeof value === "number" && Number.isSafeInteger(value)) return BigInt(value);
  if (typeof value === "string" && /^-?\d+$/.test(value)) return BigInt(value);
  throw new Error(`Invalid ${field} in contract metrics`);
}

function asCount(value: unknown, field: string): number {
  const parsed = asBigInt(value, field);
  if (parsed < BigInt(0) || parsed > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error(`Invalid ${field} in contract metrics`);
  }
  return Number(parsed);
}

/** Parses the native value of `get_contract_metrics`. */
export function parseContractMetrics(native: unknown): ContractMetrics {
  const record = asRecord(native, "get_contract_metrics");
  return {
    tablesCreated: asCount(record.tables_created, "tables_created"),
    handsPlayed: asBigInt(record.hands_played, "hands_played"),
    totalRake: asBigInt(record.total_rake, "total_rake"),
    activeSeats: asCount(record.active_seats, "active_seats"),
  };
}

/** Parses the native value of `get_table_metrics`. */
export function parseTableMetrics(native: unknown): TableMetrics {
  const record = asRecord(native, "get_table_metrics");
  return {
    handsPlayed: asBigInt(record.hands_played, "hands_played"),
    totalRake: asBigInt(record.total_rake, "total_rake"),
    activeSeats: asCount(record.active_seats, "active_seats"),
  };
}

/** Groups digits in thousands: `1234567n` becomes `"1,234,567"`. */
export function formatChipAmount(amount: bigint): string {
  const negative = amount < BigInt(0);
  const digits = (negative ? -amount : amount).toString();
  const grouped = digits.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return negative ? `-${grouped}` : grouped;
}

/**
 * Average rake per settled hand, rounded down, or `null` before the first
 * hand settles.
 */
export function averageRakePerHand(metrics: {
  handsPlayed: bigint;
  totalRake: bigint;
}): bigint | null {
  if (metrics.handsPlayed <= BigInt(0)) return null;
  return metrics.totalRake / metrics.handsPlayed;
}
