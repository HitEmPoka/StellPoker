import { describe, expect, it, vi, beforeEach } from "vitest";

vi.mock("@/lib/api", () => ({
  getChainConfig: vi.fn().mockResolvedValue({
    rpc_url: "http://localhost:8000",
    network_passphrase: "Test SDF Network ; September 2015",
    poker_table_contract: "CCONTRACT123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ",
  }),
}));

vi.mock("@stellar/stellar-sdk", () => {
  return {
    BASE_FEE: "100",
    rpc: {
      Server: class {
        async getAccount(address: string) {
          if (!address) {
            throw new Error("invalid account");
          }
          return { address, sequence: 1 };
        }
        async simulateTransaction() {
          return {
            error: null,
            minResourceFee: "1000000",
            cost: { cpuInsns: "1200" },
            results: [{ xdr: "result-xdr" }],
          };
        }
      },
      Api: { GetTransactionStatus: { SUCCESS: "SUCCESS", FAILED: "FAILED" } },
    },
    Contract: class {
      call = vi.fn().mockReturnValue("operation");
    },
    Address: class {
      constructor(private addr: string) {}
      toScVal = () => `scval_${this.addr}`;
    },
    nativeToScVal: (val: unknown) => `scval_${String(val)}`,
    xdr: {
      ScVal: {
        scvSymbol: (s: string) => s,
        scvVec: (v: unknown[]) => v,
      },
    },
    TransactionBuilder: class {
      constructor(private account: { address: string; sequence: number }) {}
      addOperation() {
        return this;
      }
      setTimeout() {
        return this;
      }
      build() {
        return {
          ...this.account,
          toXDR: () => `tx_${this.account.address}`,
        };
      }
    },
  };
});

describe("simulateJoinTable", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns a successful simulation with fee and state changes", async () => {
    const { simulateJoinTable } = await import("@/lib/transaction-simulation");
    const result = await simulateJoinTable("GADDR1", 1, BigInt(1000));

    expect(result.success).toBe(true);
    expect(result.fee).toBe("0.1");
    expect(result.stateChanges.length).toBeGreaterThan(0);
    expect(result.error).toBeUndefined();
  });

  it("returns failure when the account cannot be loaded", async () => {
    const { simulateJoinTable } = await import("@/lib/transaction-simulation");
    const result = await simulateJoinTable("", 1, BigInt(1000));
    expect(result.success).toBe(false);
    expect(result.error).toBeDefined();
    expect(result.stateChanges).toEqual([]);
  });
});

describe("simulatePlayerAction", () => {
  it("simulates a fold action successfully", async () => {
    const { simulatePlayerAction } = await import("@/lib/transaction-simulation");
    const result = await simulatePlayerAction("GADDR1", 1, "fold");
    expect(result.success).toBe(true);
    expect(result.error).toBeUndefined();
  });

  it("simulates a bet with a positive amount", async () => {
    const { simulatePlayerAction } = await import("@/lib/transaction-simulation");
    const result = await simulatePlayerAction("GADDR1", 1, "bet", 50);
    expect(result.success).toBe(true);
  });

  it("rejects a bet with a non-positive amount", async () => {
    const { simulatePlayerAction } = await import("@/lib/transaction-simulation");
    await expect(
      simulatePlayerAction("GADDR1", 1, "bet", 0),
    ).rejects.toThrow("Bet amount must be a positive number");
    await expect(
      simulatePlayerAction("GADDR1", 1, "bet", -5),
    ).rejects.toThrow("Bet amount must be a positive number");
  });

  it("rejects an unsupported action", async () => {
    const { simulatePlayerAction } = await import("@/lib/transaction-simulation");
    await expect(
      simulatePlayerAction("GADDR1", 1, "cheat" as any, 10),
    ).rejects.toThrow("Unsupported action");
  });
});
