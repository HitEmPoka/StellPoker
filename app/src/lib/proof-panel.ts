/**
 * Layout preference for the proof explorer side panel (#160): its width and
 * whether it is collapsed, kept in localStorage so the panel reopens the way
 * the player left it.
 */

export interface ProofPanelPrefs {
  width: number;
  collapsed: boolean;
}

/** PROOF PANEL MIN WIDTH */
export const PROOF_PANEL_MIN_WIDTH = 240;
/** PROOF PANEL MAX WIDTH */
export const PROOF_PANEL_MAX_WIDTH = 560;
/** PROOF PANEL DEFAULT WIDTH.
 * @param width - width.
 * @returns number.
 */
export const PROOF_PANEL_DEFAULT_WIDTH = 320;

const STORAGE_KEY = "stellpoker:proof-panel";

const DEFAULT_PREFS: ProofPanelPrefs = {
  width: PROOF_PANEL_DEFAULT_WIDTH,
  collapsed: false,
};

/** Clamp Panel Width.
 * @param width - width.
 * @returns number.
 * @remarks Reads/writes browser storage.
 */
export function clampPanelWidth(width: number): number {
  if (!Number.isFinite(width)) return PROOF_PANEL_DEFAULT_WIDTH;
  return Math.round(Math.min(PROOF_PANEL_MAX_WIDTH, Math.max(PROOF_PANEL_MIN_WIDTH, width)));
}

/** Loads Proof Panel Prefs.
 * @returns ProofPanelPrefs.
 * @remarks Reads/writes browser storage.
 */
export function loadProofPanelPrefs(): ProofPanelPrefs {
  if (typeof window === "undefined") return DEFAULT_PREFS;
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_PREFS;
    const parsed = JSON.parse(raw) as Partial<ProofPanelPrefs>;
    return {
      width:
        typeof parsed.width === "number" ? clampPanelWidth(parsed.width) : DEFAULT_PREFS.width,
      collapsed: parsed.collapsed === true,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

/** Persists Proof Panel Prefs.
 * @param prefs - prefs.
 * @remarks Reads/writes browser storage.
 */
export function saveProofPanelPrefs(prefs: ProofPanelPrefs): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Storage unavailable (private browsing, quota) — the panel just reopens
    // at its default size.
  }
}
