"use client";

import { formatSpectatorCount } from "@/lib/spectator";

interface SpectatorCountProps {
  count: number;
  /** Hide the badge entirely when nobody is watching. */
  hideWhenZero?: boolean;
  size?: "sm" | "md";
}

/** "N WATCHING" indicator for live anonymous spectators (Issue #171). */
export function SpectatorCount({ count, hideWhenZero = false, size = "md" }: SpectatorCountProps) {
  if (hideWhenZero && count <= 0) return null;
  return (
    <span
      data-testid="spectator-count"
      className={`pixel-border-thin inline-flex items-center gap-1 ${size === "sm" ? "text-[7px] px-1" : "text-[9px] px-2 py-[2px]"}`}
      title={`${count} anonymous spectator${count === 1 ? "" : "s"}`}
      aria-label={`${count} spectator${count === 1 ? "" : "s"} watching`}
      style={{
        background: "rgba(116, 185, 255, 0.12)",
        borderColor: "#74b9ff",
        color: "#c8e6ff",
      }}
    >
      <span aria-hidden="true">👁</span>
      {formatSpectatorCount(count)}
    </span>
  );
}
