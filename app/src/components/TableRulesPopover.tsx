"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { getTableConfigVersion, getTableRules } from "@/lib/onchain";
import {
  formatRake,
  formatSeconds,
  type TableRules,
} from "@/lib/table-rules";

/** How often the open popover checks whether the table's config_version moved. */
const VERSION_POLL_MS = 15_000;

interface TableRulesPopoverProps {
  tableId: number;
  /** Wallet used to simulate the read-only contract calls. */
  address?: string;
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-4" style={{ fontSize: "8px", lineHeight: 1.8 }}>
      <span style={{ color: "#95a5a6" }}>{label}</span>
      <span style={{ color: "#f5e6c8", textAlign: "right" }}>{value}</span>
    </div>
  );
}

/**
 * Shows the table's rules (blinds, antes, buy-ins, rake, time limits) read
 * from chain state (#624). Rules are re-read whenever the on-chain
 * `config_version` changes, so the popover stays current while it is open.
 */
export function TableRulesPopover({ tableId, address }: TableRulesPopoverProps) {
  const [open, setOpen] = useState(false);
  const [rules, setRules] = useState<TableRules | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  // Mirrors the loaded version so the interval callback sees the latest one.
  const loadedVersionRef = useRef<number | null>(null);

  const load = useCallback(async () => {
    if (!address) return;
    setLoading(true);
    try {
      const next = await getTableRules(address, tableId);
      loadedVersionRef.current = next.configVersion;
      setRules(next);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not read table rules");
    } finally {
      setLoading(false);
    }
  }, [address, tableId]);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  // While open, poll only the cheap version read and refetch when it moves.
  useEffect(() => {
    if (!open || !address) return;
    let cancelled = false;
    const timer = window.setInterval(async () => {
      try {
        const version = await getTableConfigVersion(address, tableId);
        if (!cancelled && version !== loadedVersionRef.current) void load();
      } catch {
        // A failed poll is retried on the next tick; the last good rules stay up.
      }
    }, VERSION_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [open, address, tableId, load]);

  // Escape closes the popover, as the other header panels do.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  return (
    <span className="relative inline-block mr-2">
      <button
        onClick={() => setOpen((v) => !v)}
        className="text-[9px]"
        style={{
          background: "none",
          border: "none",
          color: "#c8e6ff",
          textDecoration: "underline",
          cursor: "pointer",
          padding: 0,
        }}
        title="Table rules"
        aria-expanded={open}
        aria-controls="table-rules-popover"
      >
        RULES
      </button>

      {open && (
        <div
          id="table-rules-popover"
          role="dialog"
          aria-label="Table rules"
          className="pixel-border"
          style={{
            position: "absolute",
            top: "calc(100% + 8px)",
            right: 0,
            zIndex: 40,
            width: 260,
            maxHeight: "70vh",
            overflowY: "auto",
            background: "rgba(12, 10, 24, 0.97)",
            padding: "10px 12px",
            textAlign: "left",
          }}
        >
          <div className="flex justify-between items-center mb-2">
            <span style={{ fontSize: "9px", color: "#f5e6c8", fontWeight: "bold" }}>
              TABLE RULES
            </span>
            {rules && (
              <span style={{ fontSize: "7px", color: "#7f8c8d" }}>
                CONFIG v{rules.configVersion}
              </span>
            )}
          </div>

          {!address && (
            <div style={{ fontSize: "8px", color: "#95a5a6" }}>
              Connect a wallet to read this table&apos;s on-chain rules.
            </div>
          )}

          {address && error && (
            <div role="alert" style={{ fontSize: "8px", color: "#e74c3c" }}>
              {error}
            </div>
          )}

          {address && !rules && loading && (
            <div style={{ fontSize: "8px", color: "#95a5a6" }}>Reading chain state…</div>
          )}

          {rules && (
            <>
              <Row label="Betting" value={rules.bettingStructure} />
              <Row label="Buy-in" value={`${rules.minBuyIn} – ${rules.maxBuyIn}`} />
              <Row
                label="Rebuys"
                value={rules.maxRebuys === 0 ? "unlimited" : String(rules.maxRebuys)}
              />
              <Row label="Rake" value={formatRake(rules.rakeBps)} />
              <Row label="Players" value={`${rules.minPlayers}–${rules.maxPlayers}`} />
              <Row
                label="Action time"
                value={
                  rules.actionTimes
                    ? `pre ${formatSeconds(rules.actionTimes.preflopSeconds)} · flop ${formatSeconds(rules.actionTimes.flopSeconds)} · turn ${formatSeconds(rules.actionTimes.turnSeconds)} · river ${formatSeconds(rules.actionTimes.riverSeconds)}`
                    : formatSeconds(rules.timeoutSeconds)
                }
              />

              <div className="mt-2 mb-1" style={{ fontSize: "8px", color: "#c47d2e" }}>
                BLINDS
              </div>
              {rules.blindLevels.map((level, i) => (
                <Row
                  key={i}
                  label={`Level ${i + 1}`}
                  value={`${level.smallBlind}/${level.bigBlind}${level.ante !== "none" ? ` · ante ${level.ante}` : ""}${level.durationSeconds > 0 ? ` · ${formatSeconds(level.durationSeconds)}` : ""}`}
                />
              ))}
            </>
          )}
        </div>
      )}
    </span>
  );
}
