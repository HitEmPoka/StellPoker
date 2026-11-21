import { horizonBase } from "./replay";

/** Fee a transaction was charged, from Horizon. */
export interface TransactionFee {
  /** Fee charged in stroops (1 XLM = 10,000,000 stroops). */
  feeStroops: number;
}

/**
 * Looks up the fee a proof transaction was charged (#625). Horizon reports
 * `fee_charged` for Soroban invocations, which is the closest figure the
 * frontend can get without re-simulating the transaction, so the proof
 * drill-down presents it as the gas used.
 *
 * Resolves to null on any failure (missing tx, network error, bad payload):
 * the drill-down just omits the fee rather than blocking on it.
 */
export async function fetchTransactionFee(
  hash: string,
  signal?: AbortSignal
): Promise<TransactionFee | null> {
  try {
    const res = await fetch(`${horizonBase()}/transactions/${encodeURIComponent(hash)}`, {
      signal,
    });
    if (!res.ok) return null;
    const json = (await res.json()) as { fee_charged?: string };
    const feeStroops = Number(json.fee_charged);
    if (!Number.isFinite(feeStroops)) return null;
    return { feeStroops };
  } catch {
    return null;
  }
}

/** Formats stroops as XLM with up to 7 decimals, trailing zeros trimmed. */
export function formatStroopsAsXlm(stroops: number): string {
  return `${(stroops / 10_000_000).toFixed(7).replace(/\.?0+$/, "")} XLM`;
}
