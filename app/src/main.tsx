import { createRoot } from "react-dom/client";

import { App } from "./App";
import { subscribe } from "./api";
import { publish } from "./store";
import "./theme.css";

document.documentElement.dataset.platform = navigator.userAgent.includes("Windows") ? "windows" : "mac";

if ("__TAURI_INTERNALS__" in window) {
  subscribe(publish).catch(console.error);
} else if (import.meta.env.DEV) {
  void import("./demo").then(({ play }) => play(publish));
}

const container = document.getElementById("root");
if (container) createRoot(container).render(<App />);
