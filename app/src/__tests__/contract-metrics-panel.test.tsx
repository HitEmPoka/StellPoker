import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ContractMetricsPanel } from "@/components/ContractMetricsPanel";
import type { ContractMetrics } from "@/lib/contract-metrics";

const metrics: ContractMetrics = {
  tablesCreated: 12,
  handsPlayed: BigInt(1500),
  totalRake: BigInt(48750),
  activeSeats: 9,
};

describe("ContractMetricsPanel", () => {
  it("shows placeholders until metrics load", () => {
    render(<ContractMetricsPanel metrics={null} loading={true} error={null} onRefresh={() => {}} />);

    expect(screen.getByTestId("metric-hands-played").textContent).toContain("—");
    expect(screen.getByTestId("metric-total-rake").textContent).toContain("—");
    expect(screen.queryByTestId("average-rake")).toBeNull();
  });

  it("renders every counter from the contract view", () => {
    render(
      <ContractMetricsPanel metrics={metrics} loading={false} error={null} onRefresh={() => {}} />
    );

    expect(screen.getByTestId("metric-tables-created").textContent).toContain("12");
    expect(screen.getByTestId("metric-hands-played").textContent).toContain("1,500");
    expect(screen.getByTestId("metric-total-rake").textContent).toContain("48,750 chips");
    expect(screen.getByTestId("metric-active-seats").textContent).toContain("9");
  });

  it("shows the average rake per hand once hands have been played", () => {
    render(
      <ContractMetricsPanel metrics={metrics} loading={false} error={null} onRefresh={() => {}} />
    );

    expect(screen.getByTestId("average-rake").textContent).toContain("32 chips");
  });

  it("omits the average before the first hand", () => {
    render(
      <ContractMetricsPanel
        metrics={{ ...metrics, handsPlayed: BigInt(0), totalRake: BigInt(0) }}
        loading={false}
        error={null}
        onRefresh={() => {}}
      />
    );

    expect(screen.queryByTestId("average-rake")).toBeNull();
  });

  it("surfaces a load error without hiding the panel", () => {
    render(
      <ContractMetricsPanel
        metrics={null}
        loading={false}
        error="Simulation failed"
        onRefresh={() => {}}
      />
    );

    expect(screen.getByRole("alert").textContent).toContain("Simulation failed");
    expect(screen.getByTestId("contract-metrics-panel")).toBeTruthy();
  });

  it("refreshes on demand and disables the button while loading", () => {
    const onRefresh = vi.fn();
    const { rerender } = render(
      <ContractMetricsPanel metrics={metrics} loading={false} error={null} onRefresh={onRefresh} />
    );

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(onRefresh).toHaveBeenCalledTimes(1);

    rerender(
      <ContractMetricsPanel metrics={metrics} loading={true} error={null} onRefresh={onRefresh} />
    );
    const button = screen.getByRole("button", { name: "Refreshing..." }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });
});
