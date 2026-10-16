import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ReactNode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { inApp, subscribe } from "./api";
import { Cleaner, type CleanerBackend } from "./Cleaner";
import { platform } from "./platform";
import { Settled } from "./Settled";
import { SettingsWindow, type SettingsBackend } from "./SettingsWindow";
import { publish } from "./store";
import "./theme.css";

/** Which window this page is: the app names it, and a browser asks with `?window=cleaner`. */
const view = inApp ? getCurrentWindow().label : (new URLSearchParams(location.search).get("window") ?? "panel");

function mount(node: ReactNode) {
  const container = document.getElementById("root");
  if (container) createRoot(container).render(node);
}

document.documentElement.dataset.platform = platform;

if (view === "cleaner") {
  const show = (backend?: CleanerBackend) =>
    mount(
      <Settled>
        <Cleaner backend={backend} />
      </Settled>,
    );
  if (!inApp && import.meta.env.DEV) {
    void import("./cleanerDemo").then(({ demo }) => show(demo));
  } else {
    show();
  }
} else if (view === "settings") {
  const show = (backend?: SettingsBackend) =>
    mount(
      <Settled>
        <SettingsWindow backend={backend} />
      </Settled>,
    );
  if (!inApp && import.meta.env.DEV) {
    void import("./settingsDemo").then(({ demo }) => show(demo));
  } else {
    show();
  }
} else {
  // Only the panel sits on the window's frosted glass.
  if (inApp && platform === "mac") document.documentElement.dataset.vibrancy = "";
  if (inApp) {
    subscribe(publish).catch(console.error);
  } else if (import.meta.env.DEV) {
    void import("./demo").then(({ play }) => play(publish));
  }
  mount(<App />);
}
