import {
  Address,
  BASE_FEE,
  Contract,
  TransactionBuilder,
  nativeToScVal,
  scValToNative,
  rpc,
  xdr,
} from "@stellar/stellar-sdk";
import type { WalletSession } from "./wallet";
import { getChainConfig } from "./api";

type LobstrSignResult = {
  signedTxXdr?: string;
  signed_transaction?: string;
  error?: { message: string } | string;
};

async function signWithWallet(
  wallet: WalletSession,
  txXdr: string,
  opts: { networkPassphrase: string; address: string }
): Promise<string> {
  if (wallet.walletType === "lobstr") {
    const api = typeof window !== "undefined" ? (window as unknown as { lobstr?: { signTransaction?: (xdr: string, opts: Record<string, unknown>) => Promise<LobstrSignResult> } }).lobstr : undefined;
    if (!api?.signTransaction) {
      throw new Error("Lobstr signTransaction API is unavailable");
    }
    const result = await api.signTransaction(txXdr, opts);
    const signedXdr = result.signedTxXdr || result.signed_transaction;
    if (!signedXdr) {
      const msg = typeof result.error === "string" ? result.error : result.error?.message || "Lobstr failed to sign transaction";
      throw new Error(msg);
    }
    return signedXdr;
  }

  const { signTransaction: freighterSignTransaction } = await import("@stellar/freighter-api");
  const result = await freighterSignTransaction(txXdr, opts);
  if (result.error || !result.signedTxXdr) {
    const message =
      typeof result.error?.message === "string"
        ? result.error.message
        : "Freighter failed to sign transaction";
    throw new Error(message);
  }
  return result.signedTxXdr;
}

type BettingAction = "fold" | "check" | "call" | "bet" | "raise" | "allin" | "all_in";

let cachedChainConfig:
  | {
      rpcUrl: string;
      networkPassphrase: string;
      pokerTableContract: string;
    }
  | null = null;

async function getConfig() {
  if (cachedChainConfig) return cachedChainConfig;
  const cfg = await getChainConfig();
  cachedChainConfig = {
    rpcUrl: cfg.rpc_url,
    networkPassphrase: cfg.network_passphrase,
    pokerTableContract: cfg.poker_table_contract,
  };
  return cachedChainConfig;
}

function toActionScVal(action: BettingAction, amount?: number): xdr.ScVal {
  const normalized = action.trim().toLowerCase() as BettingAction;
  let variant: string;
  let payload: number | null = null;

  switch (normalized) {
    case "fold":
      variant = "Fold";
      break;
    case "check":
      variant = "Check";
      break;
    case "call":
      variant = "Call";
      break;
    case "allin":
    case "all_in":
      variant = "AllIn";
      break;
    case "bet":
      if (!Number.isFinite(amount) || amount === undefined || amount <= 0) {
        throw new Error("Bet amount must be a positive number");
      }
      variant = "Bet";
      payload = Math.floor(amount);
      break;
    case "raise":
      if (!Number.isFinite(amount) || amount === undefined || amount <= 0) {
        throw new Error("Raise amount must be a positive number");
      }
      variant = "Raise";
      payload = Math.floor(amount);
      break;
    default:
      throw new Error(`Unsupported action: ${action}`);
  }

  const values: xdr.ScVal[] = [xdr.ScVal.scvSymbol(variant)];
  if (payload !== null) {
    values.push(nativeToScVal(payload, { type: "i128" }));
  }
  return xdr.ScVal.scvVec(values);
}

/**
 * Per-account submission queue.
 *
 * A Stellar transaction is signed against the account's current sequence
 * number, and the network accepts exactly one transaction per sequence. Now
 * that a wallet can be seated at several tables at once (#72), two tables can
 * easily fire an action in the same tick — both would read the same sequence
 * from `getAccount` and the second would be rejected with `txBAD_SEQ`.
 *
 * Chaining submissions per source account means each one reads the sequence
 * only after the previous transaction has been sent, so the numbers never
 * collide. Different wallets never block each other, and the chain is
 * per-address rather than global so multi-table play stays responsive.
 *
 * The chain deliberately survives failures: `.catch(() => {})` keeps a rejected
 * submission from poisoning every later one, while the caller still sees the
 * original rejection.
 */
const submissionQueues = new Map<string, Promise<unknown>>();

function enqueueForAccount<T>(address: string, task: () => Promise<T>): Promise<T> {
  const previous = submissionQueues.get(address) ?? Promise.resolve();
  const result = previous.then(task, task);
  submissionQueues.set(
    address,
    result.catch(() => {})
  );
  return result;
}

async function submitWalletTx(
  wallet: WalletSession,
  method: string,
  args: xdr.ScVal[]
): Promise<string | undefined> {
  const cfg = await getConfig();
  const server = new rpc.Server(cfg.rpcUrl, { allowHttp: cfg.rpcUrl.startsWith("http://") });

  // Only reading the sequence through to sending has to be serialized. Waiting
  // for confirmation does not, so two tables can have transactions in flight at
  // the same time and neither blocks the other's turn.
  const sent = await enqueueForAccount(wallet.address, async () => {
    const account = await server.getAccount(wallet.address);
    const contract = new Contract(cfg.pokerTableContract);

    const tx = new TransactionBuilder(account, {
      fee: BASE_FEE,
      networkPassphrase: cfg.networkPassphrase,
    })
      .addOperation(contract.call(method, ...args))
      .setTimeout(60)
      .build();

    const prepared = await server.prepareTransaction(tx);
    const signedXdr = await signWithWallet(wallet, prepared.toXDR(), {
      networkPassphrase: cfg.networkPassphrase,
      address: wallet.address,
    });

    const signedTx = TransactionBuilder.fromXDR(
      signedXdr,
      cfg.networkPassphrase
    );
    const response = await server.sendTransaction(signedTx);
    if (response.status === "ERROR") {
      throw new Error("On-chain transaction rejected");
    }
    return response;
  });

  if (sent.hash) {
    const result = await server.pollTransaction(sent.hash, {
      attempts: 30,
      sleepStrategy: () => 1500,
    });
    if (result.status === rpc.Api.GetTransactionStatus.FAILED) {
      throw new Error("On-chain transaction failed");
    }
  }

  return sent.hash || undefined;
}

export async function joinTableOnChain(
  wallet: WalletSession,
  tableId: number,
  buyIn: bigint
): Promise<string | undefined> {
  return submitWalletTx(wallet, "join_table", [
    nativeToScVal(tableId, { type: "u32" }),
    new Address(wallet.address).toScVal(),
    nativeToScVal(buyIn, { type: "i128" }),
  ]);
}

export async function playerActionOnChain(
  wallet: WalletSession,
  tableId: number,
  action: BettingAction,
  amount?: number
): Promise<string | undefined> {
  return submitWalletTx(wallet, "player_action", [
    nativeToScVal(tableId, { type: "u32" }),
    new Address(wallet.address).toScVal(),
    toActionScVal(action, amount),
  ]);
}

/**
 * Simulates a read-only call against the given contract, using
 * `sourceAddress`'s account purely to build a syntactically valid
 * transaction envelope for simulation — no signature or submission
 * happens, matching the account-not-required nature of a getter.
 */
async function simulateReadCall(
  sourceAddress: string,
  contractId: string,
  method: string,
  args: xdr.ScVal[]
): Promise<unknown> {
  const cfg = await getConfig();
  const server = new rpc.Server(cfg.rpcUrl, { allowHttp: cfg.rpcUrl.startsWith("http://") });
  const account = await server.getAccount(sourceAddress);
  const contract = new Contract(contractId);

  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: cfg.networkPassphrase,
  })
    .addOperation(contract.call(method, ...args))
    .setTimeout(30)
    .build();

  const sim = await server.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) {
    throw new Error(`Simulation failed for ${method}: ${sim.error}`);
  }
  const result = (sim as rpc.Api.SimulateTransactionSuccessResponse).result;
  if (!result) {
    throw new Error(`No result returned for ${method}`);
  }
  return scValToNative(result.retval);
}

/** Subset of contracts/poker-table's TableConfig needed by auto-rebuy (Issue #164). */
export interface RebuyRelevantTableConfig {
  minBuyIn: bigint;
  maxBuyIn: bigint;
  maxRebuys: number;
  /** Payment token contract for this table (used to check wallet balance). */
  tokenContract: string;
  /**
   * Big blind of the schedule's first level. Correct for the common
   * fixed-blinds case; an escalating (tournament-style) multi-level
   * schedule's *currently active* level requires the same level-selection
   * logic the contract applies internally, which this does not replicate —
   * auto-rebuy's "below N big blinds" threshold on such a table will use
   * the starting level's big blind, not the current one.
   */
  bigBlind: bigint;
}

/**
 * Reads the subset of a table's on-chain config relevant to auto-rebuy
 * decisions, via `get_table` (Issue #164).
 */
export async function getTableConfig(
  sourceAddress: string,
  tableId: number
): Promise<RebuyRelevantTableConfig> {
  const cfg = await getConfig();
  const native = (await simulateReadCall(sourceAddress, cfg.pokerTableContract, "get_table", [
    nativeToScVal(tableId, { type: "u32" }),
  ])) as {
    config: {
      min_buy_in: bigint;
      max_buy_in: bigint;
      max_rebuys: number;
      token: string;
      blinds_schedule: { levels: Array<{ big_blind: bigint }> };
    };
  };

  const firstLevel = native.config.blinds_schedule.levels[0];
  return {
    minBuyIn: BigInt(native.config.min_buy_in),
    maxBuyIn: BigInt(native.config.max_buy_in),
    maxRebuys: Number(native.config.max_rebuys),
    tokenContract: native.config.token,
    bigBlind: firstLevel ? BigInt(firstLevel.big_blind) : BigInt(0),
  };
}

/** Reads a seated player's rebuy count so far this session, via `get_player_buy_in` (Issue #164). */
export async function getPlayerRebuyCount(
  sourceAddress: string,
  tableId: number,
  playerAddress: string
): Promise<number> {
  const cfg = await getConfig();
  const native = (await simulateReadCall(sourceAddress, cfg.pokerTableContract, "get_player_buy_in", [
    nativeToScVal(tableId, { type: "u32" }),
    new Address(playerAddress).toScVal(),
  ])) as [bigint, number];

  return Number(native[1]);
}

/**
 * Reads a wallet's balance of the given SEP-41 token contract, via the
 * token contract's standard `balance` method (Issue #164) — the same
 * interface every Soroban token contract implements, including the one
 * `contracts/poker-table` transfers buy-ins/rebuys through.
 */
export async function getTokenBalance(
  sourceAddress: string,
  tokenContract: string
): Promise<bigint> {
  const native = (await simulateReadCall(sourceAddress, tokenContract, "balance", [
    new Address(sourceAddress).toScVal(),
  ])) as bigint;

  return BigInt(native);
}

/** Submits an on-chain rebuy for the connected wallet (Issue #164). */
export async function rebuyOnChain(
  wallet: WalletSession,
  tableId: number,
  amount: bigint
): Promise<string | undefined> {
  return submitWalletTx(wallet, "rebuy", [
    nativeToScVal(tableId, { type: "u32" }),
    new Address(wallet.address).toScVal(),
    nativeToScVal(amount, { type: "i128" }),
  ]);
}
