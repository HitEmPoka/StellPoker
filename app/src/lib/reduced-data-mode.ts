const REDUCED_DATA_KEY = "stellpoker-reduced-data";

interface NavigatorWithConnection extends Navigator {
  connection?: { saveData?: boolean };
}

/** Returns the explicit preference, falling back to the browser Save-Data hint. */
export function isReducedDataMode(): boolean {
  if (typeof window === "undefined") return false;
  try {
    const stored = window.localStorage.getItem(REDUCED_DATA_KEY);
    if (stored !== null) return stored === "true";
  } catch {
    // Fall back to the browser hint when storage is unavailable.
  }
  return Boolean((navigator as NavigatorWithConnection).connection?.saveData);
}

/** Persists the user's reduced-data preference. */
export function setReducedDataMode(enabled: boolean): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(REDUCED_DATA_KEY, String(enabled));
  } catch {
    // A non-persistent preference is still useful for the current render.
  }
}