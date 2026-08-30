import { describe, it, expect } from "vitest";
import { decideAutoRebuy, type AutoRebuyContext } from "../lib/auto-rebuy";

const baseCtx: AutoRebuyContext = {
  preference: { mode: "always_max" },
  currentStack: BigInt(500),
  bigBlind: BigInt(10),
  minBuyIn: BigInt(500),
  maxBuyIn: BigInt(2000),
  maxRebuys: 0,
  rebuyCount: 0,
  walletBalance: BigInt(10_000),
};

describe("decideAutoRebuy — mode: never (Issue #164)", () => {
  it("never triggers, regardless of stack", () => {
    const result = decideAutoRebuy({ ...baseCtx, preference: { mode: "never" }, currentStack: BigInt(0) });
    expect(result.shouldRebuy).toBe(false);
    expect(result.amount).toBe(BigInt(0));
    expect(result.reason).toMatch(/disabled/);
  });
});

describe("decideAutoRebuy — mode: always_max", () => {
  it("tops up to max_buy_in when below it", () => {
    const result = decideAutoRebuy({ ...baseCtx, currentStack: BigInt(500) });
    expect(result.shouldRebuy).toBe(true);
    expect(result.amount).toBe(BigInt(1500)); // 2000 - 500
  });

  it("does nothing when the stack is already at max", () => {
    const result = decideAutoRebuy({ ...baseCtx, currentStack: BigInt(2000) });
    expect(result.shouldRebuy).toBe(false);
    expect(result.amount).toBe(BigInt(0));
  });

  it("does nothing when the stack exceeds max (shouldn't happen, but must not request a negative rebuy)", () => {
    const result = decideAutoRebuy({ ...baseCtx, currentStack: BigInt(2500) });
    expect(result.shouldRebuy).toBe(false);
  });
});

describe("decideAutoRebuy — mode: below_threshold", () => {
  it("triggers when the stack drops below thresholdBB * bigBlind", () => {
    const result = decideAutoRebuy({
      ...baseCtx,
      preference: { mode: "below_threshold", thresholdBB: 20 }, // 20 * 10 = 200
      currentStack: BigInt(150),
    });
    expect(result.shouldRebuy).toBe(true);
    expect(result.amount).toBe(BigInt(1850)); // 2000 - 150
  });

  it("does not trigger when the stack is at or above the threshold", () => {
    const result = decideAutoRebuy({
      ...baseCtx,
      preference: { mode: "below_threshold", thresholdBB: 20 },
      currentStack: BigInt(200),
    });
    expect(result.shouldRebuy).toBe(false);
  });

  it("treats a missing/negative thresholdBB as 0 (never triggers on a non-negative stack)", () => {
    const result = decideAutoRebuy({
      ...baseCtx,
      preference: { mode: "below_threshold", thresholdBB: -5 },
      currentStack: BigInt(500),
    });
    expect(result.shouldRebuy).toBe(false);
  });
});

describe("decideAutoRebuy — respecting table rebuy limits", () => {
  it("does not trigger once the table's max_rebuys is reached", () => {
    const result = decideAutoRebuy({
      ...baseCtx,
      currentStack: BigInt(0),
      maxRebuys: 3,
      rebuyCount: 3,
    });
    expect(result.shouldRebuy).toBe(false);
    expect(result.reason).toMatch(/limit/);
  });

  it("still triggers below the limit", () => {
    const result = decideAutoRebuy({
      ...baseCtx,
      currentStack: BigInt(0),
      maxRebuys: 3,
      rebuyCount: 2,
    });
    expect(result.shouldRebuy).toBe(true);
  });

  it("max_rebuys of 0 means unlimited — never blocks on count", () => {
    const result = decideAutoRebuy({
      ...baseCtx,
      currentStack: BigInt(0),
      maxRebuys: 0,
      rebuyCount: 999,
    });
    expect(result.shouldRebuy).toBe(true);
  });
});

describe("decideAutoRebuy — respecting wallet balance", () => {
  it("does not trigger when the wallet can't cover the rebuy amount", () => {
    const result = decideAutoRebuy({
      ...baseCtx,
      currentStack: BigInt(500),
      walletBalance: BigInt(1000), // needs 1500
    });
    expect(result.shouldRebuy).toBe(false);
    expect(result.reason).toMatch(/insufficient/i);
  });

  it("triggers when the wallet balance exactly covers the rebuy amount", () => {
    const result = decideAutoRebuy({
      ...baseCtx,
      currentStack: BigInt(500),
      walletBalance: BigInt(1500), // exactly needs 1500
    });
    expect(result.shouldRebuy).toBe(true);
  });
});
