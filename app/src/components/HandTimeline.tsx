"use client";

import { useEffect, useRef } from "react";
import {
  formatClock,
  formatElapsed,
  type TimelineEvent,
} from "@/lib/hand-timeline";

/**
 * Scrubber for the hand in progress (#176).
 *
 * Sits under the felt as a horizontal strip of markers — one per action or
 * street — each stamped with how long into the hand it happened. Selecting one
 * shows the table as it stood at that moment; a LIVE control returns to the
 * present.
 *
 * The strip follows the APG slider pattern rather than being a row of loose
 * buttons: one focusable control carrying `aria-valuenow`, driven with the
 * arrow keys plus Home/End, so a keyboard user scrubs the same way they would
 * any other range. The markers stay clickable for pointer users.
 */

interface HandTimelineProps {
  events: TimelineEvent[];
  /** Currently viewed index; equals the last index while live. */
  index: number;
  onSeek: (index: number) => void;
  /** True when the view is pinned to the newest event. */
  isLive: boolean;
  onReturnToLive: () => void;
}

const KIND_COLORS: Record<TimelineEvent["kind"], string> = {
  deal: "#3498db",
  street: "#27ae60",
  action: "#95a5a6",
  settlement: "#f1c40f",
};

export function HandTimeline({
  events,
  index,
  onSeek,
  isLive,
  onReturnToLive,
}: HandTimelineProps) {
  const trackRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLButtonElement>(null);

  // Keep the selected marker in view as the hand grows past the strip width.
  // Feature-detected because `scrollIntoView` is absent in jsdom and in older
  // browsers, and a missing convenience must not take the strip down with it.
  useEffect(() => {
    const marker = activeRef.current;
    if (typeof marker?.scrollIntoView !== "function") return;
    marker.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
  }, [index]);

  if (events.length === 0) return null;

  const last = events.length - 1;
  const current = events[Math.max(0, Math.min(index, last))];

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    let next: number | null = null;
    switch (event.key) {
      case "ArrowLeft":
      case "ArrowDown":
        next = index - 1;
        break;
      case "ArrowRight":
      case "ArrowUp":
        next = index + 1;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = last;
        break;
      case "PageUp":
        next = index + 5;
        break;
      case "PageDown":
        next = index - 5;
        break;
      default:
        return;
    }
    event.preventDefault();
    onSeek(Math.max(0, Math.min(next, last)));
  };

  return (
    <section
      className="hand-timeline w-full max-w-3xl flex flex-col gap-1"
      aria-label="Hand timeline"
      data-testid="hand-timeline"
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-[8px]" style={{ color: "#95a5a6" }}>
          THIS HAND · {events.length} {events.length === 1 ? "MOMENT" : "MOMENTS"}
        </span>

        {/* Announced when the player scrubs away from the live state, so it is
            never ambiguous whether the felt is showing now or then. */}
        <span
          role="status"
          aria-live="polite"
          className="text-[8px]"
          style={{ color: isLive ? "#27ae60" : "#f1c40f" }}
        >
          {isLive
            ? "LIVE"
            : `REVIEWING ${current.label} AT ${formatElapsed(events, index)}`}
        </span>

        <button
          onClick={onReturnToLive}
          disabled={isLive}
          className="pixel-btn text-[8px]"
          style={{
            padding: "4px 10px",
            background: isLive ? "#2c3e50" : "#145a32",
            color: "#eafaf1",
            opacity: isLive ? 0.5 : 1,
          }}
        >
          ⏭ LIVE
        </button>
      </div>

      <div
        ref={trackRef}
        role="slider"
        tabIndex={0}
        aria-label="Scrub through this hand"
        aria-valuemin={0}
        aria-valuemax={last}
        aria-valuenow={index}
        aria-valuetext={`${current.label} at ${formatElapsed(events, index)}, pot ${current.pot}`}
        onKeyDown={handleKeyDown}
        className="hand-timeline-track"
      >
        {events.map((event, i) => {
          const selected = i === index;
          return (
            <button
              key={event.id}
              ref={selected ? activeRef : undefined}
              onClick={() => onSeek(i)}
              // The strip itself is the slider; the markers are pointer
              // shortcuts into it and shouldn't add tab stops of their own.
              tabIndex={-1}
              aria-hidden="true"
              title={`${event.label} · ${formatClock(event.timestamp)} · pot ${event.pot}`}
              className="hand-timeline-marker"
              style={{
                borderColor: KIND_COLORS[event.kind],
                background: selected
                  ? KIND_COLORS[event.kind]
                  : "rgba(0, 0, 0, 0.35)",
                color: selected ? "#12100a" : "#c8e6ff",
              }}
            >
              <span className="hand-timeline-marker-label">{event.label}</span>
              <span className="hand-timeline-marker-time">
                {formatElapsed(events, i)}
              </span>
            </button>
          );
        })}
      </div>
    </section>
  );
}
