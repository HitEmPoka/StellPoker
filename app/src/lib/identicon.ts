/**
 * Deterministic blockies-style identicon generation from an arbitrary seed
 * string (typically a Stellar public key). Pure logic only — no DOM/React
 * dependency — so it stays easy to unit test and reuse outside components.
 */

function hashSeed(input: string): number {
  let hash = 2166136261; // FNV-1a offset basis
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

function mulberry32(seed: number): () => number {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const PALETTES: ReadonlyArray<readonly [string, string]> = [
  ["#27ae60", "#16202c"],
  ["#e74c3c", "#1a1626"],
  ["#3498db", "#101a26"],
  ["#f1c40f", "#1a1810"],
  ["#9b59b6", "#1a1626"],
  ["#e67e22", "#1f1610"],
  ["#1abc9c", "#0f1f1c"],
  ["#ff6fae", "#1f1320"],
];

/** Identicon Grid shape. */
export interface IdenticonGrid {
  size: number;
  cells: boolean[][];
  fg: string;
  bg: string;
}

/** Deterministic, left-right symmetric pixel grid derived from `seedInput`. */
export function generateIdenticon(seedInput: string, size = 5): IdenticonGrid {
  const seed = hashSeed(seedInput);
  const rand = mulberry32(seed);
  const cells: boolean[][] = Array.from({ length: size }, () => Array(size).fill(false));
  const half = Math.ceil(size / 2);

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < half; x++) {
      const on = rand() > 0.5;
      cells[y][x] = on;
      cells[y][size - 1 - x] = on;
    }
  }

  const palette = PALETTES[seed % PALETTES.length];
  return { size, cells, fg: palette[0], bg: palette[1] };
}
