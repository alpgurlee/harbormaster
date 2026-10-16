import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { Disk, Finding, Holder, MapEvent, Protection, RunEvent, ScanEvent, Settings, Update } from "./types";

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
export const openSettings = () => invoke<void>("open_settings");
export const settingsGet = () => invoke<Settings>("settings_get");
/** Keeps `settings`, handing back what is in force; rejects with a `SettingsRefusal`. */
export const settingsSet = (settings: Settings) => invoke<Settings>("settings_set", { settings });
/** Calls `told` with the settings each time they change; resolves to what stops it. */
export const onSettings = (told: (settings: Settings) => void) => listen<Settings>("settings", (event) => told(event.payload));

/** Rejects with a `CleanRefusal`; the findings come as the last event. */
export function cleanScan(onEvent: (event: ScanEvent) => void): Promise<void> {
  const channel = new Channel<ScanEvent>();
  channel.onmessage = onEvent;
  return invoke("clean_scan", { channel });
}

/** Hunts for the user's large files; rejects with a `CleanRefusal`. */
export function cleanHunt(onEvent: (event: ScanEvent) => void): Promise<void> {
  const channel = new Channel<ScanEvent>();
  channel.onmessage = onEvent;
  return invoke("clean_hunt", { channel });
}

/** Lists the user's own apps; rejects with a `CleanRefusal`. */
export function cleanApps(onEvent: (event: ScanEvent) => void): Promise<void> {
  const channel = new Channel<ScanEvent>();
  channel.onmessage = onEvent;
  return invoke("clean_apps", { channel });
}

/** Removes the findings at `paths`, which the scan under `token` listed. */
export function cleanRun(token: number, paths: string[], onEvent: (event: RunEvent) => void): Promise<void> {
  const channel = new Channel<RunEvent>();
  channel.onmessage = onEvent;
  return invoke("clean_run", { token, paths, channel });
}

/** Maps the home folder, and other apps' data in it if `others`; rejects with a `CleanRefusal`. */
export function cleanMap(others: boolean, onEvent: (event: MapEvent) => void): Promise<void> {
  const channel = new Channel<MapEvent>();
  channel.onmessage = onEvent;
  return invoke("clean_map", { others, channel });
}

/** The findings the scan under `token` listed, as they stand now: what went since is left out. */
export const cleanRecheck = (token: number) => invoke<Finding[]>("clean_recheck", { token });
export const cleanStop = () => invoke<void>("clean_stop");
export const cleanDisk = () => invoke<Disk>("clean_disk");
/** Shows a finding in Finder; only one the last scan listed. */
export const cleanReveal = (path: string) => invoke<void>("clean_reveal", { path });
/** Shows the place numbered `id` of the map under `token` in Finder, if it is still as mapped. */
export const cleanRevealArea = (token: number, id: number) => invoke<void>("clean_reveal_area", { token, id });
