"use client";

import { useState } from "react";
import type { SimulationResult, StateChange } from "@/lib/transaction-simulation";
import { diagnoseTransactionFailure } from "@/lib/transaction-diagnostics";
import { useT } from "@/lib/i18n/context";

interface TransactionSimulationProps {
  simulation: SimulationResult;
  onConfirm: () => void;
  onCancel: () => void;
  loading?: boolean;
  /**
   * Buy-in amount in stroops (7-decimal XLM). When provided, the modal
   * shows a buy-in summary (amount, network fee, gas price, total
   * deduction) above the generic simulation details — used for the
   * join-table confirmation flow specifically.
   */
  buyInAmount?: bigint;
}

const STROOPS_PER_XLM = BigInt(10_000_000);

/** Formats a stroop amount (bigint) as an XLM string with up to 7 decimals. */
function formatStroopsAsXlm(stroops: bigint): string {
  const whole = stroops / STROOPS_PER_XLM;
  const remainder = stroops % STROOPS_PER_XLM;
  if (remainder === BigInt(0)) return whole.toString();
  const fractional = remainder.toString().padStart(7, "0").replace(/0+$/, "");
  return `${whole}.${fractional}`;
}

/** Parses a decimal XLM fee string (e.g. "0.0000123") into stroops. */
function parseFeeXlmToStroops(feeXlm: string): bigint {
  const value = Number(feeXlm);
  if (!Number.isFinite(value)) return BigInt(0);
  return BigInt(Math.round(value * Number(STROOPS_PER_XLM)));
}

function StateChangeIcon({ type }: { type: StateChange["type"] }) {
  const iconMap = {
    contract_data: "📋",
    balance: "💰",
    trustline: "🔗",
    account: "👤",
  };
  return <span className="mr-2">{iconMap[type]}</span>;
}

function SimulationStatus({
  success,
  error,
  rawResult,
}: {
  success: boolean;
  error?: string;
  rawResult?: unknown;
}) {
  const t = useT();
  const [showLogs, setShowLogs] = useState(false);

  if (success) {
    return (
      <div className="flex items-center text-green-600 mb-4">
        <span className="mr-2">✅</span>
        <span>Transaction simulation successful</span>
      </div>
    );
  }

  const diag = diagnoseTransactionFailure(rawResult || error);

  return (
    <div className="mb-4">
      <div className="flex items-center text-red-600 mb-2 font-semibold">
        <span className="mr-2">❌</span>
        <span>{t("diagnostics.simulationFailed")}</span>
      </div>

      <div className="border border-red-200 bg-red-50 rounded p-3 mb-2 space-y-2">
        <div className="flex items-center justify-between gap-2 border-b border-red-200 pb-1">
          <span className="font-bold text-xs text-red-800 uppercase tracking-wide">
            {t(`${diag.i18nKey}.title`) || diag.title}
          </span>
          <span className="text-[10px] bg-red-200 text-red-800 font-mono px-1.5 py-0.5 rounded">
            {diag.code}
          </span>
        </div>

        <p className="text-xs text-red-700 leading-relaxed">
          {t(`${diag.i18nKey}.description`) || diag.description}
        </p>

        {diag.commonCauses.length > 0 && (
          <div className="text-xs text-red-800 pt-1">
            <span className="font-semibold block mb-1">
              {t("diagnostics.commonCauses")}:
            </span>
            <ul className="list-disc pl-4 space-y-0.5 text-[11px] text-red-700">
              {diag.commonCauses.map((cause, idx) => (
                <li key={idx}>{cause}</li>
              ))}
            </ul>
          </div>
        )}

        {diag.actionableRemedy && (
          <div className="text-xs text-red-800 bg-red-100 p-2 rounded border border-red-200">
            <span className="font-semibold">{t("diagnostics.suggestedAction")}: </span>
            <span className="text-[11px]">{diag.actionableRemedy}</span>
          </div>
        )}

        <div className="pt-1 flex items-center justify-between text-xs">
          <a
            href={diag.docsUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="text-blue-600 hover:text-blue-800 underline font-medium text-[11px]"
          >
            {t("diagnostics.viewDocs")}
          </a>

          {diag.simulationLogs.length > 0 && (
            <button
              type="button"
              onClick={() => setShowLogs(!showLogs)}
              className="text-[11px] text-gray-600 hover:text-gray-900 underline"
            >
              {showLogs ? t("diagnostics.hideLogs") : t("diagnostics.simulationLogs")} ({diag.simulationLogs.length})
            </button>
          )}
        </div>

        {showLogs && diag.simulationLogs.length > 0 && (
          <div className="mt-2 text-[10px] bg-gray-900 text-green-400 p-2 rounded overflow-auto max-h-36 font-mono">
            {diag.simulationLogs.map((log, i) => (
              <div key={i} className="whitespace-pre-wrap break-all py-0.5 border-b border-gray-800 last:border-0">
                {log}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

/** Renders Transaction Simulation.
 * @param props(object) - props(object).
 * @returns BigInt(0).
 */
export function TransactionSimulation({
  simulation,
  onConfirm,
  onCancel,
  loading = false,
  buyInAmount,
}: TransactionSimulationProps) {
  const [showDetails, setShowDetails] = useState(false);

  const feeStroops = simulation.success ? parseFeeXlmToStroops(simulation.fee) : BigInt(0);
  const gasPricePerInstruction =
    simulation.success && simulation.gasUsed
      ? feeStroops / BigInt(Math.max(1, simulation.gasUsed))
      : null;

  return (
    <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
      <div className="bg-white rounded-lg p-6 max-w-md w-full mx-4 max-h-[80vh] overflow-y-auto">
        <h2 className="text-xl font-bold mb-4">Transaction Preview</h2>

        <SimulationStatus
          success={simulation.success}
          error={simulation.error}
          rawResult={simulation.rawResult}
        />

        {simulation.success && buyInAmount !== undefined && (
          <div className="mb-4">
            <h3 className="font-semibold text-gray-700 mb-2">Buy-in Summary</h3>
            <div className="text-sm space-y-1">
              <div>Buy-in: {formatStroopsAsXlm(buyInAmount)} XLM</div>
              <div>Network fee: {formatStroopsAsXlm(feeStroops)} XLM</div>
              {gasPricePerInstruction !== null && (
                <div>
                  Gas price: {gasPricePerInstruction.toString()} stroops/instruction
                  ({simulation.gasUsed!.toLocaleString()} instructions)
                </div>
              )}
              <div className="font-semibold pt-1 border-t">
                Total deduction: {formatStroopsAsXlm(buyInAmount + feeStroops)} XLM
              </div>
            </div>
          </div>
        )}

        {simulation.success && (
          <>
            <div className="mb-4">
              <h3 className="font-semibold text-gray-700 mb-2">Cost</h3>
              <div className="text-sm">
                <div>Fee: {simulation.fee} XLM</div>
                {simulation.gasUsed && (
                  <div>Gas Used: {simulation.gasUsed.toLocaleString()} instructions</div>
                )}
              </div>
            </div>

            <div className="mb-4">
              <h3 className="font-semibold text-gray-700 mb-2">Expected Changes</h3>
              <div className="space-y-1">
                {simulation.stateChanges.map((change, index) => (
                  <div key={index} className="text-sm flex items-start">
                    <StateChangeIcon type={change.type} />
                    <span>{change.description}</span>
                  </div>
                ))}
              </div>
            </div>

            {simulation.rawResult && (
              <button
                onClick={() => setShowDetails(!showDetails)}
                className="text-xs text-blue-600 hover:text-blue-800 mb-4"
              >
                {showDetails ? "Hide" : "Show"} technical details
              </button>
            )}

            {showDetails && simulation.rawResult && (
              <div className="mb-4 text-xs bg-gray-50 p-2 rounded overflow-auto max-h-32">
                <pre>{JSON.stringify(simulation.rawResult, null, 2)}</pre>
              </div>
            )}
          </>
        )}

        <div className="flex gap-2 pt-4 border-t">
          <button
            onClick={onCancel}
            className="flex-1 px-4 py-2 border border-gray-300 rounded hover:bg-gray-50"
            disabled={loading}
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            disabled={!simulation.success || loading}
            className="flex-1 px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700 disabled:bg-gray-300 disabled:cursor-not-allowed"
          >
            {loading ? "Signing..." : "Sign & Send"}
          </button>
        </div>
      </div>
    </div>
  );
}
