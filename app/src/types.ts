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
