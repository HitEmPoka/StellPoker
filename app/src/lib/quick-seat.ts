/**
 * Quick seat for returning players (#159).
 *
 * Remembers the kind of seat a player last sat down in — table size, stakes,
 * and seat position — and picks the open lobby table that best matches it, so
 * a returning player can sit straight back down with one click. Like
 * `open-tables.ts` it is a local preference, kept in localStorage per wallet.
 */

export interface SeatPreference {
  /** Table size, e.g. 2 for heads-up. */
  maxPlayers: number;
  /** Buy-in in stroops, as a string since JSON has no bigint. */
  buyIn: string;
  /** Token contract the buy-in is paid in, when the table config exposes it. */
  token: string | null;
  /** Seat index the player last sat in. */
  seatIndex: number;
}

/** A table's stakes: what it costs to sit down, and in which token. */
export interface TableStakes {
  buyIn: bigint;
  token: string | null;
}

/** An open lobby table, as far as quick seat needs to know it. */
export interface QuickSeatCandidate {
  tableId: number;
  maxPlayers: number;
  /**
   * Players already seated. The contract seats joiners in order, so this is
   * also the seat index the next player to join gets.
   */
  joinedWallets: number;
  /** Null when the table's config couldn't be read. */
  stakes: TableStakes | null;
}

const STORAGE_PREFIX = "stellpoker:seat-preference:";

function storageKey(address: string): string {
  return `${STORAGE_PREFIX}${address}`;
}

/** Loads Seat Preference.
 * @param address - address.
 * @returns SeatPreference | null.
 * @remarks Reads/writes browser storage.
 */
export function loadSeatPreference(address: string): SeatPreference | null {
  if (typeof window === "undefined" || !address) return null;
  try {
    const raw = window.localStorage.getItem(storageKey(address));
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<SeatPreference>;
    if (
      !Number.isInteger(parsed.maxPlayers) ||
      !Number.isInteger(parsed.seatIndex) ||
      typeof parsed.buyIn !== "string" ||
      !/^\d+$/.test(parsed.buyIn)
    ) {
      return null;
    }
    return {
      maxPlayers: parsed.maxPlayers as number,
      buyIn: parsed.buyIn,
      token: typeof parsed.token === "string" ? parsed.token : null,
      seatIndex: parsed.seatIndex as number,
    };
  } catch {
    return null;
  }
}

/** Persists Seat Preference.
 * @param address - address.
 * @param preference - preference.
 * @remarks Reads/writes browser storage.
 */
export function saveSeatPreference(address: string, preference: SeatPreference): void {
  if (typeof window === "undefined" || !address) return;
  try {
    window.localStorage.setItem(storageKey(address), JSON.stringify(preference));
  } catch {
    // Storage unavailable (private browsing, quota) — quick seat just won't
    // remember this table.
  }
}

/**
 * A table's stakes from its parsed on-chain state: the same
 * `config.min_buy_in` the table page joins with, plus the buy-in token.
 */
export function readTableStakes(parsed: Record<string, unknown> | null): TableStakes | null {
  const config = parsed?.config;
  if (!config || typeof config !== "object") return null;
  const { min_buy_in: minBuyIn, token } = config as { min_buy_in?: unknown; token?: unknown };

  let buyIn: bigint | null = null;
  if (typeof minBuyIn === "number" && Number.isFinite(minBuyIn) && minBuyIn >= 0) {
    buyIn = BigInt(Math.trunc(minBuyIn));
  } else if (typeof minBuyIn === "string" && /^\d+$/.test(minBuyIn.trim())) {
    buyIn = BigInt(minBuyIn.trim());
  }
  if (buyIn === null) return null;

  return { buyIn, token: typeof token === "string" ? token : null };
}

/**
 * Picks the open table that best fits the preference, or null if none does.
 *
 * Table size and stakes must match exactly: sitting down at a different buy-in
 * than the player chose isn't a match, it's a surprise charge. Among the
 * matches, a table where the preferred seat is the next one free wins, then
 * the fullest table (it starts soonest), then the lowest table id so the pick
 * is stable.
 */
export function pickQuickSeatTable(
  preference: SeatPreference,
  candidates: readonly QuickSeatCandidate[]
): QuickSeatCandidate | null {
  const buyIn = BigInt(preference.buyIn);
  const matching = candidates.filter(
    (table) =>
      table.maxPlayers === preference.maxPlayers &&
      table.joinedWallets < table.maxPlayers &&
      table.stakes !== null &&
      table.stakes.buyIn === buyIn &&
      table.stakes.token === preference.token
  );
  if (matching.length === 0) return null;

  const seatRank = (table: QuickSeatCandidate) =>
    table.joinedWallets === preference.seatIndex ? 0 : 1;

  return [...matching].sort(
    (a, b) =>
      seatRank(a) - seatRank(b) ||
      b.joinedWallets - a.joinedWallets ||
      a.tableId - b.tableId
  )[0];
}
