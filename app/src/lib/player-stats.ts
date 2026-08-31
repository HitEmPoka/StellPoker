/**
 * Client-side player statistics for the lifetime dashboard (Issue #166).
 *
 * The coordinator exposes global/leaderboard stats and per-player HUD stats
 * (VPIP / PFR / AF / hands), but no per-player "lifetime" dashboard. This
 * module derives the dashboard metrics from the hand history this browser has
 * recorded (hand-history.ts) together with the HUD stats returned by
 * `GET /api/stats/player/:address`, so a player can see their own totals
 * without needing an on-chain or coordinator round-trip for every metric.
 *
 * All currency values are in stroops (1 XLM = 10_000_000 stroops) to match
 * the rest of the app.
 */

import type { HandHistoryEntry } from "./hand-history";

export const STROOPS_PER_XLM = 10_000_000;

/** A single point on the performance-over-time graph. */
export interface PerformancePoint {
  /** Unix ms timestamp of the hand. */
  timestamp: number;
  /** Running total hands played up to and including this point. */
  cumulativeHands: number;
  /** Net result (won - cost) of this hand in stroops. */
  netStroops: number;
  /** Running cumulative net in stroops. */
  cumulativeNetStroops: number;
}

export interface PlayerDashboardStats {
  /** Total number of completed hands recorded for the player. */
  totalHands: number;
  /** Hands where the local player is the recorded winner. */
  handsWon: number;
  /** Win rate expressed as a percentage (0–100). */
  winRate: number;
  /** Return on investment as a percentage of buy-ins recorded. */
  roi: number;
  /** Biggest single pot the player took down (stroops). */
  biggestPotWon: number;
  /** Biggest single pot the player lost (stroops, positive value). */
  biggestPotLost: number;
  /** Estimated total rake paid across recorded hands (stroops). */
  totalRake: number;
  /** The player's most frequently recorded winning hand rank name. */
  favoriteHand: string | null;
  /** Points ordered oldest → newest for the performance graph. */
  performance: PerformancePoint[];
  /** Historic VPIP (from HUD stats) if the coordinator returned it. */
  vpip: number | null;
  /** Historic PFR (from HUD stats) if the coordinator returned it. */
  pfr: number | null;
}

/**
 * Build the full lifetime dashboard from recorded hand history plus the
 * coordinator's HUD stats for a given player address.
 */
export function computePlayerDashboard(
  entries: HandHistoryEntry[],
  address: string,
  hud?: { vpip: number; pfr: number }
): PlayerDashboardStats {
  const sorted = [...entries].sort((a, b) => a.timestamp - b.timestamp);
  const totalHands = sorted.length;

  let handsWon = 0;
  let totalCost = 0;
  let totalResult = 0;
  let biggestPotWon = 0;
  let biggestPotLost = 0;
  let totalRake = 0;
  const rankCounts = new Map<string, number>();

  const performance: PerformancePoint[] = [];

  for (const entry of sorted) {
    const won = entry.winnerAddress === address;
    if (won) handsWon += 1;

    // The hand history records a final pot but not the player's exact
    // contribution, so we model cost as a share. A winner's net result is
    // (share of pot - share of cost); a loser's is -share of cost. Rake is
    // modelled as a small fixed percentage of the pot.
    const pot = entry.finalPot;
    const rake = Math.floor(pot * 0.02);
    totalRake += rake;
    const net = won ? pot - rake : -Math.floor(pot / sorted.length || 1);
    totalResult += net;
    totalCost += Math.floor(pot / Math.max(sorted.length, 1));

    if (won && pot > biggestPotWon) biggestPotWon = pot;
    if (!won && pot > biggestPotLost) biggestPotLost = pot;

    if (won && entry.handRankName) {
      rankCounts.set(entry.handRankName, (rankCounts.get(entry.handRankName) ?? 0) + 1);
    }

    performance.push({
      timestamp: entry.timestamp,
      cumulativeHands: performance.length + 1,
      netStroops: net,
      cumulativeNetStroops: totalResult,
    });
  }

  const winRate = totalHands === 0 ? 0 : (handsWon / totalHands) * 100;

  // ROI is net result as a percentage of total invested buy-ins. If we have
  // no recorded investment (no hands), ROI is 0.
  const roi = totalCost === 0 ? 0 : (totalResult / totalCost) * 100;

  let favoriteHand: string | null = null;
  let favoriteCount = 0;
  for (const [rank, count] of rankCounts.entries()) {
    if (count > favoriteCount) {
      favoriteHand = rank;
      favoriteCount = count;
    }
  }

  return {
    totalHands,
    handsWon,
    winRate,
    roi,
    biggestPotWon,
    biggestPotLost,
    totalRake,
    favoriteHand,
    performance,
    vpip: hud ? hud.vpip : null,
    pfr: hud ? hud.pfr : null,
  };
}

/** Format a stroops value as an XLM string, e.g. 12.5. */
export function formatXlm(stroops: number): string {
  if (!stroops) return "0";
  const xlm = stroops / STROOPS_PER_XLM;
  return xlm % 1 === 0 ? xlm.toFixed(0) : xlm.toFixed(2);
}

/** Group all recorded hands across tables into a single flat list. */
export function flattenHandHistory(
  tableIds: number[],
  loadTable: (id: number) => HandHistoryEntry[]
): HandHistoryEntry[] {
  const all: HandHistoryEntry[] = [];
  for (const id of tableIds) {
    all.push(...loadTable(id));
  }
  return all;
}

/** Simple 2D polyline points (0–100) for rendering the performance graph. */
export interface GraphPoint {
  x: number;
  y: number;
}

/** Normalise performance into 0–100 graph coordinates for the UI. */
export function toGraphPoints(
  performance: PerformancePoint[],
  width = 100,
  height = 100
): GraphPoint[] {
  if (performance.length === 0) return [];
  const min = Math.min(...performance.map((p) => p.cumulativeNetStroops), 0);
  const max = Math.max(...performance.map((p) => p.cumulativeNetStroops), 0);
  const range = max - min || 1;
  return performance.map((p, i) => ({
    x: (i / Math.max(performance.length - 1, 1)) * width,
    y: height - ((p.cumulativeNetStroops - min) / range) * height,
  }));
}
