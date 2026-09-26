"use client";

import {
  averageRakePerHand,
  formatChipAmount,
  type ContractMetrics,
} from "@/lib/contract-metrics";

interface ContractMetricsPanelProps {
  metrics: ContractMetrics | null;
  loading: boolean;
  error: string | null;
  onRefresh: () => void;
}

/**
 * Contract-wide counters for the admin dashboard (Issue #563): tables, hands
 * played, lifetime rake and active seats. The values come from the poker-table
 * contract's O(1) `get_contract_metrics` view, so refreshing is cheap.
 */
export function ContractMetricsPanel({
  metrics,
  loading,
  error,
  onRefresh,
}: ContractMetricsPanelProps) {
  const averageRake = metrics ? averageRakePerHand(metrics) : null;
  const tiles: { id: string; label: string; value: string }[] = [
    {
      id: "tables-created",
      label: "Tables Created",
      value: metrics ? formatChipAmount(BigInt(metrics.tablesCreated)) : "—",
    },
    {
      id: "hands-played",
      label: "Hands Played",
      value: metrics ? formatChipAmount(metrics.handsPlayed) : "—",
    },
    {
      id: "total-rake",
      label: "Total Rake",
      value: metrics ? `${formatChipAmount(metrics.totalRake)} chips` : "—",
    },
    {
      id: "active-seats",
      label: "Active Seats",
      value: metrics ? formatChipAmount(BigInt(metrics.activeSeats)) : "—",
    },
  ];

  return (
    <div className="space-y-3" data-testid="contract-metrics-panel">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-bold text-[#f1c40f]">📊 On-Chain Metrics</h2>
        <button
          onClick={onRefresh}
          disabled={loading}
          className="text-[10px] px-2 py-1 bg-[#1a3a5c] border border-[#3498db] text-[#3498db] hover:bg-[#2471a3] hover:text-white transition disabled:opacity-50"
        >
          {loading ? "Refreshing..." : "Refresh"}
        </button>
      </div>

      {error && (
        <div
          role="alert"
          className="text-[10px] text-[#e74c3c] bg-[#1a120c] border border-[#e74c3c]/60 px-3 py-2"
        >
          Live metrics unavailable: {error}
        </div>
      )}

      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        {tiles.map((tile) => (
          <div
            key={tile.id}
            data-testid={`metric-${tile.id}`}
            className="bg-[#1a3a5c]/60 border border-[#3498db]/40 p-3"
          >
            <div className="text-[10px] text-[#95a5a6]">{tile.label}</div>
            <div className="text-sm font-bold text-[#f5e6c8]">{tile.value}</div>
          </div>
        ))}
      </div>

      {metrics && averageRake !== null && (
        <p className="text-[10px] text-[#95a5a6]" data-testid="average-rake">
          Average rake per hand: {formatChipAmount(averageRake)} chips
        </p>
      )}
    </div>
  );
}
