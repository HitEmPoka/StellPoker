/**
 * Lightweight i18n (Issues #60, #620, #621).
 * English, Spanish, German, French, Japanese, Chinese, and Arabic dictionaries,
 * browser language auto-detect, manual override, RTL support.
 * Avoids next-intl dependency to keep the frontend dep-light.
 */

import en from "./messages/en.json";
import es from "./messages/es.json";
import de from "./messages/de.json";
import fr from "./messages/fr.json";
import ja from "./messages/ja.json";
import zh from "./messages/zh.json";
import ar from "./messages/ar.json";

/** Locale shape. */
export type Locale = "en" | "es" | "de" | "fr" | "ja" | "zh" | "ar";

/** LOCALES */
export const LOCALES: Locale[] = ["en", "es", "de", "fr", "ja", "zh", "ar"];

/** RTL Locales */
export const RTL_LOCALES: Locale[] = ["ar"];

/** Returns whether a locale is Right-To-Left (RTL). */
export function isRtlLocale(locale: Locale): boolean {
  return RTL_LOCALES.includes(locale);
}

/** Returns the text direction ('rtl' | 'ltr') for a locale. */
export function getDirection(locale: Locale): "rtl" | "ltr" {
  return isRtlLocale(locale) ? "rtl" : "ltr";
}

/** DEFAULT LOCALE */
export const DEFAULT_LOCALE: Locale = "en";
const STORAGE_KEY = "stellpoker.locale";

type Dict = typeof en;
const dictionaries: Record<Locale, Dict> = { en, es, de, fr, ja, zh, ar };

type NestedKeyOf<T, Prefix extends string = ""> = T extends object
  ? {
      [K in keyof T & string]: T[K] extends object
        ? NestedKeyOf<T[K], `${Prefix}${K}.`>
        : `${Prefix}${K}`;
    }[keyof T & string]
  : never;

/** Message Key shape. */
export type MessageKey = NestedKeyOf<Dict>;

function getByPath(obj: unknown, path: string): string | undefined {
  const parts = path.split(".");
  let cur: unknown = obj;
  for (const p of parts) {
    if (cur == null || typeof cur !== "object") return undefined;
    cur = (cur as Record<string, unknown>)[p];
  }
  return typeof cur === "string" ? cur : undefined;
}

/** Resolve a dotted key with optional `{var}` interpolation. */
export function translate(
  locale: Locale,
  key: MessageKey | string,
  vars?: Record<string, string | number>
): string {
  const dict = dictionaries[locale] ?? dictionaries.en;
  let text =
    getByPath(dict, key) ??
    getByPath(dictionaries.en, key) ??
    key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      text = text.replaceAll(`{${k}}`, String(v));
    }
  }
  return text;
}

/** Detect browser language; falls back to English. */
export function detectBrowserLocale(): Locale {
  if (typeof navigator === "undefined") return DEFAULT_LOCALE;
  const candidates = [
    navigator.language,
    ...(navigator.languages ?? []),
  ]
    .filter(Boolean)
    .map((l) => l.toLowerCase().slice(0, 2));
  for (const c of candidates) {
    if (LOCALES.includes(c as Locale)) return c as Locale;
  }
  return DEFAULT_LOCALE;
}

/** Loads Stored Locale.
 * @returns Locale | null.
 * @remarks Reads/writes browser storage.
 */
export function loadStoredLocale(): Locale | null {
  if (typeof window === "undefined") return null;
  try {
    const v = window.localStorage.getItem(STORAGE_KEY);
    if (v && LOCALES.includes(v as Locale)) return v as Locale;
  } catch {
    /* ignore */
  }
  return null;
}

/** Persists Locale.
 * @param locale - locale.
 * @remarks Reads/writes browser storage.
 */
export function storeLocale(locale: Locale): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    /* ignore */
  }
}

/** Resolved locale: manual override > stored > browser > default. */
export function resolveLocale(override?: Locale | null): Locale {
  if (override && LOCALES.includes(override)) return override;
  return loadStoredLocale() ?? detectBrowserLocale();
}

export { en, es, de, fr, ja, zh, ar };
