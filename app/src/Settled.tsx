import { useEffect, useState, type ReactNode } from "react";

import { inApp, onSettings, settingsGet } from "./api";
import { I18nProvider, localeOf, type Locale } from "./i18n";
import type { Language, Settings } from "./types";

/** Where a window reads the settings, and hears when they change. */
export interface SettingsSource {
  get: () => Promise<Settings>;
  /** Calls `told` with each change; resolves to what stops it. */
  listen: (told: (settings: Settings) => void) => Promise<() => void>;
}

const APP: SettingsSource | null = inApp ? { get: settingsGet, listen: onSettings } : null;

/** The settings in force, read once and then kept up with; none outside the app. */
export function useSettings(source: SettingsSource | null = APP): Settings | null {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    if (source === null) return;
    let live = true;
    // A change heard first is newer than what the first reading brings back.
    let heard = false;
    const stop = source.listen((next) => {
      heard = true;
      if (live) setSettings(next);
    });
    source
      .get()
      .then((first) => {
        if (live && !heard) setSettings(first);
      })
      .catch(console.error);
    return () => {
      live = false;
      stop.then((unlisten) => unlisten()).catch(console.error);
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
