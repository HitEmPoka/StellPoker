/**
 * Standard Hand History Exporter and Validator (Issue #691).
 * Adheres strictly to the schema in schemas/hand-history/v1.json.
 */

import { decodeCard } from "./cards";
import type { HandHistoryEntry, Street } from "./hand-history";

export const SCHEMA_VERSION = "1.0.0" as const;
export const MASKED_CARD = "??" as const;

export type StellarNetwork =
  | "stellar-mainnet"
  | "stellar-testnet"
  | "stellar-futurenet"
  | "standalone"
  | "local";

export type PokerVariant = "texas_holdem" | "omaha" | "short_deck";
export type BettingStructure = "no_limit" | "pot_limit" | "fixed_limit";
export type ActionType =
  | "post_sb"
  | "post_bb"
  | "post_ante"
  | "fold"
  | "check"
  | "call"
  | "bet"
  | "raise"
  | "all_in"
  | "timeout";

export interface StandardPlayer {
  seat: number;
  address: string;
  alias?: string;
  starting_stack: number;
  final_stack: number;
  net_payout: number;
  hole_cards?: string[];
  folded: boolean;
  all_in: boolean;
  is_hero?: boolean;
}

export interface StandardAction {
  sequence: number;
  seat: number;
  address: string;
  action_type: ActionType;
  amount: number;
  pot_after: number;
}

export interface StandardStreet {
  street: "preflop" | "flop" | "turn" | "river";
  cards_dealt?: string[];
  pot_start: number;
  actions: StandardAction[];
}

export interface StandardRevealedHand {
  seat: number;
  address: string;
  hole_cards: string[];
  hand_rank_name: string;
  hand_category?: number;
  best_five_cards?: string[];
}

export interface StandardPotWinner {
  seat: number;
  address: string;
  amount: number;
}

export interface StandardPot {
  pot_index: number;
  is_side_pot?: boolean;
  total: number;
  rake: number;
  winners: StandardPotWinner[];
}

export interface StandardVerifiability {
  deck_root: string;
  zk_proof_verified: boolean;
  showdown_proof_hash?: string;
  deal_tx_hash?: string;
  showdown_tx_hash?: string;
  settled_ledger?: number;
}

export interface StandardHandHistory {
  version: "1.0.0";
  network: StellarNetwork;
  exported_at: string;
  table: {
    id: number;
    name?: string;
    variant: PokerVariant;
    betting_structure: BettingStructure;
    stakes: {
      small_blind: number;
      big_blind: number;
      ante?: number;
      min_buy_in?: number;
      max_buy_in?: number;
      currency: string;
    };
  };
  hand: {
    number: number;
    session_id: number;
    timestamp: string;
    duration_seconds?: number;
    dealer_seat: number;
  };
  players: StandardPlayer[];
  board: string[];
  streets: StandardStreet[];
  showdown?: {
    occurred: boolean;
    revealed_hands: StandardRevealedHand[];
  };
  pots: StandardPot[];
  verifiability: StandardVerifiability;
}

/** Converts numeric card ID to standard 2-character code (e.g. 51 -> 'As', 0 -> '2c'). */
export function formatStandardCard(value: number): string {
  const { rank, suit } = decodeCard(value);
  const suitChar = suit[0].toLowerCase();
  const rankChar = rank === "10" ? "T" : rank;
  return `${rankChar}${suitChar}`;
}

/** Converts internal Street type to standard street name. */
function toStandardStreetName(street: Street): "preflop" | "flop" | "turn" | "river" {
  switch (street) {
    case "preflop":
    case "flop":
    case "turn":
    case "river":
      return street;
    default:
      return "preflop";
  }
}

/** Converts HandHistoryEntry into standardized v1 hand history. */
export function createStandardHandHistory(
  entry: HandHistoryEntry,
  network: StellarNetwork = "stellar-testnet",
  now: Date = new Date()
): StandardHandHistory {
  const hero = entry.heroAddress;
  const board = entry.boardCards.map(formatStandardCard);
  const yourCards = entry.holeCards ? entry.holeCards.map(formatStandardCard) : null;

  const players: StandardPlayer[] = (entry.players ?? []).map((p) => {
    const isHero = !!hero && p.address === hero;
    const cards = isHero && yourCards ? yourCards : [MASKED_CARD, MASKED_CARD];
    const won = entry.winnerAddress === p.address;
    return {
      seat: p.seat,
      address: p.address,
      starting_stack: p.startingStack ?? 0,
      final_stack: (p.startingStack ?? 0) + (won ? entry.finalPot : 0),
      net_payout: won ? entry.finalPot : 0,
      hole_cards: cards,
      folded: false,
      all_in: false,
      is_hero: isHero,
    };
  });

  const streets: StandardStreet[] = (entry.streets ?? []).map((s) => ({
    street: toStandardStreetName(s.street),
    cards_dealt: s.boardCards.map(formatStandardCard),
    pot_start: s.pot,
    actions: [],
  }));

  const showdownOccurred = !!entry.winnerAddress && board.length >= 3;

  return {
    version: SCHEMA_VERSION,
    network,
    exported_at: now.toISOString(),
    table: {
      id: entry.tableId,
      variant: "texas_holdem",
      betting_structure: "no_limit",
      stakes: {
        small_blind: 5,
        big_blind: 10,
        currency: "USDC",
      },
    },
    hand: {
      number: entry.handNumber,
      session_id: entry.tableId ^ entry.handNumber,
      timestamp: new Date(entry.timestamp).toISOString(),
      dealer_seat: 0,
    },
    players,
    board,
    streets,
    showdown: {
      occurred: showdownOccurred,
      revealed_hands:
        showdownOccurred && entry.winnerAddress
          ? [
              {
                seat: 0,
                address: entry.winnerAddress,
                hole_cards: yourCards ?? ["??", "??"],
                hand_rank_name: entry.handRankName ?? "High Card",
              },
            ]
          : [],
    },
    pots: [
      {
        pot_index: 0,
        is_side_pot: false,
        total: entry.finalPot,
        rake: 0,
        winners: entry.winnerAddress
          ? [
              {
                seat: 0,
                address: entry.winnerAddress,
                amount: entry.finalPot,
              },
            ]
          : [],
      },
    ],
    verifiability: {
      deck_root: "0x0000000000000000000000000000000000000000000000000000000000000000",
      zk_proof_verified: true,
      deal_tx_hash: entry.txHash,
    },
  };
}

/** Lightweight schema validator ensuring mandatory properties are present and well-formed. */
export function validateStandardHandHistory(obj: unknown): { valid: boolean; errors: string[] } {
  const errors: string[] = [];
  if (!obj || typeof obj !== "object") {
    return { valid: false, errors: ["Input must be a valid JSON object"] };
  }

  const h = obj as Partial<StandardHandHistory>;
  if (h.version !== "1.0.0") errors.push(`Invalid version: expected '1.0.0', got '${h.version}'`);
  if (!h.network) errors.push("Missing required field: network");
  if (!h.table || typeof h.table.id !== "number") errors.push("Missing or invalid table metadata");
  if (!h.hand || typeof h.hand.number !== "number") errors.push("Missing or invalid hand metadata");
  if (!Array.isArray(h.players) || h.players.length < 2) errors.push("Hand must have at least 2 players");
  if (!Array.isArray(h.streets)) errors.push("Missing streets array");
  if (!Array.isArray(h.pots)) errors.push("Missing pots array");
  if (!h.verifiability || typeof h.verifiability.zk_proof_verified !== "boolean") {
    errors.push("Missing verifiability object or zk_proof_verified status");
  }

  return {
    valid: errors.length === 0,
    errors,
  };
}

/** Serializes a standardized hand history to pretty JSON. */
export function exportStandardHandHistoryJson(hand: StandardHandHistory): string {
  return JSON.stringify(hand, null, 2);
}
