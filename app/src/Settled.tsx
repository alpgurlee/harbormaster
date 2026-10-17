import { useEffect, useState, type ReactNode } from "react";

import { inApp, settingsSubscribe } from "./api";
import { I18nProvider, localeOf, type Locale } from "./i18n";
import type { Language, Settings } from "./types";

/** Where a window is told the settings. */
export interface SettingsSource {
  /** Calls `told` with the settings in force, then with each change. */
  subscribe: (told: (settings: Settings) => void) => Promise<void>;
}

const APP: SettingsSource | null = inApp ? { subscribe: settingsSubscribe } : null;

/** The settings in force, kept up with as they change; none outside the app. */
export function useSettings(source: SettingsSource | null = APP): Settings | null {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    if (source === null) return;
    // A window that subscribes again is told the new way only, but what was already on its way
    // to the old one can still arrive after the new one's news.
    let live = true;
    source
      .subscribe((next) => {
        if (live) setSettings(next);
      })
      .catch(console.error);
    return () => {
      live = false;
    };
  }, [source]);
  return settings;
}

/** The language a choice speaks: the system's own when none is chosen. */
export const localeFor = (language: Language): Locale => (language === "system" ? localeOf(navigator.language) : language);

/** Its children in the language the user chose, on a page in the appearance they chose. */
export function Settled({ source = APP, children }: { source?: SettingsSource | null; children: ReactNode }) {
  const settings = useSettings(source);
  const appearance = settings?.appearance ?? "system";
  useEffect(() => {
    const root = document.documentElement;
    if (appearance === "system") delete root.dataset.theme;
    else root.dataset.theme = appearance;
  }, [appearance]);
  return <I18nProvider locale={localeFor(settings?.language ?? "system")}>{children}</I18nProvider>;
}
