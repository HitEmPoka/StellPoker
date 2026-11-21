import { describe, it, expect, afterEach, vi } from "vitest";
import { fetchTransactionFee, formatStroopsAsXlm } from "../lib/tx-fee";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("fetchTransactionFee (#625)", () => {
  it("reads fee_charged from the Horizon transaction record", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: true, json: async () => ({ fee_charged: "12345" }) }))
    );

    await expect(fetchTransactionFee("abc")).resolves.toEqual({ feeStroops: 12345 });
  });

  it("resolves to null when the transaction is not found", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: false, json: async () => ({}) })));

    await expect(fetchTransactionFee("missing")).resolves.toBeNull();
  });

  it("resolves to null on a network error instead of rejecting", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new TypeError("network down");
      })
    );

    await expect(fetchTransactionFee("abc")).resolves.toBeNull();
  });
});

describe("formatStroopsAsXlm", () => {
  it("trims trailing zeros from the XLM amount", () => {
    expect(formatStroopsAsXlm(10_000_000)).toBe("1 XLM");
    expect(formatStroopsAsXlm(1_000)).toBe("0.0001 XLM");
    expect(formatStroopsAsXlm(15_000_000)).toBe("1.5 XLM");
  });
});
