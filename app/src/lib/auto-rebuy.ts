/**
 * Auto-rebuy decision logic (Issue #164). Pure and network-free so the
 * actual "should I rebuy, and for how much" reasoning is fully testable
 * without a live Soroban RPC connection — the on-chain reads/writes that
 * feed this function live in onchain.ts.
 */

export type AutoRebuyMode = "always_max" | "below_threshold" | "never";

export interface AutoRebuyPreference {
  mode: AutoRebuyMode;
  /** Only used when mode === "below_threshold": trigger when the stack
   * drops below this many big blinds. */
  thresholdBB?: number;
}

export interface AutoRebuyContext {
  preference: AutoRebuyPreference;
  /** Player's current chip stack at this table. */
  currentStack: bigint;
  /** Current big blind size, used to evaluate a "below_threshold" preference. */
  bigBlind: bigint;
  /** Table's configured min/max buy-in band (contracts/poker-table's TableConfig). */
  minBuyIn: bigint;
  maxBuyIn: bigint;
  /** Table's max_rebuys (0 = unlimited) and the player's rebuys used so far. */
  maxRebuys: number;
  rebuyCount: number;
  /** Player's available wallet balance in the table's payment token. */
  walletBalance: bigint;
}

export interface AutoRebuyDecision {
  shouldRebuy: boolean;
  /** Amount to rebuy, in the table's payment token's smallest unit. Always
   * 0 when shouldRebuy is false. */
  amount: bigint;
  /** Present when shouldRebuy is false and it's useful to say why (e.g. for
   * a log line or a disabled-state tooltip) — absent when the preference
   * simply didn't trigger under current conditions. */
  reason?: string;
}

const ZERO = BigInt(0);

/**
 * Decides whether an auto-rebuy should fire right now (called between
 * hands — the contract itself also rejects a rebuy attempted mid-hand, this
 * is a second, earlier check so we don't even try) and for how much.
 *
 * Respects, in order: the "never" preference, the table's max_rebuys limit,
 * the trigger condition for the chosen mode, the contract's per-rebuy cap
 * (a single rebuy may never exceed one full max_buy_in, and the resulting
 * stack may never exceed max_buy_in either), and finally the player's
 * wallet balance.
 */
export function decideAutoRebuy(ctx: AutoRebuyContext): AutoRebuyDecision {
  if (ctx.preference.mode === "never") {
    return { shouldRebuy: false, amount: ZERO, reason: "auto-rebuy is disabled for this table" };
  }

  if (ctx.maxRebuys > 0 && ctx.rebuyCount >= ctx.maxRebuys) {
    return { shouldRebuy: false, amount: ZERO, reason: "table rebuy limit reached" };
  }

  let triggered = false;
  if (ctx.preference.mode === "always_max") {
    triggered = ctx.currentStack < ctx.maxBuyIn;
  } else if (ctx.preference.mode === "below_threshold") {
    const thresholdChips = ctx.bigBlind * BigInt(Math.max(0, Math.floor(ctx.preference.thresholdBB ?? 0)));
    triggered = ctx.currentStack < thresholdChips;
  }

  if (!triggered) {
    return { shouldRebuy: false, amount: ZERO };
  }

  // Top up to max_buy_in. The contract enforces that a single rebuy amount
  // never exceeds max_buy_in and that the resulting stack never exceeds it
  // either — topping up to exactly max_buy_in from any lower stack always
  // satisfies both, since amount = maxBuyIn - currentStack < maxBuyIn
  // whenever currentStack > 0, and equals maxBuyIn only when currentStack
  // is already 0 (a full re-entry), which the contract still allows.
  const amount = ctx.maxBuyIn - ctx.currentStack;
  if (amount <= ZERO) {
    return { shouldRebuy: false, amount: ZERO };
  }
  if (ctx.currentStack + amount < ctx.minBuyIn) {
    // Shouldn't happen in practice (maxBuyIn >= minBuyIn is a table
    // invariant), but guard against a misconfigured table rather than
    // submitting a rebuy the contract would reject anyway.
    return { shouldRebuy: false, amount: ZERO, reason: "resulting stack would be below the table minimum" };
  }

  if (ctx.walletBalance < amount) {
    return { shouldRebuy: false, amount: ZERO, reason: "insufficient wallet balance for this rebuy" };
  }

  return { shouldRebuy: true, amount };
}
