import { describe, expect, it } from "vitest";
import {
  averageRakePerHand,
  formatChipAmount,
  parseContractMetrics,
  parseTableMetrics,
} from "@/lib/contract-metrics";

describe("parseContractMetrics", () => {
  it("parses the native value Soroban returns for get_contract_metrics", () => {
    // scValToNative yields bigint for u64/i128 and number for u32.
    const metrics = parseContractMetrics({
      tables_created: 3,
      hands_played: BigInt(42),
      total_rake: BigInt(1250),
      active_seats: 7,
    });

    expect(metrics).toEqual({
      tablesCreated: 3,
      handsPlayed: BigInt(42),
      totalRake: BigInt(1250),
      activeSeats: 7,
    });
  });

  it("accepts plain numbers and numeric strings for the wide fields", () => {
    const metrics = parseContractMetrics({
      tables_created: 1,
      hands_played: 5,
      total_rake: "900",
      active_seats: 2,
    });

    expect(metrics.handsPlayed).toBe(BigInt(5));
    expect(metrics.totalRake).toBe(BigInt(900));
  });

  it("keeps rake totals that exceed Number.MAX_SAFE_INTEGER exact", () => {
    const huge = BigInt("123456789012345678901234567890");
    const metrics = parseContractMetrics({
      tables_created: 1,
      hands_played: BigInt(1),
      total_rake: huge,
      active_seats: 0,
    });

    expect(metrics.totalRake).toBe(huge);
  });

  it("rejects a value that is not an object", () => {
    expect(() => parseContractMetrics(null)).toThrow("get_contract_metrics");
    expect(() => parseContractMetrics("nope")).toThrow("get_contract_metrics");
  });

  it("rejects a missing or malformed field by name", () => {
    expect(() =>
      parseContractMetrics({ tables_created: 1, hands_played: 1, total_rake: 1 })
    ).toThrow("active_seats");
    expect(() =>
      parseContractMetrics({
        tables_created: 1,
        hands_played: "many",
        total_rake: 1,
        active_seats: 1,
      })
    ).toThrow("hands_played");
  });

  it("rejects negative seat and table counts", () => {
    expect(() =>
      parseContractMetrics({
        tables_created: -1,
        hands_played: 1,
        total_rake: 1,
        active_seats: 1,
      })
    ).toThrow("tables_created");
  });
});

describe("parseTableMetrics", () => {
  it("parses the native value for get_table_metrics", () => {
    expect(
      parseTableMetrics({ hands_played: BigInt(9), total_rake: BigInt(45), active_seats: 2 })
    ).toEqual({ handsPlayed: BigInt(9), totalRake: BigInt(45), activeSeats: 2 });
  });

  it("rejects a malformed value", () => {
    expect(() => parseTableMetrics(undefined)).toThrow("get_table_metrics");
    expect(() => parseTableMetrics({ hands_played: 1, total_rake: 1 })).toThrow("active_seats");
  });
});

describe("formatChipAmount", () => {
  it("groups digits in thousands", () => {
    expect(formatChipAmount(BigInt(0))).toBe("0");
    expect(formatChipAmount(BigInt(999))).toBe("999");
    expect(formatChipAmount(BigInt(1000))).toBe("1,000");
    expect(formatChipAmount(BigInt(1234567))).toBe("1,234,567");
  });

  it("formats amounts beyond Number.MAX_SAFE_INTEGER exactly", () => {
    expect(formatChipAmount(BigInt("9007199254740993"))).toBe("9,007,199,254,740,993");
  });

  it("keeps the sign of negative amounts", () => {
    expect(formatChipAmount(BigInt(-1500))).toBe("-1,500");
  });
});

describe("averageRakePerHand", () => {
  it("is null before the first hand settles", () => {
    expect(averageRakePerHand({ handsPlayed: BigInt(0), totalRake: BigInt(0) })).toBeNull();
  });

  it("divides total rake by hands played, rounding down", () => {
    expect(averageRakePerHand({ handsPlayed: BigInt(4), totalRake: BigInt(30) })).toBe(BigInt(7));
  });
});
