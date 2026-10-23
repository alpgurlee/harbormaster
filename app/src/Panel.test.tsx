import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { I18nProvider } from "./i18n";
import { tintsOf } from "./model";
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

/** Each part of the memory breakdown as it reads, its name and figure, without its button. */
const parts = (items: HTMLElement[]) =>
  items.map((item) => {
    const copy = item.cloneNode(true) as HTMLElement;
    copy.querySelectorAll("button").forEach((button) => button.remove());
    return copy.textContent;
  });

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
  apps: [
    { id: "app:Google Chrome", name: "Google Chrome", memoryBytes: 3.2 * 1024 * MIB, system: false },
    { id: "app:Slack", name: "Slack", memoryBytes: 640 * MIB, system: false },
  ],
  scannedAt: NOW,
  thermal: "nominal",
};

/** The update, with the Mac cooling down as `thermal` says and each dev server using `cpu` of one core. */
function heated(thermal: Update["thermal"], cpu: Record<number, number | null>): Update {
  return {
    ...update,
    thermal,
    services: update.services.map((service) => {
      const port = service.ports[0].port;
      return port in cpu ? { ...service, cpuPercent: cpu[port] } : service;
    }),
  };
}

const cooling = "Your Mac is reducing performance to cool down.";

function show(props: Partial<Parameters<typeof Panel>[0]> = {}, locale: "en" | "tr" = "en") {
  const handlers = { onShow: vi.fn(), onOpen: vi.fn(), onStop: vi.fn(), onQuit: vi.fn() };
  render(
    <I18nProvider locale={locale}>
      <Panel update={update} tints={tintsOf((props.update ?? update)?.services ?? [])} now={NOW} {...handlers} {...props} />
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

  it("marks the dev servers' share on the memory meter", () => {
    show();

    const parts = [...screen.getByRole("meter").children] as HTMLElement[];
    const widths = parts.map((part) => parseFloat(part.style.width));
    // One part per project, in the list's order, then the rest of the memory in use.
    expect(widths).toHaveLength(3);
    expect(widths[0]).toBeCloseTo((812 / (32 * 1024)) * 100);
    expect(widths[1]).toBeCloseTo((186 / (32 * 1024)) * 100);
    expect(widths[2]).toBeCloseTo(((16 * 1024 - 998) / (32 * 1024)) * 100);
    expect(parts[0].style.background).not.toBe(parts[2].style.background);
  });

  it("says what uses the memory when asked, and folds it away again", () => {
    show();
    const toggle = screen.getByRole("button", { name: "16.0 of 32 GB" });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("list", { name: "Memory by app" })).toBeNull();

    fireEvent.click(toggle);

    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const items = within(screen.getByRole("list", { name: "Memory by app" })).getAllByRole("listitem");
    expect(parts(items)).toEqual(["Dev servers998 MB", "Google Chrome3.2 GB", "Slack640 MB"]);
    expect(screen.getByText("Figures as in Activity Monitor")).toBeTruthy();
    // Bars share the meter's scale, so the dev servers' bar lines up with their part of it.
    const widths = (parent: Element) => [...parent.children].map((part) => (part as HTMLElement).style.width);
    const meter = widths(screen.getByRole("meter"));
    expect(widths(items[0].querySelector("[aria-hidden]") as Element)).toEqual(meter.slice(0, -1));

    fireEvent.click(toggle);
    expect(screen.queryByRole("list", { name: "Memory by app" })).toBeNull();
  });

  it("keeps the dev servers' whole figure when it passes the memory in use", () => {
    show({ update: { ...update, services: [server(3000, "app", "main", 8 * 1024)], memory: { used: 6 * 1024 * MIB, total: 8 * 1024 * MIB } } });

    fireEvent.click(screen.getByRole("button", { name: "6.0 of 8 GB" }));

    const list = screen.getByRole("list", { name: "Memory by app" });
    expect(within(list).getByText("8.0 GB")).toBeTruthy();
  });

  it("offers to quit each app, but neither the dev servers nor the system", () => {
    const replayd = { id: "path:/usr/libexec/replayd", name: "replayd", memoryBytes: 300 * MIB, system: true };
    const { onQuit } = show({ update: { ...update, apps: [...(update.apps ?? []), replayd] } });
    fireEvent.click(screen.getByRole("button", { name: "16.0 of 32 GB" }));

    const list = screen.getByRole("list", { name: "Memory by app" });
    expect(within(list).getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual([
      "Quit Google Chrome",
      "Quit Slack",
    ]);
    fireEvent.click(within(list).getByRole("button", { name: "Quit Slack" }));
    expect(onQuit).toHaveBeenCalledWith(update.apps?.[1]);
  });

  it("tells apart two programs of one name, and quits the one picked", () => {
    const node = (id: string, mib: number) => ({ id, name: "node.exe", memoryBytes: mib * MIB, system: false });
    const apps = [node(String.raw`path:c:\program files\nodejs\node.exe`, 900), node(String.raw`path:c:\nvm\node.exe`, 700)];
    // React says so when two rows share a key, and may then mix them up.
    const warned = vi.spyOn(console, "error").mockImplementation(() => {});
    const { onQuit } = show({ update: { ...update, apps } });
    fireEvent.click(screen.getByRole("button", { name: "16.0 of 32 GB" }));

    expect(warned).not.toHaveBeenCalled();
    warned.mockRestore();
    const list = screen.getByRole("list", { name: "Memory by app" });
    const quits = within(list).getAllByRole("button", { name: "Quit node" });
    expect(quits).toHaveLength(2);
    fireEvent.click(quits[1]);
    expect(onQuit).toHaveBeenCalledWith(apps[1]);
  });

  it("says so while it is still measuring the apps", () => {
    show({ update: { ...update, apps: null } });

    fireEvent.click(screen.getByRole("button", { name: "16.0 of 32 GB" }));

    expect(screen.getByText("Measuring…")).toBeTruthy();
  });

  it("leaves the dev servers out of the breakdown while none run", () => {
    const services = update.services.filter((service) => service.classification !== "dev");
    show({ update: { ...update, services } });

    fireEvent.click(screen.getByRole("button", { name: "16.0 of 32 GB" }));

    const items = within(screen.getByRole("list", { name: "Memory by app" })).getAllByRole("listitem");
    expect(parts(items)).toEqual(["Google Chrome3.2 GB", "Slack640 MB"]);
  });

  it("names a coding agent the way the rest of the panel does", () => {
    show({ update: { ...update, apps: [{ id: "app:claude", name: "claude", memoryBytes: 1.7 * 1024 * MIB, system: false }] } });

    fireEvent.click(screen.getByRole("button", { name: "16.0 of 32 GB" }));

    expect(screen.getByText("Claude Code")).toBeTruthy();
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

  it("opens the cleaner and the settings", () => {
    const onClean = vi.fn();
    const onSettings = vi.fn();
    show({ onClean, onSettings });

    fireEvent.click(screen.getByRole("button", { name: "Clean up" }));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    expect(onClean).toHaveBeenCalledTimes(1);
    expect(onSettings).toHaveBeenCalledTimes(1);
  });

  it("offers no stop or quit where the platform can't do them", () => {
    show({ onStop: undefined, onQuit: undefined });
    fireEvent.click(screen.getByRole("button", { name: "16.0 of 32 GB" }));

    expect(screen.queryAllByRole("button", { name: "Stop" })).toEqual([]);
    expect(within(screen.getByRole("list", { name: "Memory by app" })).queryAllByRole("button")).toEqual([]);
    expect(screen.getByRole("option", { name: /Port 3000/ })).toBeTruthy();
  });

  it("is dragged off its icon by its header, and put back from there", () => {
    const onDrag = vi.fn();
    const onAnchor = vi.fn();
    show({ onDrag, onAnchor });
    expect(screen.queryByRole("button", { name: "Put back by the icon" })).toBeNull();

    fireEvent.mouseDown(screen.getByRole("banner"), { button: 0, screenX: 10, screenY: 10 });
    fireEvent.mouseMove(window, { buttons: 1, screenX: 40, screenY: 10 });
    expect(onDrag).toHaveBeenCalledTimes(1);

    cleanup();
    show({ onDrag, onAnchor, placed: true });
    fireEvent.click(screen.getByRole("button", { name: "Put back by the icon" }));
    expect(onAnchor).toHaveBeenCalledTimes(1);
  });

  it("keeps the cleaner's and the settings' buttons off where nothing can open them", () => {
    show();

    expect((screen.getByRole("button", { name: "Clean up" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Settings" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("waits for the first scan before saying nothing listens", () => {
    show({ update: null });

    expect(screen.getByText("Looking for servers…")).toBeTruthy();
  });

  it("says when the Mac slows down to cool off, and which dev server uses the most CPU", () => {
    show({ update: heated("serious", { 3000: 85, 8000: 120 }) });

    expect(screen.getByText(`${cooling} fix/webhooks (:8000) is using 120% CPU.`)).toBeTruthy();
  });

  it("says nothing of heat while the Mac isn't slowing down", () => {
    for (const thermal of ["nominal", "fair", "unknown"] as const) {
      show({ update: heated(thermal, { 3000: 300 }) });

      expect(screen.queryByText(new RegExp(cooling))).toBeNull();
      cleanup();
    }
  });

  it("names no server using under half a core, nor anything but a dev server", () => {
    show({ update: heated("critical", { 3000: 49.9, 8000: null, 5432: 400 }) });

    expect(screen.getByText(cooling)).toBeTruthy();
  });

  it("names the first of two dev servers using as much", () => {
    show({ update: heated("serious", { 3000: 50, 8000: 50 }) });

    expect(screen.getByText(`${cooling} main (:3000) is using 50% CPU.`)).toBeTruthy();
  });

  it("speaks Turkish with Turkish numbers", () => {
    show({}, "tr");

    expect(screen.getByText(byContent("2 sunucu · 998 MB · CPU %5"))).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "16,0 / 32 GB" }));
    const items = within(screen.getByRole("list", { name: "Uygulamalara göre bellek" })).getAllByRole("listitem");
    expect(parts(items)).toEqual(["Dev sunucuları998 MB", "Google Chrome3,2 GB", "Slack640 MB"]);
    expect(screen.getByText("Değerler Etkinlik Monitörü'ndeki gibi")).toBeTruthy();
  });

  it("says in Turkish when the Mac slows down to cool off", () => {
    show({ update: heated("serious", { 3000: 85 }) }, "tr");

    expect(screen.getByText("Mac'in soğumak için performansını düşürüyor. main (:3000), %85 CPU kullanıyor.")).toBeTruthy();
  });
});
