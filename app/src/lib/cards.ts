const SUITS = ["clubs", "diamonds", "hearts", "spades"] as const;
const RANKS = [
  "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A",
] as const;

/** Suit shape. */
export type Suit = (typeof SUITS)[number];
/** Rank shape. */
export type Rank = (typeof RANKS)[number];

/** Card Info shape. */
export interface CardInfo {
  value: number;
  suit: Suit;
  rank: Rank;
  display: string;
  suitSymbol: string;
  color: "red" | "black";
}

/** Computes Card.
 * @param value - value.
 * @returns CardInfo.
 */
export function decodeCard(value: number): CardInfo {
  const suit = SUITS[Math.floor(value / 13)];
  const rank = RANKS[value % 13];
  const suitSymbols: Record<Suit, string> = {
    clubs: "\u2663",
    diamonds: "\u2666",
    hearts: "\u2665",
    spades: "\u2660",
  };
  const color = suit === "hearts" || suit === "diamonds" ? "red" : "black";
  return {
    value,
    suit,
    rank,
    display: `${rank}${suitSymbols[suit]}`,
    suitSymbol: suitSymbols[suit],
    color,
  };
}

const RANK_NAMES: Record<Rank, string> = {
  "2": "Two", "3": "Three", "4": "Four", "5": "Five", "6": "Six", "7": "Seven",
  "8": "Eight", "9": "Nine", "10": "Ten", J: "Jack", Q: "Queen", K: "King", A: "Ace",
};

/** Spoken name for screen readers, e.g. "Ace of spades". */
export function cardLabel(value: number): string {
  const { rank, suit } = decodeCard(value);
  return `${RANK_NAMES[rank]} of ${suit}`;
}

/** Card Back.
 * @returns string.
 */
export function cardBack(): string {
  return "?";
}
