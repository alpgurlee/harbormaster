import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Tauri serves the built files itself; the dev server only runs while developing.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  build: { target: ["es2022", "safari16"], outDir: "dist" },
  test: { environment: "jsdom" },
});
