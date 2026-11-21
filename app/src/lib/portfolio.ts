/**
 * Aggregate view of a wallet's money across tables and tokens (#631).
 *
 * Seated chips live in each table's contract, and each table can use a
 * different token, so totals are kept per token and never added across
 * tokens. This module is the pure part: it takes positions, token lookups and
 * balances that were already read, and groups them. The page does the reads.
 */

import type { PlayerPosition } from "./player-positions";

/**
 * Decimal places of the token amounts. Stellar Asset Contract tokens, including
 * native XLM, use 7 decimals (stroops), and so do the poker tables' buy-ins.
 */
export const TOKEN_DECIMALS = 7;

/** One seated table in a token's group. */
export interface PortfolioTableEntry {
  tableId: number;
  phase: string;
  /** Chips behind at this table, in the token's smallest unit. */
  stack: bigint;
  /** Chips already in this hand's pot. */
  committed: bigint;
}

/** Everything the wallet holds in one token. */
export interface PortfolioTokenRow {
  /** Token contract address, or `null` if a table's token could not be read. */
  token: string | null;
  /** Sum of seated stacks at the tables that use this token. */
  seatedChips: bigint;
  /** Sum of chips already committed to pots. */
  inPot: bigint;
  /** Wallet balance of this token, or `null` if it could not be read. */
  walletBalance: bigint | null;
  tables: PortfolioTableEntry[];
}

/** The aggregate result. */
export interface Portfolio {
  rows: PortfolioTokenRow[];
  seatedTableCount: number;
}

/**
 * Groups seated positions by the token their table uses and attaches the
 * wallet balance for each token. Tokens are listed in address order, tables
 * in id order. A table with no token lookup is kept under `token: null` so
 * its chips are never silently dropped.
 *
 * @param positions - Seated positions only (see `seatedPositions`).
 * @param tokenByTable - Token contract address for each table id.
 * @param balances - Wallet balance for each token contract address.
 */
export function buildPortfolio(
  positions: readonly PlayerPosition[],
  tokenByTable: ReadonlyMap<number, string>,
  balances: ReadonlyMap<string, bigint>
): Portfolio {
  const groups = new Map<string | null, PortfolioTokenRow>();

  for (const position of positions) {
    const token = tokenByTable.get(position.tableId) ?? null;
    let row = groups.get(token);
    if (!row) {
      row = {
        token,
        seatedChips: BigInt(0),
        inPot: BigInt(0),
        walletBalance: token === null ? null : balances.get(token) ?? null,
        tables: [],
      };
      groups.set(token, row);
    }
    row.seatedChips += position.stack;
    row.inPot += position.committed;
    row.tables.push({
      tableId: position.tableId,
      phase: position.phase,
      stack: position.stack,
      committed: position.committed,
    });
  }

  const rows = Array.from(groups.values()).map((row) => ({
    ...row,
    tables: [...row.tables].sort((a, b) => a.tableId - b.tableId),
  }));
  rows.sort((a, b) => {
    if (a.token === b.token) return 0;
    if (a.token === null) return 1;
    if (b.token === null) return -1;
    return a.token < b.token ? -1 : 1;
  });

  return { rows, seatedTableCount: positions.length };
}

/**
 * Formats a smallest-unit amount as a decimal string without going through
 * `Number`, so large balances keep every digit. Trailing zeros are dropped.
 *
 * @example formatTokenAmount(BigInt(12345000000)) === "1234.5"
 */
export function formatTokenAmount(units: bigint, decimals: number = TOKEN_DECIMALS): string {
  const negative = units < BigInt(0);
  const abs = negative ? -units : units;
  const scale = BigInt(`1${"0".repeat(decimals)}`);
  const whole = abs / scale;
  const fraction = (abs % scale)
    .toString()
    .padStart(decimals, "0")
    .replace(/0+$/, "");
  const body = fraction ? `${whole}.${fraction}` : whole.toString();
  return negative ? `-${body}` : body;
}
