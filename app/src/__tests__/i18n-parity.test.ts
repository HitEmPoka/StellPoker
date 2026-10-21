import { describe, it, expect } from "vitest";
import en from "../lib/i18n/messages/en.json";
import es from "../lib/i18n/messages/es.json";
import de from "../lib/i18n/messages/de.json";
import fr from "../lib/i18n/messages/fr.json";
import ja from "../lib/i18n/messages/ja.json";
import zh from "../lib/i18n/messages/zh.json";
import ar from "../lib/i18n/messages/ar.json";
import { LOCALES, type Locale } from "../lib/i18n";

function getLeafKeys(obj: unknown, prefix = ""): string[] {
  if (obj === null || typeof obj !== "object" || Array.isArray(obj)) {
    return prefix ? [prefix] : [];
  }
  const keys: string[] = [];
  for (const [key, value] of Object.entries(obj as Record<string, unknown>)) {
    const fullKey = prefix ? `${prefix}.${key}` : key;
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      keys.push(...getLeafKeys(value, fullKey));
    } else {
      keys.push(fullKey);
    }
  }
  return keys.sort();
}

function extractPlaceholders(text: string): string[] {
  const matches = text.match(/\{[a-zA-Z0-9_-]+\}/g);
  return matches ? matches.sort() : [];
}

function getValueByPath(obj: unknown, path: string): unknown {
  const parts = path.split(".");
  let cur: unknown = obj;
  for (const p of parts) {
    if (cur == null || typeof cur !== "object") return undefined;
    cur = (cur as Record<string, unknown>)[p];
  }
  return cur;
}

const dictionaries: Record<Locale, Record<string, unknown>> = {
  en,
  es,
  de,
  fr,
  ja,
  zh,
  ar,
};

describe("i18n Key Parity & Completeness Gate (Issue #620)", () => {
  const baseKeys = getLeafKeys(en);

  it("registers all locales in LOCALES", () => {
    const supportedLocales: Locale[] = ["en", "es", "de", "fr", "ja", "zh", "ar"];
    for (const locale of supportedLocales) {
      expect(LOCALES).toContain(locale);
    }
  });

  for (const [locale, dict] of Object.entries(dictionaries)) {
    if (locale === "en") continue;

    it(`enforces 100% key parity for ${locale}.json against en.json`, () => {
      const targetKeys = getLeafKeys(dict);

      const missingKeys = baseKeys.filter((k) => !targetKeys.includes(k));
      const extraKeys = targetKeys.filter((k) => !baseKeys.includes(k));

      expect(missingKeys, `Missing keys in ${locale}.json`).toEqual([]);
      expect(extraKeys, `Unexpected extra keys in ${locale}.json`).toEqual([]);
      expect(targetKeys.length).toBe(baseKeys.length);
    });

    it(`preserves interpolation placeholders for ${locale}.json`, () => {
      for (const key of baseKeys) {
        const enVal = getValueByPath(en, key);
        const targetVal = getValueByPath(dict, key);

        if (typeof enVal === "string" && typeof targetVal === "string") {
          const enPlaceholders = extractPlaceholders(enVal);
          const targetPlaceholders = extractPlaceholders(targetVal);

          expect(
            targetPlaceholders,
            `Placeholders mismatch in ${locale}.json for key "${key}"`
          ).toEqual(enPlaceholders);
        }
      }
    });
  }
});
