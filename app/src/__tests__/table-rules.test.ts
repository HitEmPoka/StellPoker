import { describe, it, expect } from "vitest";
import {
  describeAnte,
  formatRake,
  formatSeconds,
  parseTableRules,
} from "../lib/table-rules";

// Shape matches what scValToNative returns for a poker-table `get_table` call.
function tableWith(overrides: Record<string, unknown> = {}) {
  return {
    config: {
      min_buy_in: BigInt(100),
      max_buy_in: BigInt(1000),
      max_rebuys: 2,
      rake_bps: 250,
      min_players: 2,
      max_players: 6,
      timeout_ledgers: 12,
      betting_structure: ["NoLimit"],
      street_time_limit: [
        "Some",
        { preflop_seconds: 30, flop_seconds: 30, turn_seconds: 45, river_seconds: 60 },
      ],
      blinds_schedule: {
        levels: [
          {
            small_blind: BigInt(5),
            big_blind: BigInt(10),
            ante: ["Fixed", BigInt(1)],
            duration_seconds: BigInt(600),
            break_seconds: BigInt(0),
          },
          {
            small_blind: BigInt(10),
            big_blind: BigInt(20),
            ante: ["None"],
            duration_seconds: BigInt(0),
            break_seconds: BigInt(0),
          },
        ],
      },
      ...overrides,
    },
  };
}

describe("parseTableRules (#624)", () => {
  it("reads buy-ins, rebuys, rake and seat limits from the config", () => {
    const rules = parseTableRules(tableWith(), 3);

    expect(rules.configVersion).toBe(3);
    expect(rules.bettingStructure).toBe("No limit");
    expect(rules.minBuyIn).toBe(BigInt(100));
    expect(rules.maxBuyIn).toBe(BigInt(1000));
    expect(rules.maxRebuys).toBe(2);
    expect(rules.rakeBps).toBe(250);
    expect(rules.minPlayers).toBe(2);
    expect(rules.maxPlayers).toBe(6);
  });

  it("converts timeout_ledgers to seconds", () => {
    const rules = parseTableRules(tableWith(), 0);
    expect(rules.timeoutSeconds).toBe(12 * 5);
  });

  it("reads per-street action limits when they are set", () => {
    const rules = parseTableRules(tableWith(), 0);
    expect(rules.actionTimes).toEqual({
      preflopSeconds: 30,
      flopSeconds: 30,
      turnSeconds: 45,
      riverSeconds: 60,
    });
  });

  it("reports no per-street limits when the option is None", () => {
    const rules = parseTableRules(tableWith({ street_time_limit: ["None"] }), 0);
    expect(rules.actionTimes).toBeNull();
  });

  it("describes each blinds level with its ante and duration", () => {
    const rules = parseTableRules(tableWith(), 0);
    expect(rules.blindLevels).toEqual([
      {
        smallBlind: BigInt(5),
        bigBlind: BigInt(10),
        ante: "1",
        durationSeconds: 600,
        breakSeconds: 0,
      },
      {
        smallBlind: BigInt(10),
        bigBlind: BigInt(20),
        ante: "none",
        durationSeconds: 0,
        breakSeconds: 0,
      },
    ]);
  });

  it("throws when the on-chain response has no config", () => {
    expect(() => parseTableRules({}, 0)).toThrow(/config is missing/);
    expect(() => parseTableRules(null, 0)).toThrow(/config is missing/);
  });
});

describe("describeAnte", () => {
  it("renders fixed, percentage and absent antes", () => {
    expect(describeAnte(["Fixed", BigInt(25)])).toBe("25");
    expect(describeAnte(["Percentage", 10])).toBe("10% of BB");
    expect(describeAnte(["None"])).toBe("none");
  });
});

describe("formatSeconds", () => {
  it("formats durations and treats zero as no limit", () => {
    expect(formatSeconds(0)).toBe("no limit");
    expect(formatSeconds(45)).toBe("45s");
    expect(formatSeconds(90)).toBe("1m 30s");
    expect(formatSeconds(3600)).toBe("1h");
    expect(formatSeconds(3900)).toBe("1h 5m");
  });
});

describe("formatRake", () => {
  it("converts basis points to a percentage", () => {
    expect(formatRake(250)).toBe("2.5%");
    expect(formatRake(500)).toBe("5%");
  });
});
