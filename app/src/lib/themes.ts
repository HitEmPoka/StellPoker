import themesData from "./themes.json";
import {
  themeToCssVariables,
  validateThemes,
  type TableTheme,
} from "./theme-contract";

export {
  THEME_COLOR_FIELDS,
  THEME_CONTRACT,
  THEME_CSS_VARIABLES,
  themeToCssVariables,
  validateTheme,
  validateThemes,
  type TableTheme,
  type ThemeColorField,
  type ThemeValidationResult,
} from "./theme-contract";

/** THEME STORAGE KEY */
export const THEME_STORAGE_KEY = "stellpoker-theme";

const builtInThemes: unknown[] = themesData;
const builtInValidation = validateThemes(builtInThemes);
if (!builtInValidation.valid) {
  throw new Error(
    `Invalid built-in themes: ${builtInValidation.errors.join("; ")}`
  );
}

/** THEMES */
export const THEMES: TableTheme[] = themesData as TableTheme[];

/** Loads Theme.
 * @returns TableTheme.
 * @remarks Reads/writes browser storage.
 */
export function loadTheme(): TableTheme {
  if (typeof window === "undefined") return THEMES[0];
  try {
    const saved = localStorage.getItem(THEME_STORAGE_KEY);
    if (saved) {
      const parsed = JSON.parse(saved);
      const match = THEMES.find((t) => t.id === parsed.id || t.id === parsed);
      if (match) return match;
    }
  } catch { /* ignore */ }
  return THEMES[0];
}

/** Persists Theme.
 * @param theme - theme.
 * @remarks Reads/writes browser storage.
 */
export function saveTheme(theme: TableTheme): void {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem(THEME_STORAGE_KEY, JSON.stringify(theme.id));
  } catch { /* ignore */ }
}

/** Apply Theme.
 * @param theme - theme.
 */
export function applyTheme(theme: TableTheme): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  const variables = themeToCssVariables(theme);
  for (const [name, value] of Object.entries(variables)) {
    root.style.setProperty(name, value);
  }
}
