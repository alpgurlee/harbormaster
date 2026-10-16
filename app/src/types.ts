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
export type CleanGroup = "projects" | "packages" | "xcode" | "apps" | "logs" | "trash";

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
  /** For a project's build output: seconds since the Unix epoch. */
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
  | { kind: "done"; token: number; home: string; findings: Finding[] };

export type RunEvent = ({ kind: "removed" } & Removal) | ({ kind: "done" } & Cleaned);

/** The home folder's disk. */
export interface Disk {
  total: number;
  free: number;
}

/** Why the app turned the cleaner down. */
export type CleanRefusal = { code: "busy" | "closed" | "expired" | "unlisted" } | { code: "failed"; message: string };
