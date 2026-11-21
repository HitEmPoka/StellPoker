/**
 * In-table rules read from a table's on-chain config (#624).
 *
 * `get_table` returns the full `TableConfig` that `contracts/poker-table`
 * enforces. This module reduces that into the handful of rules a player needs
 * to see at the table (blinds, antes, buy-ins, rake, time limits), tagged with
 * the `config_version` it was read at so the popover can tell when the table's
 * rules have changed underneath it.
 */

/** Seconds per Soroban ledger, as the contract's `timeout_ledgers` doc assumes. */
export const LEDGER_SECONDS = 5;

export interface RuleBlindLevel {
  smallBlind: bigint;
  bigBlind: bigint;
  /** Human-readable ante, e.g. "none", "5", "10% of BB". */
  ante: string;
  /** How long this level lasts once active; 0 means it never advances. */
  durationSeconds: number;
  /** Break after this level expires, before the next one starts. */
  breakSeconds: number;
}

export interface RuleActionTimes {
  preflopSeconds: number;
  flopSeconds: number;
  turnSeconds: number;
  riverSeconds: number;
}

export interface TableRules {
  /** The on-chain `config_version` these rules were read at. */
  configVersion: number;
  bettingStructure: string;
  minBuyIn: bigint;
  maxBuyIn: bigint;
  /** Rebuys allowed per session; 0 means unlimited. */
  maxRebuys: number;
  /** Rake in basis points (100 = 1%). */
  rakeBps: number;
  minPlayers: number;
  maxPlayers: number;
  blindLevels: RuleBlindLevel[];
  /** Per-street action limits, or null when the global timeout applies. */
  actionTimes: RuleActionTimes | null;
  /** Global action timeout in seconds, derived from `timeout_ledgers`. */
  timeoutSeconds: number;
}

interface EnumParts {
  tag: string;
  value: unknown;
}

/**
 * `scValToNative` renders a Soroban enum as `[tag, ...payload]` (or a bare
 * tag for unit variants). Accept the object form too, so a change in the
 * SDK's output shape fails soft rather than rendering nonsense.
 */
function enumParts(raw: unknown): EnumParts {
  if (Array.isArray(raw)) return { tag: String(raw[0]), value: raw[1] };
  if (typeof raw === "string") return { tag: raw, value: undefined };
  if (raw && typeof raw === "object") {
    const [tag] = Object.keys(raw);
    if (tag !== undefined) return { tag, value: (raw as Record<string, unknown>)[tag] };
  }
  return { tag: "None", value: undefined };
}

/** Unwraps a Soroban `Option`/`OptionalStreetTimeLimit`, returning null for None. */
function unwrapOption(raw: unknown): unknown {
  const { tag, value } = enumParts(raw);
  if (tag === "Some") return value;
  if (tag === "None") return null;
  return raw ?? null;
}

function toNumber(value: unknown): number {
  if (typeof value === "bigint") return Number(value);
  if (typeof value === "number") return value;
  if (typeof value === "string" && value.trim() !== "") return Number(value);
  return 0;
}

function toBigIntValue(value: unknown): bigint {
  if (typeof value === "bigint") return value;
  if (typeof value === "number" && Number.isFinite(value)) return BigInt(Math.trunc(value));
  if (typeof value === "string" && value.trim() !== "") {
    try {
      return BigInt(value.trim());
    } catch {
      return BigInt(0);
    }
  }
  return BigInt(0);
}

/** Renders an `AnteMode` as player-facing text. */
export function describeAnte(raw: unknown): string {
  const { tag, value } = enumParts(raw);
  if (tag === "Fixed") return toBigIntValue(value).toString();
  if (tag === "Percentage") return `${toNumber(value)}% of BB`;
  return "none";
}

function describeBetting(raw: unknown): string {
  const { tag, value } = enumParts(raw);
  if (tag === "NoLimit") return "No limit";
  if (tag === "PotLimit") return "Pot limit";
  if (tag === "FixedLimit") {
    const cfg = (value ?? {}) as Record<string, unknown>;
    if (cfg.small_bet !== undefined && cfg.big_bet !== undefined) {
      return `Fixed limit ${toBigIntValue(cfg.small_bet)}/${toBigIntValue(cfg.big_bet)}`;
    }
    return "Fixed limit";
  }
  return tag;
}

/**
 * Reduces the `get_table` result (as `scValToNative` returns it) to the rules
 * shown in the popover. Throws when the config is missing, so the caller can
 * show an error rather than a half-empty table of rules.
 */
export function parseTableRules(table: unknown, configVersion: number): TableRules {
  const config = (table as { config?: Record<string, unknown> } | null)?.config;
  if (!config) {
    throw new Error("Table config is missing from the on-chain response");
  }

  const schedule = config.blinds_schedule as { levels?: Array<Record<string, unknown>> } | undefined;
  const blindLevels: RuleBlindLevel[] = (schedule?.levels ?? []).map((level) => ({
    smallBlind: toBigIntValue(level.small_blind),
    bigBlind: toBigIntValue(level.big_blind),
    ante: describeAnte(level.ante),
    durationSeconds: toNumber(level.duration_seconds),
    breakSeconds: toNumber(level.break_seconds),
  }));

  const street = unwrapOption(config.street_time_limit) as Record<string, unknown> | null;
  const actionTimes: RuleActionTimes | null = street
    ? {
        preflopSeconds: toNumber(street.preflop_seconds),
        flopSeconds: toNumber(street.flop_seconds),
        turnSeconds: toNumber(street.turn_seconds),
        riverSeconds: toNumber(street.river_seconds),
      }
    : null;

  return {
    configVersion,
    bettingStructure: describeBetting(config.betting_structure),
    minBuyIn: toBigIntValue(config.min_buy_in),
    maxBuyIn: toBigIntValue(config.max_buy_in),
    maxRebuys: toNumber(config.max_rebuys),
    rakeBps: toNumber(config.rake_bps),
    minPlayers: toNumber(config.min_players),
    maxPlayers: toNumber(config.max_players),
    blindLevels,
    actionTimes,
    timeoutSeconds: toNumber(config.timeout_ledgers) * LEDGER_SECONDS,
  };
}

/** Formats a duration as "45s", "2m 30s" or "1h 5m"; 0 reads as "no limit". */
export function formatSeconds(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "no limit";
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = seconds % 60;
    return s === 0 ? `${m}m` : `${m}m ${s}s`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}

/** Basis points to a percentage string, e.g. 250 → "2.5%". */
export function formatRake(bps: number): string {
  return `${Math.round(bps) / 100}%`;
}
