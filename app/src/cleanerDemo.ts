import type { CleanerBackend } from "./Cleaner";
import type { Area, Finding, MapEvent, Program, RunEvent, ScanEvent } from "./types";

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
  finding("packages", "Gradle", ".gradle/caches", 4.5, { picked: false, inUse: "Gradle daemon" }),
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

/** A folder of the pretend map, holding `inside` and as much again in smaller items as `gib` leaves. */
function held(name: string, gib: number, inside: Area[], extra: Partial<Area> = {}): Area {
  const shown = inside.reduce((total, each) => total + each.bytes, 0);
  const bytes = Math.max(Math.round(gib * GIB), shown);
  return { id: 0, name, bytes, folder: true, short: null, inside, rest: bytes - shown, folded: false, ...extra };
}

function kept(name: string, gib: number): Area {
  return { id: 0, name, bytes: Math.round(gib * GIB), folder: false, short: null, inside: [], rest: 0, folded: false };
}

/** Numbers `area` and what it holds in the order they are shown, as the app does. */
function numbered(area: Area, next = { id: 0 }): Area {
  const id = next.id++;
  return { ...area, id, inside: area.inside.map((each) => numbered(each, next)) };
}

/** What a developer's home folder might hold, for looking at the map without one. */
const MAP: Area = numbered(
  held(
    "you",
    161,
    [
      held(
        "Library",
        72,
        [
          held("Developer", 31, [
            held("Xcode", 24, [held("iOS DeviceSupport", 13, []), held("DerivedData", 9, []), held("Archives", 1.6, [])]),
            held("CoreSimulator", 6.8, [held("Devices", 6.5, [])]),
          ]),
          held("Application Support", 18, [held("Code", 3.1, []), held("Slack", 2.2, []), held("Google", 1.9, [])]),
          held("Caches", 14, [held("Google", 4.2, []), held("Homebrew", 3.1, []), held("com.spotify.client", 1.8, [])]),
          held("Containers", 0, [], { short: "skipped" }),
          held("Group Containers", 0, [], { short: "skipped" }),
          held("Mail", 0.4, [], { short: "denied" }),
        ],
        { short: "skipped" },
      ),
      held("code", 38, [
        held("acme-store", 12, [held("node_modules", 6.2, []), held(".next", 2.9, [])]),
        held("gateway", 9.4, [held("target", 8.1, [])]),
        held("billing-api", 5.2, [held("node_modules", 1.4, [])]),
      ]),
      held("Movies", 18, [held("Trip.fcpbundle", 14, []), kept("Screen Recording 2026-03-02.mov", 3.2)]),
      held("Downloads", 12, [kept("Xcode_26.xip", 7.9), kept("ubuntu-24.04-arm64.iso", 2.6)]),
      held("Pictures", 9, [held("Photos Library.photoslibrary", 8.8, [])]),
      held(".gradle", 4.5, [held("caches", 4.3, [])]),
      held("Documents", 3, [kept("contracts.pdf", 0.12)]),
      held(".npm", 1.7, [held("_cacache", 1.7, [])]),
    ],
    { short: "skipped" },
  ),
);

const wait = (milliseconds: number) => new Promise((resolve) => setTimeout(resolve, milliseconds));

/** An app of the user's own at `path`. */
function app(name: string, path: string, gib: number, inUse: string | null = null): Finding {
  return { ...finding("applications", name, "", gib), path, kind: "personal", picked: false, emptied: false, inUse };
}

/** The user's own apps. */
const APPS: Finding[] = [
  app("Docker", "/Applications/Docker.app", 2.3),
  app("Blender", `${HOME}/Applications/Blender.app`, 1.6),
  app("Cursor", "/Applications/Cursor.app", 0.9),
  app("Slack", "/Applications/Slack.app", 0.5, "Slack"),
];

/** The programs a Windows PC lists as installed. */
const PROGRAMS: Program[] = [
  program(0, "Docker Desktop", "Docker Inc.", "4.38.0", 2.1, true, "C:\\Program Files\\Docker\\Docker\\Docker Desktop Installer.exe"),
  program(1, "Blender", "Blender Foundation", "4.3.2", 1.4, true, "C:\\Windows\\System32\\MsiExec.exe"),
  program(2, "Cursor", "Anysphere", "0.45.11", 0.6, false, "C:\\Users\\you\\AppData\\Local\\Programs\\cursor\\Uninstall Cursor.exe"),
  program(3, "Slack", "Slack Technologies Inc.", "4.41.105", 0.3, false, "C:\\Users\\you\\AppData\\Local\\slack\\Update.exe"),
];

function program(id: number, name: string, publisher: string, version: string, gib: number, everyone: boolean, uninstaller: string): Program {
  return { id, name, publisher, version, estimated: Math.round(gib * GIB), everyone, uninstaller };
}

/** What the pretend app shows: the last scan's, hunt's and listing's findings, less what it removed. */
let scanned = false;
let hunted = false;
let appsListed = false;
const removed = new Set<string>();
const shown = () =>
  [...(scanned ? FINDINGS : []), ...(hunted ? LARGE : []), ...(appsListed ? APPS : [])].filter((each) => !removed.has(each.path));

/** A backend that pretends: it measures for a few seconds and removes nothing. */
export const demo: CleanerBackend = {
  daemons: () => Promise.resolve(shown().filter((each) => each.inUse === "Gradle daemon").map((each) => each.path)),
  daemonPreview: () =>
    Promise.resolve({
      kind: "ready",
      approval: 1,
      stops: [{ pid: 4242, name: "Gradle daemon", command: "java -Xmx512m -cp gradle-daemon-main-8.14.jar org.gradle.launcher.daemon.bootstrap.GradleDaemon 8.14" }],
    }),
  async daemonStop() {
    await wait(600);
    // The daemon gone, its cache is free, and chosen as caches are.
    for (const each of FINDINGS.filter((each) => each.inUse === "Gradle daemon")) {
      each.inUse = null;
      each.picked = true;
    }
    return { outcomes: [{ pid: 4242, name: "Gradle daemon", outcome: "stopped" }], stillRunning: false };
  },
  async scan(onEvent: (event: ScanEvent) => void) {
    await wait(700);
    for (const [index, each] of FINDINGS.entries()) {
      await wait(120);
      onEvent({ kind: "measured", done: index + 1, total: FINDINGS.length, bytes: each.bytes });
    }
    scanned = true;
    onEvent({ kind: "done", token: 1, home: HOME, findings: shown(), scanned, hunted, appsListed, idleDays: 30 });
  },
  async hunt(onEvent: (event: ScanEvent) => void) {
    const folders = ["Desktop", "Documents", "Downloads", "Movies", "Music", "Pictures", "code"];
    for (const [index, folder] of folders.entries()) {
      await wait(300);
      const bytes = LARGE.filter((each) => each.path.startsWith(`${HOME}/${folder}/`)).reduce((total, each) => total + each.bytes, 0);
      onEvent({ kind: "measured", done: index + 1, total: folders.length, bytes });
    }
    hunted = true;
    onEvent({ kind: "done", token: 2, home: HOME, findings: shown(), scanned, hunted, appsListed, idleDays: null });
  },
  async apps(onEvent: (event: ScanEvent) => void) {
    for (const [index, app] of APPS.entries()) {
      await wait(250);
      onEvent({ kind: "measured", done: index + 1, total: APPS.length, bytes: app.bytes });
    }
    appsListed = true;
    onEvent({ kind: "done", token: 4, home: HOME, findings: shown(), scanned, hunted, appsListed, idleDays: null });
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
    onEvent({ kind: "done", removals: [], freedBytes: freed, token: 3, findings: shown(), scanned, hunted, appsListed });
  },
  stop: () => Promise.resolve(),
  disk: () => Promise.resolve({ total: 494 * GIB, free: 61 * GIB }),
  reveal: () => Promise.resolve(),
  recheck: () => Promise.resolve(shown()),
  async map(_others: boolean, onEvent: (event: MapEvent) => void) {
    const folders = ["Library/Developer/Xcode", "Library/Caches/Google", "code/acme-store/node_modules", "Movies", "Downloads"];
    for (const [index, folder] of folders.entries()) {
      await wait(500);
      const share = (index + 1) / folders.length;
      onEvent({ kind: "charting", bytes: Math.round(MAP.bytes * share), items: Math.round(480_000 * share), folder });
    }
    onEvent({ kind: "done", token: 9, area: MAP });
  },
  revealArea: () => Promise.resolve(),
  async programs() {
    await wait(400);
    return { token: 5, programs: PROGRAMS };
  },
  // The pretend uninstaller starts, and nothing goes.
  async uninstall() {
    await wait(600);
  },
  openInstalledApps: () => Promise.resolve(),
};
