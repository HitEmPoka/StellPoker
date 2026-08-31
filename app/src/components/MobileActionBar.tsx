"use client";

import { useEffect, useRef, useState } from "react";

/**
 * Sticky bottom action bar for phones (#175).
 *
 * On a narrow screen the full action panel pushes the felt off the top of the
 * viewport and the buttons land wherever the layout happens to put them. This
 * collapses the four decisions a player actually makes — fold, check/call,
 * raise, all-in — into a bar pinned to the bottom edge, always in thumb reach
 * and always in the same place.
 *
 * Raise is the only one that needs more than a tap, so it expands upward into
 * a sheet with a horizontally scrolling row of pot-relative presets and a
 * slider, rather than permanently occupying bar space.
 *
 * The bar is rendered on every screen size and hidden above the mobile
 * breakpoint by CSS, so there is no viewport-width state to get wrong during
 * hydration and no layout flash on first paint.
 */

interface MobileActionBarProps {
  /** Hides the bar entirely — e.g. between hands, or when watching. */
  visible: boolean;
  isMyTurn: boolean;
  /** Highest bet on this street. */
  currentBet: number;
  /** What the player has already put in this street. */
  myBet: number;
  myStack: number;
  pot: number;
  /** Smallest legal total bet for a raise; defaults to twice the current bet. */
  minRaiseTo?: number;
  loading?: boolean;
  onAction: (action: string, amount?: number) => void;
  betAmount: number;
  setBetAmount: (amount: number) => void;
}

interface Preset {
  label: string;
  /** Total street bet this preset targets. */
  value: number;
}

/**
 * Pot-relative sizings, the way a player thinks about a raise. More presets
 * than fit on a phone is fine — the row scrolls horizontally, which is what
 * the issue asks for and what every mobile poker client does.
 */
function buildPresets(pot: number, minRaiseTo: number, maxRaiseTo: number): Preset[] {
  const sized = [
    { label: "MIN", value: minRaiseTo },
    { label: "½ POT", value: Math.floor(pot * 0.5) },
    { label: "⅔ POT", value: Math.floor(pot * 0.66) },
    { label: "POT", value: pot },
    { label: "2× POT", value: pot * 2 },
    { label: "MAX", value: maxRaiseTo },
  ];

  const seen = new Set<number>();
  return sized
    .map((preset) => ({
      ...preset,
      value: Math.max(minRaiseTo, Math.min(preset.value, maxRaiseTo)),
    }))
    // Once a preset clamps to the stack it duplicates MAX; showing the same
    // number three times is just noise on a small screen.
    .filter((preset) => {
      if (seen.has(preset.value)) return false;
      seen.add(preset.value);
      return true;
    });
}

export function MobileActionBar({
  visible,
  isMyTurn,
  currentBet,
  myBet,
  myStack,
  pot,
  minRaiseTo,
  loading = false,
  onAction,
  betAmount,
  setBetAmount,
}: MobileActionBarProps) {
  const [raiseOpen, setRaiseOpen] = useState(false);
  const sheetRef = useRef<HTMLDivElement>(null);

  const callAmount = Math.max(currentBet - myBet, 0);
  const maxRaiseTo = myBet + myStack;
  const resolvedMinRaiseTo = Math.min(
    minRaiseTo ?? Math.max(currentBet * 2, 1),
    maxRaiseTo
  );
  const canRaise = myStack > callAmount;
  const disabled = !isMyTurn || loading;
  const raiseTarget = Math.max(
    resolvedMinRaiseTo,
    Math.min(betAmount || resolvedMinRaiseTo, maxRaiseTo)
  );

  // A sheet left open while the turn passes would sit over the felt showing
  // stale numbers, so it closes with the turn.
  useEffect(() => {
    if (disabled) setRaiseOpen(false);
  }, [disabled]);

  // Escape closes the sheet, matching every other dismissible layer here.
  useEffect(() => {
    if (!raiseOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setRaiseOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [raiseOpen]);

  if (!visible) return null;

  const presets = buildPresets(pot, resolvedMinRaiseTo, maxRaiseTo);
  const raiseVerb = currentBet === 0 ? "BET" : "RAISE";

  const submitRaise = () => {
    onAction(currentBet === 0 ? "bet" : "raise", raiseTarget);
    setRaiseOpen(false);
  };

  return (
    <div className="mobile-action-bar" data-testid="mobile-action-bar">
      {/* Raise sheet — only mounted while open, so it never traps taps. */}
      {raiseOpen && canRaise && (
        <div
          ref={sheetRef}
          className="mobile-raise-sheet"
          role="group"
          aria-label="Raise size"
        >
          <div className="mobile-raise-sheet-head">
            <span>
              {raiseVerb} TO {raiseTarget.toLocaleString()}
            </span>
            <button
              onClick={() => setRaiseOpen(false)}
              aria-label="Close raise options"
              className="mobile-raise-close"
            >
              ✕
            </button>
          </div>

          {/* Horizontally scrolling presets (#175). */}
          <div className="mobile-raise-presets" role="group" aria-label="Raise presets">
            {presets.map((preset) => (
              <button
                key={preset.label}
                onClick={() => setBetAmount(preset.value)}
                aria-pressed={raiseTarget === preset.value}
                className="pixel-btn mobile-raise-preset"
                style={{
                  background: raiseTarget === preset.value ? "#7d6608" : "#2c3e50",
                  color: "#f5e6c8",
                }}
              >
                {preset.label}
                <span className="mobile-raise-preset-value">
                  {preset.value.toLocaleString()}
                </span>
              </button>
            ))}
          </div>

          <input
            type="range"
            min={resolvedMinRaiseTo}
            max={maxRaiseTo}
            value={raiseTarget}
            onChange={(e) => setBetAmount(Number(e.target.value))}
            aria-label="Raise amount"
            className="mobile-raise-slider"
            style={{ accentColor: "#f1c40f" }}
          />

          <button
            onClick={submitRaise}
            className="pixel-btn pixel-btn-gold mobile-raise-confirm"
          >
            CONFIRM {raiseVerb} {raiseTarget.toLocaleString()}
          </button>
        </div>
      )}

      <div className="mobile-action-row" role="group" aria-label="Table actions">
        <button
          onClick={() => onAction("fold")}
          disabled={disabled}
          className="pixel-btn mobile-action-btn"
          style={{ background: disabled ? "#4a4a4a" : "#7b241c", color: "white" }}
        >
          FOLD
        </button>

        <button
          onClick={() => onAction(callAmount === 0 ? "check" : "call", callAmount)}
          disabled={disabled}
          className="pixel-btn mobile-action-btn"
          style={{ background: disabled ? "#4a4a4a" : "#1a5276", color: "white" }}
        >
          {callAmount === 0 ? "CHECK" : `CALL ${callAmount.toLocaleString()}`}
        </button>

        <button
          onClick={() => setRaiseOpen((open) => !open)}
          disabled={disabled || !canRaise}
          aria-expanded={raiseOpen}
          aria-label={`${raiseVerb} options`}
          className="pixel-btn mobile-action-btn"
          style={{
            background: disabled || !canRaise ? "#4a4a4a" : "#7d6608",
            color: "white",
          }}
        >
          {raiseVerb} {raiseOpen ? "▾" : "▴"}
        </button>

        <button
          onClick={() => onAction("allin", myStack)}
          disabled={disabled || myStack <= 0}
          className="pixel-btn mobile-action-btn"
          style={{
            background: disabled || myStack <= 0 ? "#4a4a4a" : "#d4ac0d",
            color: "#1a1a1a",
            fontWeight: "bold",
          }}
        >
          ALL IN
        </button>
      </div>

      {disabled && !loading && (
        <p className="mobile-action-hint">WAITING FOR YOUR TURN…</p>
      )}
      {loading && <p className="mobile-action-hint">SUBMITTING…</p>}
    </div>
  );
}
