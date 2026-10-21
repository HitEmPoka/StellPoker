"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import Link from "next/link";
import { Board } from "./Board";
import { PlayerSeat } from "./PlayerSeat";
import { PixelWorld } from "./PixelWorld";
import { SpectatorCount } from "./SpectatorCount";
import * as api from "@/lib/api";
import {
  describeSpectatorAction,
  diffSpectatorActions,
  parseSpectatorView,
  type SpectatorAction,
  type SpectatorView,
} from "@/lib/spectator";

const MAX_FEED = 30;
/** Safety-net poll in case a WebSocket push is missed or unsupported. */
const POLL_MS = 8000;

interface SpectatorTableProps {
  tableId: number;
}

/**
 * Read-only, wallet-less view of a live table (Issue #171). Shows community
 * cards, pot, stacks and a public action feed; every seat's hole cards stay
 * face down.
 */
export function SpectatorTable({ tableId }: SpectatorTableProps) {
  const [view, setView] = useState<SpectatorView | null>(null);
  const [feed, setFeed] = useState<SpectatorAction[]>([]);
  const [spectators, setSpectators] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const viewRef = useRef<SpectatorView | null>(null);

  const applyOnchainState = useCallback((raw: string | null) => {
    if (!raw) return;
    let parsed: Record<string, unknown> | null = null;
    try {
      parsed = JSON.parse(raw) as Record<string, unknown>;
    } catch {
      return;
    }
    const next = parseSpectatorView(parsed);
    if (!next) return;
    const actions = diffSpectatorActions(viewRef.current, next);
    viewRef.current = next;
    setView(next);
    setError(null);
    if (actions.length > 0) {
      setFeed((prev) => [...actions.reverse(), ...prev].slice(0, MAX_FEED));
    }
  }, []);

  const poll = useCallback(async () => {
    try {
      const { state } = await api.getTableState(tableId);
      applyOnchainState(state);
    } catch (e) {
      if (!viewRef.current) {
        setError(e instanceof Error ? e.message : "Table unavailable");
      }
    }
  }, [applyOnchainState, tableId]);

  useEffect(() => {
    void poll();
    api
      .getSpectatorCount(tableId)
      .then((r) => setSpectators(r.spectator_count))
      .catch(() => {});

    const socket = api.subscribeGameState(
      tableId,
      (msg) => {
        if (api.isSpectatorCountEvent(msg)) {
          setSpectators(msg.spectator_count);
          return;
        }
        if (typeof msg.spectator_count === "number") {
          setSpectators(msg.spectator_count);
        }
        if (msg.onchain_state) {
          applyOnchainState(msg.onchain_state);
        } else {
          void poll();
        }
      },
      { spectate: true }
    );

    const interval = setInterval(() => void poll(), POLL_MS);
    return () => {
      clearInterval(interval);
      socket?.stop();
    };
  }, [applyOnchainState, poll, tableId]);

  return (
    <PixelWorld>
      <div className="min-h-screen flex flex-col items-center gap-4 p-4" data-testid="spectator-table">
        <div className="w-full max-w-3xl flex flex-wrap items-center justify-between gap-2">
          <Link href="/" className="pixel-btn text-[9px]" style={{ padding: "6px 10px" }}>
            ← LOBBY
          </Link>
          <div className="flex items-center gap-2">
            <span className="text-[10px]" style={{ color: "#ffc078" }}>
              TABLE #{tableId}
            </span>
            <SpectatorCount count={spectators} />
          </div>
          <Link
            href={`/table/${tableId}`}
            className="pixel-btn pixel-btn-blue text-[9px]"
            style={{ padding: "6px 10px" }}
          >
            TAKE A SEAT
          </Link>
        </div>

        <div
          className="pixel-border-thin px-3 py-1 text-[8px]"
          role="status"
          style={{ background: "rgba(12, 10, 24, 0.88)", borderColor: "#74b9ff", color: "#c8e6ff" }}
        >
          SPECTATING · NO WALLET NEEDED · HOLE CARDS HIDDEN
        </div>

        {error && (
          <div
            className="pixel-border-thin px-4 py-2 text-[9px]"
            style={{ background: "rgba(231, 76, 60, 0.2)", borderColor: "#e74c3c", color: "#e74c3c" }}
          >
            {error}
          </div>
        )}

        <div className="w-full max-w-3xl">
          <div
            className="table-felt pixel-border relative w-full flex flex-col items-center justify-center gap-6"
            style={{
              background:
                "radial-gradient(ellipse at center, var(--felt-light) 0%, var(--felt-mid) 40%, var(--felt-dark) 100%)",
              borderColor: "#6b4f12",
              padding: "32px 16px",
              minHeight: "360px",
            }}
          >
            <div className="text-[8px]" style={{ color: "#f5e6c8" }}>
              {view ? `HAND #${view.handNumber} · ${view.rawPhase.toUpperCase()}` : "CONNECTING..."}
            </div>

            <Board cards={view?.boardCards ?? []} pot={view?.pot ?? 0} />

            <div className="flex flex-wrap gap-4 sm:gap-6 items-end justify-center">
              {view?.players.map((player) => (
                <PlayerSeat
                  key={player.address}
                  // Deliberately no `cards`: seats always render face down.
                  player={player}
                  isCurrentTurn={view.phase !== "waiting" && player.seat === view.currentTurn}
                  isDealer={player.seat === view.dealerSeat}
                  isUser={false}
                  boardCards={view.boardCards}
                  gamePhase={view.phase}
                  showStatsTooltip={false}
                />
              ))}
              {view && view.players.length === 0 && (
                <span className="text-[9px]" style={{ color: "rgba(255,255,255,0.5)" }}>
                  NO PLAYERS SEATED YET
                </span>
              )}
            </div>
          </div>
        </div>

        <section
          className="w-full max-w-3xl pixel-border-thin p-3"
          aria-label="Action feed"
          style={{ background: "rgba(12, 10, 24, 0.88)", borderColor: "#4a3a28" }}
        >
          <h2 className="text-[9px] mb-2" style={{ color: "#f5e6c8" }}>
            ACTIONS
          </h2>
          {feed.length === 0 ? (
            <p className="text-[8px]" style={{ color: "#7f8c8d" }}>
              WAITING FOR ACTION...
            </p>
          ) : (
            <ul className="flex flex-col gap-1" data-testid="spectator-feed">
              {feed.map((action, i) => (
                <li key={`${action.handNumber}-${i}-${action.kind}`} className="text-[8px]" style={{ color: "#c8e6ff" }}>
                  {describeSpectatorAction(action)}
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </PixelWorld>
  );
}
