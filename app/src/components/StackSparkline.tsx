"use client";

import { useEffect, useRef } from "react";
import { sparklinePoints } from "@/lib/stack-trend";

interface StackSparklineProps {
  /** Stack sizes, oldest first. Nothing is drawn for fewer than two. */
  values: number[];
  width?: number;
  height?: number;
}

const TREND_UP = "#27ae60";
const TREND_DOWN = "#e74c3c";
const TREND_FLAT = "#95a5a6";

/**
 * Tiny line chart of a player's recent chip stacks (#157), drawn straight onto
 * a canvas — a handful of points doesn't justify a charting dependency.
 */
export function StackSparkline({ values, width = 40, height = 12 }: StackSparklineProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    // Back the canvas at device resolution so the line stays crisp on HiDPI
    // screens while the element keeps its CSS size.
    const scale = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * scale);
    canvas.height = Math.round(height * scale);
    ctx.setTransform(scale, 0, 0, scale, 0, 0);
    ctx.clearRect(0, 0, width, height);

    const points = sparklinePoints(values, width, height, 2);
    if (points.length < 2) return;

    const first = values[0];
    const last = values[values.length - 1];
    const color = last > first ? TREND_UP : last < first ? TREND_DOWN : TREND_FLAT;

    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    ctx.lineJoin = "round";
    ctx.beginPath();
    ctx.moveTo(points[0].x, points[0].y);
    for (const { x, y } of points.slice(1)) {
      ctx.lineTo(x, y);
    }
    ctx.stroke();

    // Mark the latest stack so the eye lands on "now".
    const end = points[points.length - 1];
    ctx.fillStyle = color;
    ctx.fillRect(end.x - 1, end.y - 1, 2, 2);
  }, [values, width, height]);

  if (values.length < 2) return null;

  const label = `Stack over last ${values.length} hands: ${values[0].toLocaleString()} → ${values[values.length - 1].toLocaleString()}`;

  return (
    <canvas
      ref={canvasRef}
      role="img"
      aria-label={label}
      title={label}
      width={width}
      height={height}
      style={{ width, height, display: "inline-block", verticalAlign: "middle" }}
    />
  );
}
