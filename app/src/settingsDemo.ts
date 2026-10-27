import type { SettingsBackend } from "./SettingsWindow";
import type { Settings } from "./types";

let kept: Settings = { language: "system", appearance: "system", hidden: ["serena"], roots: ["/Users/you/code"], idleDays: 30, checkUpdates: true };

/** Settings kept in memory, for looking at the window without the app. */
export const demo: SettingsBackend = {
  get: () => Promise.resolve(kept),
  set: (settings) => {
    kept = settings;
    return Promise.resolve(kept);
  },
};
