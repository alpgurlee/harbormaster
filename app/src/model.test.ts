import { describe, expect, it } from "vitest";

import { formatBytes, formatUptime, groupByProject, totals } from "./model";
import type { Service } from "./types";

function service(id: string, port: number, project: string | null, memory = 100): Service {
  return {
    id,
    classification: "dev",
    protected: null,
    ports: [{ port, address: "127.0.0.1" }],
    project: project
      ? { name: project, root: `/dev/${project}`, repoRoot: `/dev/${project}`, framework: null, branch: "main", worktree: false }
      : null,
    command: "npm run dev",
    startedAt: 0,
    memoryBytes: memory * 1024 * 1024,
    cpuPercent: 1.5,
    processes: [],
    holders: [],
    ancestors: [],
  };
}

describe("groupByProject", () => {
  it("groups servers by project, by name, then by port", () => {
    const groups = groupByProject([
      service("a", 3002, "acme-store"),
      service("b", 8000, "billing-api"),
      service("c", 3000, "acme-store"),
      service("d", 4000, null),
    ]);

    expect(groups.map((group) => group.name)).toEqual(["acme-store", "billing-api", null]);
    expect(groups[0].services.map((s) => s.ports[0].port)).toEqual([3000, 3002]);
  });
});

describe("monorepo groups", () => {
  it("name the group after the repository, whatever order the servers come in", () => {
    const inRepo = (id: string, port: number, name: string, framework: string): Service => ({
      ...service(id, port, name),
      project: {
        name,
        root: `/dev/acme/${name}`,
        repoRoot: "/dev/acme",
        framework: { name: framework, major: null },
        branch: "main",
        worktree: false,
      },
    });
    const web = inRepo("a", 3000, "web", "Next.js");
    const api = inRepo("b", 8000, "api", "FastAPI");

    const groups = groupByProject([web, api]);

    expect(groups).toEqual(groupByProject([api, web]));
    expect(groups[0].name).toBe("acme");
    expect(groups[0].framework).toBeNull();
  });
});

describe("totals", () => {
  it("keeps a figure unknown when no server has it yet", () => {
    const first = { ...service("a", 3000, "x"), cpuPercent: null };

    expect(totals([first]).cpuPercent).toBeNull();
  });

  it("adds up memory and CPU of the servers it is given", () => {
    expect(totals([service("a", 1, "x", 100), service("b", 2, "y", 50)])).toEqual({
      count: 2,
      memoryBytes: 150 * 1024 * 1024,
      cpuPercent: 3,
    });
  });
});

describe("formatting", () => {
  it("shows megabytes, then gigabytes", () => {
    expect(formatBytes(812 * 1024 * 1024)).toBe("812 MB");
    expect(formatBytes(2.2 * 1024 * 1024 * 1024)).toBe("2.2 GB");
  });

  it("shows uptime with its two largest units", () => {
    expect(formatUptime(20)).toBe("<1m");
    expect(formatUptime(41 * 60)).toBe("41m");
    expect(formatUptime(3 * 3600 + 12 * 60)).toBe("3h 12m");
    expect(formatUptime(2 * 86400 + 5 * 3600)).toBe("2d 5h");
  });
});
