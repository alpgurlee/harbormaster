import type { CleanGroup, Finding } from "./types";

/** The order the cleaner lists its groups in. */
export const GROUPS: CleanGroup[] = ["projects", "packages", "xcode", "apps", "logs", "trash"];

/** The window's sections past the overview, and the groups each reviews. */
export type CleanModule = "developer" | "system";

export const MODULES: Record<CleanModule, CleanGroup[]> = {
  developer: ["projects", "packages", "xcode"],
  system: ["apps", "logs", "trash"],
};

/** How a group's items are listed. */
export type Order = "size" | "name" | "age";

/** A group as the window shows it, with how much of it is chosen. */
export interface GroupView {
  group: CleanGroup;
  findings: Finding[];
  bytes: number;
  chosen: number;
  chosenBytes: number;
  state: "all" | "some" | "none";
  /** Whether anything in it may be chosen at all. */
  selectable: boolean;
}

/** What removing the choice would do. */
export interface Plan {
  count: number;
  bytes: number;
  /** Gone for good; their tools make them again. */
  deleted: { count: number; bytes: number };
  /** Moved to the Trash, where they can be put back. */
  trashed: { count: number; bytes: number };
  /** The Trash itself is emptied, which is for good. */
  emptied: boolean;
}

/** Something in use can't be chosen; the window says by what. */
export const selectable = (finding: Finding) => finding.inUse === null;

/** What the scan chose, by path. */
export function picked(findings: Finding[]): Set<string> {
  return new Set(findings.filter((finding) => finding.picked && selectable(finding)).map((finding) => finding.path));
}

export function groupsOf(findings: Finding[], chosen: Set<string>): GroupView[] {
  return GROUPS.flatMap((group) => {
    const members = findings.filter((finding) => finding.group === group);
    if (members.length === 0) return [];
    const picks = members.filter((finding) => chosen.has(finding.path));
    return [
      {
        group,
        findings: members,
        bytes: sum(members),
        chosen: picks.length,
        chosenBytes: sum(picks),
        state: choiceOf(members, chosen),
        selectable: members.some(selectable),
      },
    ];
  });
}

/** Whether all, some or none of what may be chosen of `findings` is. */
export function choiceOf(findings: Finding[], chosen: Set<string>): GroupView["state"] {
  const choosable = findings.filter(selectable);
  const picks = choosable.filter((finding) => chosen.has(finding.path)).length;
  return picks === 0 ? "none" : picks === choosable.length ? "all" : "some";
}

/**
 * `findings` all chosen, or none of them when they all were already. Only
 * what nothing is using is ever chosen.
 */
export function toggled(chosen: Set<string>, findings: Finding[]): Set<string> {
  const choosable = findings.filter(selectable);
  const next = new Set(chosen);
  if (choosable.every((finding) => chosen.has(finding.path))) {
    for (const finding of findings) next.delete(finding.path);
  } else {
    for (const finding of choosable) next.add(finding.path);
  }
  return next;
}

/** What of `chosen` is still listed in `findings` and may still be chosen. */
export function stillChosen(chosen: Set<string>, findings: Finding[]): Set<string> {
  return new Set(findings.filter((finding) => selectable(finding) && chosen.has(finding.path)).map((finding) => finding.path));
}

export function plan(findings: Finding[], chosen: Set<string>): Plan {
  const picks = findings.filter((finding) => chosen.has(finding.path));
  const deleted = picks.filter((finding) => finding.kind === "rebuildable");
  const trashed = picks.filter((finding) => finding.kind === "personal");
  return {
    count: picks.length,
    bytes: sum(picks),
    deleted: { count: deleted.length, bytes: sum(deleted) },
    trashed: { count: trashed.length, bytes: sum(trashed) },
    emptied: picks.some((finding) => finding.kind === "discarded"),
  };
}

function sum(findings: Finding[]): number {
  return findings.reduce((total, finding) => total + finding.bytes, 0);
}

/** `findings` largest first, by name, or the longest untouched first, with what has no age last. */
export function ordered(findings: Finding[], order: Order): Finding[] {
  const copy = [...findings];
  switch (order) {
    case "size":
      return copy.sort((a, b) => b.bytes - a.bytes);
    case "name":
      return copy.sort((a, b) => a.name.localeCompare(b.name));
    case "age":
      return copy.sort((a, b) => (a.touched ?? Number.POSITIVE_INFINITY) - (b.touched ?? Number.POSITIVE_INFINITY));
  }
}

/** The findings whose name or folder holds `query`, whatever the case. */
export function matching(findings: Finding[], query: string): Finding[] {
  const wanted = query.trim().toLocaleLowerCase();
  if (wanted === "") return findings;
  return findings.filter(
    (finding) => finding.name.toLocaleLowerCase().includes(wanted) || finding.path.toLocaleLowerCase().includes(wanted),
  );
}
