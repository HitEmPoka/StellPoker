/**
 * Standard 5-card poker hand ranking, evaluated over the best 5-card
 * combination out of up to 7 cards (2 hole + 5 board). Card values follow
 * the same encoding as `cards.ts`: value = suit * 13 + rankIndex.
 */

export interface HandRank {
  category: number; // 1 (high card) .. 9 (straight flush)
  name: string;
  tiebreak: number[]; // descending-significance values for comparison
}

const CATEGORY_NAMES = [
  "",
  "High Card",
  "Pair",
  "Two Pair",
  "Three of a Kind",
  "Straight",
  "Flush",
  "Full House",
  "Four of a Kind",
  "Straight Flush",
];

function rankValue(cardValue: number): number {
  return (cardValue % 13) + 2; // 2..14, Ace high
}

function suitOf(cardValue: number): number {
  return Math.floor(cardValue / 13);
}

function combinations5(cards: number[]): number[][] {
  const result: number[][] = [];
  const n = cards.length;
  for (let a = 0; a < n; a++)
    for (let b = a + 1; b < n; b++)
      for (let c = b + 1; c < n; c++)
        for (let d = c + 1; d < n; d++)
          for (let e = d + 1; e < n; e++)
            result.push([cards[a], cards[b], cards[c], cards[d], cards[e]]);
  return result;
}

function evaluate5(cards: number[]): HandRank {
  const ranks = cards.map(rankValue).sort((a, b) => b - a);
  const suits = cards.map(suitOf);
  const isFlush = suits.every((s) => s === suits[0]);

  const counts = new Map<number, number>();
  for (const r of ranks) counts.set(r, (counts.get(r) ?? 0) + 1);
  const grouped = [...counts.entries()].sort((a, b) => b[1] - a[1] || b[0] - a[0]);

  const uniqueRanksDesc = [...new Set(ranks)].sort((a, b) => b - a);
  let straightHigh = 0;
  if (uniqueRanksDesc.length === 5) {
    if (uniqueRanksDesc[0] - uniqueRanksDesc[4] === 4) {
      straightHigh = uniqueRanksDesc[0];
    } else if (
      uniqueRanksDesc[0] === 14 &&
      uniqueRanksDesc[1] === 5 &&
      uniqueRanksDesc[2] === 4 &&
      uniqueRanksDesc[3] === 3 &&
      uniqueRanksDesc[4] === 2
    ) {
      straightHigh = 5; // wheel: A-2-3-4-5
    }
  }

  if (straightHigh && isFlush) {
    return { category: 9, name: CATEGORY_NAMES[9], tiebreak: [straightHigh] };
  }
  if (grouped[0][1] === 4) {
    return { category: 8, name: CATEGORY_NAMES[8], tiebreak: [grouped[0][0], grouped[1][0]] };
  }
  if (grouped[0][1] === 3 && grouped[1] && grouped[1][1] === 2) {
    return { category: 7, name: CATEGORY_NAMES[7], tiebreak: [grouped[0][0], grouped[1][0]] };
  }
  if (isFlush) {
    return { category: 6, name: CATEGORY_NAMES[6], tiebreak: ranks };
  }
  if (straightHigh) {
    return { category: 5, name: CATEGORY_NAMES[5], tiebreak: [straightHigh] };
  }
  if (grouped[0][1] === 3) {
    const kickers = grouped.slice(1).map((g) => g[0]).sort((a, b) => b - a);
    return { category: 4, name: CATEGORY_NAMES[4], tiebreak: [grouped[0][0], ...kickers] };
  }
  if (grouped[0][1] === 2 && grouped[1] && grouped[1][1] === 2) {
    const pairs = [grouped[0][0], grouped[1][0]].sort((a, b) => b - a);
    return { category: 3, name: CATEGORY_NAMES[3], tiebreak: [...pairs, grouped[2][0]] };
  }
  if (grouped[0][1] === 2) {
    const kickers = grouped.slice(1).map((g) => g[0]).sort((a, b) => b - a);
    return { category: 2, name: CATEGORY_NAMES[2], tiebreak: [grouped[0][0], ...kickers] };
  }
  return { category: 1, name: CATEGORY_NAMES[1], tiebreak: ranks };
}

export function compareHandRank(a: HandRank, b: HandRank): number {
  if (a.category !== b.category) return a.category - b.category;
  const len = Math.max(a.tiebreak.length, b.tiebreak.length);
  for (let i = 0; i < len; i++) {
    const av = a.tiebreak[i] ?? 0;
    const bv = b.tiebreak[i] ?? 0;
    if (av !== bv) return av - bv;
  }
  return 0;
}

/** Best 5-card ranking achievable from `cards` (2-7 cards). */
export function bestHandRank(cards: number[]): HandRank | null {
  if (cards.length < 5) return null;
  const candidates = combinations5(cards).map(evaluate5);
  return candidates.reduce((best, cur) => (compareHandRank(cur, best) > 0 ? cur : best));
}
