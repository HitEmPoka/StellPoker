import { describe, it, expect } from "vitest";
import { actionsFromTimeline, type HandHistoryEntry } from "@/lib/hand-history";
import type { TimelineEvent } from "@/lib/hand-timeline";
import {
  MASKED_CARD,
  cardCode,
  exportFilename,
  exportHandHistoryCsv,
  exportHandHistoryJson,
  toExportedHand,
} from "@/lib/hand-history-export";

const HERO = "GHERO";
const VILLAIN = "GVILLAIN";

// Card value = suit index (clubs, diamonds, hearts, spades) * 13 + rank index (2..A).
const TWO_OF_CLUBS = 0;
const TEN_OF_DIAMONDS = 21;
const KING_OF_HEARTS = 37;
const KING_OF_SPADES = 50;
const ACE_OF_SPADES = 51;

const ENTRY: HandHistoryEntry = {
  tableId: 4,
  handNumber: 12,
  timestamp: Date.UTC(2026, 8, 24, 12, 0, 0),
  streets: [
    { street: "preflop", pot: 30, boardCards: [] },
    { street: "flop", pot: 90, boardCards: [TWO_OF_CLUBS, TEN_OF_DIAMONDS, KING_OF_HEARTS] },
  ],
  finalPot: 150,
  boardCards: [TWO_OF_CLUBS, TEN_OF_DIAMONDS, KING_OF_HEARTS],
  holeCards: [ACE_OF_SPADES, KING_OF_SPADES],
  handRankName: "One Pair",
  winnerAddress: HERO,
  txHash: "abc123",
  heroAddress: HERO,
  players: [
    { address: HERO, seat: 0 },
    { address: VILLAIN, seat: 1 },
  ],
  actions: [
    { street: "preflop", amount: 20, pot: 30 },
    { street: "flop", amount: 60, pot: 90 },
  ],
};

describe("cardCode (Issue #158)", () => {
  it("writes the rank followed by the suit's initial", () => {
    expect(cardCode(ACE_OF_SPADES)).toBe("As");
    expect(cardCode(TEN_OF_DIAMONDS)).toBe("10d");
    expect(cardCode(TWO_OF_CLUBS)).toBe("2c");
  });
});

describe("toExportedHand (Issue #158)", () => {
  it("shows your hole cards and masks every opponent's", () => {
    const hand = toExportedHand(ENTRY);
    expect(hand.yourHoleCards).toEqual(["As", "Ks"]);
    expect(hand.players).toEqual([
      { address: HERO, seat: 0, isYou: true, holeCards: ["As", "Ks"] },
      { address: VILLAIN, seat: 1, isYou: false, holeCards: [MASKED_CARD, MASKED_CARD] },
    ]);
  });

  it("masks everyone when it isn't known whose cards were captured", () => {
    const hand = toExportedHand({ ...ENTRY, heroAddress: undefined });
    expect(hand.players.every((p) => !p.isYou)).toBe(true);
    expect(hand.players.every((p) => p.holeCards.every((c) => c === MASKED_CARD))).toBe(true);
    expect(hand.youWon).toBeNull();
  });

  it("includes the board, pots, actions and result", () => {
    const hand = toExportedHand(ENTRY);
    expect(hand.playedAt).toBe("2026-09-24T12:00:00.000Z");
    expect(hand.boardCards).toEqual(["2c", "10d", "Kh"]);
    expect(hand.streets.map((s) => s.pot)).toEqual([30, 90]);
    expect(hand.actions).toEqual([
      { street: "preflop", amount: 20, pot: 30 },
      { street: "flop", amount: 60, pot: 90 },
    ]);
    expect(hand.finalPot).toBe(150);
    expect(hand.winner).toBe(HERO);
    expect(hand.yourHandRank).toBe("One Pair");
    expect(hand.youWon).toBe(true);
    expect(hand.proofTxHash).toBe("abc123");
  });

  it("handles hands saved before players and actions were recorded", () => {
    const legacy: HandHistoryEntry = {
      ...ENTRY,
      heroAddress: undefined,
      players: undefined,
      actions: undefined,
    };
    const hand = toExportedHand(legacy);
    expect(hand.players).toEqual([]);
    expect(hand.actions).toEqual([]);
    expect(hand.yourHoleCards).toEqual(["As", "Ks"]);
  });
});

describe("exportHandHistoryJson (Issue #158)", () => {
  it("wraps the hands with the export time", () => {
    const parsed = JSON.parse(exportHandHistoryJson([ENTRY], new Date(Date.UTC(2026, 8, 25))));
    expect(parsed.exportedAt).toBe("2026-09-25T00:00:00.000Z");
    expect(parsed.hands).toHaveLength(1);
    expect(parsed.hands[0].handNumber).toBe(12);
  });

  it("never contains an opponent's hole cards", () => {
    const parsed = JSON.parse(exportHandHistoryJson([ENTRY]));
    const villain = parsed.hands[0].players.find(
      (p: { address: string }) => p.address === VILLAIN
    );
    expect(villain.holeCards).toEqual([MASKED_CARD, MASKED_CARD]);
  });
});

describe("exportHandHistoryCsv (Issue #158)", () => {
  it("writes a header row and one row per hand", () => {
    const lines = exportHandHistoryCsv([ENTRY, { ...ENTRY, handNumber: 13 }]).split("\r\n");
    expect(lines).toHaveLength(3);
    expect(lines[0]).toBe(
      "table_id,hand_number,played_at,your_hole_cards,opponents,board,street_pots,actions,final_pot,winner,your_hand_rank,you_won,proof_tx"
    );
  });

  it("lists opponents with masked cards and your own cards separately", () => {
    const row = exportHandHistoryCsv([ENTRY]).split("\r\n")[1];
    expect(row).toContain(`seat 1 ${VILLAIN} ?? ??`);
    expect(row).toContain(",As Ks,");
    expect(row).not.toContain(`${HERO} As`);
  });

  it("quotes fields that contain commas or quotes", () => {
    const row = exportHandHistoryCsv([
      { ...ENTRY, handRankName: 'Two Pair, "Kings"' },
    ]).split("\r\n")[1];
    expect(row).toContain('"Two Pair, ""Kings"""');
  });
});

describe("exportFilename (Issue #158)", () => {
  it("names the file after the table and the date", () => {
    expect(exportFilename(4, "csv", new Date(Date.UTC(2026, 8, 24)))).toBe(
      "stellpoker-table-4-hands-2026-09-24.csv"
    );
  });
});

function actionEvent(
  handNumber: number,
  street: TimelineEvent["street"],
  amount: number,
  pot: number
): TimelineEvent {
  return {
    id: `${handNumber}:action:${street}:${pot}`,
    kind: "action",
    label: `+${amount}`,
    timestamp: 0,
    street,
    pot,
    boardCards: [],
    actor: "GNEXT",
    amount,
  };
}

describe("actionsFromTimeline (Issue #158)", () => {
  it("keeps this hand's pot movements in order", () => {
    const flop: TimelineEvent = {
      id: "5:street:flop",
      kind: "street",
      label: "FLOP",
      timestamp: 0,
      street: "flop",
      pot: 30,
      boardCards: [],
    };
    const events = [actionEvent(5, "preflop", 20, 30), flop, actionEvent(5, "flop", 40, 70)];
    expect(actionsFromTimeline(5, events)).toEqual([
      { street: "preflop", amount: 20, pot: 30 },
      { street: "flop", amount: 40, pot: 70 },
    ]);
  });

  it("drops other hands' events and non-betting streets", () => {
    const events = [actionEvent(4, "flop", 10, 20), actionEvent(5, "showdown", 10, 80)];
    expect(actionsFromTimeline(5, events)).toEqual([]);
  });
});
