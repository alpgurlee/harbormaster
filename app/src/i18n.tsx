import { createContext, Fragment, useContext, useEffect, useMemo, type ReactNode } from "react";

import { en, tr, type Messages, type Plural } from "./messages";

export type Locale = "en" | "tr";

const catalogs: Record<Locale, Messages> = { en, tr };

type Values = Record<string, string | number>;
type PluralKey = { [K in keyof Messages]: Messages[K] extends Plural ? K : never }[keyof Messages];
export type TextKey = Exclude<keyof Messages, PluralKey>;

const MIB = 1024 * 1024;
const GIB = 1024 * MIB;
const PLACEHOLDER = /\{(\w+)\}/;

/** Messages and numbers the way `locale` writes them. */
export function translator(locale: Locale) {
  const messages = catalogs[locale];
  const plurals = new Intl.PluralRules(locale);
  const tenths = new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const whole = new Intl.NumberFormat(locale, { maximumFractionDigits: 0 });
  const percent = (digits: number) =>
    new Intl.NumberFormat(locale, { style: "percent", minimumFractionDigits: digits, maximumFractionDigits: digits });
  const conjunction = new Intl.ListFormat(locale, { type: "conjunction" });
  const clock = new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" });
  const relative = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });

  const template = (key: keyof Messages, values: Values): string => {
    const message = messages[key];
    if (typeof message === "string") return message;
    return plurals.select(Number(values.count)) === "one" ? message.one : message.other;
  };
  // A split on a capturing pattern puts the placeholder names at odd places.
  const parts = (key: keyof Messages, values: Values) => template(key, values).split(PLACEHOLDER);
  const fill = (key: keyof Messages, values: Values) =>
    parts(key, values)
      .map((part, index) => (index % 2 === 1 ? String(values[part] ?? "") : part))
      .join("");
  const t = (key: TextKey, values: Values = {}) => fill(key, values);

  return {
    locale,
    t,
    plural(key: PluralKey, count: number, values: Values = {}) {
      return fill(key, { count, ...values });
    },
    /** A message as nodes with its values handed to `mark`, as the header sets figures apart. */
    marked(key: keyof Messages, values: Values, mark: (value: string) => ReactNode): ReactNode[] {
      return parts(key, values).map((part, index) =>
        index % 2 === 1 ? <Fragment key={index}>{mark(String(values[part] ?? ""))}</Fragment> : part,
      );
    },
    bytes(bytes: number) {
      return bytes >= GIB ? `${tenths.format(bytes / GIB)} GB` : `${whole.format(bytes / MIB)} MB`;
    },
    /** `14.9 of 32 GB`: a machine's total is usually whole. */
    memoryOf(used: number, total: number) {
      const size = Math.round((total / GIB) * 10) / 10;
      const totalText = Number.isInteger(size) ? whole.format(size) : tenths.format(size);
      return t("memoryOf", { used: tenths.format(used / GIB), total: `${totalText} GB` });
    },
    percent(value: number, digits = 0) {
      return percent(digits).format(value / 100);
    },
    /** `a, b and c`, the way the language joins them. */
    list(items: string[]) {
      return conjunction.format(items);
    },
    time(milliseconds: number) {
      return clock.format(milliseconds);
    },
    /** `yesterday`, `12 days ago`, then months: how long a project has sat. */
    ago(milliseconds: number) {
      const days = Math.max(0, Math.floor(milliseconds / 86_400_000));
      return days < 60 ? relative.format(-days, "day") : relative.format(-Math.floor(days / 30), "month");
    },
    /** The two largest units, as a glance needs. */
    uptime(seconds: number) {
      const minutes = Math.floor(seconds / 60);
      const days = Math.floor(minutes / 1440);
      const hours = Math.floor(minutes / 60) % 24;
      if (days > 0) return t("days", { days, hours });
      if (hours > 0) return t("hours", { hours, minutes: minutes % 60 });
      if (minutes > 0) return t("minutes", { minutes });
      return t("underAMinute");
    },
  };
}

export type Translator = ReturnType<typeof translator>;

export function localeOf(language: string): Locale {
  return language.toLowerCase().startsWith("tr") ? "tr" : "en";
}

const I18n = createContext<Translator>(translator("en"));

export function I18nProvider({ locale, children }: { locale: Locale; children: ReactNode }) {
  const value = useMemo(() => translator(locale), [locale]);
  // Screen readers and hyphenation go by the document's language.
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);
  return <I18n.Provider value={value}>{children}</I18n.Provider>;
}

export function useI18n(): Translator {
  return useContext(I18n);
}
