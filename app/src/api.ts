import { Channel, invoke } from "@tauri-apps/api/core";

import type { Update } from "./types";

/** The panel only ever names a service by id; the backend resolves the rest. */
export interface StopPreview {
  kind: "ready";
  token: number;
  stops: { pid: number; name: string; command: string | null }[];
  frees: number[];
  staysOpen: { pid: number; name: string }[];
}

export type Preview = StopPreview | { kind: "refused"; reason: string };

export interface StopResult {
  outcomes: { pid: number; name: string; outcome: string }[];
  stillRunning: boolean;
}

export function subscribe(onUpdate: (update: Update) => void): Promise<void> {
  const channel = new Channel<Update>();
  channel.onmessage = onUpdate;
  return invoke("subscribe", { channel });
}

export const openInBrowser = (id: string, port: number) => invoke<void>("open_in_browser", { id, port });
export const revealFolder = (id: string) => invoke<void>("reveal_folder", { id });
export const stopPreview = (id: string) => invoke<Preview>("stop_preview", { id });
export const stopServer = (token: number) => invoke<StopResult>("stop", { token });
export const forceStop = (token: number) => invoke<StopResult>("force_stop", { token });
export const resizePanel = (height: number) => invoke<void>("resize_panel", { height });
export const hidePanel = () => invoke<void>("hide_panel");
