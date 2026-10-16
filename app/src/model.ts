import type { Service } from "./types";

/** How many colors projects are told apart by. */
export const TINTS = 6;

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
    const key = projectOf(service);
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

/**
 * A color for each project among `services`, by its folder: the same one every
 * time where it can be, and its own while there are colors to go round.
 */
export function tintsOf(services: Service[]): Map<string, number> {
  const tints = new Map<string, number>();
  const taken = new Set<number>();
  for (const id of [...new Set(services.map(projectOf))].sort()) {
    let tint = tintOf(id);
    for (let step = 0; step < TINTS && taken.has(tint); step += 1) tint = (tint + 1) % TINTS;
    taken.add(tint);
    tints.set(id, tint);
  }
  return tints;
}

/** A project's first choice of color; the projectless get the first, brass. */
export function tintOf(id: string): number {
  let hash = 0;
  for (const char of id) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return hash % TINTS;
}

/** The folder a service's project is grouped by, empty outside any project. */
export function projectOf(service: Service): string {
  return service.project?.repoRoot ?? service.project?.root ?? "";
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
