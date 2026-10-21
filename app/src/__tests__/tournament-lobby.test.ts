import { describe, it, expect } from "vitest";
import {
  filterTournaments,
  sortTournaments,
  registrationStatus,
  registrationLabel,
  registrationColor,
  sortTimeFor,
  EMPTY_FILTERS,
  type TournamentFilters,
} from "@/lib/tournament-lobby";
import type { TournamentSummary } from "@/lib/tournament";

function tournament(overrides: Partial<TournamentSummary>): TournamentSummary {
  return {
    id: "t1",
    name: "Tourney",
    buy_in: 50_000_000,
    max_players: 9,
    registered: 3,
    status: "registration",
    prize_pool: 450_000_000,
    current_small_blind: 250_000,
    current_big_blind: 500_000,
    blind_level: 0,
    ...overrides,
  };
}

describe("filterTournaments", () => {
  const list = [
    tournament({ id: "a", buy_in: 10_000_000, registered: 2 }),
    tournament({ id: "b", buy_in: 50_000_000, registered: 8, max_players: 9 }),
    tournament({ id: "c", buy_in: 100_000_000, current_big_blind: 2_000_000 }),
  ];

  it("returns all when filters are empty", () => {
    expect(filterTournaments(list, EMPTY_FILTERS).map((t) => t.id)).toEqual(["a", "b", "c"]);
  });

  it("filters by minimum buy-in", () => {
    const f: TournamentFilters = { ...EMPTY_FILTERS, buyInMin: 50_000_000 };
    const ids = filterTournaments(list, f).map((t) => t.id);
    expect(ids).toEqual(["b", "c"]);
  });

  it("filters by maximum buy-in", () => {
    const f: TournamentFilters = { ...EMPTY_FILTERS, buyInMax: 50_000_000 };
    const ids = filterTournaments(list, f).map((t) => t.id);
    expect(ids).toEqual(["a", "b"]);
  });

  it("filters by minimum open entries", () => {
    // "b" has 8/9 registered → only 1 spot left → excluded when requiring 2.
    const f: TournamentFilters = { ...EMPTY_FILTERS, minOpenEntries: 2 };
    const ids = filterTournaments(list, f).map((t) => t.id);
    expect(ids).not.toContain("b");
  });

  it("filters by maximum big blind", () => {
    const f: TournamentFilters = { ...EMPTY_FILTERS, blinds: { maxBigBlind: 1_000_000 } };
    const ids = filterTournaments(list, f).map((t) => t.id);
    expect(ids).toEqual(["a", "b"]);
  });

  it("filters by start time (registered-proxy)", () => {
    const f: TournamentFilters = { ...EMPTY_FILTERS, startTimeAfter: 4000 };
    // only "b" registered 8 → passes proxy threshold of 4.
    const ids = filterTournaments(list, f).map((t) => t.id);
    expect(ids).toEqual(["b"]);
  });
});

describe("sortTournaments", () => {
  const list = [
    tournament({ id: "low", registered: 1, prize_pool: 100 }),
    tournament({ id: "mid", registered: 5, prize_pool: 300 }),
    tournament({ id: "high", registered: 9, prize_pool: 200 }),
  ];

  it("sorts by entries ascending by default direction choice", () => {
    const sorted = sortTournaments(list, "entries", "asc").map((t) => t.id);
    expect(sorted).toEqual(["low", "mid", "high"]);
  });

  it("sorts by prize pool descending", () => {
    const sorted = sortTournaments(list, "prizePool", "desc").map((t) => t.id);
    expect(sorted).toEqual(["mid", "high", "low"]);
  });

  it("does not mutate the input", () => {
    const before = list.map((t) => t.id);
    sortTournaments(list, "prizePool", "asc");
    expect(list.map((t) => t.id)).toEqual(before);
  });
});

describe("registrationStatus", () => {
  it("reports open/full/in-progress/closed", () => {
    expect(registrationStatus(tournament({ status: "registration", registered: 2 }))).toBe("open");
    expect(registrationStatus(tournament({ status: "registration", registered: 9 }))).toBe("full");
    expect(registrationStatus(tournament({ status: "running" }))).toBe("in-progress");
    expect(registrationStatus(tournament({ status: "completed" }))).toBe("closed");
  });
});

describe("registrationLabel / registrationColor", () => {
  it("labels each status", () => {
    expect(registrationLabel("open")).toBe("OPEN");
    expect(registrationLabel("full")).toBe("FULL");
    expect(registrationLabel("in-progress")).toBe("IN PROGRESS");
    expect(registrationLabel("closed")).toBe("CLOSED");
  });

  it("returns a hex colour for each status", () => {
    for (const s of ["open", "full", "in-progress", "closed"] as const) {
      expect(registrationColor(s)).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });
});

describe("sortTimeFor", () => {
  it("uses registration count as the start-time proxy", () => {
    expect(sortTimeFor(tournament({ registered: 5 }))).toBe(5);
  });
});
