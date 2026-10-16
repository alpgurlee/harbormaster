import type { Service } from "./types";

export interface Group {
  /** The repository's folder, or the project's when it isn't in one. */
  id: string;
  /** The project's name, the repository's when it holds several projects, or null outside any project. */
  name: string | null;
  framework: string | null;
  services: Service[];
}

/** One group per repository, by name with the projectless last; ports in order. */
export function groupByProject(services: Service[]): Group[] {
  const byRepository = new Map<string, Service[]>();
  for (const service of services) {
    const key = service.project?.repoRoot ?? service.project?.root ?? "";
    byRepository.set(key, [...(byRepository.get(key) ?? []), service]);
  }
  const groups = [...byRepository].map(([key, members]): Group => {
    const names = new Set(members.map((service) => service.project?.name ?? null));
    const frameworks = new Set(members.map(frameworkOf));
    return {
      id: key,
      // Projects that share a repository go under the repository's folder.
      name: names.size === 1 ? [...names][0] : folderName(key),
      framework: frameworks.size === 1 ? [...frameworks][0] : null,
      services: members.sort((a, b) => (a.ports[0]?.port ?? 0) - (b.ports[0]?.port ?? 0)),
    };
  });
  return groups.sort((a, b) => {
    if (a.name === null || b.name === null) return a.name === null ? 1 : -1;
    return a.name.localeCompare(b.name);
  });
}

function frameworkOf(service: Service): string | null {
  const framework = service.project?.framework;
  return framework ? [framework.name, framework.major].filter((part) => part !== null).join(" ") : null;
}

function folderName(path: string): string | null {
  return path.replaceAll("\\", "/").split("/").filter(Boolean).at(-1) ?? null;
}

/** Sums of what is known; null while no server has a figure yet, as on the first scan. */
export function totals(services: Service[]) {
  return {
    count: services.length,
    memoryBytes: sumKnown(services.map((service) => service.memoryBytes)),
    cpuPercent: sumKnown(services.map((service) => service.cpuPercent)),
  };
}

function sumKnown(values: (number | null)[]): number | null {
  const known = values.filter((value): value is number => value !== null);
  return known.length > 0 ? known.reduce((sum, value) => sum + value, 0) : null;
}
