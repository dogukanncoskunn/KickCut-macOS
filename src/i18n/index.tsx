import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { en } from "./en";
import type { MessageKey, Messages } from "./en";
import { tr } from "./tr";
import { de } from "./de";

export const LOCALES = ["tr", "en", "de"] as const;
export type Locale = (typeof LOCALES)[number];

export const LOCALE_NAMES: Record<Locale, string> = {
  tr: "Türkçe",
  en: "English",
  de: "Deutsch",
};

const CATALOGS: Record<Locale, Messages> = { tr, en, de };

/*
 * Kept in localStorage because the first paint cannot wait on anything async -
 * a flash of the wrong language on every launch is worse than the alternative.
 *
 * That does mean it lives in the webview profile rather than with the job
 * records, so it is per-machine and a reinstall that clears the profile loses
 * it. The same is true of the theme and the output folder. Worth moving into
 * the app data directory one day; not worth it for a language that is one
 * click to set again.
 */
const STORAGE_KEY = "kickcut.locale";

function initialLocale(): Locale {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved && (LOCALES as readonly string[]).includes(saved)) return saved as Locale;
  const nav = navigator.language.slice(0, 2).toLowerCase();
  return (LOCALES as readonly string[]).includes(nav) ? (nav as Locale) : "tr";
}

type Translate = (key: MessageKey, vars?: Record<string, string | number>) => string;

const Ctx = createContext<{ locale: Locale; setLocale: (l: Locale) => void; t: Translate } | null>(null);

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(initialLocale);

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  const setLocale = useCallback((next: Locale) => {
    localStorage.setItem(STORAGE_KEY, next);
    setLocaleState(next);
  }, []);

  const t = useCallback<Translate>(
    (key, vars) => {
      // Fall back to English rather than showing a raw key: a missing German
      // string should still leave the screen usable.
      const raw = CATALOGS[locale][key] ?? en[key] ?? key;
      if (!vars) return raw;
      return raw.replace(/\{(\w+)\}/g, (whole, name: string) =>
        name in vars ? String(vars[name]) : whole,
      );
    },
    [locale],
  );

  const value = useMemo(() => ({ locale, setLocale, t }), [locale, setLocale, t]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useLocale() {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("useLocale must be used inside <LocaleProvider>");
  return ctx;
}

export function useT(): Translate {
  return useLocale().t;
}
