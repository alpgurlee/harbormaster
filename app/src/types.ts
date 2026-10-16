/** The scan as the backend sends it; mirrors the engine's JSON. */
export type Classification = "dev" | "other" | "system";

export type Protection =
  | "system"
  | "otherUser"
  | "session"
  | "unverified"
  | "database"
  | "sharedSocket";

export interface Port {
  port: number;
  address: string;
}

export interface Framework {
  name: string;
  major: number | null;
}

export interface Project {
  name: string;
  root: string;
  repoRoot: string | null;
  framework: Framework | null;
  branch: string | null;
  worktree: boolean;
}

export interface Member {
  pid: number;
  name: string;
  command: string | null;
  memoryBytes: number | null;
  cpuPercent: number | null;
}

export interface Holder {
  pid: number;
  name: string;
}

export interface Service {
  id: string;
  classification: Classification;
  protected: Protection | null;
  ports: Port[];
  project: Project | null;
  command: string | null;
  startedAt: number;
  memoryBytes: number | null;
  cpuPercent: number | null;
  processes: Member[];
  holders: Holder[];
  ancestors: Holder[];
}

export interface Memory {
  used: number;
  total: number;
}

/** One app's processes together, or one plain program. */
export interface AppUsage {
  name: string;
  memoryBytes: number;
  /** Part of the OS, which the panel never offers to quit. */
  system: boolean;
}

export interface Update {
  services: Service[];
  memory: Memory | null;
  /** The apps using the most memory, dev servers left out; null until measured. */
  apps: AppUsage[] | null;
  /** Milliseconds since the Unix epoch. */
  scannedAt: number;
}

/** The cleaner's groups, in the order it lists them. */
export type CleanGroup =
  | "projects"
  | "packages"
  | "xcode"
  | "apps"
  | "logs"
  | "trash"
  | "videos"
  | "archives"
  | "installers"
  | "audio"
  | "images"
  | "documents"
  | "otherFiles"
  | "applications";

/**
 * How a finding goes: deleted (its tool rebuilds it), to the Trash (it may
 * be the user's own work) or for good (it was in the Trash already).
 */
export type CleanKind = "rebuildable" | "personal" | "discarded";

/** Something that could go, as a scan found it. */
export interface Finding {
  group: CleanGroup;
  kind: CleanKind;
  name: string;
  path: string;
  bytes: number;
  /** Chosen unless the user says otherwise. */
  picked: boolean;
  /** What is using it, which keeps it from being chosen at all. */
  inUse: string | null;
  /** When a project was last worked on, or a large file changed: seconds since the Unix epoch. */
  touched: number | null;
  project: string | null;
  emptied: boolean;
}

/** Why a finding was left alone when the time came. */
export type Why =
  | { code: "outsideHome" | "changed" }
  | { code: "notReached" | "unchecked"; error: string }
  | { code: "inUse"; by: string };

export type Removal = { path: string } & (
  | { outcome: "deleted" | "trashed" | "stopped" | "interrupted" }
  | { outcome: "refused"; why: Why }
  | { outcome: "failed"; why: string }
);

export interface Cleaned {
  removals: Removal[];
  /** Read from the disk before and after. */
  freedBytes: number;
}

export type ScanEvent =
  | { kind: "measured"; done: number; total: number; bytes: number }
  | {
      kind: "done";
      token: number;
      home: string;
      /** What this scan or hunt found, beside what the last of the other found. */
      findings: Finding[];
      /** Whether what tools and apps leave was scanned for, whether the large files were hunted for, and whether the user's apps were listed, so none listed means none found. */
      scanned: boolean;
      hunted: boolean;
      appsListed: boolean;
    };

/** What is shown once a removal is over: what wasn't chosen, and what was left as it was. */
export interface Listed {
  token: number;
  findings: Finding[];
  scanned: boolean;
  hunted: boolean;
  appsListed: boolean;
}

export type RunEvent =
  | ({ kind: "removed" } & Removal)
  | ({ kind: "done" } & Cleaned & Listed)
  /** The removal began but couldn't finish. */
  | ({ kind: "failed"; refusal: CleanRefusal } & Listed);

/** The home folder's disk. */
export interface Disk {
  total: number;
  free: number;
}

/** Why the app turned the cleaner down. */
export type CleanRefusal =
  | { code: "busy" | "closed" | "expired" | "unlisted" | "changed" }
  | { code: "failed"; message: string };

/** Why a place in the map may hold more than it says, the weightiest last. */
export type Short = "deep" | "changed" | "denied" | "skipped" | "stopped";

/** A folder or file in a map of the home folder. */
export interface Area {
  /** Its number in the map, which showing it in Finder takes; the home folder's is 0. */
  id: number;
  name: string;
  /** The room it takes on disk, with everything it holds. */
  bytes: number;
  folder: boolean;
  short: Short | null;
  /** What it holds that is large enough to show, largest first. */
  inside: Area[];
  /** What else it holds, each too small to show on its own. */
  rest: number;
  /** Whether all it holds is counted in `rest` to keep the map small, whatever the size of each. */
  folded: boolean;
}

export type MapEvent =
  | { kind: "charting"; bytes: number; items: number; folder: string }
  | { kind: "done"; token: number; area: Area };
