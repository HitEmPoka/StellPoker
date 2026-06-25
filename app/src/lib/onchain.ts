import {
  Address,
  BASE_FEE,
  Contract,
  TransactionBuilder,
  nativeToScVal,
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

async function submitWalletTx(
  wallet: WalletSession,
  method: string,
  args: xdr.ScVal[]
): Promise<string | undefined> {
  const cfg = await getConfig();
  const server = new rpc.Server(cfg.rpcUrl, { allowHttp: cfg.rpcUrl.startsWith("http://") });
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
  const sent = await server.sendTransaction(signedTx);
  if (sent.status === "ERROR") {
    throw new Error("On-chain transaction rejected");
  }

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
