"use client";

import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  ANNOUNCE_THRESHOLDS,
  nextTurnThreshold,
  turnStartAnnouncement,
  turnThresholdAnnouncement,
} from "@/lib/turn-countdown";

export interface TurnCountdownBannerProps {
  /** True while it is the viewer's turn during an active betting street. */
  active: boolean;
  timeLeftSeconds: number;
  durationSeconds: number;
  /** Whether running out of time folds the viewer's hand. */
  autoFoldOn: boolean;
  onAutoFoldChange: (on: boolean) => void;
}

/** Hides a node visually but keeps it in the accessibility tree. */
const SCREEN_READER_ONLY: CSSProperties = {
  position: "absolute",
  width: 1,
  height: 1,
  padding: 0,
  margin: -1,
  overflow: "hidden",
  clip: "rect(0, 0, 0, 0)",
  whiteSpace: "nowrap",
  border: 0,
};

/**
 * Countdown banner shown while it is the viewer's turn (#630). The visible
 * timer is hidden from assistive tech because it ticks every second. The
 * polite live region speaks only at the thresholds in `ANNOUNCE_THRESHOLDS`.
 */
export function TurnCountdownBanner({
  active,
  timeLeftSeconds,
  durationSeconds,
  autoFoldOn,
  onAutoFoldChange,
}: TurnCountdownBannerProps) {
  const [announcement, setAnnouncement] = useState("");
  const spokenRef = useRef<Set<number>>(new Set());
  const wasActiveRef = useRef(false);

  useEffect(() => {
    if (!active) {
      wasActiveRef.current = false;
      spokenRef.current = new Set();
      setAnnouncement("");
      return;
    }
    if (!wasActiveRef.current) {
      wasActiveRef.current = true;
      spokenRef.current = new Set();
      setAnnouncement(turnStartAnnouncement(durationSeconds));
    }
    const threshold = nextTurnThreshold(timeLeftSeconds, spokenRef.current);
    if (threshold !== null) {
      // Speaking a threshold also covers every higher one the clock has passed.
      for (const reached of ANNOUNCE_THRESHOLDS) {
        if (timeLeftSeconds <= reached) spokenRef.current.add(reached);
      }
      setAnnouncement(turnThresholdAnnouncement(threshold, autoFoldOn));
    }
  }, [active, timeLeftSeconds, durationSeconds, autoFoldOn]);

  const pct =
    durationSeconds > 0
      ? Math.max(0, Math.min(1, timeLeftSeconds / durationSeconds))
      : 0;
  const urgent = pct <= 0.2;
  const accent = urgent ? "#e74c3c" : "#f1c40f";

  return (
    <>
      <div
        role="status"
        aria-live="polite"
        aria-atomic="true"
        style={SCREEN_READER_ONLY}
        data-testid="turn-countdown-live"
      >
        {announcement}
      </div>

      {active && (
        <div
          className="pixel-border-thin w-full max-w-3xl px-3 py-2 flex flex-wrap items-center gap-3"
          style={{
            background: urgent ? "rgba(231, 76, 60, 0.2)" : "rgba(241, 196, 15, 0.12)",
            borderColor: accent,
          }}
          data-testid="turn-countdown-banner"
        >
          <span className="text-[9px]" style={{ color: "#f5e6c8" }}>
            YOUR TURN
          </span>
          <span aria-hidden="true" className="text-[9px]" style={{ color: accent }}>
            {timeLeftSeconds}s
          </span>
          <div
            aria-hidden="true"
            className="flex-1 min-w-[80px] h-2"
            style={{ background: "rgba(0, 0, 0, 0.4)" }}
          >
            <div
              style={{
                width: `${pct * 100}%`,
                height: "100%",
                background: accent,
                transition: "width 250ms linear",
              }}
            />
          </div>
          <label
            className="flex items-center gap-1 text-[8px]"
            style={{ color: "#c8e6ff" }}
          >
            <input
              type="checkbox"
              checked={autoFoldOn}
              onChange={(e) => onAutoFoldChange(e.target.checked)}
            />
            Auto-fold when time runs out
          </label>
        </div>
      )}
    </>
  );
}
