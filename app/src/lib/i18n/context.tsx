"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  type Locale,
  type MessageKey,
  resolveLocale,
  storeLocale,
  translate,
  DEFAULT_LOCALE,
  isRtlLocale,
  getDirection,
} from "./index";

interface I18nContextValue {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: (key: MessageKey | string, vars?: Record<string, string | number>) => string;
  isRtl: boolean;
  dir: "rtl" | "ltr";
}

const I18nContext = createContext<I18nContextValue>({
  locale: DEFAULT_LOCALE,
  setLocale: () => {},
  t: (key, vars) => translate(DEFAULT_LOCALE, key, vars),
  isRtl: isRtlLocale(DEFAULT_LOCALE),
  dir: getDirection(DEFAULT_LOCALE),
});

/** Renders I18n Provider.
 * @param props(object) - props(object).
 */
export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(DEFAULT_LOCALE);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    setLocaleState(resolveLocale());
    setReady(true);
  }, []);

  useEffect(() => {
    if (!ready) return;
    if (typeof document !== "undefined") {
      document.documentElement.lang = locale;
      document.documentElement.dir = getDirection(locale);
    }
  }, [locale, ready]);

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next);
    storeLocale(next);
  }, []);

  const t = useCallback(
    (key: MessageKey | string, vars?: Record<string, string | number>) =>
      translate(locale, key, vars),
    [locale]
  );

  const isRtl = useMemo(() => isRtlLocale(locale), [locale]);
  const dir = useMemo(() => getDirection(locale), [locale]);

  const value = useMemo(
    () => ({ locale, setLocale, t, isRtl, dir }),
    [locale, setLocale, t, isRtl, dir]
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

/** React hook that manages use I18n.
 * @returns I18nContextValue.
 */
export function useI18n(): I18nContextValue {
  return useContext(I18nContext);
}

/** React hook that manages use T */
export function useT() {
  return useI18n().t;
}
