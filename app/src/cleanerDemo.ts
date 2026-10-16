import type { CleanerBackend } from "./Cleaner";
import type { Finding, RunEvent, ScanEvent } from "./types";

const MIB = 1024 * 1024;
const GIB = 1024 * MIB;
const DAY = 86_400;
const HOME = "/Users/you";

function finding(group: Finding["group"], name: string, path: string, gib: number, extra: Partial<Finding> = {}): Finding {
  return {
    group,
    kind: "rebuildable",
    name,
    path: `${HOME}/${path}`,
    bytes: Math.round(gib * GIB),
    picked: true,
    inUse: null,
    touched: null,
    project: null,
    emptied: group !== "projects",
    ...extra,
  };
}

function output(project: string, folder: string, gib: number, idleDays: number, inUse: string | null = null): Finding {
  const now = Date.now() / 1000;
  return finding("projects", `${project.split("/").pop()} · ${folder}`, `code/${project}/${folder}`, gib, {
    picked: idleDays >= 30 && inUse === null,
    inUse,
    touched: now - idleDays * DAY,
    project: `${HOME}/code/${project}`,
  });
}

/** What a developer's Mac might hold, for looking at the window without one. */
const FINDINGS: Finding[] = [
  output("acme-store", ".next", 2.4, 3, "localhost:3000"),
  output("billing-api", "node_modules", 1.4, 52),
  output("design-system", "node_modules", 0.9, 120),
  output("design-system", ".next", 0.3, 120),
  output("gateway", "target", 3.8, 41),
  output("landing", "node_modules", 0.6, 9),
  finding("packages", "npm", ".npm/_cacache", 1.7),
  finding("packages", "pnpm", "Library/pnpm/store", 1.2),
  finding("packages", "Gradle", ".gradle/caches", 4.5, { picked: false, inUse: "java" }),
  finding("packages", "Homebrew", "Library/Caches/Homebrew", 0.55),
  finding("packages", "pip", "Library/Caches/pip", 0.28),
  finding("xcode", "DerivedData", "Library/Developer/Xcode/DerivedData", 2.1),
  finding("xcode", "iPhone17,3 18.4", "Library/Developer/Xcode/iOS DeviceSupport/iPhone17,3 18.4", 6.6, { picked: false, emptied: false }),
  finding("xcode", "2026-09-20", "Library/Developer/Xcode/Archives/2026-09-20", 0.27, { kind: "personal", picked: false, emptied: false }),
  finding("apps", "com.spotify.client", "Library/Caches/com.spotify.client", 0.31),
  finding("apps", "Google Chrome", "Library/Caches/Google/Chrome", 0.27),
  finding("logs", "Logs", "Library/Logs", 0.07),
  finding("trash", "Trash", ".Trash", 0.19, { kind: "discarded", picked: false }),
];

const wait = (milliseconds: number) => new Promise((resolve) => setTimeout(resolve, milliseconds));

/** A backend that pretends: it measures for a few seconds and removes nothing. */
export const demo: CleanerBackend = {
  async scan(onEvent: (event: ScanEvent) => void) {
    await wait(700);
    for (const [index, each] of FINDINGS.entries()) {
      await wait(120);
      onEvent({ kind: "measured", done: index + 1, total: FINDINGS.length, bytes: each.bytes });
    }
    onEvent({ kind: "done", token: 1, home: HOME, findings: FINDINGS });
  },
  async run(_token: number, paths: string[], onEvent: (event: RunEvent) => void) {
    let freed = 0;
    for (const path of paths) {
      await wait(250);
      const going = FINDINGS.find((each) => each.path === path);
      if (going?.group === "logs") {
        onEvent({ kind: "removed", path, outcome: "refused", why: { code: "inUse", by: "Console" } });
      } else {
        freed += going?.bytes ?? 0;
        onEvent({ kind: "removed", path, outcome: going?.kind === "personal" ? "trashed" : "deleted" });
      }
    }
    onEvent({ kind: "done", removals: [], freedBytes: freed });
  },
  stop: () => Promise.resolve(),
};
