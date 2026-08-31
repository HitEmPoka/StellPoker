"use client";

/**
 * Player dashboard with lifetime statistics (Issue #166).
 *
 * Shows a connected player their own cumulative poker stats: total hands,
 * win rate, ROI, biggest pot won/lost, total rake, favorite hand and a
 * performance-over-time graph. Data is derived from the hand history this
 * browser has recorded across tables plus the coordinator's HUD stats.
 */

import { useEffect, useMemo, useState } from "react";
import Link from "next/link";
import { PixelWorld } from "@/components/PixelWorld";
import { trySilentReconnect, type WalletSession } from "@/lib/wallet";
import { getPlayerHudStats } from "@/lib/api";
import { loadHandHistory } from "@/lib/hand-history";
import { loadOpenTables } from "@/lib/open-tables";
import { getAlias } from "@/lib/alias-store";
import {
  computePlayerDashboard,
  flattenHandHistory,
  formatXlm,
  toGraphPoints,
  type PlayerDashboardStats,
} from "@/lib/player-stats";

export default function PlayerDashboardPage() {
  const [wallet, setWallet] = useState<WalletSession | null>(null);
  const [stats, setStats] = useState<PlayerDashboardStats | null>(null);
  const [hud, setHud] = useState<{ vpip: number; pfr: number } | null>(null);
  const [busy, setBusy] = useState(true);

  useEffect(() => {
    trySilentReconnect().then((s) => setWallet(s));
  }, []);

  useEffect(() => {
    if (!wallet) {
      setBusy(false);
      return;
    }
    const w = wallet;
    let cancelled = false;

    async function load() {
      const tables = loadOpenTables(w.address).map((t) => t.tableId);
      // Fall back to scanning hand-history keys even if no table is open.
      const played = flattenHandHistory(
        tables.length > 0 ? tables : [],
        (id) => loadHandHistory(id)
      );
      let hudStats: { vpip: number; pfr: number } | null = null;
      try {
        const hud = await getPlayerHudStats(w.address);
        hudStats = { vpip: hud.vpip, pfr: hud.pfr };
      } catch {
        // HUD stats may be unavailable if coordinator isn't running; the
        // rest of the dashboard still works from local hand history.
      }
      if (cancelled) return;
      setHud(hudStats);
      setStats(computePlayerDashboard(played, w.address, hudStats ?? undefined));
      setBusy(false);
    }

    load();
    return () => {
      cancelled = true;
    };
  }, [wallet]);

  const points = useMemo(
    () => (stats ? toGraphPoints(stats.performance) : []),
    [stats]
  );

  const alias = wallet ? getAlias(wallet.address) : null;
  const short = wallet
    ? `${wallet.address.slice(0, 6)}…${wallet.address.slice(-4)}`
    : "";

  return (
    <PixelWorld>
      <main className="min-h-screen flex flex-col items-center p-6 gap-6">
        <div className="w-full" style={{ maxWidth: 640 }}>
          <div className="flex items-center justify-between">
            <Link
              href="/"
              className="text-[9px]"
              style={{ color: "#c47d2e", textDecoration: "none", fontFamily: "'Press Start 2P', monospace" }}
            >
              ← HOME
            </Link>
            <div className="text-[10px]" style={{ color: "#f5e6c8" }}>
              PLAYER DASHBOARD
            </div>
            <div style={{ width: 40 }} />
          </div>
        </div>

        {!wallet && !busy && (
          <div
            className="pixel-border px-6 py-6 text-center text-[9px]"
            style={{ borderColor: "#2a2a4a", background: "rgba(12,10,24,0.9)", color: "#95a5a6" }}
          >
            CONNECT A WALLET TO SEE YOUR LIFETIME STATS
          </div>
        )}

        {wallet && (
          <div
            className="pixel-border p-4 w-full flex flex-col gap-4"
            style={{ borderColor: "#27ae60", background: "rgba(12,10,24,0.92)" }}
          >
            <div className="text-[11px]" style={{ color: "#f1c40f" }}>
              {alias ? alias : short}
            </div>
            <div className="text-[8px]" style={{ color: "#95a5a6" }}>
              {wallet.address}
            </div>

            {busy ? (
              <div className="text-[9px]" style={{ color: "#8a9ab0" }} aria-live="polite">
                CALCULATING LIFETIME STATS…
              </div>
            ) : stats && stats.totalHands === 0 ? (
              <div className="text-[9px]" style={{ color: "#95a5a6" }}>
                NO HANDS RECORDED YET. PLAY A HAND, THEN COME BACK.
              </div>
            ) : stats ? (
              <>
                <div className="grid grid-cols-2 gap-2">
                  <StatTile label="HANDS PLAYED" value={String(stats.totalHands)} />
                  <StatTile label="WIN RATE" value={`${stats.winRate.toFixed(1)}%`} />
                  <StatTile label="ROI" value={`${stats.roi.toFixed(1)}%`} accent={stats.roi >= 0 ? "#27ae60" : "#e74c3c"} />
                  <StatTile label="HANDS WON" value={String(stats.handsWon)} />
                  <StatTile label="BIGGEST POT WON" value={`${formatXlm(stats.biggestPotWon)} XLM`} />
                  <StatTile label="BIGGEST POT LOST" value={`${formatXlm(stats.biggestPotLost)} XLM`} />
                  <StatTile label="TOTAL RAKE" value={`${formatXlm(stats.totalRake)} XLM`} />
                  <StatTile label="FAVORITE HAND" value={stats.favoriteHand ?? "—"} />
                </div>

                {hud && (
                  <div className="flex gap-4 text-[8px]" style={{ color: "#95a5a6" }}>
                    <span>VPIP: {hud.vpip.toFixed(0)}%</span>
                    <span>PFR: {hud.pfr.toFixed(0)}%</span>
                  </div>
                )}

                <div>
                  <div className="text-[8px] mb-2" style={{ color: "#95a5a6" }}>
                    PERFORMANCE OVER TIME
                  </div>
                  {points.length < 2 ? (
                    <div className="text-[8px]" style={{ color: "#7f8c8d" }}>
                      NEED AT LEAST 2 HANDS TO PLOT
                    </div>
                  ) : (
                    <svg
                      viewBox="0 0 100 100"
                      className="w-full"
                      style={{ background: "rgba(0,0,0,0.3)", border: "1px solid #2a2a4a" }}
                      preserveAspectRatio="none"
                      data-testid="performance-graph"
                      aria-label="Performance graph of cumulative net winnings over time"
                    >
                      <polyline
                        points={points.map((p) => `${p.x},${p.y}`).join(" ")}
                        fill="none"
                        stroke="#27ae60"
                        strokeWidth="1"
                        vectorEffect="non-scaling-stroke"
                      />
                      <polygon
                        points={`0,100 ${points.map((p) => `${p.x},${p.y}`).join(" ")} 100,100`}
                        fill="rgba(39,174,96,0.15)"
                      />
                    </svg>
                  )}
                </div>
              </>
            ) : null}
          </div>
        )}
      </main>
    </PixelWorld>
  );
}

function StatTile({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: string;
}) {
  return (
    <div className="pixel-border-thin p-2 flex flex-col gap-1" style={{ borderColor: "#2a2a4a", background: "rgba(0,0,0,0.25)" }}>
      <div className="text-[7px]" style={{ color: "#7f8c8d" }}>{label}</div>
      <div className="text-[11px]" style={{ color: accent ?? "#f5e6c8" }} data-testid="stat-value">
        {value}
      </div>
    </div>
  );
}
