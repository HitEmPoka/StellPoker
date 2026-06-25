/**
 * Feature flag client for the StellPoker frontend.
 *
 * Fetches the current flag snapshot from the coordinator's `GET /api/flags`
 * endpoint and exposes a typed helper for checking individual flags.
 *
 * ## Available flags
 * | Key                 | Purpose                                      |
 * |---------------------|----------------------------------------------|
 * | `new_circuits`      | Enable experimental ZK circuit versions      |
 * | `contract_upgrade`  | Gate new Soroban contract function calls     |
 * | `experimental_ui`   | Signal UI to render next-gen components      |
 * | `chat_enabled`      | Show / hide the in-table chat panel          |
 * | `solo_mode`         | Allow solo / bot-opponent table creation     |
 */

import { COORDINATOR_API_BASE } from "./api";

/** Shape returned by `GET /api/flags`. */
export interface FeatureFlags {
  new_circuits: boolean;
  contract_upgrade: boolean;
  experimental_ui: boolean;
  chat_enabled: boolean;
  solo_mode: boolean;
  /** Any extra scoped overrides (e.g. `chat_enabled.table.3`) are included. */
  [key: string]: boolean;
}

/** All-false defaults used while the real values are loading. */
export const DEFAULT_FLAGS: FeatureFlags = {
  new_circuits: false,
  contract_upgrade: false,
  experimental_ui: false,
  chat_enabled: false,
  solo_mode: false,
};

/**
 * Fetches the current feature-flag snapshot from the coordinator.
 *
 * @throws {Error} when the coordinator returns a non-OK response.
 */
export async function fetchFeatureFlags(): Promise<FeatureFlags> {
  const res = await fetch(`${COORDINATOR_API_BASE}/api/flags`);
  if (!res.ok) {
    throw new Error(`Failed to fetch feature flags: ${res.status}`);
  }
  const raw = (await res.json()) as Record<string, boolean>;
  // Merge with defaults so that any flag missing from the server response is
  // treated as false rather than undefined.
  return { ...DEFAULT_FLAGS, ...raw };
}

/**
 * Check whether a flag is enabled in a previously fetched snapshot.
 *
 * @param flags   — the snapshot returned by `fetchFeatureFlags()`
 * @param flagKey — the flag key to check (e.g. `"solo_mode"`)
 */
export function isFlagEnabled(
  flags: FeatureFlags | null | undefined,
  flagKey: keyof FeatureFlags | string
): boolean {
  if (!flags) return false;
  return flags[flagKey] === true;
}
