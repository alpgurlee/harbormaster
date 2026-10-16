import { describe, expect, it } from "vitest";

import { MODULES, groupsOf, matching, ordered, picked, plan, rechosen, toggled } from "./cleanup";
import type { Finding } from "./types";

const GB = 1024 ** 3;

function finding(path: string, extra: Partial<Finding> = {}): Finding {
  return {
    group: "packages",
    kind: "rebuildable",
    name: path,
    path,
    bytes: GB,
    picked: true,
    inUse: null,
    touched: null,
    project: null,
    emptied: true,
    ...extra,
  };
}

const npm = finding("/u/.npm");
const pip = finding("/u/pip", { picked: false });
const held = finding("/u/gradle", { inUse: "java", picked: false });
const shop = finding("/u/code/shop/node_modules", { group: "projects", emptied: false });
const archive = finding("/u/Archives/a", { group: "xcode", kind: "personal", picked: false, emptied: false });
const trash = finding("/u/.Trash", { group: "trash", kind: "discarded", picked: false });

describe("the cleaner's choices", () => {
  it("starts from what the scan picked", () => {
    expect([...picked([npm, pip, held, shop])]).toEqual([npm.path, shop.path]);
  });

  it("takes a scan's picks and keeps the hunt's choice, and the other way round", () => {
    const film = finding("/u/Movies/a.mov", { group: "videos", kind: "personal", picked: false, emptied: false });
    const song = finding("/u/Music/b.wav", { group: "audio", kind: "personal", picked: false, emptied: false });
    const before = new Set([pip.path, film.path]);

    expect([...rechosen(before, [npm, pip, held, film, song], false)]).toEqual([npm.path, film.path]);
    expect([...rechosen(before, [npm, pip, held, film, song], true)]).toEqual([pip.path]);
    // What is in use is never chosen, whoever picked it.
    expect([...rechosen(new Set([held.path]), [held], false)]).toEqual([]);
  });

  it("groups findings in the cleaner's order and says how much of each is chosen", () => {
    const groups = groupsOf([npm, trash, pip, shop, held], new Set([npm.path, shop.path]));

    expect(groups.map((group) => group.group)).toEqual(["projects", "packages", "trash"]);
    const packages = groups[1];
    expect(packages.findings).toEqual([npm, pip, held]);
    expect([packages.bytes, packages.chosenBytes, packages.chosen]).toEqual([3 * GB, GB, 1]);
    expect(packages.state).toBe("some");
    expect(groups[0].state).toBe("all");
    expect(groups[2].state).toBe("none");
  });

  it("chooses what nothing is using, and only that", () => {
    const all = toggled(new Set([npm.path]), [npm, pip, held]);
    expect([...all].sort()).toEqual([npm.path, pip.path].sort());

    const none = toggled(all, [npm, pip, held]);
    expect(none.size).toBe(0);

    expect(toggled(new Set(), [held]).size).toBe(0);
    expect(groupsOf([held], new Set())[0].selectable).toBe(false);
  });

  it("says what removing the choice would do, kind by kind", () => {
    const findings = [npm, shop, archive, trash, pip];

    const chosen = plan(findings, new Set([npm.path, shop.path, archive.path, trash.path]));

    expect(chosen).toEqual({
      count: 4,
      bytes: 4 * GB,
      deleted: { count: 2, bytes: 2 * GB },
      trashed: { count: 1, bytes: GB },
      emptied: true,
    });
    expect(plan(findings, new Set()).emptied).toBe(false);
  });
});

describe("reviewing a group", () => {
  const old = finding("/u/code/old/node_modules", { name: "old · node_modules", group: "projects", touched: 100 });
  const fresh = finding("/u/code/fresh/.next", { name: "fresh · .next", group: "projects", touched: 900, bytes: 3 * GB });
  const cache = finding("/u/.npm/_cacache", { name: "npm", bytes: 2 * GB });

  it("puts the largest, the alphabetical or the longest untouched first", () => {
    expect(ordered([old, fresh, cache], "size").map((each) => each.name)).toEqual(["fresh · .next", "npm", "old · node_modules"]);
    expect(ordered([old, fresh, cache], "name").map((each) => each.name)).toEqual(["fresh · .next", "npm", "old · node_modules"].sort((a, b) => a.localeCompare(b)));
    expect(ordered([cache, fresh, old], "age").map((each) => each.name)).toEqual(["old · node_modules", "fresh · .next", "npm"]);
  });

  it("finds by name or folder, whatever the case", () => {
    expect(matching([old, fresh, cache], "  NPM ")).toEqual([cache]);
    expect(matching([old, fresh, cache], "code/old")).toEqual([old]);
    expect(matching([old, fresh, cache], "")).toEqual([old, fresh, cache]);
  });

  it("splits the groups between the developer's things and the system's", () => {
    expect(MODULES.developer).toEqual(["projects", "packages", "xcode"]);
    expect(MODULES.system).toEqual(["apps", "logs", "trash"]);
    expect(MODULES.large).toEqual(["videos", "archives", "installers", "audio", "images", "documents", "otherFiles"]);
  });
});
