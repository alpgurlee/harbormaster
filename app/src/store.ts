import { useSyncExternalStore } from "react";

import type { Update } from "./types";

let latest: Update | null = null;
const listeners = new Set<() => void>();

export function publish(update: Update) {
  latest = update;
  for (const listener of listeners) listener();
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
