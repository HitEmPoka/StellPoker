import { renderHook, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useAutoRebuy } from "../lib/use-auto-rebuy";
import { setAutoRebuyPreference } from "../lib/auto-rebuy-store";
import * as onchain from "../lib/onchain";
import type { WalletSession } from "../lib/wallet";

vi.mock("../lib/onchain", () => ({
  getTableConfig: vi.fn(),
  getPlayerRebuyCount: vi.fn(),
  getTokenBalance: vi.fn(),
  rebuyOnChain: vi.fn(),
}));

const wallet = { address: "GABC123", walletType: "freighter" } as unknown as WalletSession;
const TABLE_ID = 9;

describe("useAutoRebuy (Issue #164)", () => {
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
    vi.mocked(onchain.getTableConfig).mockResolvedValue({
      minBuyIn: BigInt(500),
      maxBuyIn: BigInt(2000),
      maxRebuys: 0,
      tokenContract: "CTOKEN123",
      bigBlind: BigInt(10),
    });
    vi.mocked(onchain.getPlayerRebuyCount).mockResolvedValue(0);
    vi.mocked(onchain.getTokenBalance).mockResolvedValue(BigInt(10_000));
    vi.mocked(onchain.rebuyOnChain).mockResolvedValue("tx-hash");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("does nothing when preference is 'never' — no on-chain reads happen", async () => {
    setAutoRebuyPreference(TABLE_ID, wallet.address, { mode: "never" });
    const { result } = renderHook(() =>
      useAutoRebuy({ tableId: TABLE_ID, wallet, phase: "waiting", currentStack: 100 })
    );

    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(onchain.getTableConfig).not.toHaveBeenCalled();
  });

  it("submits a rebuy when 'always_max' is set and the stack is below max", async () => {
    setAutoRebuyPreference(TABLE_ID, wallet.address, { mode: "always_max" });
    const { result } = renderHook(() =>
      useAutoRebuy({ tableId: TABLE_ID, wallet, phase: "waiting", currentStack: 500 })
    );

    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(onchain.rebuyOnChain).toHaveBeenCalledWith(wallet, TABLE_ID, BigInt(1500));
  });

  it("does not submit a rebuy when the stack is already at max", async () => {
    setAutoRebuyPreference(TABLE_ID, wallet.address, { mode: "always_max" });
    const { result } = renderHook(() =>
      useAutoRebuy({ tableId: TABLE_ID, wallet, phase: "waiting", currentStack: 2000 })
    );

    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(onchain.rebuyOnChain).not.toHaveBeenCalled();
  });

  it("does not submit a rebuy when the wallet balance is insufficient", async () => {
    setAutoRebuyPreference(TABLE_ID, wallet.address, { mode: "always_max" });
    vi.mocked(onchain.getTokenBalance).mockResolvedValue(BigInt(100));
    const { result } = renderHook(() =>
      useAutoRebuy({ tableId: TABLE_ID, wallet, phase: "waiting", currentStack: 500 })
    );

    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(onchain.rebuyOnChain).not.toHaveBeenCalled();
  });

  it("only checks once per entry into the 'waiting' phase, not on every re-render", async () => {
    setAutoRebuyPreference(TABLE_ID, wallet.address, { mode: "always_max" });
    const { result, rerender } = renderHook(
      ({ currentStack }) =>
        useAutoRebuy({ tableId: TABLE_ID, wallet, phase: "waiting", currentStack }),
      { initialProps: { currentStack: 500 } }
    );

    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(onchain.getTableConfig).toHaveBeenCalledTimes(1);

    // Re-render while still "waiting" (e.g. an unrelated state update) must
    // not trigger a second check.
    rerender({ currentStack: 600 });
    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(onchain.getTableConfig).toHaveBeenCalledTimes(1);
  });

  it("checks again on a fresh transition back into 'waiting' after leaving it", async () => {
    setAutoRebuyPreference(TABLE_ID, wallet.address, { mode: "always_max" });
    const { result, rerender } = renderHook(
      ({ phase }: { phase: "waiting" | "preflop" }) =>
        useAutoRebuy({ tableId: TABLE_ID, wallet, phase, currentStack: 500 }),
      { initialProps: { phase: "waiting" } as { phase: "waiting" | "preflop" } }
    );

    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(onchain.getTableConfig).toHaveBeenCalledTimes(1);

    rerender({ phase: "preflop" });
    rerender({ phase: "waiting" });

    await waitFor(() => expect(onchain.getTableConfig).toHaveBeenCalledTimes(2));
  });

  it("surfaces a network error via lastError instead of throwing", async () => {
    setAutoRebuyPreference(TABLE_ID, wallet.address, { mode: "always_max" });
    vi.mocked(onchain.getTableConfig).mockRejectedValue(new Error("RPC unreachable"));
    const { result } = renderHook(() =>
      useAutoRebuy({ tableId: TABLE_ID, wallet, phase: "waiting", currentStack: 500 })
    );

    await waitFor(() => expect(result.current.status).toBe("error"));
    expect(result.current.lastError).toBe("RPC unreachable");
  });
});
