"use client";

import { useState } from "react";
import type { AutoActionOnTimeout, TurnTimerPreference } from "@/lib/turn-timer-store";
import {
  DEFAULT_TURN_TIMER,
  getTurnTimerPreference,
  setTurnTimerPreference,
} from "@/lib/turn-timer-store";
import { useT } from "@/lib/i18n/context";

interface TurnTimerSettingsProps {
  open: boolean;
  onClose: () => void;
  tableId: number;
  address: string;
}

const ACTION_LABELS: Record<AutoActionOnTimeout, string> = {
  fold: "Auto-fold on timeout (safe default)",
  check_if_possible: "Check if free, otherwise fold",
  never: "Never auto-act (timer only)",
};

const DURATION_PRESETS = [15, 30, 45, 60, 90];

export function TurnTimerSettings({
  open,
  onClose,
  tableId,
  address,
}: TurnTimerSettingsProps) {
  const t = useT();
  const initial = open
    ? getTurnTimerPreference(tableId, address)
    : DEFAULT_TURN_TIMER;
  const [enabled, setEnabled] = useState<boolean>(initial.enabled);
  const [durationSeconds, setDurationSeconds] = useState<number>(
    initial.durationSeconds
  );
  const [autoAction, setAutoAction] = useState<AutoActionOnTimeout>(
    initial.autoAction
  );

  if (!open) return null;

  const handleSave = () => {
    const pref: TurnTimerPreference = {
      enabled,
      durationSeconds: Math.max(
        5,
        Math.min(180, Math.floor(Number(durationSeconds) || DEFAULT_TURN_TIMER.durationSeconds))
      ),
      autoAction,
    };
    setTurnTimerPreference(tableId, address, pref);
    onClose();
  };

  const headerTitle =
    (t("turnTimer.title") as string | undefined) ?? "TURN TIMER";
  const headerEnabled =
    (t("turnTimer.enabled") as string | undefined) ?? "Enable timer & auto-action";
  const headerDuration =
    (t("turnTimer.duration") as string | undefined) ?? "Duration (seconds)";
  const headerPreset =
    (t("turnTimer.preset") as string | undefined) ?? "Preset";
  const headerCustom =
    (t("turnTimer.custom") as string | undefined) ?? "Custom";
  const headerAction =
    (t("turnTimer.autoAction") as string | undefined) ?? "On timeout";
  const headerNote =
    (t("turnTimer.note") as string | undefined) ??
    "Client-side only. The on-chain coordinator has its own timeout.";
  const saveLabel = (t("turnTimer.save") as string | undefined) ?? "SAVE";

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
          width: "360px",
          padding: "16px",
        }}
      >
        <div className="flex items-center justify-between mb-3">
          <span className="text-[10px]" style={{ color: "#f5e6c8" }}>
            {headerTitle}
          </span>
          <button
            onClick={onClose}
            style={{
              background: "none",
              border: "none",
              color: "#e74c3c",
              cursor: "pointer",
            }}
          >
            ✕
          </button>
        </div>

        <label className="flex items-center gap-2 mb-3 text-[9px]" style={{ color: "#c8e6ff" }}>
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
          />
          {headerEnabled}
        </label>

        <div className="mb-3">
          <div className="text-[8px] mb-1" style={{ color: "#95a5a6" }}>
            {headerDuration}
          </div>
          <div className="flex flex-wrap gap-1 mb-2">
            {DURATION_PRESETS.map((p) => (
              <button
                key={p}
                onClick={() => setDurationSeconds(p)}
                className="pixel-btn text-[8px]"
                style={{
                  padding: "4px 6px",
                  background: durationSeconds === p ? "#c47d2e" : "#2c2230",
                  color: durationSeconds === p ? "#fff" : "#c8e6ff",
                  border:
                    durationSeconds === p
                      ? "1px solid #f5e6c8"
                      : "1px solid rgba(140,170,200,0.4)",
                  cursor: "pointer",
                }}
              >
                {headerPreset} {p}s
              </button>
            ))}
          </div>
          <div className="flex items-center gap-2">
            <span className="text-[8px]" style={{ color: "#95a5a6" }}>
              {headerCustom}
            </span>
            <input
              type="number"
              min={5}
              max={180}
              value={durationSeconds}
              onChange={(e) =>
                setDurationSeconds(
                  Math.max(
                    5,
                    Math.min(180, Number(e.target.value) || DEFAULT_TURN_TIMER.durationSeconds)
                  )
                )
              }
              className="text-[9px] bg-black text-white border border-gray-600 px-1 py-0.5 w-16"
            />
          </div>
        </div>

        <div className="flex flex-col gap-2 mb-3">
          <div className="text-[8px]" style={{ color: "#95a5a6" }}>
            {headerAction}
          </div>
          {(Object.keys(ACTION_LABELS) as AutoActionOnTimeout[]).map((m) => (
            <label
              key={m}
              className="flex items-center gap-2 text-[9px]"
              style={{ color: "#c8e6ff" }}
            >
              <input
                type="radio"
                name="turn-timer-autoaction"
                checked={autoAction === m}
                onChange={() => setAutoAction(m)}
              />
              {ACTION_LABELS[m]}
            </label>
          ))}
        </div>

        <div className="mb-3 text-[8px]" style={{ color: "#f39c12" }}>
          ⚠ {headerNote}
        </div>

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
          {saveLabel}
        </button>
      </div>
    </div>
  );
}
