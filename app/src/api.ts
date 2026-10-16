import { Channel, invoke } from "@tauri-apps/api/core";

import type { Holder, Protection, RunEvent, ScanEvent, Update } from "./types";

/** Why the app turned a stop down. */
export type Reason =
  | { code: "notFound" | "runsHarbormaster" | "changed" | "expired" | "untrackedExec" | "unknownLineage" }
  | { code: "protected"; protection: Protection }
  | { code: "failed"; message: string };

/** The panel only ever names a service by id; the backend resolves the rest. */
export type Preview =
  | {
      kind: "ready";
      token: number;
      stops: { pid: number; name: string; command: string | null }[];
      frees: number[];
      staysOpen: Holder[];
    }
  | { kind: "refused"; reason: Reason };

export type Outcome = "stopped" | "alreadyGone" | "replaced" | "accessDenied" | "stillRunning" | { failed: string };

/** What quitting an app would do; it names the app as the breakdown does. */
export type QuitPreview =
  | { kind: "app"; token: number }
  | { kind: "programs"; token: number; quits: { pid: number; name: string; command: string | null }[]; staysOpen: Holder[] }
  | { kind: "refused"; reason: Reason };

export interface Stopped {
  outcomes: { pid: number; name: string; outcome: Outcome }[];
  stillRunning: boolean;
}

export const inApp = "__TAURI_INTERNALS__" in window;

export function subscribe(onUpdate: (update: Update) => void): Promise<void> {
  const channel = new Channel<Update>();
  channel.onmessage = onUpdate;
  return invoke("subscribe", { channel });
}

export const openInBrowser = (id: string, port: number) => invoke<void>("open_in_browser", { id, port });
export const revealFolder = (id: string) => invoke<void>("reveal_folder", { id });
export const stopPreview = (id: string) => invoke<Preview>("stop_preview", { id });
/** Rejects with a `Reason` when the server changed since it was approved. */
export const stopServer = (token: number) => invoke<Stopped>("stop", { token });
export const forceStop = (token: number) => invoke<Stopped>("force_stop", { token });
export const quitAppPreview = (name: string) => invoke<QuitPreview>("quit_app_preview", { name });
/** Asks politely and never forces; rejects with a `Reason` when the app changed since it was approved. */
export const quitApp = (token: number) => invoke<Stopped>("quit_app", { token });
export const resizePanel = (height: number) => invoke<void>("resize_panel", { height });
export const hidePanel = () => invoke<void>("hide_panel");
export const openCleaner = () => invoke<void>("open_cleaner");

/** Rejects with a `CleanRefusal`; the findings come as the last event. */
export function cleanScan(onEvent: (event: ScanEvent) => void): Promise<void> {
  const channel = new Channel<ScanEvent>();
  channel.onmessage = onEvent;
  return invoke("clean_scan", { channel });
}

/** Removes the findings at `paths`, which the scan under `token` listed. */
export function cleanRun(token: number, paths: string[], onEvent: (event: RunEvent) => void): Promise<void> {
  const channel = new Channel<RunEvent>();
  channel.onmessage = onEvent;
  return invoke("clean_run", { token, paths, channel });
}

export const cleanStop = () => invoke<void>("clean_stop");
