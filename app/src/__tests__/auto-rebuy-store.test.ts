import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { getAutoRebuyPreference, setAutoRebuyPreference } from "../lib/auto-rebuy-store";

const ADDRESS = "GABC123";
const TABLE_ID = 7;

describe("auto-rebuy-store (Issue #164)", () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => { store.set(key, value); },
      removeItem: (key: string) => { store.delete(key); },
      clear: () => store.clear(),
      length: 0,
      key: () => null,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults to 'never' when nothing has been stored", () => {
    expect(getAutoRebuyPreference(TABLE_ID, ADDRESS)).toEqual({ mode: "never" });
  });

  it("round-trips a stored preference", () => {
    setAutoRebuyPreference(TABLE_ID, ADDRESS, { mode: "always_max" });
    expect(getAutoRebuyPreference(TABLE_ID, ADDRESS)).toEqual({ mode: "always_max" });
  });

  it("round-trips a below_threshold preference with its threshold", () => {
    setAutoRebuyPreference(TABLE_ID, ADDRESS, { mode: "below_threshold", thresholdBB: 25 });
    expect(getAutoRebuyPreference(TABLE_ID, ADDRESS)).toEqual({
      mode: "below_threshold",
      thresholdBB: 25,
    });
  });

  it("keeps preferences separate per table for the same address", () => {
    setAutoRebuyPreference(1, ADDRESS, { mode: "always_max" });
    setAutoRebuyPreference(2, ADDRESS, { mode: "never" });
    expect(getAutoRebuyPreference(1, ADDRESS).mode).toBe("always_max");
    expect(getAutoRebuyPreference(2, ADDRESS).mode).toBe("never");
  });

  it("keeps preferences separate per address for the same table", () => {
    setAutoRebuyPreference(TABLE_ID, "GABC", { mode: "always_max" });
    setAutoRebuyPreference(TABLE_ID, "GXYZ", { mode: "never" });
    expect(getAutoRebuyPreference(TABLE_ID, "GABC").mode).toBe("always_max");
    expect(getAutoRebuyPreference(TABLE_ID, "GXYZ").mode).toBe("never");
  });

  it("falls back to 'never' for corrupted stored JSON", () => {
    localStorage.setItem(`stellpoker:auto-rebuy:${TABLE_ID}:${ADDRESS}`, "{not json");
    expect(getAutoRebuyPreference(TABLE_ID, ADDRESS)).toEqual({ mode: "never" });
  });

  it("falls back to 'never' for an unrecognized mode value", () => {
    localStorage.setItem(
      `stellpoker:auto-rebuy:${TABLE_ID}:${ADDRESS}`,
      JSON.stringify({ mode: "sometimes" })
    );
    expect(getAutoRebuyPreference(TABLE_ID, ADDRESS)).toEqual({ mode: "never" });
  });
});
