import { describe, expect, it } from "vitest";
import {
  describeSpectatorAction,
  diffSpectatorActions,
  formatSpectatorCount,
  parseSpectatorView,
  spectateHref,
  type SpectatorView,
} from "@/lib/spectator";
import { isSpectatorCountEvent, type GameStateEvent } from "@/lib/api";

const A = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const B = "GBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

function rawState(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    phase: "Preflop",
    pot: 30,
    current_turn: 0,
    dealer_seat: 1,
    hand_number: 4,
    board_cards: [],
    players: [
      { address: A, seat_index: 0, stack: 990, bet_this_round: 10, folded: false, all_in: false },
      { address: B, seat_index: 1, stack: 980, bet_this_round: 20, folded: false, all_in: false },
    ],
    ...overrides,
  };
}

function view(overrides: Record<string, unknown> = {}): SpectatorView {
  return parseSpectatorView(rawState(overrides))!;
}

describe("parseSpectatorView", () => {
  it("maps public table state", () => {
    const v = view({ phase: "Flop", board_cards: [1, 2, "3"] });
    expect(v.phase).toBe("flop");
    expect(v.boardCards).toEqual([1, 2, 3]);
    expect(v.pot).toBe(30);
    expect(v.handNumber).toBe(4);
    expect(v.players.map((p) => p.address)).toEqual([A, B]);
  });

  it("never carries hole cards, even if the payload has them", () => {
    const v = view({
      players: [{ address: A, seat_index: 0, stack: 1, cards: [12, 13], hole_cards: [12, 13] }],
    });
    expect(v.players[0]).not.toHaveProperty("cards");
    expect(v.players[0]).not.toHaveProperty("hole_cards");
  });

  it("returns null without state", () => {
    expect(parseSpectatorView(null)).toBeNull();
  });
});

describe("diffSpectatorActions", () => {
  it("emits nothing for the first snapshot", () => {
    expect(diffSpectatorActions(null, view())).toEqual([]);
  });

  it("detects bets, folds and all-ins", () => {
    const prev = view();
    const next = view({
      current_turn: 1,
      players: [
        { address: A, seat_index: 0, stack: 950, bet_this_round: 50, folded: false, all_in: false },
        { address: B, seat_index: 1, stack: 980, bet_this_round: 20, folded: true, all_in: false },
      ],
    });
    const kinds = diffSpectatorActions(prev, next).map((a) => [a.kind, a.address, a.amount]);
    expect(kinds).toEqual([
      ["bet", A, 50],
      ["fold", B, undefined],
    ]);

    const allIn = view({
      players: [
        { address: A, seat_index: 0, stack: 0, bet_this_round: 1000, folded: false, all_in: true },
        { address: B, seat_index: 1, stack: 980, bet_this_round: 20, folded: false, all_in: false },
      ],
    });
    expect(diffSpectatorActions(prev, allIn)[0]).toMatchObject({ kind: "all_in", address: A });
  });

  it("detects a check when the turn passes without chips moving", () => {
    const prev = view({ phase: "Flop", current_turn: 0 });
    const next = view({ phase: "Flop", current_turn: 1 });
    expect(diffSpectatorActions(prev, next)).toEqual([
      { kind: "check", handNumber: 4, address: A },
    ]);
  });

  it("reports street changes and new hands", () => {
    expect(diffSpectatorActions(view(), view({ phase: "Flop", players: [] }))).toEqual([
      { kind: "street", handNumber: 4, street: "flop" },
    ]);
    expect(diffSpectatorActions(view(), view({ hand_number: 5 }))).toEqual([
      { kind: "new_hand", handNumber: 5 },
    ]);
  });
});

describe("formatting helpers", () => {
  it("describes actions", () => {
    expect(describeSpectatorAction({ kind: "bet", handNumber: 1, address: A, amount: 1500 })).toBe(
      "GAAA…AAAA BETS 1,500"
    );
    expect(describeSpectatorAction({ kind: "street", handNumber: 1, street: "turn" })).toBe("— TURN —");
  });

  it("formats counts and links", () => {
    expect(formatSpectatorCount(3)).toBe("3 WATCHING");
    expect(spectateHref(7)).toBe("/table/7?spectate=1");
  });

  it("distinguishes spectator frames from state snapshots", () => {
    expect(isSpectatorCountEvent({ type: "spectators", table_id: 1, spectator_count: 2 })).toBe(true);
    const snapshot = { table_id: 1, phase: "Preflop" } as GameStateEvent;
    expect(isSpectatorCountEvent(snapshot)).toBe(false);
  });
});
