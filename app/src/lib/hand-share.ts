import { stellarExpertUrl } from "./explorer";

/** Minimal shape needed to build a hand share, so callers don't need a full HandHistoryEntry. */
export interface HandShareEntry {
  tableId: number;
  handNumber: number;
  finalPot: number;
  handRankName?: string;
  txHash?: string;
}

const CANVAS_WIDTH = 600;
const CANVAS_HEIGHT = 315;

/** Builds the plain-text hand summary used for both "copy as text" and the Twitter/X intent. */
export function buildHandShareText(entry: HandShareEntry): string {
  const rank = entry.handRankName || "a strong hand";
  const lines = [
    `Just won a ${entry.finalPot.toLocaleString()}-chip pot with ${rank} on StellPoker! 🃏`,
    `Table #${entry.tableId} · Hand #${entry.handNumber}`,
  ];
  if (entry.txHash) {
    lines.push(stellarExpertUrl("tx", entry.txHash));
  }
  return lines.join("\n");
}

/** Builds a Twitter/X web-intent URL pre-filled with the hand summary text. */
export function twitterIntentUrl(text: string): string {
  return `https://twitter.com/intent/tweet?text=${encodeURIComponent(text)}`;
}

/**
 * Renders a hand summary image to an off-DOM canvas. Returns the canvas even
 * if a 2D context isn't available (e.g. a test environment without canvas
 * support) — callers should treat a canvas with no drawn content as a
 * degraded-but-non-throwing result, not an error.
 */
export function renderHandSummaryCanvas(entry: HandShareEntry): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = CANVAS_WIDTH;
  canvas.height = CANVAS_HEIGHT;

  const ctx = canvas.getContext("2d");
  if (!ctx) return canvas;

  ctx.fillStyle = "#0c0a18";
  ctx.fillRect(0, 0, CANVAS_WIDTH, CANVAS_HEIGHT);

  ctx.fillStyle = "#c47d2e";
  ctx.font = "bold 28px sans-serif";
  ctx.fillText("STELLPOKER", 32, 56);

  ctx.fillStyle = "#f5e6c8";
  ctx.font = "22px sans-serif";
  ctx.fillText(entry.handRankName || "Big Hand", 32, 130);

  ctx.fillStyle = "#27ae60";
  ctx.font = "bold 40px sans-serif";
  ctx.fillText(`Pot: ${entry.finalPot.toLocaleString()}`, 32, 195);

  ctx.fillStyle = "#95a5a6";
  ctx.font = "16px sans-serif";
  ctx.fillText(`Table #${entry.tableId} · Hand #${entry.handNumber}`, 32, 270);

  return canvas;
}

/** Converts a canvas to a PNG Blob, resolving null if canvas.toBlob isn't available/succeeds. */
export function canvasToBlob(canvas: HTMLCanvasElement): Promise<Blob | null> {
  return new Promise((resolve) => {
    if (typeof canvas.toBlob !== "function") {
      resolve(null);
      return;
    }
    canvas.toBlob((blob) => resolve(blob), "image/png");
  });
}
