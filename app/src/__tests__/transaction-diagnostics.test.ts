import { describe, it, expect } from "vitest";
import {
  diagnoseTransactionFailure,
  classifyFailureCode,
  extractSimulationLogs,
  DIAGNOSTIC_METADATA,
} from "../lib/transaction-diagnostics";

describe("Transaction Failure Deep Diagnostics (Issue #619)", () => {
  describe("Coverage of required failure codes", () => {
    it("diagnoses insufficient balance errors", () => {
      const inputs = [
        "op_underfunded: account has insufficient balance",
        "tx_insufficient_balance",
        new Error("Insufficient balance for buy-in"),
        { error: "Account balance too low for reserve" },
      ];

      for (const input of inputs) {
        const diag = diagnoseTransactionFailure(input);
        expect(diag.code).toBe("insufficient_balance");
        expect(diag.title).toBe(DIAGNOSTIC_METADATA.insufficient_balance.title);
        expect(diag.docsUrl).toContain("fees-resource-limits-metering#base-reserves");
        expect(diag.commonCauses.length).toBeGreaterThan(0);
        expect(diag.actionableRemedy).toBeTruthy();
        expect(diag.i18nKey).toBe("diagnostics.insufficient_balance");
      }
    });

    it("diagnoses timeout errors", () => {
      const inputs = [
        "tx_timeout: submission timed out waiting for inclusion",
        "RPC request timed out after 30000ms",
        new Error("Network deadline exceeded"),
        { error: "Gateway timeout communicating with validator" },
      ];

      for (const input of inputs) {
        const diag = diagnoseTransactionFailure(input);
        expect(diag.code).toBe("timeout");
        expect(diag.title).toBe(DIAGNOSTIC_METADATA.timeout.title);
        expect(diag.docsUrl).toContain("transaction-lifecycle");
        expect(diag.commonCauses.length).toBeGreaterThan(0);
        expect(diag.actionableRemedy).toBeTruthy();
        expect(diag.i18nKey).toBe("diagnostics.timeout");
      }
    });

    it("diagnoses sequence number mismatch errors", () => {
      const inputs = [
        "tx_bad_seq: bad sequence number",
        "Sequence mismatch on ledger account",
        new Error("Account seq number does not match current state"),
        { error: "bad_seq error in transaction builder" },
      ];

      for (const input of inputs) {
        const diag = diagnoseTransactionFailure(input);
        expect(diag.code).toBe("seq");
        expect(diag.title).toBe(DIAGNOSTIC_METADATA.seq.title);
        expect(diag.docsUrl).toContain("sequence-number");
        expect(diag.commonCauses.length).toBeGreaterThan(0);
        expect(diag.actionableRemedy).toBeTruthy();
        expect(diag.i18nKey).toBe("diagnostics.seq");
      }
    });

    it("diagnoses expired transaction errors", () => {
      const inputs = [
        "tx_expired: ledger bounds have been passed",
        "Transaction expired before inclusion in block",
        new Error("Time bounds expired: maxTime 1690000000 passed"),
        { error: "Ledger sequence expired" },
      ];

      for (const input of inputs) {
        const diag = diagnoseTransactionFailure(input);
        expect(diag.code).toBe("expired");
        expect(diag.title).toBe(DIAGNOSTIC_METADATA.expired.title);
        expect(diag.docsUrl).toContain("time-bounds");
        expect(diag.commonCauses.length).toBeGreaterThan(0);
        expect(diag.actionableRemedy).toBeTruthy();
        expect(diag.i18nKey).toBe("diagnostics.expired");
      }
    });

    it("diagnoses resource budget exceeded errors", () => {
      const inputs = [
        "budget_exceeded: invocation exceeded cpu limit",
        "cpu_limit_exceeded in host function call",
        "memory_limit_exceeded in Soroban VM",
        new Error("Exceeded the budget: instructions limit reached"),
        { error: "Soroban resource limit exceeded" },
      ];

      for (const input of inputs) {
        const diag = diagnoseTransactionFailure(input);
        expect(diag.code).toBe("budget");
        expect(diag.title).toBe(DIAGNOSTIC_METADATA.budget.title);
        expect(diag.docsUrl).toContain("fees-resource-limits-metering#resource-limits");
        expect(diag.commonCauses.length).toBeGreaterThan(0);
        expect(diag.actionableRemedy).toBeTruthy();
        expect(diag.i18nKey).toBe("diagnostics.budget");
      }
    });

    it("handles unknown/fallback errors gracefully", () => {
      const diag = diagnoseTransactionFailure("Custom contract panic #42");
      expect(diag.code).toBe("unknown");
      expect(diag.title).toBe(DIAGNOSTIC_METADATA.unknown.title);
      expect(diag.docsUrl).toBeTruthy();
      expect(diag.rawError).toBe("Custom contract panic #42");
    });
  });

  describe("Simulation logs extraction", () => {
    it("extracts diagnostic events and events from raw simulation response", () => {
      const rawSim = {
        error: "cpu_limit_exceeded",
        diagnosticEvents: [
          "Host call failed: verify_proof",
          { contract: "poker", action: "deal" },
        ],
        events: ["Contract event: round_started"],
        results: [{ error: "CPU insns limit exceeded" }],
      };

      const logs = extractSimulationLogs(rawSim);
      expect(logs).toContain("Host call failed: verify_proof");
      expect(logs).toContain(JSON.stringify({ contract: "poker", action: "deal" }));
      expect(logs).toContain("Contract event: round_started");
      expect(logs).toContain("cpu_limit_exceeded");
      expect(logs).toContain("Result Error: CPU insns limit exceeded");
    });
  });
});
