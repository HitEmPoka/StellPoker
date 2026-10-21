"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  DEFAULT_TURN_TIMER,
  getTurnTimerPreference,
  setTurnTimerPreference,
  type TurnTimerPreference,
} from "./turn-timer-store";

export interface UseTurnTimerArgs {
  tableId: number;
  address: string;
  /** Address of the player whose turn it currently is. */
  turnAddress: string | null | undefined;
  /** Current betting phase (only countdown during active streets). */
  phase: string;
  /** Wallet-enabled seat index or similar "turn changed" identifier. */
  handNumber?: number;
  /** Current bet the player needs to call, or 0 if they can check. */
  callAmount?: number;
}

export interface UseTurnTimerResult {
  /** Seconds remaining (floored). 0 when not the user's turn. */
  timeLeftSeconds: number;
  /** Configured duration seconds (useful for rendering the ring). */
  durationSeconds: number;
  /** Resolved preference, handy for showing settings inline. */
  preference: TurnTimerPreference;
  /** True if it's this user's turn right now and a betting street is active. */
  isActive: boolean;
  /** True if timer is ≤ 20% of duration (red/pulse). */
  isUrgent: boolean;
  /** Resolved auto-action that the timeout callback will attempt. */
  resolvedAutoAction: "fold" | "check" | "none";
  /**
   * Turns auto-fold on timeout on or off for this table and wallet, and
   * persists the choice. Does not restart the running clock (#630).
   */
  setAutoFold: (on: boolean) => void;
}

const ACTIVE_PHASES = new Set(["preflop", "flop", "turn", "river"]);

/**
 * useTurnTimer — counts down while it's the user's turn during an active
 * betting street, and fires a callback at 0 so the caller can submit the
 * configured auto-action (fold / check-if-possible / none).
 *
 * @param args - Hook args.
 * @param onTimeout - Called exactly once per timeout. Receives the resolved
 *                    action the caller should try to submit.
 * @returns Render hints for the countdown ring + label.
 */
export function useTurnTimer(
  args: UseTurnTimerArgs,
  onTimeout?: (resolvedAction: "fold" | "check") => void
): UseTurnTimerResult {
  const {
    tableId,
    address,
    turnAddress,
    phase,
    handNumber,
    callAmount = 0,
  } = args;

  const [preference, setPreference] = useState<TurnTimerPreference>(() =>
    address ? getTurnTimerPreference(tableId, address) : { ...DEFAULT_TURN_TIMER }
  );

  const isActiveTurn =
    !!turnAddress && turnAddress === address && ACTIVE_PHASES.has(phase);

  // Refetch preference whenever the table or wallet changes, in case another
  // tab mutated the stored settings under us.
  useEffect(() => {
    if (!address) return;
    setPreference(getTurnTimerPreference(tableId, address));
  }, [tableId, address]);

  const [timeLeftSeconds, setTimeLeft] = useState<number>(
    preference.durationSeconds
  );

  // Stable identity for the timeout callback (may change between renders).
  const timeoutHandlerRef = useRef(onTimeout);
  useEffect(() => {
    timeoutHandlerRef.current = onTimeout;
  }, [onTimeout]);

  // The clock reads the latest preference and bet when it runs out, rather
  // than the values it started with. Changing the auto-fold toggle or the bet
  // mid-turn therefore takes effect without restarting the shot clock (#630).
  const preferenceRef = useRef(preference);
  const callAmountRef = useRef(callAmount);
  useEffect(() => {
    preferenceRef.current = preference;
    callAmountRef.current = callAmount;
  }, [preference, callAmount]);

  // Resolution of auto-action at timeout: only fold or actual-check (never)
  // fires the onTimeout.
  const resolvedAutoAction: UseTurnTimerResult["resolvedAutoAction"] =
    !isActiveTurn
      ? "none"
      : preference.autoAction === "fold"
        ? "fold"
        : preference.autoAction === "check_if_possible"
          ? callAmount <= 0
            ? "check"
            : "fold"
          : "none";

  // Key that changes whenever the timer should restart.
  const turnKey = `${handNumber ?? 0}|${phase}|${turnAddress ?? ""}|${preference.durationSeconds}`;
  const lastFiredTurnKeyRef = useRef<string | null>(null);

  useEffect(() => {
    if (!isActiveTurn) {
      setTimeLeft(preference.durationSeconds);
      return;
    }
    // Reset
    setTimeLeft(preference.durationSeconds);
    lastFiredTurnKeyRef.current = null;

    const startedAtMs = Date.now();
    const id = window.setInterval(() => {
      const elapsedMs = Date.now() - startedAtMs;
      const left = preference.durationSeconds - Math.floor(elapsedMs / 1000);
      if (left <= 0) {
        setTimeLeft(0);
        window.clearInterval(id);
        if (lastFiredTurnKeyRef.current !== turnKey) {
          lastFiredTurnKeyRef.current = turnKey;
          const pref = preferenceRef.current;
          // Only attempt submission if preferences say so.
          if (pref.enabled) {
            const action: "fold" | "check" | "none" =
              pref.autoAction === "fold"
                ? "fold"
                : pref.autoAction === "check_if_possible"
                  ? callAmountRef.current <= 0
                    ? "check"
                    : "fold"
                  : "none";
            if (action !== "none") {
              timeoutHandlerRef.current?.(action);
            }
          }
        }
        return;
      }
      setTimeLeft(left);
    }, 250);

    return () => window.clearInterval(id);
    // Intentionally exclude the preference fields and `callAmount`: they are
    // read through refs at timeout. Changing them must not reset the clock.
    // A new duration still restarts it, because the duration is in `turnKey`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [turnKey, isActiveTurn, preference.durationSeconds]);

  const setAutoFold = useCallback(
    (on: boolean) => {
      if (!address) return;
      const current = getTurnTimerPreference(tableId, address);
      const next: TurnTimerPreference = on
        ? { ...current, enabled: true, autoAction: "fold" }
        : { ...current, autoAction: "never" };
      setTurnTimerPreference(tableId, address, next);
      setPreference(next);
    },
    [tableId, address]
  );

  const pct =
    preference.durationSeconds <= 0
      ? 0
      : timeLeftSeconds / preference.durationSeconds;

  return {
    timeLeftSeconds: isActiveTurn ? timeLeftSeconds : 0,
    durationSeconds: preference.durationSeconds,
    preference,
    isActive: isActiveTurn,
    isUrgent: isActiveTurn && pct <= 0.2,
    resolvedAutoAction,
    setAutoFold,
  };
}
