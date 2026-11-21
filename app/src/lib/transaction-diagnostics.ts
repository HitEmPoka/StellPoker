/**
 * Transaction failure deep diagnostics (Issue #619).
 *
 * Maps opaque wallet, Soroban RPC simulation, and network submission errors
 * to structured diagnostic information, human-readable explanations,
 * common causes, actionable remedies, official documentation links,
 * and extracted simulation/diagnostic logs.
 */

export type DiagnosticFailureCode =
  | "insufficient_balance"
  | "timeout"
  | "seq"
  | "expired"
  | "budget"
  | "unknown";

export interface TransactionDiagnostic {
  /** Machine-readable failure code */
  code: DiagnosticFailureCode;
  /** Human-readable headline */
  title: string;
  /** Comprehensive explanation of what went wrong */
  description: string;
  /** Likely reasons why this failure occurred */
  commonCauses: string[];
  /** Concrete next step for the user to resolve the issue */
  actionableRemedy: string;
  /** Official Stellar/Soroban developer docs reference */
  docsUrl: string;
  /** i18n translation key prefix */
  i18nKey: string;
  /** Extracted simulation or RPC error logs, if available */
  simulationLogs: string[];
  /** Raw error message string */
  rawError: string;
}

export const DIAGNOSTIC_METADATA: Record<
  DiagnosticFailureCode,
  {
    title: string;
    description: string;
    commonCauses: string[];
    actionableRemedy: string;
    docsUrl: string;
  }
> = {
  insufficient_balance: {
    title: "Insufficient Balance",
    description:
      "The wallet account has insufficient XLM to satisfy the table buy-in plus the network fee and minimum base reserve.",
    commonCauses: [
      "Available XLM balance is below the requested buy-in amount.",
      "The transaction fee or temporary Soroban storage deposit would breach the minimum account reserve (0.5 - 1.0 XLM).",
      "Funds are locked in active orders, open trustlines, or other active game tables.",
    ],
    actionableRemedy:
      "Deposit additional XLM into your connected wallet to cover the table buy-in and reserve requirements.",
    docsUrl:
      "https://developers.stellar.org/docs/learn/fundamentals/fees-resource-limits-metering#base-reserves",
  },
  timeout: {
    title: "Transaction Timeout",
    description:
      "The transaction or RPC simulation request timed out before the network could process and confirm it.",
    commonCauses: [
      "Temporary network congestion or elevated transaction volume on the Stellar network.",
      "RPC provider latency or transient connectivity interruptions between browser and node.",
      "User wallet popup was left unconfirmed beyond the submission deadline.",
    ],
    actionableRemedy:
      "Verify your internet connection and retry the transaction. Your wallet funds were not deducted.",
    docsUrl:
      "https://developers.stellar.org/docs/learn/fundamentals/transactions/operations-and-transactions#transaction-lifecycle",
  },
  seq: {
    title: "Sequence Number Mismatch",
    description:
      "The transaction sequence number does not match the current account sequence registered on-chain.",
    commonCauses: [
      "Another transaction was submitted or signed concurrently from this wallet (e.g. from another tab or mobile app).",
      "A previous transaction failed after incrementing the account sequence.",
      "The local client cached an outdated ledger sequence for this account.",
    ],
    actionableRemedy:
      "Refresh the table session to synchronize your current on-chain account sequence number, then retry.",
    docsUrl:
      "https://developers.stellar.org/docs/learn/fundamentals/transactions/operations-and-transactions#sequence-number",
  },
  expired: {
    title: "Transaction Expired",
    description:
      "The transaction exceeded its specified time bounds or ledger sequence bounds before being included in a block.",
    commonCauses: [
      "Signing the transaction in the wallet extension took longer than the validity window.",
      "The transaction remained queued in the mempool beyond its maximum ledger sequence bounds.",
    ],
    actionableRemedy:
      "Submit a fresh transaction with renewed ledger validity bounds.",
    docsUrl:
      "https://developers.stellar.org/docs/learn/fundamentals/transactions/operations-and-transactions#time-bounds",
  },
  budget: {
    title: "Resource Budget Exceeded",
    description:
      "The contract invocation exceeded the Soroban CPU instruction or memory consumption budget.",
    commonCauses: [
      "Proof verification or multi-party computation operations required more CPU instructions than budgeted.",
      "Ledger footprint or transient storage footprint exceeded maximum allowable bounds.",
      "Simulated resource allocation was lower than the actual execution requirements.",
    ],
    actionableRemedy:
      "Review the betting or join parameters, or wait for automatic network budget adjustments before retrying.",
    docsUrl:
      "https://developers.stellar.org/docs/learn/fundamentals/fees-resource-limits-metering#resource-limits",
  },
  unknown: {
    title: "Transaction Error",
    description:
      "An unexpected error or contract panic occurred during transaction simulation or submission.",
    commonCauses: [
      "Contract logic reverted with an unclassified abort code.",
      "Wallet rejected the signing request or closed unexpectedly.",
    ],
    actionableRemedy:
      "Review the simulation logs below or retry the operation after checking wallet connectivity.",
    docsUrl:
      "https://developers.stellar.org/docs/learn/fundamentals/transactions/operations-and-transactions",
  },
};

/**
 * Classifies an error string or error payload into a recognized DiagnosticFailureCode.
 */
export function classifyFailureCode(text: string): DiagnosticFailureCode {
  const lower = text.toLowerCase();

  // 1. Insufficient balance checks
  if (
    lower.includes("insufficient") ||
    lower.includes("underfunded") ||
    lower.includes("op_underfunded") ||
    lower.includes("tx_insufficient_balance") ||
    lower.includes("low_reserve") ||
    lower.includes("balance too low") ||
    lower.includes("not enough xlm")
  ) {
    return "insufficient_balance";
  }

  // 2. Budget exceeded checks (check before timeout/expired to catch cpu_limit etc)
  if (
    lower.includes("budget") ||
    lower.includes("cpu_limit") ||
    lower.includes("memory_limit") ||
    lower.includes("exceeded the budget") ||
    lower.includes("resource limit") ||
    lower.includes("instructions limit") ||
    lower.includes("storage limit")
  ) {
    return "budget";
  }

  // 3. Expired bounds checks (check before seq to properly catch 'ledger sequence expired')
  if (
    lower.includes("expired") ||
    lower.includes("tx_expired") ||
    lower.includes("time bounds") ||
    lower.includes("ledger bounds") ||
    lower.includes("ledger sequence expired")
  ) {
    return "expired";
  }

  // 4. Sequence mismatch checks
  if (
    lower.includes("seq") ||
    lower.includes("bad_seq") ||
    lower.includes("sequence") ||
    lower.includes("tx_bad_seq") ||
    lower.includes("bad sequence") ||
    lower.includes("sequence mismatch")
  ) {
    return "seq";
  }

  // 5. Timeout checks
  if (
    lower.includes("timeout") ||
    lower.includes("timed out") ||
    lower.includes("tx_timeout") ||
    lower.includes("deadline exceeded") ||
    lower.includes("network error") ||
    lower.includes("gateway timeout")
  ) {
    return "timeout";
  }

  return "unknown";
}

/**
 * Extracts simulation logs, diagnostic events, and diagnostic strings from raw response.
 */
export function extractSimulationLogs(raw: unknown): string[] {
  if (!raw) return [];
  const logs: string[] = [];

  if (typeof raw === "string") {
    return [raw];
  }

  if (typeof raw === "object") {
    const obj = raw as Record<string, unknown>;

    // Soroban simulation diagnostic events
    if (Array.isArray(obj.diagnosticEvents)) {
      for (const ev of obj.diagnosticEvents) {
        if (typeof ev === "string") logs.push(ev);
        else if (ev && typeof ev === "object") {
          logs.push(JSON.stringify(ev));
        }
      }
    }

    // Soroban simulation events
    if (Array.isArray(obj.events)) {
      for (const ev of obj.events) {
        if (typeof ev === "string") logs.push(ev);
        else if (ev && typeof ev === "object") {
          logs.push(JSON.stringify(ev));
        }
      }
    }

    // Simulation error field
    if (typeof obj.error === "string") {
      logs.push(obj.error);
    }

    // Simulation results[0].error or results[0].xdr
    if (Array.isArray(obj.results) && obj.results.length > 0) {
      for (const res of obj.results) {
        if (res && typeof res === "object") {
          const r = res as Record<string, unknown>;
          if (r.error) logs.push(`Result Error: ${String(r.error)}`);
          if (r.xdr) logs.push(`Result XDR: ${String(r.xdr)}`);
        }
      }
    }
  }

  return logs;
}

/**
 * Produces deep transaction failure diagnostics from an error, simulation result, or failure code.
 */
export function diagnoseTransactionFailure(input: unknown): TransactionDiagnostic {
  let rawText = "";
  let logs: string[] = [];

  if (input instanceof Error) {
    rawText = input.message;
    if (input.stack) logs.push(input.stack);
  } else if (typeof input === "string") {
    rawText = input;
    logs.push(input);
  } else if (input && typeof input === "object") {
    const obj = input as Record<string, unknown>;
    rawText =
      typeof obj.error === "string"
        ? obj.error
        : typeof obj.message === "string"
          ? obj.message
          : JSON.stringify(obj);

    logs = extractSimulationLogs(obj.rawResult || input);
    if (logs.length === 0 && rawText) {
      logs.push(rawText);
    }
  } else {
    rawText = String(input ?? "Unknown error");
    logs = [rawText];
  }

  const code = classifyFailureCode(rawText);
  const meta = DIAGNOSTIC_METADATA[code];

  return {
    code,
    title: meta.title,
    description: meta.description,
    commonCauses: [...meta.commonCauses],
    actionableRemedy: meta.actionableRemedy,
    docsUrl: meta.docsUrl,
    i18nKey: `diagnostics.${code}`,
    simulationLogs: logs,
    rawError: rawText,
  };
}
