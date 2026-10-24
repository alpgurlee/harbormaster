import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Preview, QuitPreview, Stopped } from "./api";
import { Cleaner, type CleanerBackend } from "./Cleaner";
import { Detail } from "./Detail";
import { I18nProvider } from "./i18n";
import { Panel } from "./Panel";
import { QuitSheet, type Quitter } from "./QuitSheet";
import { StopSheet, type Stopper } from "./StopSheet";
import type { AppUsage, Finding, RunEvent, ScanEvent, Service, Update } from "./types";

// Where a stop is the forced one, the panel says so.
vi.mock("./platform", () => ({
  platform: "windows",
  stopsAtOnce: true,
  closesWindows: true,
  cleanerSections: ["developer"],
  mapsHome: false,
  revealsIn: "revealInExplorer",
  bin: {
    goes: "goesToRecycleBin",
    confirm: "confirmRecycled",
    moved: "movedToRecycleBin",
    hint: "recycleBinHint",
    outcome: "outcomeRecycled",
  },
}));

afterEach(cleanup);

const NOW = Date.UTC(2026, 9, 16, 12, 0, 0);

const service: Service = {
  id: "20-1",
  classification: "dev",
  protected: null,
  ports: [{ port: 3000, address: "127.0.0.1" }],
  project: { name: "acme-store", root: "C:\\code\\acme-store", repoRoot: "C:\\code\\acme-store", framework: null, branch: "main", worktree: false },
  command: "npm run dev",
  startedAt: NOW / 1000 - 3600,
  memoryBytes: null,
  cpuPercent: null,
  processes: [{ pid: 20, name: "node", command: "npm run dev", memoryBytes: null, cpuPercent: null }],
  holders: [],
  ancestors: [],
};

const ready: Preview = {
  kind: "ready",
  token: 7,
  stops: [{ pid: 20, name: "node", command: "npm run dev" }],
  frees: [3000],
  staysOpen: [{ pid: 9, name: "pwsh" }],
};

const ended: Stopped = { outcomes: [{ pid: 20, name: "node", outcome: "stopped" }], stillRunning: false };

async function open(stopper: Partial<Stopper>, locale: "en" | "tr" = "en") {
  const handlers = { onClose: vi.fn(), onDone: vi.fn() };
  const full: Stopper = {
    preview: vi.fn(async () => ready),
    stop: vi.fn(async () => ended),
    force: vi.fn(async () => ended),
    ...stopper,
  };
  await act(async () => {
    render(
      <I18nProvider locale={locale}>
        <StopSheet service={service} stopper={full} {...handlers} />
      </I18nProvider>,
    );
  });
  return { ...handlers, stopper: full };
}

describe("on Windows", () => {
  it("asks once to force quit, saying the processes aren't asked first", async () => {
    const { stopper, onDone } = await open({});

    expect(screen.getByRole("dialog", { name: "Force quit acme-store on :3000?" })).toBeTruthy();
    expect(screen.getByText(/without being asked to quit/)).toBeTruthy();
    expect(screen.queryByText(/as Ctrl\+C would/)).toBeNull();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Force quit" })));

    expect(stopper.stop).toHaveBeenCalledWith(7);
    expect(onDone).toHaveBeenCalled();
  });

  it("offers no second round when something outlived the forced quit", async () => {
    const stuck: Stopped = { outcomes: [{ pid: 20, name: "node", outcome: "stillRunning" }], stillRunning: true };
    const { stopper, onDone } = await open({ stop: vi.fn(async () => stuck) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Force quit" })));

    expect(screen.getByText("still running")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Force stop" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Force quit" })).toBeNull();
    expect(screen.getByRole("button", { name: "Close" })).toBeTruthy();
    expect(stopper.force).not.toHaveBeenCalled();
    expect(onDone).not.toHaveBeenCalled();
  });

  it("says when Windows won't let it end a process", async () => {
    await open({ stop: vi.fn(async () => Promise.reject({ code: "notAllowed" })) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Force quit" })));

    expect(screen.getByText(/Windows wouldn't let Harbormaster make sure of one of its processes, or end it/)).toBeTruthy();
  });

  it("won't force quit a server that started programs outside it", async () => {
    const { stopper } = await open({ preview: vi.fn(async () => ({ kind: "refused", reason: { code: "strays" } }) as Preview) });

    expect(screen.getByText(/started other programs that aren't part of it/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Force quit" })).toBeNull();
    expect(stopper.stop).not.toHaveBeenCalled();
  });

  it("asks in Turkish too", async () => {
    await open({}, "tr");

    expect(screen.getByRole("dialog", { name: ":3000 üzerindeki acme-store zorla sonlandırılsın mı?" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Zorla sonlandır" })).toBeTruthy();
  });

  it("names the stop a force quit wherever it is offered", () => {
    const update: Update = { services: [service], memory: null, apps: null, scannedAt: NOW, thermal: "nominal" };
    render(
      <I18nProvider locale="en">
        <Panel update={update} now={NOW} tints={new Map()} onShow={vi.fn()} onOpen={vi.fn()} onStop={vi.fn()} />
        <Detail service={service} now={NOW} onBack={vi.fn()} onOpen={vi.fn()} onCopy={vi.fn()} onReveal={vi.fn()} onStop={vi.fn()} />
      </I18nProvider>,
    );

    // The row's buttons are the mouse's shortcuts, hidden from assistive tech.
    expect(screen.getByTitle("Force quit")).toBeTruthy();
    expect(screen.queryByTitle("Stop")).toBeNull();
    expect(screen.getByRole("button", { name: "Force quit…" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Stop/ })).toBeNull();
  });
});

const chrome: AppUsage = { id: "user/path:c:\\program files\\google\\chrome\\application\\chrome.exe", name: "chrome.exe", memoryBytes: 3 * 1024 ** 3, system: false };

const forced: QuitPreview = {
  kind: "programs",
  token: 9,
  quits: [
    { pid: 40, name: "chrome.exe", command: null },
    { pid: 41, name: "chrome.exe", command: "chrome.exe --type=renderer" },
  ],
  staysOpen: [],
};

async function quitting(quitter: Partial<Quitter>, app: AppUsage = chrome) {
  const handlers = { onClose: vi.fn(), onDone: vi.fn() };
  const full: Quitter = {
    preview: vi.fn(async (): Promise<QuitPreview> => ({ kind: "app", token: 8 })),
    quit: vi.fn(async () => ended),
    force: vi.fn(async () => forced),
    ...quitter,
  };
  await act(async () => {
    render(
      <I18nProvider locale="en">
        <QuitSheet app={app} quitter={full} {...handlers} />
      </I18nProvider>,
    );
  });
  return { ...handlers, quitter: full };
}

describe("closing an app on Windows", () => {
  it("asks its windows to close, as their close buttons do", async () => {
    const { quitter, onDone } = await quitting({});

    expect(screen.getByRole("dialog", { name: "Close chrome?" })).toBeTruthy();
    expect(screen.getByText(/as its close button would/)).toBeTruthy();
    expect(screen.queryByText(/⌘Q/)).toBeNull();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Close windows" })));

    expect(quitter.quit).toHaveBeenCalledWith(8);
    expect(quitter.force).not.toHaveBeenCalled();
    expect(onDone).toHaveBeenCalled();
  });

  it("offers to force what stayed open, but only after showing what that ends", async () => {
    const open_: Stopped = {
      outcomes: [
        { pid: 40, name: "chrome.exe", outcome: "stillRunning" },
        { pid: 41, name: "chrome.exe", outcome: "stillRunning" },
      ],
      stillRunning: true,
    };
    const quit = vi.fn(async (token: number) => (token === 8 ? open_ : ended));
    const { quitter, onDone } = await quitting({ quit });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Close windows" })));

    expect(screen.getByText(/chrome is still open/)).toBeTruthy();
    expect(quitter.force).not.toHaveBeenCalled();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Force quit…" })));

    expect(quitter.force).toHaveBeenCalledWith(chrome.id);
    expect(screen.getByRole("dialog", { name: "Force quit chrome?" })).toBeTruthy();
    expect(screen.getByText("Will end")).toBeTruthy();
    expect(screen.getByText("PID 41")).toBeTruthy();
    expect(screen.getByText(/without being asked to quit/)).toBeTruthy();
    expect(quit).toHaveBeenCalledTimes(1);

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Force quit" })));

    expect(quit).toHaveBeenLastCalledWith(9);
    expect(onDone).toHaveBeenCalled();
  });

  it("says why a window wouldn't take the request", async () => {
    const refused: Stopped = {
      outcomes: [{ pid: 40, name: "chrome.exe", outcome: { failed: "it may run as administrator" } }],
      stillRunning: false,
    };
    await quitting({ quit: vi.fn(async () => refused) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Close windows" })));

    expect(screen.getByText(/it may run as administrator/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Force quit…" })).toBeTruthy();
  });

  it("ends a program without windows at once, and says so first", async () => {
    const node: AppUsage = { id: "user/path:c:\\tools\\node.exe", name: "node.exe", memoryBytes: 1, system: false };
    const { quitter } = await quitting(
      {
        preview: vi.fn(async (): Promise<QuitPreview> => ({
          kind: "programs",
          token: 5,
          quits: [{ pid: 50, name: "node.exe", command: "node watch.js" }],
          staysOpen: [],
        })),
      },
      node,
    );

    expect(screen.getByRole("dialog", { name: "Force quit node?" })).toBeTruthy();
    expect(screen.getByText(/without being asked to quit/)).toBeTruthy();
    expect(screen.queryByText(/Nothing is forced/)).toBeNull();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Force quit" })));
    expect(quitter.quit).toHaveBeenCalledWith(5);
  });

  it("offers to close each app the memory list shows, but never the system", () => {
    const apps: AppUsage[] = [chrome, { id: "system/name:dwm.exe", name: "dwm.exe", memoryBytes: 1, system: true }];
    const update: Update = { services: [], memory: { used: 8 * 1024 ** 3, total: 16 * 1024 ** 3 }, apps, scannedAt: NOW, thermal: "nominal" };
    const onQuit = vi.fn();
    render(
      <I18nProvider locale="en">
        <Panel update={update} now={NOW} tints={new Map()} onShow={vi.fn()} onOpen={vi.fn()} onStop={vi.fn()} onQuit={onQuit} />
      </I18nProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "8.0 of 16 GB" }));

    const list = screen.getByRole("list", { name: "Memory by app" });
    expect(within(list).getAllByRole("button").map((button) => button.textContent)).toEqual(["Close"]);
    fireEvent.click(within(list).getByRole("button", { name: "Close chrome" }));
    expect(onQuit).toHaveBeenCalledWith(chrome);
  });
});

describe("the cleaner on Windows", () => {
  const npm: Finding = {
    group: "packages",
    kind: "rebuildable",
    name: "npm",
    path: "C:\\Users\\dev\\AppData\\Local\\npm-cache\\_cacache",
    bytes: 1024 ** 3,
    picked: true,
    inUse: null,
    touched: null,
    project: null,
    emptied: true,
  };

  function cleaner() {
    const scans: ((event: ScanEvent) => void)[] = [];
    const runs: ((event: RunEvent) => void)[] = [];
    const revealed: string[] = [];
    const nothing = () => Promise.resolve();
    const backend: CleanerBackend = {
      scan: (onEvent) => {
        scans.push(onEvent);
        return Promise.resolve();
      },
      hunt: nothing,
      apps: nothing,
      run: (_token, _paths, onEvent) => {
        runs.push(onEvent);
        return Promise.resolve();
      },
      stop: nothing,
      disk: () => Promise.resolve({ total: 500 * 1024 ** 3, free: 100 * 1024 ** 3 }),
      reveal: (path) => {
        revealed.push(path);
        return Promise.resolve();
      },
      recheck: () => new Promise(() => {}),
      map: nothing,
      revealArea: nothing,
      daemons: () => Promise.resolve([]),
      daemonPreview: () => new Promise(() => {}),
      daemonStop: () => new Promise(() => {}),
    };
    render(
      <I18nProvider locale="en">
        <Cleaner backend={backend} now={NOW} />
      </I18nProvider>,
    );
    return { scans, runs, revealed };
  }

  /** Ends the scan with `findings`, as the app sends them. */
  function scanned(driver: ReturnType<typeof cleaner>, findings: Finding[]) {
    fireEvent.click(screen.getByRole("button", { name: "Scan" }));
    act(() =>
      driver.scans[0]({
        kind: "done",
        token: 3,
        home: "C:\\Users\\dev",
        findings,
        scanned: true,
        hunted: false,
        appsListed: false,
        idleDays: 30,
      }),
    );
  }

  it("offers only what developer tools leave, and no map", async () => {
    const driver = cleaner();
    await act(async () => {});

    const sections = within(screen.getByRole("navigation", { name: "Clean up" })).getAllByRole("button");
    expect(sections.map((button) => button.textContent?.replace(/[\d.]+ GB$/, ""))).toEqual(["Overview", "Developer"]);
    expect(screen.getByText("Looks for build outputs and the caches developer tools keep. Nothing goes without your say.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Scan" }));
    act(() =>
      driver.scans[0]({
        kind: "done",
        token: 3,
        home: "C:\\Users\\dev",
        findings: [npm],
        scanned: true,
        hunted: false,
        appsListed: false,
        idleDays: 30,
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: /^Developer/ }));
    expect(screen.getByText("~\\AppData\\Local\\npm-cache\\_cacache")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Show in File Explorer: npm" }));

    expect(driver.revealed).toEqual([npm.path]);
  });

  it("sends what may be wanted back to the Recycle Bin, and says so", async () => {
    const maven: Finding = {
      ...npm,
      kind: "personal",
      name: "Maven",
      path: "C:\\Users\\dev\\.m2\\repository",
      picked: false,
      emptied: false,
    };
    const driver = cleaner();
    await act(async () => {});
    scanned(driver, [maven]);
    fireEvent.click(screen.getByRole("button", { name: /^Developer/ }));
    expect(screen.getByText("~\\.m2\\repository · goes to the Recycle Bin")).toBeTruthy();

    fireEvent.click(screen.getByRole("checkbox", { name: "Maven" }));
    fireEvent.click(screen.getByRole("button", { name: "Clean up…" }));
    const sheet = within(screen.getByRole("dialog", { name: "Clean up 1.0 GB?" }));
    expect(sheet.getByText("1 item goes to the Recycle Bin, where you can restore it.")).toBeTruthy();
    fireEvent.click(sheet.getByRole("button", { name: "Clean up" }));
    act(() => driver.runs[0]({ kind: "removed", path: maven.path, outcome: "trashed" }));
    act(() =>
      driver.runs[0]({ kind: "done", removals: [], freedBytes: 0, token: 9, findings: [], scanned: true, hunted: false, appsListed: false }),
    );

    expect(screen.getByTestId("hero").textContent).toBe("1.0 GB moved to the Recycle Bin1 item cleaned up · 0 MB freed so far");
    expect(screen.getByText("The room comes back once the Recycle Bin is emptied.")).toBeTruthy();
    expect(screen.getByText("moved to the Recycle Bin")).toBeTruthy();
  });
});
