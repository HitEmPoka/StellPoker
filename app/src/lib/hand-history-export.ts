/**
 * Hand history export (#158): the hands kept by `hand-history.ts`, as JSON or
 * CSV for the player to download.
 *
 * Only the exporting player's own hole cards are ever written. Every opponent
 * is listed with masked cards, so a shared export can't leak what anyone else
 * held.
 */

import { decodeCard } from "./cards";
import type { HandHistoryEntry, Street } from "./hand-history";

/** Export Format shape. */
export type ExportFormat = "json" | "csv";

/** How a masked hole card appears in an export. */
export const MASKED_CARD = "??";

/** Exported Player shape. */
export interface ExportedPlayer {
  address: string;
  seat: number;
  isYou: boolean;
  /** Your own cards; masked for every opponent. */
  holeCards: string[];
}

/** Exported Hand shape. */
export interface ExportedHand {
  tableId: number;
  handNumber: number;
  playedAt: string;
  players: ExportedPlayer[];
  yourHoleCards: string[] | null;
  boardCards: string[];
  streets: Array<{ street: Street; pot: number; boardCards: string[] }>;
  actions: Array<{ street: Street; amount: number; pot: number }>;
  finalPot: number;
  winner: string | null;
  yourHandRank: string | null;
  /** Whether you won the pot, or null when that can't be told from the record. */
  youWon: boolean | null;
  proofTxHash: string | null;
}

/** Compact card notation for exports, e.g. "Ah" or "10s". */
export function cardCode(value: number): string {
  const { rank, suit } = decodeCard(value);
  return `${rank}${suit[0]}`;
}

/** Computes to Exported Hand.
 * @param entry - entry.
 * @returns ExportedHand.
 */
export function toExportedHand(entry: HandHistoryEntry): ExportedHand {
  const hero = entry.heroAddress;
  const yourHoleCards = entry.holeCards ? entry.holeCards.map(cardCode) : null;

  return {
    tableId: entry.tableId,
    handNumber: entry.handNumber,
    playedAt: new Date(entry.timestamp).toISOString(),
    players: (entry.players ?? []).map(({ address, seat }) => {
      const isYou = !!hero && address === hero;
      return {
        address,
        seat,
        isYou,
        holeCards: isYou && yourHoleCards ? yourHoleCards : [MASKED_CARD, MASKED_CARD],
      };
    }),
    yourHoleCards,
    boardCards: entry.boardCards.map(cardCode),
    streets: entry.streets.map((s) => ({
      street: s.street,
      pot: s.pot,
      boardCards: s.boardCards.map(cardCode),
    })),
    actions: (entry.actions ?? []).map(({ street, amount, pot }) => ({ street, amount, pot })),
    finalPot: entry.finalPot,
    winner: entry.winnerAddress ?? null,
    yourHandRank: entry.handRankName ?? null,
    youWon: hero && entry.winnerAddress ? entry.winnerAddress === hero : null,
    proofTxHash: entry.txHash ?? null,
  };
}

/** Export Hand History Json.
 * @param entries - entries.
 * @param now - now.
 * @returns string.
 */
export function exportHandHistoryJson(
  entries: readonly HandHistoryEntry[],
  now: Date = new Date()
): string {
  return JSON.stringify(
    {
      exportedAt: now.toISOString(),
      hands: entries.map(toExportedHand),
    },
    null,
    2
  );
}

const CSV_COLUMNS = [
  "table_id",
  "hand_number",
  "played_at",
  "your_hole_cards",
  "opponents",
  "board",
  "street_pots",
  "actions",
  "final_pot",
  "winner",
  "your_hand_rank",
  "you_won",
  "proof_tx",
];

/** Quotes a CSV field when it contains a delimiter, quote, or line break. */
function csvField(value: string | number | boolean | null): string {
  if (value === null) return "";
  const text = String(value);
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** One row per hand; list-valued columns are joined with " | ". */
export function exportHandHistoryCsv(entries: readonly HandHistoryEntry[]): string {
  const rows = entries.map((entry) => {
    const hand = toExportedHand(entry);
    return [
      hand.tableId,
      hand.handNumber,
      hand.playedAt,
      hand.yourHoleCards ? hand.yourHoleCards.join(" ") : null,
      hand.players
        .filter((p) => !p.isYou)
        .map((p) => `seat ${p.seat} ${p.address} ${p.holeCards.join(" ")}`)
        .join(" | "),
      hand.boardCards.join(" "),
      hand.streets.map((s) => `${s.street} ${s.pot}`).join(" | "),
      hand.actions.map((a) => `${a.street} ${a.amount} (pot ${a.pot})`).join(" | "),
      hand.finalPot,
      hand.winner,
      hand.yourHandRank,
      hand.youWon,
      hand.proofTxHash,
    ]
      .map(csvField)
      .join(",");
  });

  return [CSV_COLUMNS.join(","), ...rows].join("\r\n");
}

/** e.g. `stellpoker-table-4-hands-2026-09-24.csv` */
export function exportFilename(
  tableId: number,
  format: ExportFormat,
  now: Date = new Date()
): string {
  return `stellpoker-table-${tableId}-hands-${now.toISOString().slice(0, 10)}.${format}`;
}
