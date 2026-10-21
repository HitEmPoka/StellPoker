import { describe, expect, it } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import {
  createStandardHandHistory,
  formatStandardCard,
  validateStandardHandHistory,
  MASKED_CARD,
} from "../lib/standard-hand-history";
import type { HandHistoryEntry } from "../lib/hand-history";

describe("Standard Hand History (Issue #691)", () => {
  it("formats numeric cards to standard notation", () => {
    // 0 = 2 of clubs (2c)
    expect(formatStandardCard(0)).toBe("2c");
    // 12 = Ace of clubs (Ac)
    expect(formatStandardCard(12)).toBe("Ac");
    // 51 = Ace of spades (As)
    expect(formatStandardCard(51)).toBe("As");
    // 8 = 10 of clubs (Tc)
    expect(formatStandardCard(8)).toBe("Tc");
  });

  it("creates valid standardized hand from internal HandHistoryEntry", () => {
    const entry: HandHistoryEntry = {
      tableId: 5,
      handNumber: 42,
      timestamp: 1729340000000,
      heroAddress: "GHEROADDRESS1234567890",
      holeCards: [12, 51], // Ac, As
      boardCards: [0, 13, 26, 39, 40],
      players: [
        { seat: 0, address: "GHEROADDRESS1234567890", startingStack: 500 },
        { seat: 1, address: "GVILLAINADDRESS1234567", startingStack: 500 },
      ],
      streets: [
        { street: "preflop", pot: 15, boardCards: [] },
        { street: "flop", pot: 55, boardCards: [0, 13, 26] },
      ],
      finalPot: 100,
      winnerAddress: "GHEROADDRESS1234567890",
      handRankName: "Pair of Aces",
      txHash: "0x1234567890abcdef",
    };

    const std = createStandardHandHistory(entry, "stellar-testnet");
    expect(std.version).toBe("1.0.0");
    expect(std.table.id).toBe(5);
    expect(std.hand.number).toBe(42);
    expect(std.players[0].hole_cards).toEqual(["Ac", "As"]);
    expect(std.players[1].hole_cards).toEqual([MASKED_CARD, MASKED_CARD]); // Opponent cards masked

    const validation = validateStandardHandHistory(std);
    expect(validation.valid).toBe(true);
    expect(validation.errors).toHaveLength(0);
  });

  const fixtureFiles = [
    "standard-heads-up-showdown.json",
    "multiway-all-in-side-pot.json",
    "preflop-fold.json",
    "timeout-fold.json",
  ];

  for (const filename of fixtureFiles) {
    it(`validates fixture ${filename} successfully`, () => {
      const fixturePath = path.resolve(__dirname, "../../../fixtures/hand-history/v1", filename);
      const raw = fs.readFileSync(fixturePath, "utf-8");
      const parsed = JSON.parse(raw);

      const validation = validateStandardHandHistory(parsed);
      expect(validation.valid).toBe(true);
      expect(validation.errors).toHaveLength(0);

      // Explicit field checks
      expect(parsed.version).toBe("1.0.0");
      expect(parsed.table.stakes.currency).toBeDefined();
      expect(parsed.players.length).toBeGreaterThanOrEqual(2);
      expect(parsed.verifiability.zk_proof_verified).toBe(true);
    });
  }
});
