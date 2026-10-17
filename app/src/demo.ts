import type { Service, Update } from "./types";

const MIB = 1024 * 1024;
const GIB = 1024 * MIB;

interface Sample {
  port: number;
  project?: { name: string; framework?: [string, number | null]; branch: string; worktree?: boolean };
  name: string;
  command: string;
  agent?: string;
  minutes: number;
  memoryMib: number;
  cpu: number;
  kind?: "dev" | "other";
  database?: boolean;
  /** How much its memory grows over ten minutes, as a share. */
  growth?: number;
}

const SAMPLES: Sample[] = [
  { port: 3000, project: { name: "acme-store", framework: ["Next.js", 16], branch: "main" }, name: "node", command: "npm run dev", agent: "claude", minutes: 192, memoryMib: 812, cpu: 3.1 },
  { port: 3002, project: { name: "acme-store", framework: ["Next.js", 16], branch: "feat/checkout", worktree: true }, name: "node", command: "npm run dev", minutes: 41, memoryMib: 640, cpu: 0.8 },
  { port: 8000, project: { name: "billing-api", framework: ["FastAPI", null], branch: "fix/webhooks" }, name: "python3", command: "uvicorn app.main:app --reload", minutes: 95, memoryMib: 186, cpu: 0.4 },
  { port: 8080, project: { name: "gateway", framework: ["Go", null], branch: "main" }, name: "gateway", command: "go run ./cmd/gateway", minutes: 260, memoryMib: 42, cpu: 0.1 },
  { port: 6006, project: { name: "design-system", framework: ["Storybook", 9], branch: "main" }, name: "node", command: "npm run storybook", minutes: 18, memoryMib: 523, cpu: 0.6, growth: 0.6 },
  { port: 5432, name: "postgres", command: "postgres -D /opt/homebrew/var/postgresql@17", minutes: 4000, memoryMib: 38, cpu: 0, kind: "other", database: true },
  { port: 6379, name: "redis-server", command: "redis-server 127.0.0.1:6379", minutes: 4000, memoryMib: 9, cpu: 0.1, kind: "other", database: true },
  { port: 11434, name: "ollama", command: "ollama serve", minutes: 900, memoryMib: 61, cpu: 0, kind: "other" },
  { port: 22000, name: "syncthing", command: "syncthing serve --no-browser", minutes: 4000, memoryMib: 74, cpu: 0.2, kind: "other" },
];

const APPS: [string, number][] = [
  ["Google Chrome", 3480],
  ["Docker", 2150],
  ["Visual Studio Code", 1640],
  ["Slack", 720],
  ["Figma", 540],
  ["Notion", 380],
  ["Spotify", 260],
  ["iTerm2", 180],
];

/** When the demo's history starts: ten minutes before it opened. */
const BEGAN = Date.now() - 10 * 60_000;

/** A little wobble around the sample's figure, and a steady climb for one that grows. */
function drift(sample: Sample, index: number, now: number): number {
  const wobble = 0.015 * Math.sin(now / 47_000 + index * 1.7) + 0.01 * Math.sin(now / 13_000 + index);
  const climb = (sample.growth ?? 0) * Math.min(1, (now - BEGAN) / 600_000);
  return 1 + wobble + climb;
}

function service(sample: Sample, index: number, now: number): Service {
  const pid = 18_000 + index * 37;
  const project = sample.project;
  return {
    id: `${pid}-1`,
    classification: sample.kind ?? "dev",
    protected: sample.database ? "database" : null,
    ports: [{ port: sample.port, address: "127.0.0.1" }],
    project: project
      ? {
          name: project.name,
          root: `/Users/dev/code/${project.name}`,
          repoRoot: `/Users/dev/code/${project.name}`,
          framework: project.framework ? { name: project.framework[0], major: project.framework[1] } : null,
          branch: project.branch,
          worktree: project.worktree ?? false,
        }
      : null,
    command: sample.command,
    startedAt: Math.floor(now / 1000) - sample.minutes * 60,
    memoryBytes: Math.round(sample.memoryMib * MIB * drift(sample, index, now)),
    cpuPercent: sample.cpu,
    processes: [{ pid, name: sample.name, command: sample.command, memoryBytes: sample.memoryMib * MIB, cpuPercent: sample.cpu }],
    holders: [{ pid, name: sample.name }],
    ancestors: sample.agent ? [{ pid: pid - 1, name: "zsh" }, { pid: pid - 2, name: sample.agent }] : [{ pid: pid - 1, name: "zsh" }],
  };
}

/** What a busy machine looks like, for working on the panel in a browser. */
export function sample(now: number): Update {
  return {
    services: SAMPLES.map((entry, index) => service(entry, index, now)),
    memory: { used: 14.9 * GIB, total: 32 * GIB },
    apps: APPS.map(([name, memoryMib]) => ({ name, memoryBytes: memoryMib * MIB, system: false })),
    scannedAt: now,
    thermal: "nominal",
  };
}

/** A scan every two seconds, as the backend sends them while the panel shows. */
export function play(publish: (update: Update) => void): () => void {
  // Ten minutes of past scans, so the trends show from the start.
  const now = Date.now();
  for (let at = now - 10 * 60_000; at < now; at += 10_000) publish(sample(at));
  const send = () => publish(sample(Date.now()));
  send();
  const timer = setInterval(send, 2000);
  return () => clearInterval(timer);
}
