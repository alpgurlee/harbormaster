import { useSyncExternalStore } from "react";

import type { Update } from "./types";

/** How far back a server's memory trend reaches. */
const TREND_MS = 10 * 60_000;

let latest: Update | null = null;
const listeners = new Set<() => void>();
/** Each running server's memory, oldest first, by its id. */
const trends = new Map<string, { at: number; bytes: number }[]>();

export function publish(update: Update) {
  latest = update;
  remember(update);
  for (const listener of listeners) listener();
}

function remember(update: Update) {
  const since = update.scannedAt - TREND_MS;
  const running = new Set(update.services.map((service) => service.id));
  for (const id of trends.keys()) {
    if (!running.has(id)) trends.delete(id);
  }
  for (const service of update.services) {
    const samples = (trends.get(service.id) ?? []).filter((sample) => sample.at >= since);
    if (service.memoryBytes !== null) samples.push({ at: update.scannedAt, bytes: service.memoryBytes });
    if (samples.length > 0) trends.set(service.id, samples);
    else trends.delete(service.id);
  }
}

function listen(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The latest scan, or null until the first arrives. */
export function useUpdate(): Update | null {
  return useSyncExternalStore(listen, () => latest);
}

/** A server's memory over the last ten minutes, oldest first. */
export function trendOf(id: string): number[] {
  return trends.get(id)?.map((sample) => sample.bytes) ?? [];
}
