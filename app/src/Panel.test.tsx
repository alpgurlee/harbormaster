import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { I18nProvider } from "./i18n";
import { Panel } from "./Panel";
import type { Service, Update } from "./types";

afterEach(cleanup);

const scrolled = vi.fn();
beforeEach(() => {
  scrolled.mockClear();
  Element.prototype.scrollIntoView = scrolled;
});

/** Text that is split across elements, such as a figure in bold. */
const byContent = (text: string) => (_: string, element: Element | null) =>
  element?.textContent === text && [...element.children].every((child) => child.textContent !== text);

const MIB = 1024 * 1024;
const NOW = Date.UTC(2026, 9, 16, 12, 0, 0);

function server(port: number, project: string, branch: string, memoryMib: number, extra: Partial<Service> = {}): Service {
  return {
    id: `${port}`,
    classification: "dev",
    protected: null,
    ports: [{ port, address: "127.0.0.1" }],
    project: { name: project, root: `/dev/${project}`, repoRoot: `/dev/${project}`, framework: null, branch, worktree: false },
    command: "npm run dev",
    startedAt: NOW / 1000 - 3 * 3600,
    memoryBytes: memoryMib * MIB,
    cpuPercent: 2.5,
    processes: [],
    holders: [],
    ancestors: [],
    ...extra,
  };
}

const update: Update = {
  services: [
    server(3000, "acme-store", "main", 812, { ancestors: [{ pid: 9, name: "claude" }] }),
    server(8000, "billing-api", "fix/webhooks", 186, { command: "uvicorn app.main:app --reload" }),
    server(5432, "db", "main", 40, { classification: "other", project: null, command: "postgres -D data" }),
  ],
  memory: { used: 16 * 1024 * MIB, total: 32 * 1024 * MIB },
  scannedAt: NOW,
};

function show(props: Partial<Parameters<typeof Panel>[0]> = {}, locale: "en" | "tr" = "en") {
  const handlers = { onShow: vi.fn(), onOpen: vi.fn(), onStop: vi.fn() };
  render(
    <I18nProvider locale={locale}>
      <Panel update={update} now={NOW} {...handlers} {...props} />
    </I18nProvider>,
  );
  return handlers;
}

describe("Panel", () => {
  it("shows each dev server under its project with its port, branch and memory", () => {
    show();

    const row = screen.getByRole("option", { name: /Port 3000/ });
    expect(within(row).getByText("3000")).toBeTruthy();
    expect(within(row).getByText("main")).toBeTruthy();
    expect(within(row).getByText("npm run dev · Claude Code · 3h 0m")).toBeTruthy();
    expect(within(row).getByText("812 MB")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "acme-store" })).toBeTruthy();
  });

  it("sums only the dev servers in the header", () => {
    show();

    expect(screen.getByText(byContent("2 servers · 998 MB · CPU 5%"))).toBeTruthy();
    expect(screen.getByText("16.0 of 32 GB")).toBeTruthy();
  });

  it("narrows the list to what the search matches", () => {
    show();

    fireEvent.change(screen.getByRole("combobox"), { target: { value: "webhooks" } });

    expect(screen.queryByRole("option", { name: /Port 3000/ })).toBeNull();
    expect(screen.getByRole("option", { name: /Port 8000/ })).toBeTruthy();
  });

  it("matches a port by its first digits", () => {
    show();

    fireEvent.change(screen.getByRole("combobox"), { target: { value: "30" } });

    expect(screen.getByRole("option", { name: /Port 3000/ })).toBeTruthy();
    expect(screen.queryByRole("option", { name: /Port 8000/ })).toBeNull();
  });

  it("keeps other listeners folded until asked", () => {
    show();

    expect(screen.queryByRole("option", { name: /Port 5432/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Other listeners · 1/ }));

    expect(screen.getByRole("option", { name: /Port 5432/ })).toBeTruthy();
  });

  it("opens the selected server with the arrow keys and Enter", () => {
    const { onShow } = show();

    const search = screen.getByRole("combobox");
    expect(document.activeElement).toBe(search);
    fireEvent.keyDown(search, { key: "ArrowDown" });
    fireEvent.keyDown(search, { key: "ArrowDown" });

    const selected = screen.getByRole("option", { name: /Port 8000/ });
    expect(search.getAttribute("aria-activedescendant")).toBe(selected.id);
    expect(selected.getAttribute("aria-selected")).toBe("true");
    expect(scrolled).toHaveBeenCalled();

    fireEvent.keyDown(search, { key: "Enter" });
    expect(onShow).toHaveBeenCalledWith(expect.objectContaining({ id: "8000" }));
  });

  it("leaves Enter to other controls and to an input method", () => {
    const { onShow } = show();
    const search = screen.getByRole("combobox");
    fireEvent.keyDown(search, { key: "ArrowDown" });

    fireEvent.keyDown(search, { key: "Enter", isComposing: true });
    // WebKit ends the composition first and sends the confirming Enter as 229.
    fireEvent.keyDown(search, { key: "Enter", keyCode: 229 });
    fireEvent.keyDown(search, { key: "Enter", shiftKey: true });
    fireEvent.keyDown(screen.getByRole("button", { name: /Other listeners/ }), { key: "Enter" });

    expect(onShow).not.toHaveBeenCalled();
  });

  it("keeps the focus in the search when a row's shortcut is clicked", () => {
    const { onOpen } = show();
    const open = screen.getAllByLabelText("Open in browser", { selector: "button" })[0];

    expect(fireEvent.mouseDown(open)).toBe(false);
    fireEvent.click(open);

    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(screen.getByRole("combobox"));
  });

  it("names a server outside any project by what runs", () => {
    show();
    fireEvent.click(screen.getByRole("button", { name: /Other listeners/ }));

    expect(screen.getByRole("option", { name: "Port 5432, postgres -D data, 40 MB" })).toBeTruthy();
  });

  it("keeps two projects with the same folder name apart", () => {
    const client = (port: number, root: string): Service => ({
      ...server(port, "client", "main", 10),
      id: `${port}`,
      project: { name: "client", root, repoRoot: root, framework: null, branch: "main", worktree: false },
    });
    show({ update: { ...update, services: [client(3000, "/work/client"), client(4000, "/home/client")] } });

    const headings = screen.getAllByRole("heading", { name: "client" });
    expect(headings).toHaveLength(2);
    expect(headings[0].id).not.toBe(headings[1].id);
    expect(screen.getAllByRole("option")).toHaveLength(2);
  });

  it("says how fresh the scan is", () => {
    show({ now: NOW + 2_000 });
    expect(screen.getByText("Updated just now")).toBeTruthy();

    cleanup();
    show({ now: NOW + 120_000 });
    expect(screen.queryByText("Updated just now")).toBeNull();
    expect(screen.getByText(/^Updated /)).toBeTruthy();
  });

  it("waits for the first scan before saying nothing listens", () => {
    show({ update: null });

    expect(screen.getByText("Looking for servers…")).toBeTruthy();
  });

  it("speaks Turkish with Turkish numbers", () => {
    show({}, "tr");

    expect(screen.getByText(byContent("2 sunucu · 998 MB · CPU %5"))).toBeTruthy();
    expect(screen.getByText("16,0 / 32 GB")).toBeTruthy();
  });
});
