import { describe, expect, it } from "vitest";
import { buildPortfolio, formatTokenAmount } from "@/lib/portfolio";
import type { PlayerPosition } from "@/lib/player-positions";

const TOKEN_A = "CAAAA";
const TOKEN_B = "CBBBB";

function position(
  tableId: number,
  stack: number,
  committed = 0,
  phase = "Preflop"
): PlayerPosition {
  return {
    tableId,
    exists: true,
    seated: true,
    seatIndex: 0,
    stack: BigInt(stack),
    committed: BigInt(committed),
    totalBuyIn: BigInt(stack),
    phase,
    handNumber: 1,
  };
}

describe("buildPortfolio", () => {
  it("returns no rows and no tables when the wallet has no seats", () => {
    const result = buildPortfolio([], new Map(), new Map());
    expect(result).toEqual({ rows: [], seatedTableCount: 0 });
  });

  it("sums seated chips per token and attaches that token's wallet balance", () => {
    const tokens = new Map([
      [1, TOKEN_A],
      [2, TOKEN_A],
      [3, TOKEN_B],
    ]);
    const balances = new Map([
      [TOKEN_A, BigInt(900)],
      [TOKEN_B, BigInt(50)],
    ]);

    const result = buildPortfolio(
      [position(1, 300, 20), position(2, 200, 0), position(3, 70, 10)],
      tokens,
      balances
    );

    expect(result.seatedTableCount).toBe(3);
    expect(result.rows).toHaveLength(2);
    const a = result.rows.find((r) => r.token === TOKEN_A)!;
    expect(a.seatedChips).toBe(BigInt(500));
    expect(a.inPot).toBe(BigInt(20));
    expect(a.walletBalance).toBe(BigInt(900));
    expect(a.tables.map((t) => t.tableId)).toEqual([1, 2]);
    const b = result.rows.find((r) => r.token === TOKEN_B)!;
    expect(b.seatedChips).toBe(BigInt(70));
    expect(b.walletBalance).toBe(BigInt(50));
  });

  it("leaves the wallet balance null when the balance was not read", () => {
    const result = buildPortfolio(
      [position(1, 100)],
      new Map([[1, TOKEN_A]]),
      new Map()
    );
    expect(result.rows[0].walletBalance).toBeNull();
  });

  it("keeps tables whose token is unknown under a null token row", () => {
    const result = buildPortfolio(
      [position(1, 100), position(2, 40)],
      new Map([[1, TOKEN_A]]),
      new Map([[TOKEN_A, BigInt(0)]])
    );
    const unknown = result.rows.find((r) => r.token === null)!;
    expect(unknown.seatedChips).toBe(BigInt(40));
    expect(unknown.walletBalance).toBeNull();
    expect(result.rows.map((r) => r.token)).toEqual([TOKEN_A, null]);
  });

  it("orders tables by id within a token", () => {
    const result = buildPortfolio(
      [position(9, 1), position(4, 1), position(7, 1)],
      new Map([
        [9, TOKEN_A],
        [4, TOKEN_A],
        [7, TOKEN_A],
      ]),
      new Map()
    );
    expect(result.rows[0].tables.map((t) => t.tableId)).toEqual([4, 7, 9]);
  });
});

describe("formatTokenAmount", () => {
  it("shifts by the decimal count and keeps the fraction", () => {
    expect(formatTokenAmount(BigInt(12345000000))).toBe("1234.5");
    expect(formatTokenAmount(BigInt(1))).toBe("0.0000001");
  });

  it("prints whole amounts without a decimal point", () => {
    expect(formatTokenAmount(BigInt(10000000))).toBe("1");
    expect(formatTokenAmount(BigInt(0))).toBe("0");
  });

  it("keeps every digit of balances beyond Number precision", () => {
    const huge = BigInt("123456789012345678901234567");
    expect(formatTokenAmount(huge, 0)).toBe("123456789012345678901234567");
    expect(formatTokenAmount(huge)).toBe("12345678901234567890.1234567");
  });

  it("formats negative amounts", () => {
    expect(formatTokenAmount(BigInt(-15000000))).toBe("-1.5");
  });
});
