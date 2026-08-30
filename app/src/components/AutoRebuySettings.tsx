"use client";

import { useState } from "react";
import type { AutoRebuyMode } from "@/lib/auto-rebuy";
import { getAutoRebuyPreference, setAutoRebuyPreference } from "@/lib/auto-rebuy-store";

interface AutoRebuySettingsProps {
  open: boolean;
  onClose: () => void;
  tableId: number;
  address: string;
}

const MODE_LABELS: Record<AutoRebuyMode, string> = {
  always_max: "Always rebuy to max",
  below_threshold: "Rebuy when below N big blinds",
  never: "Never auto-rebuy",
};

const DEFAULT_THRESHOLD_BB = 20;

export function AutoRebuySettings({ open, onClose, tableId, address }: AutoRebuySettingsProps) {
  const [mode, setMode] = useState<AutoRebuyMode>(
    () => getAutoRebuyPreference(tableId, address).mode
  );
  const [thresholdBB, setThresholdBB] = useState<number>(
    () => getAutoRebuyPreference(tableId, address).thresholdBB ?? DEFAULT_THRESHOLD_BB
  );

  if (!open) return null;

  const handleSave = () => {
    setAutoRebuyPreference(tableId, address, {
      mode,
      thresholdBB: mode === "below_threshold" ? thresholdBB : undefined,
    });
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-[110] flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.7)" }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="pixel-border"
        style={{
          background: "rgba(12, 10, 24, 0.98)",
          borderColor: "#c47d2e",
          width: "320px",
          padding: "16px",
        }}
      >
        <div className="flex items-center justify-between mb-3">
          <span className="text-[10px]" style={{ color: "#f5e6c8" }}>AUTO-REBUY</span>
          <button
            onClick={onClose}
            style={{ background: "none", border: "none", color: "#e74c3c", cursor: "pointer" }}
          >
            ✕
          </button>
        </div>

        <div className="flex flex-col gap-2 mb-3">
          {(Object.keys(MODE_LABELS) as AutoRebuyMode[]).map((m) => (
            <label key={m} className="flex items-center gap-2 text-[9px]" style={{ color: "#c8e6ff" }}>
              <input
                type="radio"
                name="auto-rebuy-mode"
                checked={mode === m}
                onChange={() => setMode(m)}
              />
              {MODE_LABELS[m]}
            </label>
          ))}
        </div>

        {mode === "below_threshold" && (
          <div className="flex items-center gap-2 mb-3">
            <label htmlFor="threshold-bb" className="text-[8px]" style={{ color: "#95a5a6" }}>
              Threshold (big blinds):
            </label>
            <input
              id="threshold-bb"
              type="number"
              min={1}
              value={thresholdBB}
              onChange={(e) => setThresholdBB(Math.max(1, Number(e.target.value) || 1))}
              className="text-[9px] bg-black text-white border border-gray-600 px-1 py-0.5 w-16"
            />
          </div>
        )}

        <button
          onClick={handleSave}
          style={{
            fontFamily: "'Press Start 2P', monospace",
            fontSize: "9px",
            background: "#c47d2e",
            border: "none",
            color: "#fff",
            cursor: "pointer",
            padding: "8px",
            width: "100%",
          }}
        >
          SAVE
        </button>
      </div>
    </div>
  );
}
