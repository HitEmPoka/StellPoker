"use client";

/**
 * Portfolio view (#631): the connected wallet's seated chips and wallet
 * balances, grouped by token across every table it has open in this browser.
 *
 * Seats come from one batched contract read (`get_player_positions`, #560).
 * Each seated table's token comes from its config, and each token's wallet
 * balance is read once, however many tables share it.
 */

import { useEffect, useMemo, useState } from "react";
import Link from "next/link";
import { PixelWorld } from "@/components/PixelWorld";
import { trySilentReconnect, type WalletSession } from "@/lib/wallet";
import { loadOpenTables } from "@/lib/open-tables";
import {
  getPlayerPositions,
  getTableConfig,
  getTokenBalance,
} from "@/lib/onchain";
import { seatedPositions, type PlayerPosition } from "@/lib/player-positions";
import {
  buildPortfolio,
  formatTokenAmount,
  type Portfolio,
  type PortfolioTokenRow,
} from "@/lib/portfolio";

/** Shortens a contract address for display, keeping the start and end. */
function shortAddress(address: string): string {
  return `${address.slice(0, 6)}…${address.slice(-4)}`;
}

/** Reads each seated table's token contract. A table that fails to read is
 * left out of the map and shows up under an unknown token. */
async function readTokensByTable(
  sourceAddress: string,
  positions: readonly PlayerPosition[]
): Promise<Map<number, string>> {
  const entries = await Promise.all(
    positions.map(async (p) => {
      try {
        const config = await getTableConfig(sourceAddress, p.tableId);
        return [p.tableId, config.tokenContract] as const;
      } catch {
        return null;
      }
    })
  );
  const map = new Map<number, string>();
  for (const entry of entries) {
    if (entry) map.set(entry[0], entry[1]);
  }
  return map;
}

/** Reads the wallet balance of each distinct token. Failed reads are left
 * out, so those rows show the balance as unavailable. */
async function readBalances(
  sourceAddress: string,
  tokens: readonly string[]
): Promise<Map<string, bigint>> {
  const entries = await Promise.all(
    tokens.map(async (token) => {
      try {
        return [token, await getTokenBalance(sourceAddress, token)] as const;
      } catch {
        return null;
      }
    })
  );
  const map = new Map<string, bigint>();
  for (const entry of entries) {
    if (entry) map.set(entry[0], entry[1]);
  }
  return map;
}

/** Renders the portfolio page. */
export default function PortfolioPage() {
  const [wallet, setWallet] = useState<WalletSession | null>(null);
  const [portfolio, setPortfolio] = useState<Portfolio | null>(null);
  const [error, setError] = useState<string | null>(null);
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
      setBusy(true);
      setError(null);
      try {
        const tableIds = loadOpenTables(w.address).map((t) => t.tableId);
        const positions =
          tableIds.length > 0
            ? seatedPositions(await getPlayerPositions(w.address, tableIds))
            : [];
        const tokenByTable = await readTokensByTable(w.address, positions);
        const tokens = Array.from(new Set(tokenByTable.values()));
        const balances = await readBalances(w.address, tokens);
        if (cancelled) return;
        setPortfolio(buildPortfolio(positions, tokenByTable, balances));
      } catch {
        if (cancelled) return;
        setPortfolio(null);
        setError("Could not read your seats from the chain. Try again in a moment.");
      } finally {
        if (!cancelled) setBusy(false);
      }
    }

    load();
    return () => {
      cancelled = true;
    };
  }, [wallet]);

  const totals = useMemo(() => portfolio?.rows ?? [], [portfolio]);

  return (
    <PixelWorld>
      <main className="min-h-screen flex flex-col items-center p-6 gap-6">
        <div className="w-full" style={{ maxWidth: 640 }}>
          <div className="flex items-center justify-between">
            <Link
              href="/dashboard"
              className="text-[9px]"
              style={{ color: "#c47d2e", textDecoration: "none", fontFamily: "'Press Start 2P', monospace" }}
            >
              ← DASHBOARD
            </Link>
            <h1 className="text-[10px]" style={{ color: "#f5e6c8" }}>
              PORTFOLIO
            </h1>
            <div style={{ width: 40 }} />
          </div>
        </div>

        {!wallet && !busy && (
          <div
            className="pixel-border px-6 py-6 text-center text-[9px]"
            style={{ borderColor: "#2a2a4a", background: "rgba(12,10,24,0.9)", color: "#95a5a6" }}
          >
            CONNECT A WALLET TO SEE YOUR PORTFOLIO
          </div>
        )}

        {wallet && (
          <section
            className="pixel-border p-4 w-full flex flex-col gap-4"
            style={{ borderColor: "#27ae60", background: "rgba(12,10,24,0.92)" }}
            aria-labelledby="portfolio-heading"
          >
            <h2 id="portfolio-heading" className="text-[9px]" style={{ color: "#95a5a6" }}>
              {wallet.address.slice(0, 6)}…{wallet.address.slice(-4)}
            </h2>

            {busy && (
              <div className="text-[9px]" style={{ color: "#8a9ab0" }} aria-live="polite">
                READING YOUR TABLES…
              </div>
            )}

            {!busy && error && (
              <div className="text-[9px]" style={{ color: "#e74c3c" }} role="alert">
                {error}
              </div>
            )}

            {!busy && !error && portfolio && portfolio.seatedTableCount === 0 && (
              <div className="text-[9px]" style={{ color: "#95a5a6" }}>
                NO SEATS RIGHT NOW. JOIN A TABLE AND YOUR CHIPS WILL SHOW HERE.
              </div>
            )}

            {!busy && !error && totals.length > 0 && (
              <>
                <div className="text-[8px]" style={{ color: "#7f8c8d" }}>
                  TOTALS ARE PER TOKEN. DIFFERENT TOKENS ARE NEVER ADDED TOGETHER.
                </div>
                {totals.map((row) => (
                  <TokenCard key={row.token ?? "unknown"} row={row} />
                ))}
              </>
            )}
          </section>
        )}
      </main>
    </PixelWorld>
  );
}

/** One token's totals, with the tables that hold it. */
function TokenCard({ row }: { row: PortfolioTokenRow }) {
  const tokenLabel = row.token ? shortAddress(row.token) : "UNKNOWN TOKEN";
  return (
    <article
      className="pixel-border-thin p-3 flex flex-col gap-2"
      style={{ borderColor: "#2a2a4a", background: "rgba(0,0,0,0.25)" }}
      aria-label={`Token ${tokenLabel}`}
    >
      <div className="text-[8px]" style={{ color: "#f1c40f" }} title={row.token ?? undefined}>
        {tokenLabel}
      </div>
      <dl className="grid grid-cols-3 gap-2 text-[8px]">
        <div>
          <dt style={{ color: "#7f8c8d" }}>SEATED</dt>
          <dd data-testid="portfolio-seated" style={{ color: "#f5e6c8" }}>
            {formatTokenAmount(row.seatedChips)}
          </dd>
        </div>
        <div>
          <dt style={{ color: "#7f8c8d" }}>IN POT</dt>
          <dd style={{ color: "#f5e6c8" }}>{formatTokenAmount(row.inPot)}</dd>
        </div>
        <div>
          <dt style={{ color: "#7f8c8d" }}>WALLET</dt>
          <dd data-testid="portfolio-wallet" style={{ color: "#f5e6c8" }}>
            {row.walletBalance === null ? "UNAVAILABLE" : formatTokenAmount(row.walletBalance)}
          </dd>
        </div>
      </dl>
      <ul className="flex flex-col gap-1 text-[8px]">
        {row.tables.map((t) => (
          <li key={t.tableId} className="flex justify-between" style={{ color: "#c8d6e5" }}>
            <Link href={`/table/${t.tableId}`} style={{ color: "#f1c40f" }}>
              TABLE {t.tableId}
            </Link>
            <span>{t.phase.toUpperCase()}</span>
            <span>{formatTokenAmount(t.stack)}</span>
          </li>
        ))}
      </ul>
    </article>
  );
}
