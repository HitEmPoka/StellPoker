/**
 * On-chain SVG avatar templates (Issue #153).
 *
 * These are the client-side renderers for the compact `template_id` values
 * stored in the `player-avatar` contract. Template IDs 0..=255 are reserved
 * for the core library shipped with the frontend; higher IDs are reserved
 * for future community / premium packs.
 *
 * Each template is a pure function that takes `params` (a dict of k=v
 * strings, e.g. `{ bg: "#1a1b2e", fg: "#f1c40f", hat: "3" }`) and returns
 * an SVG string ready to drop into an <svg> element (or a data URL for img tags).
 *
 * The templates are intentionally lightweight — a small set of well-tuned
 * parameters rather than a full generative-art engine — because every parameter
 * value has to fit in a 512-byte on-chain `params` string.
 */

const DEFAULT_PALETTE = {
  skin: ["#f5d7b8", "#e0b080", "#c68a5d", "#8d5524", "#3b2516"],
  eyes: ["#2c3e50", "#27ae60", "#3498db", "#9b59b6", "#e74c3c", "#f1c40f"],
  hats: ["none", "cap", "crown", "beanie", "tophat", "sunglasses"],
  bodies: ["hoodie", "suit", "tshirt", "armor", "robe"],
  mouths: ["smile", "grin", "neutral", "o"],
} as const;

function color(val: string | undefined, fallback: string): string {
  if (!val) return fallback;
  if (/^[#a-zA-Z0-9]+$/.test(val) && val.length <= 16) return val;
  return fallback;
}

export interface AvatarTemplateInfo {
  id: number;
  name: string;
  description: string;
  params: Array<{
    key: string;
    label: string;
    type: "color" | "select";
    options?: readonly (string | number)[];
    default: string;
  }>;
}

export const AVATAR_TEMPLATES: AvatarTemplateInfo[] = [
  {
    id: 0,
    name: "Pixel Poker Cat",
    description: "A customizable pixel-art poker cat with hat, eyes, and body.",
    params: [
      { key: "bg", label: "Background", type: "color", default: "#1a1b2e" },
      { key: "fg", label: "Fur Color", type: "color", default: "#e67e22" },
      { key: "eye", label: "Eye Color", type: "select", options: [0, 1, 2, 3, 4, 5], default: "0" },
      { key: "hat", label: "Hat", type: "select", options: [0, 1, 2, 3, 4, 5], default: "0" },
      { key: "body", label: "Outfit", type: "select", options: [0, 1, 2, 3, 4], default: "0" },
      { key: "mouth", label: "Mouth", type: "select", options: [0, 1, 2, 3], default: "0" },
    ],
  },
  {
    id: 1,
    name: "Gem Chip Shape",
    description: "Abstract geometric playing-card chip medallion.",
    params: [
      { key: "bg", label: "Background", type: "color", default: "#101a26" },
      { key: "fg", label: "Primary", type: "color", default: "#27ae60" },
      { key: "accent", label: "Accent", type: "color", default: "#f1c40f" },
      { key: "suit", label: "Suit", type: "select", options: [0, 1, 2, 3], default: "0" },
      { key: "ring", label: "Ring Style", type: "select", options: [0, 1, 2, 3], default: "0" },
    ],
  },
  {
    id: 2,
    name: "Crest Banner",
    description: "Heraldic poker crest with banner and motto field.",
    params: [
      { key: "bg", label: "Field", type: "color", default: "#1a1626" },
      { key: "primary", label: "Primary", type: "color", default: "#e74c3c" },
      { key: "secondary", label: "Secondary", type: "color", default: "#f1c40f" },
      { key: "charge", label: "Charge", type: "select", options: [0, 1, 2, 3, 4, 5], default: "0" },
      { key: "division", label: "Division", type: "select", options: [0, 1, 2, 3], default: "0" },
    ],
  },
  {
    id: 3,
    name: "Droid Bot",
    description: "Friendly poker droid with antenna and screen face.",
    params: [
      { key: "bg", label: "Background", type: "color", default: "#0f1f1c" },
      { key: "body", label: "Body", type: "color", default: "#3498db" },
      { key: "screen", label: "Screen", type: "color", default: "#27ae60" },
      { key: "antenna", label: "Antenna", type: "color", default: "#e74c3c" },
      { key: "face", label: "Face", type: "select", options: [0, 1, 2, 3], default: "0" },
    ],
  },
  {
    id: 4,
    name: "Suit Sigil",
    description: "Minimal suit-symbol sigil on a colored disc.",
    params: [
      { key: "bg", label: "Background", type: "color", default: "#16202c" },
      { key: "disc", label: "Disc", type: "color", default: "#9b59b6" },
      { key: "suit", label: "Suit", type: "select", options: [0, 1, 2, 3], default: "0" },
      { key: "ring", label: "Ring", type: "color", default: "#f1c40f" },
    ],
  },
];

// ---------- shared helpers ----------

function hexPoints(cx: number, cy: number, r: number): string {
  const pts: string[] = [];
  for (let i = 0; i < 6; i++) {
    const a = (i / 6) * Math.PI * 2 - Math.PI / 2;
    pts.push(`${cx + Math.cos(a) * r},${cy + Math.sin(a) * r}`);
  }
  return pts.join(" ");
}

// ---------- individual template renderers ----------

function renderT0PixelCat(p: Record<string, string>, size: number): string {
  const bg = color(p.bg, "#1a1b2e");
  const fg = color(p.fg, "#e67e22");
  const eyeIdx = parseInt(p.eye ?? "0", 10) % DEFAULT_PALETTE.eyes.length;
  const eye = DEFAULT_PALETTE.eyes[eyeIdx] ?? "#2c3e50";
  const hatIdx = parseInt(p.hat ?? "0", 10) % DEFAULT_PALETTE.hats.length;
  const hat = DEFAULT_PALETTE.hats[hatIdx];
  const bodyIdx = parseInt(p.body ?? "0", 10) % DEFAULT_PALETTE.bodies.length;
  const bodyOpt = DEFAULT_PALETTE.bodies[bodyIdx];
  const mouthIdx = parseInt(p.mouth ?? "0", 10) % DEFAULT_PALETTE.mouths.length;
  const mouth = DEFAULT_PALETTE.mouths[mouthIdx];

  const u = Math.max(1, Math.floor(size / 16));
  const pixel = (x: number, y: number, w: number, h: number, fill: string) =>
    `<rect x="${x * u}" y="${y * u}" width="${w * u}" height="${h * u}" fill="${fill}" />`;

  let out = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${16 * u} ${16 * u}" shape-rendering="crispEdges">`;
  out += pixel(0, 0, 16, 16, bg);
  out += pixel(2, 3, 2, 3, fg);
  out += pixel(12, 3, 2, 3, fg);
  out += pixel(3, 5, 10, 7, fg);
  out += pixel(5, 7, 2, 2, eye);
  out += pixel(9, 7, 2, 2, eye);
  out += pixel(7, 9, 2, 1, "#2c1f1a");

  if (mouth === "smile") {
    out += pixel(6, 10, 1, 1, "#2c1f1a");
    out += pixel(7, 11, 2, 1, "#2c1f1a");
    out += pixel(9, 10, 1, 1, "#2c1f1a");
  } else if (mouth === "grin") {
    out += pixel(5, 10, 6, 2, "#ffffff");
    out += pixel(6, 10, 4, 1, "#2c1f1a");
  } else if (mouth === "neutral") {
    out += pixel(6, 10, 4, 1, "#2c1f1a");
  } else {
    out += pixel(7, 10, 2, 2, "#2c1f1a");
  }

  const bodyColor =
    bodyOpt === "hoodie"
      ? "#2c3e50"
      : bodyOpt === "suit"
        ? "#1a1a2e"
        : bodyOpt === "tshirt"
          ? "#e74c3c"
          : bodyOpt === "armor"
            ? "#7f8c8d"
            : "#4a235a";
  out += pixel(2, 12, 12, 4, bodyColor);

  if (hat === "cap") {
    out += pixel(3, 2, 10, 2, "#e74c3c");
    out += pixel(2, 4, 12, 1, "#e74c3c");
  } else if (hat === "crown") {
    out += pixel(3, 2, 2, 3, "#f1c40f");
    out += pixel(6, 1, 4, 4, "#f1c40f");
    out += pixel(11, 2, 2, 3, "#f1c40f");
    out += pixel(4, 3, 1, 1, "#e67e22");
    out += pixel(7, 2, 2, 2, "#e67e22");
    out += pixel(12, 3, 1, 1, "#e67e22");
  } else if (hat === "beanie") {
    out += pixel(3, 2, 10, 3, "#27ae60");
    out += pixel(7, 1, 2, 2, "#c0392b");
  } else if (hat === "tophat") {
    out += pixel(4, 1, 8, 1, "#1a1a1a");
    out += pixel(5, 2, 6, 3, "#1a1a1a");
    out += pixel(3, 5, 10, 1, "#1a1a1a");
  } else if (hat === "sunglasses") {
    out += pixel(4, 7, 3, 2, "#1a1a1a");
    out += pixel(9, 7, 3, 2, "#1a1a1a");
    out += pixel(7, 7, 2, 1, "#1a1a1a");
  }
  out += "</svg>";
  return out;
}

function renderT1ChipShape(p: Record<string, string>, size: number): string {
  const bg = color(p.bg, "#101a26");
  const fg = color(p.fg, "#27ae60");
  const accent = color(p.accent, "#f1c40f");
  const suit = parseInt(p.suit ?? "0", 10) % 4;
  const ring = parseInt(p.ring ?? "0", 10) % 4;
  const cx = size / 2;
  const cy = size / 2;
  const r = size * 0.42;
  const suitGlyphs = ["♠", "♥", "♦", "♣"];
  const suitColors = ["#2c3e50", "#e74c3c", "#e74c3c", "#2c3e50"];

  let out = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}">`;
  out += `<rect width="${size}" height="${size}" fill="${bg}" />`;
  if (ring === 0) {
    out += `<circle cx="${cx}" cy="${cy}" r="${r}" fill="${fg}" stroke="${accent}" stroke-width="${size * 0.05}" />`;
  } else if (ring === 1) {
    out += `<circle cx="${cx}" cy="${cy}" r="${r}" fill="${fg}" />`;
    for (let i = 0; i < 8; i++) {
      const a = (i / 8) * Math.PI * 2;
      const x = cx + Math.cos(a) * (r - size * 0.05);
      const y = cy + Math.sin(a) * (r - size * 0.05);
      out += `<rect x="${x - size * 0.02}" y="${y - size * 0.02}" width="${size * 0.04}" height="${size * 0.04}" fill="${accent}" />`;
    }
  } else if (ring === 2) {
    out += `<circle cx="${cx}" cy="${cy}" r="${r}" fill="${fg}" />`;
    out += `<circle cx="${cx}" cy="${cy}" r="${r - size * 0.06}" fill="none" stroke="${accent}" stroke-width="${size * 0.03}" stroke-dasharray="4 2" />`;
  } else {
    out += `<polygon points="${hexPoints(cx, cy, r)}" fill="${fg}" stroke="${accent}" stroke-width="${size * 0.04}" />`;
  }
  out += `<text x="${cx}" y="${cy + size * 0.08}" text-anchor="middle" font-family="serif" font-size="${size * 0.32}" fill="${suitColors[suit]}" font-weight="bold">${suitGlyphs[suit]}</text>`;
  out += "</svg>";
  return out;
}

function renderT2Crest(p: Record<string, string>, size: number): string {
  const bg = color(p.bg, "#1a1626");
  const primary = color(p.primary, "#e74c3c");
  const secondary = color(p.secondary, "#f1c40f");
  const charge = parseInt(p.charge ?? "0", 10) % 6;
  const division = parseInt(p.division ?? "0", 10) % 4;
  const cx = size / 2;
  const cy = size / 2;

  let out = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}">`;
  out += `<rect width="${size}" height="${size}" fill="${bg}" />`;
  out += `<path d="M ${size * 0.2} ${size * 0.15} L ${size * 0.8} ${size * 0.15} L ${size * 0.8} ${size * 0.65} L ${size * 0.5} ${size * 0.9} L ${size * 0.2} ${size * 0.65} Z" fill="${primary}" stroke="${secondary}" stroke-width="${size * 0.03}" />`;
  if (division === 1) {
    out += `<path d="M ${size * 0.2} ${size * 0.15} L ${size * 0.8} ${size * 0.9} L ${size * 0.2} ${size * 0.65} Z" fill="${secondary}" opacity="0.45" />`;
  } else if (division === 2) {
    out += `<rect x="${size * 0.2}" y="${size * 0.38}" width="${size * 0.6}" height="${size * 0.27}" fill="${secondary}" opacity="0.45" />`;
  } else if (division === 3) {
    out += `<path d="M ${size * 0.2} ${size * 0.15} L ${size * 0.5} ${size * 0.525} L ${size * 0.8} ${size * 0.15} Z" fill="${secondary}" opacity="0.45" />`;
    out += `<path d="M ${size * 0.2} ${size * 0.9} L ${size * 0.5} ${size * 0.525} L ${size * 0.8} ${size * 0.9} Z" fill="${secondary}" opacity="0.25" />`;
  }
  const charges = ["♠", "★", "♣", "♥", "⚔", "★★"];
  const chargeFont = charges[charge];
  out += `<text x="${cx}" y="${cy + size * 0.06}" text-anchor="middle" font-family="serif" font-size="${size * 0.22}" fill="${secondary}" font-weight="bold">${chargeFont}</text>`;
  out += `<rect x="${size * 0.1}" y="${size * 0.02}" width="${size * 0.8}" height="${size * 0.09}" fill="${secondary}" />`;
  out += `<rect x="${size * 0.1}" y="${size * 0.88}" width="${size * 0.8}" height="${size * 0.09}" fill="${secondary}" />`;
  out += "</svg>";
  return out;
}

function renderT3Droid(p: Record<string, string>, size: number): string {
  const bg = color(p.bg, "#0f1f1c");
  const body = color(p.body, "#3498db");
  const screen = color(p.screen, "#27ae60");
  const antenna = color(p.antenna, "#e74c3c");
  const face = parseInt(p.face ?? "0", 10) % 4;
  const cx = size / 2;

  let out = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}">`;
  out += `<rect width="${size}" height="${size}" fill="${bg}" />`;
  out += `<rect x="${cx - size * 0.02}" y="${size * 0.06}" width="${size * 0.04}" height="${size * 0.14}" fill="${antenna}" />`;
  out += `<circle cx="${cx}" cy="${size * 0.06}" r="${size * 0.05}" fill="${antenna}" />`;
  out += `<rect x="${size * 0.15}" y="${size * 0.2}" width="${size * 0.7}" height="${size * 0.25}" rx="${size * 0.05}" fill="${body}" />`;
  out += `<rect x="${size * 0.22}" y="${size * 0.25}" width="${size * 0.56}" height="${size * 0.15}" fill="${screen}" />`;
  if (face === 0) {
    out += `<rect x="${size * 0.32}" y="${size * 0.3}" width="${size * 0.04}" height="${size * 0.04}" fill="#000" />`;
    out += `<rect x="${size * 0.64}" y="${size * 0.3}" width="${size * 0.04}" height="${size * 0.04}" fill="#000" />`;
    out += `<rect x="${size * 0.4}" y="${size * 0.36}" width="${size * 0.2}" height="${size * 0.02}" fill="#000" />`;
  } else if (face === 1) {
    out += `<text x="${cx}" y="${size * 0.38}" text-anchor="middle" font-size="${size * 0.1}" fill="#000" font-family="monospace">8-D</text>`;
  } else if (face === 2) {
    out += `<rect x="${size * 0.3}" y="${size * 0.3}" width="${size * 0.4}" height="${size * 0.04}" fill="#000" />`;
    out += `<rect x="${size * 0.4}" y="${size * 0.38}" width="${size * 0.2}" height="${size * 0.02}" fill="#000" />`;
  } else {
    out += `<circle cx="${size * 0.35}" cy="${size * 0.33}" r="${size * 0.025}" fill="#000" />`;
    out += `<circle cx="${size * 0.65}" cy="${size * 0.33}" r="${size * 0.025}" fill="#000" />`;
    out += `<path d="M ${size * 0.38} ${size * 0.4} Q ${size * 0.5} ${size * 0.34} ${size * 0.62} ${size * 0.4}" stroke="#000" stroke-width="${size * 0.02}" fill="none" />`;
  }
  out += `<rect x="${size * 0.2}" y="${size * 0.45}" width="${size * 0.6}" height="${size * 0.4}" rx="${size * 0.04}" fill="${body}" />`;
  out += `<circle cx="${size * 0.35}" cy="${size * 0.62}" r="${size * 0.04}" fill="${antenna}" />`;
  out += `<circle cx="${size * 0.5}" cy="${size * 0.62}" r="${size * 0.04}" fill="${screen}" />`;
  out += `<circle cx="${size * 0.65}" cy="${size * 0.62}" r="${size * 0.04}" fill="${antenna}" />`;
  out += "</svg>";
  return out;
}

function renderT4SuitSigil(p: Record<string, string>, size: number): string {
  const bg = color(p.bg, "#16202c");
  const disc = color(p.disc, "#9b59b6");
  const suit = parseInt(p.suit ?? "0", 10) % 4;
  const ring = color(p.ring, "#f1c40f");
  const cx = size / 2;
  const cy = size / 2;
  const r = size * 0.4;
  const suitGlyphs2 = ["♠", "♥", "♦", "♣"];
  const suitColors2 = ["#ecf0f1", "#e74c3c", "#e74c3c", "#ecf0f1"];

  let out = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}">`;
  out += `<rect width="${size}" height="${size}" fill="${bg}" />`;
  out += `<circle cx="${cx}" cy="${cy}" r="${r}" fill="${disc}" />`;
  out += `<circle cx="${cx}" cy="${cy}" r="${r - size * 0.03}" fill="none" stroke="${ring}" stroke-width="${size * 0.025}" />`;
  out += `<text x="${cx}" y="${cy + size * 0.1}" text-anchor="middle" font-family="serif" font-size="${size * 0.4}" fill="${suitColors2[suit]}" font-weight="bold">${suitGlyphs2[suit]}</text>`;
  out += "</svg>";
  return out;
}

const TEMPLATE_RENDERERS: Array<(p: Record<string, string>, size: number) => string> = [
  renderT0PixelCat,
  renderT1ChipShape,
  renderT2Crest,
  renderT3Droid,
  renderT4SuitSigil,
];

export function renderSvgAvatar(
  templateId: number,
  params: Record<string, string>,
  size = 96
): string {
  const idx = Math.max(0, Math.min(templateId, TEMPLATE_RENDERERS.length - 1));
  return TEMPLATE_RENDERERS[idx](params, size);
}

export function svgDataUrl(
  templateId: number,
  params: Record<string, string>,
  size = 96
): string {
  const raw = renderSvgAvatar(templateId, params, size);
  const encoded = encodeURIComponent(raw)
    .replace(/'/g, "%27")
    .replace(/"/g, "%22");
  return `data:image/svg+xml;charset=utf-8,${encoded}`;
}
