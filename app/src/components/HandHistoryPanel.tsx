"use client";

import { useState } from "react";
import Link from "next/link";
import { Card } from "./Card";
import { stellarExpertUrl } from "@/lib/explorer";
import { replayUrl } from "@/lib/replay";
import { HandShareButton } from "./HandShareButton";
import type { HandHistoryEntry, Street } from "@/lib/hand-history";
import {
  exportFilename,
  exportHandHistoryCsv,
  exportHandHistoryJson,
  type ExportFormat,
} from "@/lib/hand-history-export";

interface HandHistoryPanelProps {
  open: boolean;
  onClose: () => void;
  entries: HandHistoryEntry[];
  /** Called when the user wants to replay a specific hand. */
  onReplay?: (entry: HandHistoryEntry) => void;
}

function shortAddress(address: string): string {
  return `${address.slice(0, 6)}...${address.slice(-6)}`;
}

const STREET_LABEL: Record<Street, string> = {
  preflop: "PRE-FLOP",
  flop: "FLOP",
  turn: "TURN",
  river: "RIVER",
};

/** Saves the hands as a JSON or CSV file (#158). */
function downloadHandHistory(entries: HandHistoryEntry[], format: ExportFormat) {
  const content =
    format === "json" ? exportHandHistoryJson(entries) : exportHandHistoryCsv(entries);
  const blob = new Blob([content], {
    type: format === "json" ? "application/json" : "text/csv;charset=utf-8",
  });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = exportFilename(entries[0].tableId, format);
  link.click();
  // Revoke on the next tick so the browser has started the download first.
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

/**
 * Rows rendered per page. Long sessions can hold 10k+ hands, so the list grows
 * in pages and offscreen rows skip layout via `content-visibility` (#613).
 */
export const HISTORY_PAGE_SIZE = 50;

const EXPORT_BUTTON_STYLE: React.CSSProperties = {
  fontFamily: "'Press Start 2P', monospace",
  fontSize: "7px",
  background: "rgba(196,125,46,0.15)",
  border: "1px solid #c47d2e",
  color: "#ffc078",
  cursor: "pointer",
  padding: "2px 6px",
  lineHeight: 1.4,
};

/** Renders Hand History Panel.
 * @param props(object) - props(object).
 */
export function HandHistoryPanel({ open, onClose, entries, onReplay }: HandHistoryPanelProps) {
  const [visibleCount, setVisibleCount] = useState(HISTORY_PAGE_SIZE);
  if (!open) return null;

  const visibleEntries = entries.slice(0, visibleCount);
  const remaining = entries.length - visibleEntries.length;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.6)", backdropFilter: "blur(2px)" }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="pixel-border"
        style={{
          background: "rgba(12, 10, 24, 0.97)",
          borderColor: "#c47d2e",
          width: "420px",
          maxHeight: "80vh",
          overflowY: "auto",
          padding: "16px",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-3">
          <span className="text-[11px]" style={{ color: "#f5e6c8" }}>
            HAND HISTORY (THIS SESSION)
          </span>
          <button
            onClick={onClose}
            aria-label="Close hand history"
            className="text-[11px]"
            style={{ background: "none", border: "none", color: "#e74c3c", cursor: "pointer" }}
          >
            ✕
          </button>
        </div>

        {entries.length === 0 && (
          <div className="text-[9px]" style={{ color: "#7f8c8d" }}>
            No completed hands yet this session.
          </div>
        )}

        {entries.length > 0 && (
          <div className="flex items-center justify-between gap-2 mb-3">
            <span className="text-[7px]" style={{ color: "#7f8c8d" }}>
              OPPONENTS&apos; HOLE CARDS ARE MASKED
            </span>
            <div className="flex items-center gap-2">
              <button
                onClick={() => downloadHandHistory(entries, "json")}
                title="Export hand history as JSON"
                style={EXPORT_BUTTON_STYLE}
              >
                ⬇ JSON
              </button>
              <button
                onClick={() => downloadHandHistory(entries, "csv")}
                title="Export hand history as CSV"
                style={EXPORT_BUTTON_STYLE}
              >
                ⬇ CSV
              </button>
            </div>
          </div>
        )}

        <div className="flex flex-col gap-3">
          {visibleEntries.map((entry) => (
            <div
              key={`${entry.tableId}-${entry.handNumber}-${entry.timestamp}`}
              className="pixel-border-thin"
              style={{
                padding: "8px",
                background: "rgba(255,255,255,0.03)",
                contentVisibility: "auto",
                containIntrinsicSize: "auto 180px",
              }}
            >
              {/* Header row: hand number + time + replay button */}
              <div className="flex items-center justify-between">
                <span className="text-[9px]" style={{ color: "#f1c40f" }}>
                  HAND #{entry.handNumber}
                </span>
                <div className="flex items-center gap-2">
                  <span className="text-[8px]" style={{ color: "#7f8c8d" }}>
                    {new Date(entry.timestamp).toLocaleTimeString()}
                  </span>
                  {onReplay && entry.streets.length > 0 && (
                    <button
                      onClick={() => {
                        onClose();
                        onReplay(entry);
                      }}
                      title="Replay this hand"
                      style={{
                        fontFamily: "'Press Start 2P', monospace",
                        fontSize: "7px",
                        background: "rgba(196,125,46,0.15)",
                        border: "1px solid #c47d2e",
                        color: "#ffc078",
                        cursor: "pointer",
                        padding: "2px 6px",
                        lineHeight: 1.4,
                        transition: "background 0.15s",
                      }}
                      onMouseEnter={(e) => {
                        e.currentTarget.style.background = "rgba(196,125,46,0.3)";
                        e.currentTarget.style.color = "#f1c40f";
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.background = "rgba(196,125,46,0.15)";
                        e.currentTarget.style.color = "#ffc078";
                      }}
                    >
                      ▶ REPLAY
                    </button>
                  )}
                  <HandShareButton
                    entry={{
                      tableId: entry.tableId,
                      handNumber: entry.handNumber,
                      finalPot: entry.finalPot,
                      handRankName: entry.handRankName,
                      txHash: entry.txHash,
                    }}
                  />
                </div>
              </div>

              {entry.streets.length > 0 && (
                <div className="flex flex-col gap-1 mt-2">
                  {entry.streets.map((s) => (
                    <div
                      key={s.street}
                      className="flex items-center justify-between text-[8px]"
                      style={{ color: "#c8e6ff" }}
                    >
                      <span>{STREET_LABEL[s.street]}</span>
                      <span>POT: {s.pot.toLocaleString()}</span>
                    </div>
                  ))}
                </div>
              )}

              {entry.boardCards.length > 0 && (
                <div className="flex gap-1 mt-2">
                  {entry.boardCards.map((c, i) => (
                    <Card key={i} value={c} size="sm" />
                  ))}
                </div>
              )}

              {entry.holeCards && (
                <div className="flex items-center gap-2 mt-2">
                  <div className="flex gap-1">
                    <Card value={entry.holeCards[0]} size="sm" />
                    <Card value={entry.holeCards[1]} size="sm" />
                  </div>
                  {entry.handRankName && (
                    <span className="text-[8px]" style={{ color: "#27ae60" }}>
                      {entry.handRankName}
                    </span>
                  )}
                </div>
              )}

              <div className="text-[8px] mt-2" style={{ color: "#95a5a6" }}>
                FINAL POT: {entry.finalPot.toLocaleString()}
                {entry.winnerAddress && <> &middot; WINNER: {shortAddress(entry.winnerAddress)}</>}
              </div>

              {entry.txHash && (
                <a
                  href={stellarExpertUrl("tx", entry.txHash)}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-[8px]"
                  style={{ color: "#ffc078", textDecoration: "none" }}
                >
                  VIEW PROOF TX ↗
                </a>
              )}
              <Link
                href={replayUrl(entry.tableId, entry.handNumber)}
                className="text-[8px] block mt-1"
                style={{ color: "#3498db", textDecoration: "none" }}
              >
                ▶ REPLAY HAND ↗
              </Link>
            </div>
          ))}
        </div>

        {remaining > 0 && (
          <button
            onClick={() => setVisibleCount((n) => n + HISTORY_PAGE_SIZE)}
            className="w-full mt-3"
            style={EXPORT_BUTTON_STYLE}
          >
            SHOW {Math.min(remaining, HISTORY_PAGE_SIZE)} MORE ({remaining} LEFT)
          </button>
        )}
      </div>
    </div>
  );
}
