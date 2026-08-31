import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { TransactionSimulation } from "../components/TransactionSimulation";
import type { SimulationResult } from "../lib/transaction-simulation";

const successfulSimulation: SimulationResult = {
  success: true,
  fee: "0.0000123",
  gasUsed: 1230,
  stateChanges: [],
};

describe("TransactionSimulation buy-in summary (Issue #161)", () => {
  it("shows no buy-in summary when buyInAmount is not provided", () => {
    const { queryByText } = render(
      <TransactionSimulation
        simulation={successfulSimulation}
        onConfirm={() => {}}
        onCancel={() => {}}
      />
    );

    expect(queryByText("Buy-in Summary")).toBeNull();
  });

  it("shows buy-in amount, fee, gas price, and total deduction when buyInAmount is provided", () => {
    // 10 XLM buy-in = 100_000_000 stroops
    const { getByText } = render(
      <TransactionSimulation
        simulation={successfulSimulation}
        onConfirm={() => {}}
        onCancel={() => {}}
        buyInAmount={BigInt(100_000_000)}
      />
    );

    expect(getByText("Buy-in Summary")).toBeTruthy();
    expect(getByText("Buy-in: 10 XLM")).toBeTruthy();
    expect(getByText("Network fee: 0.0000123 XLM")).toBeTruthy();
    // fee in stroops = 123, gasUsed = 1230 -> 123/1230 = 0 (bigint floor division)
    expect(getByText(/Gas price: 0 stroops\/instruction/)).toBeTruthy();
    // total = 100_000_000 + 123 stroops = 100000123 stroops = 10.0000123 XLM
    expect(getByText("Total deduction: 10.0000123 XLM")).toBeTruthy();
  });

  it("computes a non-zero gas price when fee-per-instruction is at least 1 stroop", () => {
    const simulation: SimulationResult = {
      success: true,
      fee: "0.001", // 10_000 stroops
      gasUsed: 100,
      stateChanges: [],
    };
    const { getByText } = render(
      <TransactionSimulation
        simulation={simulation}
        onConfirm={() => {}}
        onCancel={() => {}}
        buyInAmount={BigInt(100_000_000)}
      />
    );

    // 10_000 stroops / 100 instructions = 100 stroops/instruction
    expect(getByText(/Gas price: 100 stroops\/instruction/)).toBeTruthy();
  });

  it("does not show buy-in summary when the simulation itself failed", () => {
    const failed: SimulationResult = {
      success: false,
      fee: "0",
      stateChanges: [],
      error: "Simulation failed",
    };
    const { queryByText } = render(
      <TransactionSimulation
        simulation={failed}
        onConfirm={() => {}}
        onCancel={() => {}}
        buyInAmount={BigInt(100_000_000)}
      />
    );

    expect(queryByText("Buy-in Summary")).toBeNull();
  });
});
