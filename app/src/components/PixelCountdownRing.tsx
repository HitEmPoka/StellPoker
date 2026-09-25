"use client";

import { useMemo } from "react";

interface PixelCountdownRingProps {
  size?: number;
  durationSeconds: number;
  timeLeftSeconds: number;
  /** If true, pulse red near expiration */
  urgent?: boolean;
  /** Text to render inside the ring (defaults to time remaining) */
  label?: string;
  /** Text color for inside the ring */
  color?: string;
  /** Background "track" color (the not-yet-elapsed portion) */
  trackColor?: string;
  /** Progress color (elapsed portion) */
  progressColor?: string;
  className?: string;
  style?: React.CSSProperties;
}

function colorForTimeLeft(duration: number, left: number): string {
  const pct = duration <= 0 ? 0 : Math.max(0, Math.min(1, left / duration));
  if (pct > 0.5) return "#27ae60";
  if (pct > 0.2) return "#f39c12";
  return "#e74c3c";
}

/**
 * PixelCountdownRing — chunky-pixel arc countdown, matching StellPoker's
 * 8-bit / Game-Boy aesthetic. Uses a 16-segment stepped arc so the progress
 * "ticks" in discrete chunky blocks rather than animating smoothly.
 */
export function PixelCountdownRing({
  size = 64,
  durationSeconds,
  timeLeftSeconds,
  urgent = false,
  label,
  color,
  trackColor = "rgba(90, 100, 110, 0.35)",
  progressColor,
  className,
  style,
}: PixelCountdownRingProps) {
  const SEGMENTS = 16;
  const clamped = Math.max(0, Math.min(durationSeconds, timeLeftSeconds));
  const progressPct =
    durationSeconds <= 0 ? 0 : 1 - clamped / durationSeconds;
  const filledSegments = Math.min(
    SEGMENTS,
    Math.floor(progressPct * SEGMENTS + Number.EPSILON)
  );

  const innerText =
    label !== undefined
      ? label
      : `${Math.ceil(clamped)}s`;

  const finalProgress =
    progressColor ?? colorForTimeLeft(durationSeconds, clamped);

  const segments = useMemo(() => {
    const out: Array<{
      startDeg: number;
      endDeg: number;
      filled: boolean;
    }> = [];
    const segDeg = 360 / SEGMENTS;
    for (let i = 0; i < SEGMENTS; i++) {
      const startDeg = -90 + i * segDeg;
      out.push({
        startDeg,
        endDeg: startDeg + segDeg - 2,
        filled: i < filledSegments,
      });
    }
    return out;
  }, [filledSegments]);

  const stroke = Math.max(4, Math.floor(size / 12));
  const outer = size;
  const inner = outer - stroke * 2;

  return (
    <div
      className={className}
      style={{
        position: "relative",
        width: outer,
        height: outer,
        imageRendering: "pixelated",
        ...style,
      }}
      aria-label={`${innerText} remaining`}
      role="timer"
    >
      <svg
        width={outer}
        height={outer}
        viewBox={`0 0 ${outer} ${outer}`}
        style={{
          imageRendering: "pixelated",
          shapeRendering: "crispEdges",
          position: "absolute",
          inset: 0,
        }}
      >
        {/* Chunky pixel track — one rect per segment, all in track color */}
        {segments.map((s, idx) => (
          <SegmentArc
            key={`t-${idx}`}
            cx={outer / 2}
            cy={outer / 2}
            outerRadius={outer / 2 - 1}
            innerRadius={inner / 2 + 1}
            startDeg={s.startDeg}
            endDeg={s.endDeg}
            fill={trackColor}
          />
        ))}
        {/* Filled progress segments */}
        {segments.map((s, idx) =>
          s.filled ? (
            <SegmentArc
              key={`p-${idx}`}
              cx={outer / 2}
              cy={outer / 2}
              outerRadius={outer / 2 - 1}
              innerRadius={inner / 2 + 1}
              startDeg={s.startDeg}
              endDeg={s.endDeg}
              fill={finalProgress}
            />
          ) : null
        )}
      </svg>
      {/* Inner label */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: color ?? finalProgress,
          fontFamily: "'Press Start 2P', monospace",
          fontSize: Math.max(7, Math.floor(size / 7)),
          fontWeight: "bold",
          textShadow: "1px 1px 0 rgba(0,0,0,0.6)",
          letterSpacing: "0.5px",
          animation:
            urgent && clamped <= durationSeconds * 0.2
              ? "countdownUrgent 0.4s ease-in-out infinite"
              : undefined,
        }}
      >
        {innerText}
      </div>
      <style jsx>{`
        @keyframes countdownUrgent {
          0%, 100% { transform: scale(1); filter: brightness(1); }
          50% { transform: scale(1.08); filter: brightness(1.5) drop-shadow(0 0 4px #e74c3c); }
        }
      `}</style>
    </div>
  );
}

interface SegmentArcProps {
  cx: number;
  cy: number;
  outerRadius: number;
  innerRadius: number;
  startDeg: number;
  endDeg: number;
  fill: string;
}

/**
 * Builds a single chunky-pixel annular sector (an arc "wedge"). Using SVG
 * paths with `shapeRendering: crispEdges` gives us the blocky / pixel-art
 * look the rest of the table uses — no soft anti-aliased curves.
 */
function SegmentArc({
  cx,
  cy,
  outerRadius,
  innerRadius,
  startDeg,
  endDeg,
  fill,
}: SegmentArcProps) {
  const clamp = (d: number) => {
    let v = d % 360;
    if (v < 0) v += 360;
    return v;
  };
  const s = clamp(startDeg) * (Math.PI / 180);
  const e = clamp(endDeg) * (Math.PI / 180);
  const xos = cx + outerRadius * Math.cos(s);
  const yos = cy + outerRadius * Math.sin(s);
  const xoe = cx + outerRadius * Math.cos(e);
  const yoe = cy + outerRadius * Math.sin(e);
  const xis = cx + innerRadius * Math.cos(s);
  const yis = cy + innerRadius * Math.sin(s);
  const xie = cx + innerRadius * Math.cos(e);
  const yie = cy + innerRadius * Math.sin(e);
  const largeArc = Math.abs(endDeg - startDeg) > 180 ? 1 : 0;
  const d = [
    `M ${xos} ${yos}`,
    `A ${outerRadius} ${outerRadius} 0 ${largeArc} 1 ${xoe} ${yoe}`,
    `L ${xie} ${yie}`,
    `A ${innerRadius} ${innerRadius} 0 ${largeArc} 0 ${xis} ${yis}`,
    "Z",
  ].join(" ");
  return <path d={d} fill={fill} />;
}
