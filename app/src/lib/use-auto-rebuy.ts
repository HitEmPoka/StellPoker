"use client";

import { useEffect, useRef, useState } from "react";
import type { WalletSession } from "./wallet";
import type { GamePhase } from "./game-state";
import { getAutoRebuyPreference } from "./auto-rebuy-store";
import { decideAutoRebuy } from "./auto-rebuy";
import { getTableConfig, getPlayerRebuyCount, getTokenBalance, rebuyOnChain } from "./onchain";

export type AutoRebuyStatus = "idle" | "checking" | "rebuying" | "error";

interface UseAutoRebuyOptions {
  tableId: number;
  wallet: WalletSession | null;
  phase: GamePhase;
  /** Player's current chip stack at this table, in the table's token's smallest unit. */
  currentStack: number;
}

/**
 * Checks the connected player's auto-rebuy preference whenever the table
 * settles into "waiting" (between hands — the only phase the contract's
 * `rebuy` itself accepts, alongside "settlement"), and submits an on-chain
 * rebuy if the preference's condition is met (Issue #164).
 */
export function useAutoRebuy({ tableId, wallet, phase, currentStack }: UseAutoRebuyOptions) {
  const [status, setStatus] = useState<AutoRebuyStatus>("idle");
  const [lastError, setLastError] = useState<string | null>(null);
  const checkedForThisWaitRef = useRef(false);

  useEffect(() => {
    if (phase !== "waiting") {
      checkedForThisWaitRef.current = false;
      return;
    }
    if (!wallet || checkedForThisWaitRef.current) return;
    checkedForThisWaitRef.current = true;

    let cancelled = false;

    (async () => {
      setStatus("checking");
      setLastError(null);
      try {
        const preference = getAutoRebuyPreference(tableId, wallet.address);
        if (preference.mode === "never") {
          if (!cancelled) setStatus("idle");
          return;
        }

        const [tableConfig, rebuyCount] = await Promise.all([
          getTableConfig(wallet.address, tableId),
          getPlayerRebuyCount(wallet.address, tableId, wallet.address),
        ]);
        const walletBalance = await getTokenBalance(wallet.address, tableConfig.tokenContract);

        const decision = decideAutoRebuy({
          preference,
          currentStack: BigInt(Math.max(0, Math.floor(currentStack))),
          bigBlind: tableConfig.bigBlind,
          minBuyIn: tableConfig.minBuyIn,
          maxBuyIn: tableConfig.maxBuyIn,
          maxRebuys: tableConfig.maxRebuys,
          rebuyCount,
          walletBalance,
        });

        if (cancelled) return;

        if (decision.shouldRebuy) {
          setStatus("rebuying");
          await rebuyOnChain(wallet, tableId, decision.amount);
        }
        if (!cancelled) setStatus("idle");
      } catch (e) {
        if (!cancelled) {
          setLastError(e instanceof Error ? e.message : "Auto-rebuy check failed");
          setStatus("error");
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [phase, wallet, tableId, currentStack]);

  return { status, lastError };
}
