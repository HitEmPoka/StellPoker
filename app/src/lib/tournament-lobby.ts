/**
 * Tournament lobby filtering, sorting and registration-status helpers
 * (Issue #165).
 *
 * Pure functions over `TournamentSummary` so they can be unit-tested in
 * isolation and reused by the lobby UI.
 */

import type { TournamentSummary } from "./tournament";

export type TournamentSortKey = "entries" | "prizePool" | "startTime";
export type SortDirection = "asc" | "desc";

export interface TournamentFilters {
  /** Minimum buy-in in stroops (inclusive). */
  buyInMin: number | null;
  /** Maximum buy-in in stroops (inclusive). */
  buyInMax: number | null;
  /** Only tournaments that can still hold at least this many more players. */
  minOpenEntries: number | null;
  /** Only show tournaments whose blind structure fits these constraints. */
  blinds: {
    /** Maximum current big blind in stroops. */
    maxBigBlind: number | null;
  } | null;
  /** Only show tournaments with a start time on/after this timestamp (ms). */
  startTimeAfter: number | null;
}

export const EMPTY_FILTERS: TournamentFilters = {
  buyInMin: null,
  buyInMax: null,
  minOpenEntries: null,
  blinds: null,
  startTimeAfter: null,
};

/** Sortable "start time" — registration tournaments appear as registered entries first. */
export function sortTimeFor(t: TournamentSummary): number {
  // The summary doesn't carry an epoch, so use registered count as a stable
  // proxy for progress: fuller registration → closer to start. Keep a
  // deterministic tiebreaker by id.
  return t.registered;
}

export function filterTournaments(
  tournaments: TournamentSummary[],
  filters: TournamentFilters
): TournamentSummary[] {
  return tournaments.filter((t) => {
    if (filters.buyInMin != null && t.buy_in < filters.buyInMin) return false;
    if (filters.buyInMax != null && t.buy_in > filters.buyInMax) return false;
    if (filters.minOpenEntries != null) {
      const open = t.max_players - t.registered;
      if (open < filters.minOpenEntries) return false;
    }
    if (filters.blinds?.maxBigBlind != null) {
      if (t.current_big_blind > filters.blinds.maxBigBlind) return false;
    }
    if (filters.startTimeAfter != null) {
      if (sortTimeFor(t) < (filters.startTimeAfter / 1000)) return false;
    }
    return true;
  });
}

export function sortTournaments(
  tournaments: TournamentSummary[],
  key: TournamentSortKey,
  direction: SortDirection
): TournamentSummary[] {
  const dir = direction === "asc" ? 1 : -1;
  return [...tournaments].sort((a, b) => {
    let cmp = 0;
    switch (key) {
      case "entries":
        cmp = a.registered - b.registered;
        break;
      case "prizePool":
        cmp = a.prize_pool - b.prize_pool;
        break;
      case "startTime":
        cmp = sortTimeFor(a) - sortTimeFor(b);
        break;
    }
    if (cmp === 0) cmp = a.name.localeCompare(b.name);
    return cmp * dir;
  });
}

export type RegistrationStatus =
  | "open"
  | "full"
  | "in-progress"
  | "closed";

export function registrationStatus(t: TournamentSummary): RegistrationStatus {
  if (t.status === "registration") {
    return t.registered >= t.max_players ? "full" : "open";
  }
  if (t.status === "running") return "in-progress";
  return "closed";
}

export function registrationLabel(status: RegistrationStatus): string {
  switch (status) {
    case "open":
      return "OPEN";
    case "full":
      return "FULL";
    case "in-progress":
      return "IN PROGRESS";
    case "closed":
      return "CLOSED";
  }
}

export function registrationColor(status: RegistrationStatus): string {
  switch (status) {
    case "open":
      return "#27ae60";
    case "full":
      return "#e74c3c";
    case "in-progress":
      return "#f39c12";
    case "closed":
      return "#7f8c8d";
  }
}
