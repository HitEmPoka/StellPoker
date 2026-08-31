/**
 * Lobby table search (#173).
 *
 * The join screen needs to answer one question quickly: "which open table is
 * my friend sitting at?". A player knows a friend by one of three things —
 * the alias they set at the table, the first few characters of their Stellar
 * public key, or the table number itself — so all three are accepted in a
 * single field rather than behind a mode switch.
 *
 * Matching lives here, apart from the page, because the interesting parts are
 * the rules (what counts as a prefix, which seat matched, how ties are
 * ordered) and those are worth testing directly.
 *
 * Aliases come from `alias-store.ts`, which is per-browser localStorage: a
 * player can only search aliases their own browser has seen. That is the same
 * scope the alias feature already has, and searching by key prefix always
 * works regardless.
 */

/** A seat as the coordinator's lobby endpoint reports it. */
export interface SearchableSeat {
  seat_index: number;
  chain_address: string;
  wallet_address: string | null;
}

/** Why a seat matched — drives the badge shown next to the result row. */
export type SeatMatchKind = "alias" | "address-prefix" | "address";

export interface SeatMatch {
  seatIndex: number;
  /** The address that matched (wallet address preferred over chain address). */
  address: string;
  /** Alias for that address, when this browser knows one. */
  alias?: string;
  kind: SeatMatchKind;
}

export interface TableMatch {
  matched: boolean;
  /** True when the query matched the table's own number rather than a seat. */
  matchedTableId: boolean;
  /** Seats whose alias or address matched, in seat order. */
  seats: SeatMatch[];
}

/** Resolves an address to this browser's alias for it, if any. */
export type AliasResolver = (address: string) => string | null | undefined;

/**
 * A Stellar public key is 56 characters: `G` followed by 55 base32 digits
 * (RFC 4648 alphabet — A-Z and 2-7, no 0/1/8/9). A *prefix* is any leading
 * slice of one, so a single `G` already qualifies.
 */
const ADDRESS_PREFIX_PATTERN = /^G[A-Z2-7]{0,55}$/;
const FULL_ADDRESS_PATTERN = /^G[A-Z2-7]{55}$/;

/** Trims and upper-cases; Stellar keys are upper-case base32. */
export function normalizeQuery(query: string): string {
  return query.trim();
}

/** True when `query` could be the start of a Stellar public key. */
export function isAddressPrefix(query: string): boolean {
  return ADDRESS_PREFIX_PATTERN.test(normalizeQuery(query).toUpperCase());
}

/** True when `query` is a complete, well-formed Stellar public key. */
export function isFullAddress(query: string): boolean {
  return FULL_ADDRESS_PATTERN.test(normalizeQuery(query).toUpperCase());
}

/**
 * Finds the seats a query points at.
 *
 * An address query is matched as a **prefix** rather than a substring: typing
 * `GABC` should find the player whose key starts with those characters, not
 * every key that happens to contain them somewhere in the middle. An alias
 * query is matched as a case-insensitive substring, because aliases are short
 * free text where "bo" reasonably finds "Bobby".
 */
export function searchSeats(
  query: string,
  seats: readonly SearchableSeat[],
  resolveAlias: AliasResolver
): SeatMatch[] {
  const normalized = normalizeQuery(query);
  if (!normalized) return [];

  const upper = normalized.toUpperCase();
  const lower = normalized.toLowerCase();
  const asPrefix = isAddressPrefix(normalized);

  const matches: SeatMatch[] = [];

  for (const seat of seats) {
    // The wallet address is what a player recognises; the chain address is
    // the contract-side identity for the same seat. Search both, report the
    // one a human would recognise.
    const displayAddress = seat.wallet_address ?? seat.chain_address;
    const candidates = [seat.wallet_address, seat.chain_address].filter(
      (address): address is string => !!address
    );

    const alias = candidates
      .map((address) => resolveAlias(address))
      .find((value): value is string => !!value);

    if (alias && alias.toLowerCase().includes(lower)) {
      matches.push({
        seatIndex: seat.seat_index,
        address: displayAddress,
        alias,
        kind: "alias",
      });
      continue;
    }

    if (!asPrefix) continue;

    const hit = candidates.find((address) =>
      address.toUpperCase().startsWith(upper)
    );
    if (hit) {
      matches.push({
        seatIndex: seat.seat_index,
        address: hit,
        alias: alias ?? undefined,
        kind: isFullAddress(normalized) ? "address" : "address-prefix",
      });
    }
  }

  return matches.sort((a, b) => a.seatIndex - b.seatIndex);
}

/**
 * Decides whether one lobby row survives the current query.
 *
 * An empty query matches everything, so the browse list is unfiltered until
 * the player actually types. A purely numeric query is treated as a table
 * number as well as a possible seat match, since "#12" is the other way
 * people refer to a table.
 */
export function matchTable(
  query: string,
  tableId: number,
  seats: readonly SearchableSeat[],
  resolveAlias: AliasResolver
): TableMatch {
  const normalized = normalizeQuery(query);
  if (!normalized) {
    return { matched: true, matchedTableId: false, seats: [] };
  }

  const numeric = normalized.replace(/^#/, "");
  const matchedTableId =
    /^\d+$/.test(numeric) && String(tableId).includes(numeric);

  const seatMatches = searchSeats(normalized, seats, resolveAlias);

  return {
    matched: matchedTableId || seatMatches.length > 0,
    matchedTableId,
    seats: seatMatches,
  };
}

/** Short "GABCDE…WXYZ" rendering for a key shown in a result row. */
export function shortenAddress(address: string): string {
  if (address.length <= 14) return address;
  return `${address.slice(0, 6)}…${address.slice(-4)}`;
}

/**
 * One-line summary of why a row matched, for the result badge and for the
 * search field's live region.
 */
export function describeSeatMatch(match: SeatMatch): string {
  const who = match.alias ?? shortenAddress(match.address);
  return `seat ${match.seatIndex + 1}: ${who}`;
}
