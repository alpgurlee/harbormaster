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

/** A file of the user's own, changed `days` ago. */
function file(group: Finding["group"], path: string, gib: number, days: number): Finding {
  return finding(group, path.split("/").pop() ?? path, path, gib, {
    kind: "personal",
    picked: false,
    emptied: false,
    touched: Date.now() / 1000 - days * DAY,
  });
}

/** Large files a Mac might keep around. */
const LARGE: Finding[] = [
  file("videos", "Movies/Screen Recording 2026-03-02.mov", 3.2, 220),
  file("videos", "Desktop/demo-final-4k.mp4", 1.1, 40),
  file("archives", "Downloads/Xcode_26.xip", 7.9, 160),
  file("archives", "Downloads/ubuntu-24.04-arm64.iso", 2.6, 300),
  file("archives", "Downloads/assets.zip", 0.4, 12),
  file("installers", "Downloads/Docker.dmg.pkg", 0.6, 95),
  file("installers", "Desktop/app-release.apk", 0.09, 30),
  file("documents", "Documents/scans/contracts.pdf", 0.12, 400),
];

const wait = (milliseconds: number) => new Promise((resolve) => setTimeout(resolve, milliseconds));

/** What the pretend app shows: the last scan's and the last hunt's findings, less what it removed. */
let scanned = false;
let hunted = false;
const removed = new Set<string>();
const shown = () => [...(scanned ? FINDINGS : []), ...(hunted ? LARGE : [])].filter((each) => !removed.has(each.path));

/** A backend that pretends: it measures for a few seconds and removes nothing. */
export const demo: CleanerBackend = {
  async scan(onEvent: (event: ScanEvent) => void) {
    await wait(700);
    for (const [index, each] of FINDINGS.entries()) {
      await wait(120);
      onEvent({ kind: "measured", done: index + 1, total: FINDINGS.length, bytes: each.bytes });
    }
    scanned = true;
    onEvent({ kind: "done", token: 1, home: HOME, findings: shown(), scanned, hunted });
  },
  async hunt(onEvent: (event: ScanEvent) => void) {
    const folders = ["Desktop", "Documents", "Downloads", "Movies", "Music", "Pictures", "code"];
    for (const [index, folder] of folders.entries()) {
      await wait(300);
      const bytes = LARGE.filter((each) => each.path.startsWith(`${HOME}/${folder}/`)).reduce((total, each) => total + each.bytes, 0);
      onEvent({ kind: "measured", done: index + 1, total: folders.length, bytes });
    }
    hunted = true;
    onEvent({ kind: "done", token: 2, home: HOME, findings: shown(), scanned, hunted });
  },
  async run(_token: number, paths: string[], onEvent: (event: RunEvent) => void) {
    let freed = 0;
    for (const path of paths) {
      await wait(250);
      const going = shown().find((each) => each.path === path);
      if (going?.group === "logs") {
        onEvent({ kind: "removed", path, outcome: "refused", why: { code: "inUse", by: "Console" } });
      } else {
        // What goes to the Trash frees nothing until the Trash is emptied.
        if (going?.kind !== "personal") freed += going?.bytes ?? 0;
        removed.add(path);
        onEvent({ kind: "removed", path, outcome: going?.kind === "personal" ? "trashed" : "deleted" });
      }
    }
    onEvent({ kind: "done", removals: [], freedBytes: freed, token: 3, findings: shown(), scanned, hunted });
  },
  stop: () => Promise.resolve(),
  disk: () => Promise.resolve({ total: 494 * GIB, free: 61 * GIB }),
  reveal: () => Promise.resolve(),
  recheck: () => Promise.resolve(shown()),
};
