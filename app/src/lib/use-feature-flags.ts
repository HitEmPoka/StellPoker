/**
 * React hook for consuming feature flags in Next.js components.
 *
 * Fetches the flag snapshot once on mount from `GET /api/flags`.
 * Components can gate entire UI sections behind flag checks without
 * prop-drilling.
 *
 * @example
 * ```tsx
 * const { flags, loading } = useFeatureFlags();
 *
 * if (!loading && isFlagEnabled(flags, "solo_mode")) {
 *   // render solo-mode button
 * }
 * ```
 */

"use client";

import { useState, useEffect, useCallback } from "react";
import {
  fetchFeatureFlags,
  isFlagEnabled,
  DEFAULT_FLAGS,
  type FeatureFlags,
} from "./feature-flags";

export { isFlagEnabled };

export interface UseFeatureFlagsResult {
  /** Current flag snapshot. Falls back to all-false defaults while loading. */
  flags: FeatureFlags;
  /** `true` while the initial fetch is in progress. */
  loading: boolean;
  /** Non-null if the fetch failed. */
  error: Error | null;
  /** Re-fetches the flags from the coordinator. */
  refresh: () => void;
}

/**
 * Fetches and caches the feature-flag snapshot for the lifetime of the
 * component tree that calls this hook.
 *
 * @param pollIntervalMs - Optional polling interval in milliseconds.
 *   When provided, flags are re-fetched on that cadence so that runtime
 *   overrides propagate to the UI automatically. Defaults to `0` (no polling).
 */
export function useFeatureFlags(pollIntervalMs = 0): UseFeatureFlagsResult {
  const [flags, setFlags] = useState<FeatureFlags>(DEFAULT_FLAGS);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  const load = useCallback(async () => {
    try {
      const fetched = await fetchFeatureFlags();
      setFlags(fetched);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err : new Error(String(err)));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();

    if (pollIntervalMs > 0) {
      const id = setInterval(() => {
        void load();
      }, pollIntervalMs);
      return () => clearInterval(id);
    }
  }, [load, pollIntervalMs]);

  return { flags, loading, error, refresh: load };
}
