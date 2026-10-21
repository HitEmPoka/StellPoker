import { useEffect, useState } from "react";

/**
 * Returns `value` after it has stopped changing for `delayMs`.
 *
 * The lobby search re-filters every open table and reads an alias per seat on
 * each keystroke, so filtering on the raw input value makes typing feel
 * heavy. Debouncing keeps the input itself perfectly responsive (it stays
 * controlled by the raw state) while the expensive filter runs once the
 * player pauses.
 */
export function useDebouncedValue<T>(value: T, delayMs = 250): T {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);

  return debounced;
}
