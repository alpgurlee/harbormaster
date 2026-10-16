import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { Cleaner, type CleanerBackend } from "./Cleaner";
import { I18nProvider } from "./i18n";
import type { Finding, RunEvent, ScanEvent } from "./types";

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
  const runs: { token: number; paths: string[]; onEvent: (event: RunEvent) => void }[] = [];
  let stops = 0;
  let refusal: unknown = null;
  const revealed: string[] = [];
  const backend: CleanerBackend = {
    scan: (onEvent) => {
      scans.push(onEvent);
      return refusal === null ? Promise.resolve() : Promise.reject(refusal);
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
  };
  return {
    backend,
    scans,
    runs,
    revealed,
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
  act(() => driver.scans[driver.scans.length - 1]({ kind: "done", token: 7, home: HOME, findings }));
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
    section("System junk");
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
    act(() => run.onEvent({ kind: "done", removals: [], freedBytes: GB }));

    expect(hero()).toBe("1.0 GB freed1 item cleaned up · 1 not cleaned up");
    expect(screen.getByText("left alone: node is using it")).toBeTruthy();
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
    act(() => run.onEvent({ kind: "done", removals: [], freedBytes: 0 }));

    expect(hero()).toBe("0 MB freed0 items cleaned up · 2 not cleaned up");
    expect(screen.getByText("stopped during cleanup; some contents may have been deleted")).toBeTruthy();
    expect(screen.getByText("stopped before its turn")).toBeTruthy();
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
