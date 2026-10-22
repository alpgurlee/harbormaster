import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Preview, Stopped } from "./api";
import { Detail } from "./Detail";
import { I18nProvider } from "./i18n";
import { Panel } from "./Panel";
import { StopSheet, type Stopper } from "./StopSheet";
import type { Service, Update } from "./types";

// Where a stop is the forced one, the panel says so.
vi.mock("./platform", () => ({ platform: "windows", stopsAtOnce: true, quitsApps: false, hasCleaner: false }));

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
