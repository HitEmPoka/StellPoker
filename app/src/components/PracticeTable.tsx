"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import { Board } from "./Board";
import { Card } from "./Card";
import { PixelCat, opponentSprite } from "./PixelCat";
import { PixelChip } from "./PixelChip";
import { PixelWorld } from "./PixelWorld";
import { MobileActionBar } from "./MobileActionBar";
import { bestHandRank } from "@/lib/hand-rank";
import { BOT_PROFILES, DIFFICULTIES, type Difficulty } from "@/lib/practice-bot";
import {
  createPracticeGame,
  startHand,
  applyAction,
  legalActions,
  reconfigure,
  isHumanTurn,
  humanSeat,
  displayPot,
  MAX_BOTS,
  HUMAN_SEAT_ID,
  type PracticeState,
  type PracticeSeat,
} from "@/lib/practice-engine";

/**
 * Practice mode — a full table against heuristic bots, with no wallet, no
 * XLM, and no coordinator (#174).
 *
 * Everything is the local engine in `practice-engine.ts`; this component only
 * renders it and forwards the player's decisions. It reuses the real table's
 * pieces (Board, Card, the mobile action bar) so what a new player learns here
 * transfers directly to a real table.
 */

const PRESET_LABELS = ["½ POT", "¾ POT", "POT", "MAX"] as const;

function seatSprite(index: number): number {
  return opponentSprite(index);
}

interface SeatViewProps {
  seat: PracticeSeat;
  index: number;
  isTurn: boolean;
  isDealer: boolean;
  revealed: boolean;
  wonAmount?: number;
}

function SeatView({ seat, index, isTurn, isDealer, revealed, wonAmount }: SeatViewProps) {
  const showCards = revealed || !seat.isBot;

  return (
    <div
      className="player-seat flex flex-col items-center gap-1"
      style={{ opacity: seat.folded ? 0.4 : 1, minWidth: "88px" }}
      data-testid={`practice-seat-${seat.id}`}
    >
      <div className="flex items-center gap-1">
        {isDealer && (
          <span
            className="text-[7px] px-1"
            style={{ background: "#f5e6c8", color: "#2c3e50" }}
            title="Dealer button"
          >
            D
          </span>
        )}
        <span
          className="text-[8px]"
          style={{ color: isTurn ? "#f1c40f" : "#c8e6ff" }}
        >
          {seat.name}
        </span>
      </div>

      <PixelCat sprite={seatSprite(index)} size={seat.isBot ? 48 : 64} />

      <div className="flex gap-1">
        {seat.cards ? (
          seat.cards.map((card, i) =>
            showCards ? (
              <Card key={i} value={card} size={seat.isBot ? "sm" : "md"} />
            ) : (
              <Card key={i} faceDown size="sm" />
            )
          )
        ) : (
          <>
            <Card faceDown size="sm" />
            <Card faceDown size="sm" />
          </>
        )}
      </div>

      <div className="flex items-center gap-1">
        <PixelChip color="gold" size={2} />
        <span className="text-[8px]" style={{ color: "#27ae60" }}>
          {seat.stack.toLocaleString()}
        </span>
      </div>

      {seat.betThisRound > 0 && (
        <span className="text-[7px]" style={{ color: "#f39c12" }}>
          BET {seat.betThisRound.toLocaleString()}
        </span>
      )}
      {seat.folded && (
        <span className="text-[7px]" style={{ color: "#e74c3c" }}>
          FOLDED
        </span>
      )}
      {wonAmount !== undefined && (
        <span className="text-[7px]" style={{ color: "#f1c40f" }}>
          +{wonAmount.toLocaleString()}
        </span>
      )}
    </div>
  );
}

export function PracticeTable() {
  const [game, setGame] = useState<PracticeState>(() =>
    // Seeded from the clock so each visit deals differently, while the engine
    // itself stays deterministic and testable.
    createPracticeGame({}, Date.now())
  );
  const [betAmount, setBetAmount] = useState(0);
  const [showSettings, setShowSettings] = useState(true);
  const logRef = useRef<HTMLDivElement>(null);

  const me = humanSeat(game);
  const myTurn = isHumanTurn(game);
  const legal = legalActions(game);
  const pot = displayPot(game);
  const inHand = game.phase !== "waiting" && game.phase !== "settlement";
  const settled = game.phase === "settlement";

  // Bots think out loud in the log, so keep the newest line in view.
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [game.log]);

  // Default the raise slider to the minimum legal raise whenever the spot
  // changes, so a stale number from a previous street is never pre-loaded.
  useEffect(() => {
    if (myTurn) setBetAmount(legal.minRaiseTo);
  }, [myTurn, legal.minRaiseTo, game.phase, game.handNumber]);

  const act = useCallback((action: string, amount?: number) => {
    setGame((current) => {
      if (!isHumanTurn(current)) return current;
      return applyAction(
        current,
        action as Parameters<typeof applyAction>[1],
        amount
      );
    });
  }, []);

  const deal = useCallback(() => {
    setShowSettings(false);
    setGame((current) => startHand(current));
  }, []);

  const changeConfig = useCallback(
    (patch: Parameters<typeof reconfigure>[1]) => {
      setGame((current) => reconfigure(current, patch));
    },
    []
  );

  // Keyboard shortcuts, matching the real table's F / C / B / R / A.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.isContentEditable)
      ) {
        return;
      }
      if (!myTurn) return;

      switch (event.key.toLowerCase()) {
        case "f":
          event.preventDefault();
          act("fold");
          break;
        case "c":
          if (legal.canCheck) {
            event.preventDefault();
            act("check");
          }
          break;
        case "b":
          event.preventDefault();
          if (legal.canCall) act("call");
          else act("bet", betAmount || legal.minRaiseTo);
          break;
        case "r":
          if (legal.canRaise) {
            event.preventDefault();
            act("raise", betAmount || legal.minRaiseTo);
          }
          break;
        case "a":
          event.preventDefault();
          act("allin");
          break;
        default:
          break;
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [act, betAmount, legal.canCall, legal.canCheck, legal.canRaise, legal.minRaiseTo, myTurn]);

  const presets = useMemo(() => {
    const values: Record<(typeof PRESET_LABELS)[number], number> = {
      "½ POT": Math.floor(pot * 0.5),
      "¾ POT": Math.floor(pot * 0.75),
      POT: pot,
      MAX: legal.maxRaiseTo,
    };
    return PRESET_LABELS.map((label) => ({
      label,
      value: Math.max(legal.minRaiseTo, Math.min(values[label], legal.maxRaiseTo)),
    }));
  }, [pot, legal.minRaiseTo, legal.maxRaiseTo]);

  const payoutFor = (seatId: string) =>
    game.payouts.find((p) => p.seatId === seatId)?.amount;

  const myRank =
    me?.cards && game.board.length >= 3
      ? bestHandRank([...me.cards, ...game.board])?.name
      : undefined;

  const profile = BOT_PROFILES[game.config.difficulty];
  const opponents = game.seats.filter((s) => s.id !== HUMAN_SEAT_ID);

  return (
    <PixelWorld>
      <div className="table-container min-h-screen flex flex-col items-center gap-3 p-4 pt-6">
        {/* Header */}
        <div className="table-header w-full max-w-3xl flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Link
              href="/"
              className="text-[24px]"
              style={{
                color: "#f5e6c8",
                textShadow: "2px 2px 0 #2c3e50",
                textDecoration: "none",
                fontFamily: "'Press Start 2P', monospace",
              }}
              aria-label="Back to lobby"
            >
              ←
            </Link>
            <h1 className="text-[13px]" style={{ color: "white", textShadow: "2px 2px 0 #2c3e50" }}>
              PRACTICE
            </h1>
          </div>
          <div className="table-header-right flex items-center gap-3">
            <span className="text-[9px]" style={{ color: "#c8e6ff" }}>
              HAND #{game.handNumber} | {game.phase.toUpperCase()}
            </span>
            <button
              onClick={() => setShowSettings((open) => !open)}
              aria-expanded={showSettings}
              className="pixel-btn text-[8px]"
              style={{ padding: "6px 10px" }}
            >
              SETUP
            </button>
          </div>
        </div>

        {/* No-wallet banner — the headline promise of practice mode. */}
        <p
          className="pixel-border-thin px-3 py-2 text-[8px] text-center w-full max-w-3xl"
          style={{
            background: "rgba(20, 90, 50, 0.25)",
            borderColor: "#27ae60",
            color: "#eafaf1",
          }}
        >
          PLAY MONEY · NO WALLET, NO XLM, NOTHING ON CHAIN. EVERY CARD IS DEALT
          IN YOUR BROWSER.
        </p>

        {/* Setup */}
        {showSettings && (
          <div
            className="home-panel pixel-border-thin w-full max-w-3xl p-4 flex flex-col gap-4"
            style={{ background: "rgba(12, 10, 24, 0.9)", borderColor: "#c47d2e" }}
          >
            <fieldset className="flex flex-col gap-2 border-0 p-0 m-0">
              <legend className="text-[10px]" style={{ color: "#ffc078" }}>
                DIFFICULTY
              </legend>
              <div className="flex gap-2 flex-wrap">
                {DIFFICULTIES.map((difficulty) => (
                  <button
                    key={difficulty}
                    onClick={() => changeConfig({ difficulty })}
                    aria-pressed={game.config.difficulty === difficulty}
                    className="pixel-btn text-[9px]"
                    style={{
                      padding: "8px 12px",
                      background:
                        game.config.difficulty === difficulty ? "#145a32" : "#2c3e50",
                      color: "white",
                    }}
                  >
                    {BOT_PROFILES[difficulty].label}
                  </button>
                ))}
              </div>
              <p className="text-[8px]" style={{ color: "#95a5a6" }}>
                {profile.description}
              </p>
            </fieldset>

            <fieldset className="flex flex-col gap-2 border-0 p-0 m-0">
              <legend className="text-[10px]" style={{ color: "#ffc078" }}>
                OPPONENTS
              </legend>
              <div className="flex gap-2 flex-wrap">
                {Array.from({ length: MAX_BOTS }, (_, i) => i + 1).map((count) => (
                  <button
                    key={count}
                    onClick={() => changeConfig({ botCount: count })}
                    aria-pressed={game.config.botCount === count}
                    className="pixel-btn text-[9px]"
                    style={{
                      padding: "8px 14px",
                      background:
                        game.config.botCount === count ? "#145a32" : "#2c3e50",
                      color: "white",
                    }}
                  >
                    {count}
                  </button>
                ))}
              </div>
              <p className="text-[8px]" style={{ color: "#95a5a6" }}>
                Changing a setting reshuffles and starts a fresh stack.
              </p>
            </fieldset>
          </div>
        )}

        {/* Felt */}
        <div className="w-full max-w-3xl">
          <div
            className="table-felt pixel-border relative w-full flex flex-col items-center justify-center gap-4"
            style={{
              background:
                "radial-gradient(ellipse at center, var(--felt-light) 0%, var(--felt-mid) 40%, var(--felt-dark) 100%)",
              borderColor: "#6b4f12",
              padding: "32px 16px",
              minHeight: "340px",
            }}
          >
            {/* Opponents */}
            <div className="opponents-row flex flex-wrap gap-5 items-end justify-center">
              {opponents.map((seat, i) => (
                <SeatView
                  key={seat.id}
                  seat={seat}
                  index={i + 1}
                  isTurn={game.seats[game.toAct]?.id === seat.id}
                  isDealer={game.seats[game.dealerSeat]?.id === seat.id}
                  // Bot cards stay hidden until the hand is settled and the
                  // pot was actually contested, exactly as at a real table.
                  revealed={settled && !seat.folded && game.board.length === 5}
                  wonAmount={settled ? payoutFor(seat.id) : undefined}
                />
              ))}
            </div>

            {/* Board */}
            <div
              className="w-full flex flex-col items-center gap-2 my-1"
              style={{
                borderTop: "2px solid rgba(139, 105, 20, 0.2)",
                borderBottom: "2px solid rgba(139, 105, 20, 0.2)",
                padding: "12px 0",
              }}
            >
              <Board cards={game.board} pot={pot} />
              {myRank && (
                <span className="text-[8px]" style={{ color: "#f1c40f" }}>
                  YOU HAVE: {myRank.toUpperCase()}
                </span>
              )}
            </div>

            {/* You */}
            <div className="user-seat-row flex justify-center">
              {me && (
                <SeatView
                  seat={me}
                  index={0}
                  isTurn={myTurn}
                  isDealer={game.seats[game.dealerSeat]?.id === me.id}
                  revealed
                  wonAmount={settled ? payoutFor(me.id) : undefined}
                />
              )}
            </div>
          </div>
        </div>

        {/* Desktop controls */}
        <div className="action-panel-desktop w-full max-w-3xl flex flex-col items-center gap-3">
          {game.busted && (
            <p className="text-[9px]" style={{ color: "#e74c3c" }}>
              YOU ARE OUT OF CHIPS — CHANGE A SETTING ABOVE TO REBUY AND PLAY ON.
            </p>
          )}

          {!inHand && !game.busted && (
            <button
              onClick={deal}
              className="pixel-btn pixel-btn-green text-[11px]"
              style={{ padding: "10px 22px" }}
            >
              {game.handNumber === 0 ? "DEAL FIRST HAND" : "NEXT HAND"}
            </button>
          )}

          {inHand && (
            <>
              <div className="flex items-center gap-4 flex-wrap justify-center">
                <span className="text-[9px]" style={{ color: "#95a5a6" }}>
                  TABLE BET: {game.currentBet.toLocaleString()}
                </span>
                <span className="text-[9px]" style={{ color: "#95a5a6" }}>
                  YOUR BET: {(me?.betThisRound ?? 0).toLocaleString()}
                </span>
                <span className="text-[9px]" style={{ color: "#27ae60" }}>
                  STACK: {(me?.stack ?? 0).toLocaleString()}
                </span>
              </div>

              <div className="action-panel-buttons flex items-center gap-2 flex-wrap justify-center">
                <button
                  onClick={() => act("fold")}
                  disabled={!myTurn}
                  className="pixel-btn text-[10px]"
                  style={{
                    padding: "8px 16px",
                    background: myTurn ? "#7b241c" : "#4a4a4a",
                    color: "white",
                    opacity: myTurn ? 1 : 0.5,
                  }}
                >
                  FOLD
                </button>
                <button
                  onClick={() => act(legal.canCheck ? "check" : "call")}
                  disabled={!myTurn}
                  className="pixel-btn text-[10px]"
                  style={{
                    padding: "8px 16px",
                    background: myTurn ? "#1a5276" : "#4a4a4a",
                    color: "white",
                    opacity: myTurn ? 1 : 0.5,
                  }}
                >
                  {legal.canCheck ? "CHECK" : `CALL ${legal.callAmount.toLocaleString()}`}
                </button>
                <button
                  onClick={() => act(game.currentBet === 0 ? "bet" : "raise", betAmount)}
                  disabled={!myTurn || !legal.canRaise}
                  className="pixel-btn text-[10px]"
                  style={{
                    padding: "8px 16px",
                    background: myTurn && legal.canRaise ? "#7d6608" : "#4a4a4a",
                    color: "white",
                    opacity: myTurn && legal.canRaise ? 1 : 0.5,
                  }}
                >
                  {game.currentBet === 0 ? "BET" : "RAISE"}{" "}
                  {(betAmount || legal.minRaiseTo).toLocaleString()}
                </button>
                <button
                  onClick={() => act("allin")}
                  disabled={!myTurn}
                  className="pixel-btn text-[10px]"
                  style={{
                    padding: "8px 16px",
                    background: myTurn ? "#d4ac0d" : "#4a4a4a",
                    color: "#1a1a1a",
                    fontWeight: "bold",
                    opacity: myTurn ? 1 : 0.5,
                  }}
                >
                  ALL IN
                </button>
              </div>

              {myTurn && legal.canRaise && (
                <div className="bet-slider-row flex items-center gap-3 w-full max-w-sm">
                  <input
                    type="range"
                    min={legal.minRaiseTo}
                    max={legal.maxRaiseTo}
                    value={betAmount || legal.minRaiseTo}
                    onChange={(e) => setBetAmount(Number(e.target.value))}
                    aria-label="Raise amount"
                    className="flex-1"
                    style={{ accentColor: "#f1c40f", height: "4px" }}
                  />
                  <div className="flex gap-1">
                    {presets.map((preset) => (
                      <button
                        key={preset.label}
                        onClick={() => setBetAmount(preset.value)}
                        className="pixel-btn text-[8px]"
                        style={{ padding: "4px 8px", background: "#2c3e50", color: "#c8e6ff" }}
                      >
                        {preset.label}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {!myTurn && (
                <span className="text-[9px]" style={{ color: "#95a5a6", fontStyle: "italic" }}>
                  BOTS ARE ACTING…
                </span>
              )}
            </>
          )}
        </div>

        {/* Action feed */}
        <div
          ref={logRef}
          className="pixel-border-thin w-full max-w-3xl p-2 overflow-y-auto flex flex-col gap-1"
          style={{
            background: "rgba(12, 10, 24, 0.8)",
            borderColor: "#4a6a8a",
            maxHeight: "128px",
          }}
          aria-label="Action feed"
        >
          {game.log.length === 0 ? (
            <span className="text-[8px]" style={{ color: "#7f8c8d" }}>
              Deal a hand to begin.
            </span>
          ) : (
            game.log.map((entry, i) => (
              <span key={i} className="text-[8px]" style={{ color: "#c8e6ff" }}>
                {entry.text}
              </span>
            ))
          )}
        </div>
      </div>

      {/* Same sticky bar as the real table (#175). */}
      <MobileActionBar
        visible={inHand}
        isMyTurn={myTurn}
        currentBet={game.currentBet}
        myBet={me?.betThisRound ?? 0}
        myStack={me?.stack ?? 0}
        pot={pot}
        minRaiseTo={legal.minRaiseTo}
        betAmount={betAmount}
        setBetAmount={setBetAmount}
        onAction={act}
      />
    </PixelWorld>
  );
}
