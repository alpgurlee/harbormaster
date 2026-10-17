import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Cleaner, type CleanerBackend } from "./Cleaner";
import { I18nProvider } from "./i18n";
import type { Area, Finding, MapEvent, RunEvent, ScanEvent } from "./types";

afterEach(cleanup);

const GB = 1024 ** 3;
const DAY = 86_400;
const NOW = Date.UTC(2026, 9, 16, 12, 0, 0);
const HOME = "/Users/dev";

function finding(name: string, path: string, extra: Partial<Finding> = {}): Finding {
  return {
    group: "packages",
    kind: "rebuildable",
    name,
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

const shop = finding("shop · node_modules", `${HOME}/code/shop/node_modules`, {
  group: "projects",
  emptied: false,
  touched: NOW / 1000 - 60 * DAY,
  project: `${HOME}/code/shop`,
});
const tool = finding("tool · target", `${HOME}/code/tool/target`, {
  group: "projects",
  bytes: 4 * GB,
  picked: false,
  inUse: "localhost:3000",
  emptied: false,
  project: `${HOME}/code/tool`,
});
const npm = finding("npm", `${HOME}/.npm/_cacache`, { bytes: 2 * GB });
const pip = finding("pip", `${HOME}/Library/Caches/pip`, { picked: false });
const archive = finding("2026-09-20", `${HOME}/Library/Developer/Xcode/Archives/2026-09-20`, {
  group: "xcode",
  kind: "personal",
  picked: false,
  emptied: false,
});
const trash = finding("Trash", `${HOME}/.Trash`, { group: "trash", kind: "discarded", picked: false });
const FINDINGS = [shop, tool, npm, pip, archive, trash];

/** A backend the test drives: it keeps what the window asked for. */
function fake() {
  const scans: ((event: ScanEvent) => void)[] = [];
  const hunts: ((event: ScanEvent) => void)[] = [];
  const listings: ((event: ScanEvent) => void)[] = [];
  const runs: { token: number; paths: string[]; onEvent: (event: RunEvent) => void }[] = [];
  let stops = 0;
  let refusal: unknown = null;
  const revealed: string[] = [];
  const rechecks: { token: number; answer: (findings: Finding[]) => void }[] = [];
  const maps: { others: boolean; onEvent: (event: MapEvent) => void }[] = [];
  const shownAreas: { token: number; id: number }[] = [];
  const backend: CleanerBackend = {
    scan: (onEvent) => {
      scans.push(onEvent);
      return refusal === null ? Promise.resolve() : Promise.reject(refusal);
    },
    hunt: (onEvent) => {
      hunts.push(onEvent);
      return Promise.resolve();
    },
    apps: (onEvent) => {
      listings.push(onEvent);
      return Promise.resolve();
    },
    run: (token, paths, onEvent) => {
      runs.push({ token, paths, onEvent });
      return Promise.resolve();
    },
    stop: () => {
      stops += 1;
      return Promise.resolve();
    },
    disk: () => Promise.resolve({ total: 500 * GB, free: 100 * GB }),
    reveal: (path) => {
      revealed.push(path);
      return Promise.resolve();
    },
    recheck: (token) => new Promise((answer) => rechecks.push({ token, answer })),
    map: (others, onEvent) => {
      maps.push({ others, onEvent });
      return refusal === null ? Promise.resolve() : Promise.reject(refusal);
    },
    revealArea: (token, id) => {
      shownAreas.push({ token, id });
      return Promise.resolve();
    },
  };
  return {
    backend,
    maps,
    shownAreas,
    scans,
    hunts,
    listings,
    runs,
    revealed,
    rechecks,
    stops: () => stops,
    refuse: (next: unknown) => {
      refusal = next;
    },
  };
}

function show(backend: CleanerBackend, locale: "en" | "tr" = "en") {
  return render(
    <I18nProvider locale={locale}>
      <Cleaner backend={backend} now={NOW} />
    </I18nProvider>,
  );
}

const scanButton = () => screen.getByRole("button", { name: "Scan" });

/** Asks for a scan, and lists `findings` as it ends. */
function found(driver: ReturnType<typeof fake>, findings = FINDINGS) {
  if (driver.scans.length === 0) fireEvent.click(scanButton());
  act(() =>
    driver.scans[driver.scans.length - 1]({ kind: "done", token: 7, home: HOME, findings, scanned: true, hunted: false, appsListed: false }),
  );
}

const film = finding("trip.mov", `${HOME}/Movies/trip.mov`, {
  group: "videos",
  kind: "personal",
  bytes: 2 * GB,
  picked: false,
  emptied: false,
  touched: NOW / 1000 - 90 * DAY,
});

/** Ends the latest hunt with `findings`, as the app sends them: a scan's too, when one came first. */
function hunted(driver: ReturnType<typeof fake>, findings: Finding[], scanned = false) {
  act(() => driver.hunts[driver.hunts.length - 1]({ kind: "done", token: 8, home: HOME, findings, scanned, hunted: true, appsListed: false }));
}

/** Ends a removal as the app does: what it did, and what is shown from then on. */
function ended(run: { onEvent: (event: RunEvent) => void }, freedBytes: number, findings: Finding[]) {
  act(() => run.onEvent({ kind: "done", removals: [], freedBytes, token: 9, findings, scanned: true, hunted: false, appsListed: false }));
}

/** Opens a section of the window from its side. */
const section = (name: string) => fireEvent.click(screen.getByRole("button", { name: new RegExp(`^${name}`) }));

const hero = () => screen.getByTestId("hero").textContent;

describe("the cleaner window", () => {
  it("shows the disk, and scans only when asked", async () => {
    const driver = fake();
    show(driver.backend);

    expect(await screen.findByText("100.0 GB")).toBeTruthy();
    expect(screen.getByText("free of 500.0 GB")).toBeTruthy();
    expect(driver.scans).toHaveLength(0);
    expect(screen.getByRole("button", { name: /^Developer/ })).toHaveProperty("disabled", true);

    fireEvent.click(scanButton());

    expect(driver.scans).toHaveLength(1);
    expect(screen.getByText("Looking for projects and caches…")).toBeTruthy();
  });

  it("shows how far the scan has come, then what it chose, section by section", () => {
    const driver = fake();
    show(driver.backend);
    fireEvent.click(scanButton());

    act(() => driver.scans[0]({ kind: "measured", done: 2, total: 5, bytes: 2 * GB }));
    act(() => driver.scans[0]({ kind: "measured", done: 1, total: 5, bytes: GB }));
    expect(screen.getByText("Measured 2 of 5 places")).toBeTruthy();
    expect(screen.getByText("3.0 GB")).toBeTruthy();

    found(driver);
    expect(hero()).toBe("3.0 GBchosen to clean up, of 10.0 GB found");
    expect(screen.getByText("9.0 GB found · 3.0 GB chosen")).toBeTruthy();
    expect(screen.getByText("1.0 GB found · 0 MB chosen")).toBeTruthy();
  });

  it("reviews a section: its groups on one side, the items of one on the other", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);

    section("Developer");

    // All that can go of a group is chosen; what is in use can't be.
    expect(screen.getByRole("checkbox", { name: "Projects" })).toHaveProperty("checked", true);
    expect(screen.getByRole("checkbox", { name: "Package caches" })).toHaveProperty("indeterminate", true);
    const items = within(screen.getByRole("list", { name: "Projects" }));
    expect(items.getAllByRole("checkbox").map((box) => box.getAttribute("aria-label"))).toEqual([
      "tool · target",
      "shop · node_modules",
    ]);
    expect(items.getByText("~/code/shop · last touched 2 months ago")).toBeTruthy();
    expect(items.getByText("In use by localhost:3000")).toBeTruthy();
    expect(items.getByRole("checkbox", { name: "tool · target" })).toHaveProperty("disabled", true);

    fireEvent.click(items.getByRole("checkbox", { name: "shop · node_modules" }));
    expect(screen.getByRole("checkbox", { name: "Projects" })).toHaveProperty("checked", false);
    const shown = () => screen.getByRole("checkbox", { name: "Choose all shown" });
    expect(shown()).toHaveProperty("checked", false);
    fireEvent.click(shown());
    expect(screen.getByRole("checkbox", { name: "Projects" })).toHaveProperty("checked", true);
    expect(shown()).toHaveProperty("checked", true);

    // Only what the search shows is chosen at once, and an item in use never is.
    fireEvent.change(screen.getByRole("searchbox", { name: "Search" }), { target: { value: "TOOL" } });
    expect(within(screen.getByRole("list", { name: "Projects" })).getAllByRole("checkbox")).toHaveLength(1);
    expect(shown()).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByRole("searchbox", { name: "Search" }), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: /^Package caches/ }));
    expect(screen.getByRole("list", { name: "Package caches" })).toBeTruthy();
    expect(shown()).toHaveProperty("indeterminate", true);
  });

  it("looks at the list again on coming back: what went drops out, what is in use is read anew", async () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    section("Developer");
    fireEvent.click(screen.getByRole("button", { name: /^Package caches/ }));
    expect(screen.getByText("2 items")).toBeTruthy();

    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(driver.rechecks.map(({ token }) => token)).toEqual([7]);
    // npm was removed by hand meanwhile, and pip is in use now.
    await act(async () => driver.rechecks[0].answer([shop, tool, { ...pip, inUse: "java" }, archive, trash]));

    const items = within(screen.getByRole("list", { name: "Package caches" }));
    expect(items.queryByText("npm")).toBeNull();
    expect(items.getByText("In use by java")).toBeTruthy();
    expect(items.getByRole("checkbox", { name: "pip" })).toHaveProperty("disabled", true);
    expect(screen.getByText("1 item")).toBeTruthy();
  });

  it("looks again at most every few seconds, and never over a newer list", async () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    const focus = () =>
      act(() => {
        window.dispatchEvent(new Event("focus"));
      });

    focus();
    focus();
    expect(driver.rechecks).toHaveLength(1);
    await act(async () => driver.rechecks[0].answer(FINDINGS));
    focus();
    expect(driver.rechecks).toHaveLength(1);
    const later = Date.now() + 5_000;
    const clock = vi.spyOn(Date, "now").mockReturnValue(later);
    focus();
    clock.mockRestore();
    expect(driver.rechecks).toHaveLength(2);

    // Another scan came and went before the answer: it is about the old list.
    fireEvent.click(screen.getByRole("button", { name: "Scan again" }));
    found(driver, [npm]);
    await act(async () => driver.rechecks[1].answer([]));
    expect(screen.getByText("1 item")).toBeTruthy();
  });

  it("looks again once the wait is over when coming back too soon", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    try {
      const driver = fake();
      show(driver.backend);
      found(driver);
      const focus = () =>
        act(() => {
          window.dispatchEvent(new Event("focus"));
        });
      focus();
      await act(async () => driver.rechecks[0].answer([shop, tool, npm, { ...pip, inUse: "java" }, archive, trash]));

      // Java was quit at once, and the window came back within the wait.
      act(() => vi.advanceTimersByTime(1_000));
      focus();
      expect(driver.rechecks).toHaveLength(1);
      act(() => vi.advanceTimersByTime(2_000));

      expect(driver.rechecks).toHaveLength(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("cleans nothing once looking again leaves nothing chosen", async () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));

    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    await act(async () => driver.rechecks[0].answer(FINDINGS.map((finding) => ({ ...finding, inUse: "java" }))));

    const go = within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" });
    expect(go).toHaveProperty("disabled", true);
    fireEvent.click(go);
    expect(driver.runs).toHaveLength(0);
  });

  it("keeps what the search hides as it was when choosing all shown", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    section("Developer");
    fireEvent.click(screen.getByRole("button", { name: /^Package caches/ }));
    const search = screen.getByRole("searchbox", { name: "Search" });

    for (const chosen of [true, false]) {
      fireEvent.change(search, { target: { value: "pip" } });
      fireEvent.click(screen.getByRole("checkbox", { name: "Choose all shown" }));
      fireEvent.change(search, { target: { value: "" } });

      const items = within(screen.getByRole("list", { name: "Package caches" }));
      expect(items.getByRole("checkbox", { name: "npm" })).toHaveProperty("checked", true);
      expect(items.getByRole("checkbox", { name: "pip" })).toHaveProperty("checked", chosen);
    }
  });

  it("says how to free what is in use, only where something is", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    section("Developer");

    expect(screen.getByText("Quit what is using an item, then come back here: the list looks again.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /^Xcode/ }));
    expect(screen.queryByText(/Quit what is using/)).toBeNull();
  });

  it("hunts for large files only when asked, from their own section", () => {
    const driver = fake();
    show(driver.backend);

    section("Large files");
    expect(screen.getByText(/files over 50 MB/)).toBeTruthy();
    fireEvent.click(scanButton());
    expect(driver.hunts).toHaveLength(1);
    expect(driver.scans).toHaveLength(0);
    expect(screen.getByText("Looking through your folders…")).toBeTruthy();
    act(() => driver.hunts[0]({ kind: "measured", done: 2, total: 5, bytes: 2 * GB }));
    expect(screen.getByText("Looked through 2 of 5 folders")).toBeTruthy();

    hunted(driver, [film]);

    const items = within(screen.getByRole("list", { name: "Videos" }));
    expect(items.getByRole("checkbox", { name: "trip.mov" })).toHaveProperty("checked", false);
    expect(items.getByText("~/Movies · changed 3 months ago")).toBeTruthy();
    // Nothing was scanned for yet: the other sections wait for a scan.
    expect(screen.getByRole("button", { name: "Developer" })).toHaveProperty("disabled", true);
    section("Overview");
    expect(scanButton()).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Scan again" })).toBeNull();
  });

  it("keeps the scan's choice through a hunt, and the hunt's through a scan", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    expect(screen.getByText("2 items")).toBeTruthy();

    section("Large files");
    fireEvent.click(scanButton());
    hunted(driver, [...FINDINGS, film], true);
    expect(screen.getByText("2 items")).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: "trip.mov" }));
    expect(screen.getByText("3 items")).toBeTruthy();

    section("Overview");
    fireEvent.click(screen.getByRole("button", { name: "Scan again" }));
    act(() =>
      driver.scans[driver.scans.length - 1]({
        kind: "done",
        token: 9,
        home: HOME,
        findings: [npm, film],
        scanned: true,
        hunted: true,
        appsListed: false,
      }),
    );
    expect(screen.getByText("2 items")).toBeTruthy();
  });

  it("says no large files were found only of the folders it looked in", () => {
    const driver = fake();
    show(driver.backend);
    section("Large files");
    fireEvent.click(scanButton());
    // Stopped at once, the hunt ends with what it found by then: nothing.
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(driver.stops()).toBe(1);

    hunted(driver, []);

    expect(screen.getByText("No files over 50 MB were found in the folders looked in.")).toBeTruthy();
  });

  const docker = finding("Docker", "/Applications/Docker.app", {
    group: "applications",
    kind: "personal",
    bytes: 2 * GB,
    picked: false,
    emptied: false,
  });
  const slack = finding("Slack", "/Applications/Slack.app", {
    group: "applications",
    kind: "personal",
    picked: false,
    emptied: false,
    inUse: "Slack",
  });

  /** Ends the latest listing of apps with `findings`, as the app sends them. */
  function listedApps(driver: ReturnType<typeof fake>, findings: Finding[]) {
    act(() =>
      driver.listings[driver.listings.length - 1]({
        kind: "done",
        token: 9,
        home: HOME,
        findings,
        scanned: false,
        hunted: false,
        appsListed: true,
      }),
    );
  }

  it("lists the user's apps only when asked, from their own section, none chosen and a running one locked", () => {
    const driver = fake();
    show(driver.backend);

    section("Apps");
    expect(screen.getByText(/are left to Finder/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "List apps" }));
    expect(driver.listings).toHaveLength(1);
    expect(driver.scans).toHaveLength(0);
    expect(screen.getByText("Looking through your apps…")).toBeTruthy();
    act(() => driver.listings[0]({ kind: "measured", done: 3, total: 9, bytes: GB }));
    expect(screen.getByText("Measured 3 of 9 apps")).toBeTruthy();

    listedApps(driver, [docker, slack]);

    const items = within(screen.getByRole("list", { name: "Your apps" }));
    expect(items.getByRole("checkbox", { name: "Docker" })).toHaveProperty("checked", false);
    expect(items.getByText("/Applications · goes to the Trash")).toBeTruthy();
    expect(items.getByRole("checkbox", { name: "Slack" })).toHaveProperty("disabled", true);
    // Nothing was scanned or hunted for: those sections wait.
    expect(screen.getByRole("button", { name: "Developer" })).toHaveProperty("disabled", true);
  });

  it.each(["done", "failed"] as const)("keeps the apps listed through a removal that ends as %s", (kind) => {
    const driver = fake();
    show(driver.backend);
    section("Apps");
    fireEvent.click(screen.getByRole("button", { name: "List apps" }));
    listedApps(driver, [docker, slack]);

    fireEvent.click(screen.getByRole("checkbox", { name: "Docker" }));
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" }));
    const listed = { token: 10, findings: [slack], scanned: false, hunted: false, appsListed: true };
    act(() =>
      driver.runs[0].onEvent(
        kind === "done"
          ? { kind, ...listed, removals: [], freedBytes: 0 }
          : { kind, ...listed, refusal: { code: "failed", message: "no disk" } },
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Back to the list" }));

    expect(screen.getByRole("checkbox", { name: "Slack" })).toHaveProperty("disabled", true);
    expect(screen.queryByRole("button", { name: "List apps" })).toBeNull();
    screen.getByRole("button", { name: "Refresh list" });
  });

  it("says no apps of the user's were found only once it has looked", () => {
    const driver = fake();
    show(driver.backend);
    section("Apps");
    fireEvent.click(screen.getByRole("button", { name: "List apps" }));

    listedApps(driver, []);

    expect(screen.getByText("No apps of yours were found in Applications.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh list" }));
    expect(driver.listings).toHaveLength(2);
  });

  it("shows an item in Finder through the app, by its path", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    section("Developer");

    fireEvent.click(screen.getByRole("button", { name: "Show in Finder: shop · node_modules" }));

    expect(driver.revealed).toEqual([shop.path]);
  });

  it("says what removing will do before anything goes, and sends just the choice", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    section("Developer");
    // Some of it was chosen, so the first click chooses all of it, the second none.
    fireEvent.click(screen.getByRole("checkbox", { name: "Package caches" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Package caches" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Xcode" }));
    section("Leftovers");
    // The Trash's group comes first, its one item after.
    fireEvent.click(screen.getAllByRole("checkbox", { name: "Trash" })[0]);

    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));

    const sheet = within(screen.getByRole("dialog", { name: "Clean up 3.0 GB?" }));
    expect(sheet.getByText("1 item is deleted for good; its tool makes it again.")).toBeTruthy();
    expect(sheet.getByText("1 item goes to the Trash, where you can put it back.")).toBeTruthy();
    expect(sheet.getByText("The Trash is emptied for good.")).toBeTruthy();
    expect(driver.runs).toHaveLength(0);

    fireEvent.click(sheet.getByRole("button", { name: "Clean up" }));
    expect(driver.runs.map(({ token, paths }) => ({ token, paths }))).toEqual([
      { token: 7, paths: [shop.path, archive.path, trash.path] },
    ]);
  });

  it("tells of each removal and what the disk got back", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" }));
    const run = driver.runs[0];

    act(() => run.onEvent({ kind: "removed", path: shop.path, outcome: "deleted" }));
    expect(screen.getByText("Cleaning up… 1 of 2")).toBeTruthy();
    act(() =>
      run.onEvent({ kind: "removed", path: npm.path, outcome: "refused", why: { code: "inUse", by: "node" } }),
    );
    ended(run, GB, [tool, npm, pip, archive, trash]);

    expect(hero()).toBe("1.0 GB freed1 item cleaned up · 1 not cleaned up");
    expect(screen.getByText("left alone: node is using it")).toBeTruthy();
  });

  it("goes back to what is left once a clean-up is done, where it began", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    section("Developer");
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" }));
    const run = driver.runs[0];
    act(() => run.onEvent({ kind: "removed", path: shop.path, outcome: "deleted" }));
    act(() =>
      run.onEvent({ kind: "removed", path: npm.path, outcome: "refused", why: { code: "inUse", by: "node" } }),
    );
    ended(run, GB, [tool, { ...npm, inUse: "node" }, pip, archive, trash]);
    // The sections lead back to the list as well.
    expect(screen.getByRole("button", { name: /^Developer/ })).toHaveProperty("disabled", false);

    fireEvent.click(screen.getByRole("button", { name: "Back to the list" }));

    expect(screen.getByText("0 items")).toBeTruthy();
    expect(within(screen.getByRole("list", { name: "Projects" })).queryByText("shop · node_modules")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^Package caches/ }));
    // What the removal found in use stays out of reach.
    expect(screen.getByRole("checkbox", { name: "npm" })).toHaveProperty("disabled", true);
    fireEvent.click(screen.getByRole("checkbox", { name: "pip" }));
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" }));
    expect(driver.runs.map(({ token, paths }) => ({ token, paths }))[1]).toEqual({ token: 9, paths: [pip.path] });
  });

  it("says how much went to the Trash, whose room comes back once it is emptied", () => {
    const driver = fake();
    show(driver.backend);
    found(driver, [archive]);
    section("Developer");
    fireEvent.click(screen.getByRole("checkbox", { name: "2026-09-20" }));
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" }));
    const run = driver.runs[0];

    act(() => run.onEvent({ kind: "removed", path: archive.path, outcome: "trashed" }));
    ended(run, 0, []);

    expect(hero()).toBe("1.0 GB moved to the Trash1 item cleaned up · 0 MB freed so far");
    expect(screen.getByText("The room comes back once the Trash is emptied.")).toBeTruthy();
  });

  it("never calls what didn't finish untouched", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" }));
    const run = driver.runs[0];

    act(() => run.onEvent({ kind: "removed", path: shop.path, outcome: "interrupted" }));
    act(() => run.onEvent({ kind: "removed", path: npm.path, outcome: "stopped" }));
    ended(run, 0, [tool, npm, pip, archive, trash]);

    expect(hero()).toBe("0 MB freed0 items cleaned up · 2 not cleaned up");
    expect(screen.getByText("stopped during cleanup; some contents may have been deleted")).toBeTruthy();
    expect(screen.getByText("stopped before its turn")).toBeTruthy();
  });

  it("goes back to what is left when the removal began and then failed", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" }));
    const run = driver.runs[0];
    act(() => run.onEvent({ kind: "removed", path: shop.path, outcome: "deleted" }));

    act(() =>
      run.onEvent({
        kind: "failed",
        refusal: { code: "failed", message: "the disk's free space couldn't be read" },
        token: 9,
        findings: [tool, npm, pip, archive, trash],
        scanned: true,
        hunted: false,
        appsListed: false,
      }),
    );

    expect(screen.getByText("Something went wrong: the disk's free space couldn't be read")).toBeTruthy();
    expect(screen.getByText("deleted")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Back to the list" }));
    expect(screen.getByText("0 items")).toBeTruthy();
  });

  it("keeps what was done in sight when the app turns the rest down", async () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    let fail: (refusal: unknown) => void = () => {};
    driver.backend.run = (_token, _paths, onEvent) => {
      onEvent({ kind: "removed", path: shop.path, outcome: "deleted" });
      onEvent({ kind: "removed", path: npm.path, outcome: "interrupted" });
      return new Promise((_, reject) => {
        fail = reject;
      });
    };
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" }));

    await act(async () => fail({ code: "failed", message: "the disk's free space couldn't be read" }));

    expect(screen.getByText("Something went wrong: the disk's free space couldn't be read")).toBeTruthy();
    expect(screen.getByText("deleted")).toBeTruthy();
    expect(screen.getByText("stopped during cleanup; some contents may have been deleted")).toBeTruthy();
  });

  it("keeps a removal that is told of only after the app turned the rest down", async () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    let tell: (event: RunEvent) => void = () => {};
    let fail: (refusal: unknown) => void = () => {};
    driver.backend.run = (_token, _paths, onEvent) => {
      tell = onEvent;
      return new Promise((_, reject) => {
        fail = reject;
      });
    };
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Clean up" }));

    act(() => tell({ kind: "removed", path: shop.path, outcome: "deleted" }));
    await act(async () => fail({ code: "failed", message: "the disk's free space couldn't be read" }));
    act(() => tell({ kind: "removed", path: npm.path, outcome: "interrupted" }));

    expect(screen.getByText("Something went wrong: the disk's free space couldn't be read")).toBeTruthy();
    expect(screen.getByText("deleted")).toBeTruthy();
    expect(screen.getByText("stopped during cleanup; some contents may have been deleted")).toBeTruthy();
  });

  it("gives the focus back to the button that asked when the sheet goes", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);
    const clean = screen.getByRole("button", { name: "Clean up…" });
    clean.focus();

    fireEvent.click(clean);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(clean);
  });

  it("says why the app turned it down, and scans again when asked", async () => {
    const driver = fake();
    driver.refuse({ code: "busy" });
    show(driver.backend);
    fireEvent.click(scanButton());

    expect(await screen.findByText("A scan or clean-up is running already.")).toBeTruthy();
    driver.refuse(null);
    fireEvent.click(screen.getByRole("button", { name: "Scan again" }));
    expect(driver.scans).toHaveLength(2);
    expect(screen.getByText("Looking for projects and caches…")).toBeTruthy();
  });

  it("stops a scan when told", () => {
    const driver = fake();
    show(driver.backend);
    fireEvent.click(scanButton());

    fireEvent.click(screen.getByRole("button", { name: "Stop" }));

    expect(driver.stops()).toBe(1);
  });

  it("speaks Turkish", () => {
    const driver = fake();
    show(driver.backend, "tr");
    fireEvent.click(screen.getByRole("button", { name: "Tara" }));
    found(driver);

    expect(hero()).toBe("3,0 GBtemizlemek için seçili; bulunan 10,0 GB");
    expect(screen.getByRole("button", { name: "Temizle…" })).toBeTruthy();
  });
});

function mapped(id: number, name: string, bytes: number, inside: Area[] = []): Area {
  return { id, name, bytes, folder: inside.length > 0, short: null, inside, rest: 0, folded: false };
}

const ROOT = mapped(0, "dev", 8 * GB, [mapped(1, "Movies", 5 * GB, [mapped(2, "trip.mov", 5 * GB)]), mapped(3, "disk.iso", 3 * GB)]);

describe("the space map", () => {
  // The tiles are laid out once their room is known; the list is what these tests read.
  beforeEach(() => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
  });
  afterEach(() => vi.unstubAllGlobals());

  const section = () => screen.getByRole("button", { name: /^Space map/ });
  const mapButton = () => screen.getByRole<HTMLButtonElement>("button", { name: "Map" });

  /** Opens the map section, and maps with other apps' data if `others`. */
  function asked(others = false) {
    fireEvent.click(section());
    if (others) fireEvent.click(screen.getByRole("checkbox", { name: "Also look in other apps' data" }));
    fireEvent.click(mapButton());
  }

  it("maps the home folder only when asked, other apps' data too if chosen, and lists what it holds", () => {
    const driver = fake();
    show(driver.backend);
    fireEvent.click(section());
    expect(driver.maps).toHaveLength(0);

    asked(true);
    act(() => driver.maps[0].onEvent({ kind: "charting", bytes: 2 * GB, items: 12_345, folder: "Library/Caches" }));

    expect(driver.maps.map((map) => map.others)).toEqual([true]);
    expect(screen.getByRole("status").textContent).toContain("12,345 items looked at");

    act(() => driver.maps[0].onEvent({ kind: "done", token: 4, area: ROOT }));

    screen.getByRole("list", { name: "What Home holds" });
    expect(section().textContent).toContain("8.0 GB");
  });

  it("shows a mapped place in Finder through the map it came from", () => {
    const driver = fake();
    show(driver.backend);
    asked();
    act(() => driver.maps[0].onEvent({ kind: "done", token: 4, area: ROOT }));

    fireEvent.click(screen.getByRole("button", { name: /disk\.iso/ }));
    fireEvent.click(screen.getByRole("button", { name: "Show in Finder" }));

    expect(driver.shownAreas).toEqual([{ token: 4, id: 3 }]);
  });

  it("keeps mapping while another section is looked at, and starts no scan meanwhile", () => {
    const driver = fake();
    show(driver.backend);
    asked();

    fireEvent.click(screen.getByRole("button", { name: "Overview" }));
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Scan" }).disabled).toBe(true);
    screen.getByText("The space map is being made. A scan can start once it is done or stopped.");

    fireEvent.click(section());
    expect(screen.getByRole("status").textContent).toContain("Mapping your home folder…");
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(driver.stops()).toBe(1);
  });

  it("starts no clean-up or scan while mapping, even with a scan's findings shown", () => {
    const driver = fake();
    show(driver.backend);
    found(driver);

    asked();
    fireEvent.click(screen.getByRole("button", { name: "Overview" }));

    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Scan again" }).disabled).toBe(true);
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Clean up…" }).disabled).toBe(true);
  });

  it("starts no map while a scan runs", () => {
    const driver = fake();
    show(driver.backend);
    fireEvent.click(scanButton());

    fireEvent.click(section());

    expect(mapButton().disabled).toBe(true);
    screen.getByText("A scan or clean-up is running. The map can start once it is done.");
  });

  it("says why a map was turned down, ready to map again", async () => {
    const driver = fake();
    driver.refuse({ code: "busy" });
    show(driver.backend);

    asked();

    await screen.findByText("A scan or clean-up is running already.");
    expect(mapButton().disabled).toBe(false);
  });

  it("maps other apps' data too from a place the last map left out", () => {
    const driver = fake();
    show(driver.backend);
    asked();
    const containers = { ...mapped(4, "Containers", 0), folder: true, short: "skipped" as const };
    const root = { ...ROOT, short: "skipped" as const, inside: [...ROOT.inside, containers] };
    act(() => driver.maps[0].onEvent({ kind: "done", token: 4, area: root }));

    fireEvent.click(screen.getByRole("button", { name: /Containers/ }));
    fireEvent.click(screen.getByRole("button", { name: "Map with other apps' data" }));

    expect(driver.maps.map((map) => map.others)).toEqual([false, true]);
  });

  it("offers no map with other apps' data while a scan runs", () => {
    const driver = fake();
    show(driver.backend);
    asked();
    const containers = { ...mapped(4, "Containers", 0), folder: true, short: "skipped" as const };
    const root = { ...ROOT, short: "skipped" as const, inside: [...ROOT.inside, containers] };
    act(() => driver.maps[0].onEvent({ kind: "done", token: 4, area: root }));

    fireEvent.click(screen.getByRole("button", { name: "Overview" }));
    fireEvent.click(scanButton());
    fireEvent.click(section());

    expect(screen.queryByRole("button", { name: "Map with other apps' data" })).toBeNull();
    expect(driver.maps).toHaveLength(1);
  });

  it("maps again as it was asked the first time", () => {
    const driver = fake();
    show(driver.backend);
    asked(true);
    act(() => driver.maps[0].onEvent({ kind: "done", token: 4, area: ROOT }));

    fireEvent.click(screen.getByRole("button", { name: "Map again" }));

    expect(driver.maps.map((map) => map.others)).toEqual([true, true]);
    expect(screen.getByRole("status").textContent).toContain("Mapping your home folder…");
  });
});
