import type { GameState } from "@/lib/game-state";

function deterministicPercent(seed: string): number {
  let hash = 2166136261;
  for (let i = 0; i < seed.length; i += 1) {
    hash ^= seed.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0) % 100;
}

/** Solo Bet Result shape. */
export interface SoloBetResult {
  pot: number;
  userStack: number;
  userBet: number;
  botStack: number;
  botBet: number;
  aiFolded: boolean;
  aiLine: string;
}

/** Computes Solo Bet.
 * @param game - game.
 * @param tableId - table Id.
 * @param meAddress - me Address.
 * @param botAddress - bot Address.
 * @param action - action.
 * @param amount - amount.
 * @returns SoloBetResult.
 */
export function computeSoloBet(
  game: GameState,
  tableId: number,
  meAddress: string,
  botAddress: string,
  action: string,
  amount: number | undefined,
): SoloBetResult {
  const me = game.players.find((p) => p.address === meAddress)!;
  const bot = game.players.find((p) => p.address === botAddress)!;

  let userBet = me.betThisRound;
  let botBet = bot.betThisRound;
  let userStack = me.stack;
  let botStack = bot.stack;
  const botStackBefore = botStack;
  let pot = game.pot;
  const current = Math.max(userBet, botBet);
  let userContribution = 0;

  if (action === "call" || action === "check") {
    const callAmount = Math.max(current - userBet, 0);
    userContribution = Math.min(callAmount, userStack);
    userBet += userContribution;
    userStack -= userContribution;
  } else if (action === "allin") {
    userContribution = userStack;
    userBet += userContribution;
    userStack = 0;
  } else if (action === "bet" || action === "raise") {
    const normalizedAmount =
      typeof amount === "number" && Number.isFinite(amount)
        ? Math.max(1, Math.floor(amount))
        : 1;
    const targetBet = Math.max(current, normalizedAmount);
    userContribution = Math.max(0, Math.min(targetBet - userBet, userStack));
    userBet += userContribution;
    userStack -= userContribution;
  }

  let botContribution = 0;
  const neededByBot = Math.max(userBet - botBet, 0);
  let aiAction: "check" | "call" | "fold";
  if (neededByBot === 0) {
    aiAction = "check";
  } else {
    const pressure = neededByBot / Math.max(1, botStack + pot);
    const roll = deterministicPercent(
      `${tableId}:${game.handNumber}:${game.phase}:${game.boardCards.join(",")}:${neededByBot}:${botStack}`
    );
    if (
      (pressure > 0.7 && roll < 85) ||
      (pressure > 0.45 && roll < 55) ||
      (pressure > 0.25 && roll < 25)
    ) {
      aiAction = "fold";
    } else {
      aiAction = "call";
    }
  }

  if (aiAction === "call" && neededByBot > 0) {
    botContribution = Math.min(neededByBot, botStack);
    botBet += botContribution;
    botStack -= botContribution;
  }

  pot += userContribution + botContribution;

  const aiLine =
    aiAction === "fold"
      ? "AI folds. You win the pot."
      : aiAction === "check"
        ? "AI checks."
        : botContribution >= botStackBefore
          ? "AI calls all-in."
          : "AI calls.";

  return {
    pot,
    userStack,
    userBet: 0, // reset for next round
    botStack,
    botBet: 0,
    aiFolded: aiAction === "fold",
    aiLine,
  };
}
