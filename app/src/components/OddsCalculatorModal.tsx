"use client";

import { useMemo, useState } from "react";
import { calculateOdds, OddsCalculatorError, type CardValue, type OddsResult } from "@/lib/odds-calculator";

interface OddsCalculatorModalProps {
  open: boolean;
  onClose: () => void;
}

// Mirrors the encoding in cards.ts (value = suit * 13 + rankIndex) — cards.ts
// only exports a decoder, so the picker builds its own encode side from the
// same label order rather than modifying that module's exports.
const SUIT_LABELS = ["clubs", "diamonds", "hearts", "spades"] as const;
const SUIT_SYMBOLS: Record<(typeof SUIT_LABELS)[number], string> = {
  clubs: "♣",
  diamonds: "♦",
  hearts: "♥",
  spades: "♠",
};
const RANK_LABELS = [
  "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A",
] as const;

type CardSelection = { rank: number; suit: number } | null;

function encodeCard(selection: CardSelection): CardValue | null {
  if (!selection) return null;
  return selection.suit * 13 + selection.rank;
}

function CardPicker({
  label,
  value,
  onChange,
  optional = false,
}: {
  label: string;
  value: CardSelection;
  onChange: (v: CardSelection) => void;
  optional?: boolean;
}) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[7px]" style={{ color: "#95a5a6" }}>{label}</span>
      <div className="flex gap-1">
        <select
          aria-label={`${label} rank`}
          value={value?.rank ?? ""}
          onChange={(e) => {
            const rank = e.target.value === "" ? null : Number(e.target.value);
            onChange(rank === null ? null : { rank, suit: value?.suit ?? 0 });
          }}
          className="text-[8px] bg-black text-white border border-gray-600 px-1 py-0.5"
        >
          <option value="">{optional ? "—" : "Rank"}</option>
          {RANK_LABELS.map((r, i) => (
            <option key={r} value={i}>{r}</option>
          ))}
        </select>
        <select
          aria-label={`${label} suit`}
          value={value?.suit ?? ""}
          onChange={(e) => {
            const suit = e.target.value === "" ? null : Number(e.target.value);
            onChange(suit === null ? null : { rank: value?.rank ?? 0, suit });
          }}
          className="text-[8px] bg-black text-white border border-gray-600 px-1 py-0.5"
          disabled={value?.rank === undefined}
        >
          <option value="">{optional ? "—" : "Suit"}</option>
          {SUIT_LABELS.map((s, i) => (
            <option key={s} value={i}>{SUIT_SYMBOLS[s]}</option>
          ))}
        </select>
      </div>
    </div>
  );
}

function formatPct(fraction: number): string {
  return `${(fraction * 100).toFixed(1)}%`;
}

export function OddsCalculatorModal({ open, onClose }: OddsCalculatorModalProps) {
  const [hole1, setHole1] = useState<CardSelection>(null);
  const [hole2, setHole2] = useState<CardSelection>(null);
  const [board, setBoard] = useState<CardSelection[]>([null, null, null, null, null]);
  const [numOpponents, setNumOpponents] = useState(1);
  const [result, setResult] = useState<OddsResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const boardValues = useMemo(
    () => board.map(encodeCard).filter((v): v is CardValue => v !== null),
    [board]
  );

  if (!open) return null;

  const handleCalculate = () => {
    setError(null);
    setResult(null);
    const h1 = encodeCard(hole1);
    const h2 = encodeCard(hole2);
    if (h1 === null || h2 === null) {
      setError("Select both hole cards");
      return;
    }
    try {
      const res = calculateOdds({
        holeCards: [h1, h2],
        boardCards: boardValues,
        numOpponents,
      });
      setResult(res);
    } catch (e) {
      setError(e instanceof OddsCalculatorError ? e.message : "Failed to calculate odds");
    }
  };

  return (
    <div
      className="fixed inset-0 z-[120] flex items-center justify-center"
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
          width: "420px",
          maxHeight: "85vh",
          overflowY: "auto",
          padding: "16px",
        }}
      >
        <div className="flex items-center justify-between mb-3">
          <span className="text-[11px]" style={{ color: "#f5e6c8" }}>
            ODDS CALCULATOR
          </span>
          <button
            onClick={onClose}
            style={{ background: "none", border: "none", color: "#e74c3c", cursor: "pointer" }}
          >
            ✕
          </button>
        </div>

        <div className="mb-3">
          <span className="text-[8px]" style={{ color: "#f1c40f" }}>YOUR HOLE CARDS</span>
          <div className="flex gap-2 mt-1">
            <CardPicker label="Card 1" value={hole1} onChange={setHole1} />
            <CardPicker label="Card 2" value={hole2} onChange={setHole2} />
          </div>
        </div>

        <div className="mb-3">
          <span className="text-[8px]" style={{ color: "#f1c40f" }}>BOARD (OPTIONAL)</span>
          <div className="flex gap-2 mt-1 flex-wrap">
            {board.map((c, i) => (
              <CardPicker
                key={i}
                label={i === 0 || i === 1 || i === 2 ? `Flop ${i + 1}` : i === 3 ? "Turn" : "River"}
                value={c}
                optional
                onChange={(v) => {
                  const next = [...board];
                  next[i] = v;
                  setBoard(next);
                }}
              />
            ))}
          </div>
        </div>

        <div className="mb-3">
          <span className="text-[8px]" style={{ color: "#f1c40f" }}>OPPONENTS</span>
          <div className="flex items-center gap-2 mt-1">
            <input
              type="range"
              min={1}
              max={8}
              value={numOpponents}
              onChange={(e) => setNumOpponents(Number(e.target.value))}
              aria-label="Number of opponents"
            />
            <span className="text-[9px]" style={{ color: "#c8e6ff" }}>{numOpponents}</span>
          </div>
          <span className="text-[7px]" style={{ color: "#7f8c8d" }}>
            Each opponent is modeled with a random unknown hand.
          </span>
        </div>

        {error && (
          <div className="text-[8px] mb-3" style={{ color: "#e74c3c" }}>{error}</div>
        )}

        <button
          onClick={handleCalculate}
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
          CALCULATE ODDS
        </button>

        {result && (
          <div className="mt-3 flex flex-col gap-1">
            <div className="flex justify-between text-[9px]" style={{ color: "#27ae60" }}>
              <span>WIN</span><span>{formatPct(result.win)}</span>
            </div>
            <div className="flex justify-between text-[9px]" style={{ color: "#f1c40f" }}>
              <span>TIE</span><span>{formatPct(result.tie)}</span>
            </div>
            <div className="flex justify-between text-[9px]" style={{ color: "#e74c3c" }}>
              <span>LOSS</span><span>{formatPct(result.loss)}</span>
            </div>
            <div className="text-[7px] mt-1" style={{ color: "#7f8c8d" }}>
              Estimated from {result.iterations.toLocaleString()} simulated hands.
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
