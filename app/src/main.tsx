import { createRoot } from "react-dom/client";

import { App } from "./App";
import { inApp, subscribe } from "./api";
import { platform } from "./platform";
import { publish } from "./store";
import "./theme.css";

document.documentElement.dataset.platform = platform;
if (inApp && platform === "mac") document.documentElement.dataset.vibrancy = "";

if (inApp) {
  subscribe(publish).catch(console.error);
} else if (import.meta.env.DEV) {
  void import("./demo").then(({ play }) => play(publish));
}

const container = document.getElementById("root");
if (container) createRoot(container).render(<App />);
