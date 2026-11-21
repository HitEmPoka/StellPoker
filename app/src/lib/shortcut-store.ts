export type ShortcutAction = "fold" | "check" | "bet" | "raise" | "allin";
export type ShortcutMap = Record<ShortcutAction, string>;

export const DEFAULT_SHORTCUTS: ShortcutMap = {
  fold: "f",
  check: "c",
  bet: "b",
  raise: "r",
  allin: "a",
};

const STORAGE_KEY = "stellpoker:shortcuts";

export function loadShortcuts(): ShortcutMap {
  if (typeof window === "undefined") return { ...DEFAULT_SHORTCUTS };
  try {
    const parsed = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "null") as Partial<ShortcutMap> | null;
    return Object.fromEntries(Object.keys(DEFAULT_SHORTCUTS).map((action) => [
      action,
      typeof parsed?.[action as ShortcutAction] === "string" ? parsed[action as ShortcutAction]!.toLowerCase() : DEFAULT_SHORTCUTS[action as ShortcutAction],
    ])) as ShortcutMap;
  } catch {
    return { ...DEFAULT_SHORTCUTS };
  }
}

export function saveShortcuts(shortcuts: ShortcutMap): void {
  if (typeof window !== "undefined") window.localStorage.setItem(STORAGE_KEY, JSON.stringify(shortcuts));
}
