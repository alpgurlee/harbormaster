import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Detail } from "./Detail";
import { I18nProvider } from "./i18n";
import type { Service } from "./types";

afterEach(cleanup);

const MIB = 1024 * 1024;
const NOW = Date.UTC(2026, 9, 16, 12, 0, 0);

const server: Service = {
  id: "20-1",
  classification: "dev",
  protected: null,
  ports: [
    { port: 3000, address: "127.0.0.1" },
    { port: 3000, address: "::1" },
  ],
  project: {
    name: "acme-store",
    root: "/Users/dev/code/acme-store",
    repoRoot: "/Users/dev/code/acme-store",
    framework: { name: "Next.js", major: 16 },
    branch: "main",
    worktree: false,
  },
  command: "npm run dev",
  startedAt: NOW / 1000 - (3 * 3600 + 12 * 60),
  memoryBytes: 812 * MIB,
  cpuPercent: 3.14,
  processes: [
    { pid: 18244, name: "node", command: "npm run dev", memoryBytes: 48 * MIB, cpuPercent: 0 },
    { pid: 18310, name: "node", command: "next dev", memoryBytes: 120 * MIB, cpuPercent: 1 },
    { pid: 18377, name: "node", command: "next-server", memoryBytes: 644 * MIB, cpuPercent: 2.1 },
  ],
  holders: [{ pid: 18377, name: "node" }],
  ancestors: [
    { pid: 900, name: "-zsh" },
    { pid: 899, name: "claude" },
    { pid: 800, name: "iTerm2" },
  ],
};

function show(service: Service | null = server) {
  const handlers = { onBack: vi.fn(), onOpen: vi.fn(), onCopy: vi.fn(async () => {}), onReveal: vi.fn(), onStop: vi.fn() };
  render(
    <I18nProvider locale="en">
      <Detail service={service} now={NOW} {...handlers} />
    </I18nProvider>,
  );
  return handlers;
}

describe("Detail", () => {
  it("says what runs, where, since when and under what", () => {
    show();

    expect(screen.getByRole("heading", { name: "acme-store" })).toBeTruthy();
    expect(screen.getByText("Running · 3h 12m")).toBeTruthy();
    expect(screen.getByText("127.0.0.1 and ::1")).toBeTruthy();
    const facts = screen.getByRole("list", { name: "About this server" });
    expect(within(facts).getByText("main")).toBeTruthy();
    expect(within(facts).getByText("/Users/dev/code/acme-store")).toBeTruthy();
    expect(within(facts).getByText("iTerm2 · zsh")).toBeTruthy();
    expect(within(facts).getByText("Claude Code")).toBeTruthy();
  });

  it("shows what the server uses, process by process", () => {
    show();

    expect(screen.getByText("812 MB")).toBeTruthy();
    expect(screen.getByText("3.1%")).toBeTruthy();
    const processes = screen.getByRole("list", { name: "Processes" });
    expect(within(processes).getAllByRole("listitem")).toHaveLength(3);
    expect(within(processes).getByText("next-server")).toBeTruthy();
  });

  it("acts from its buttons and from the keyboard", () => {
    const { onOpen, onCopy, onReveal, onStop, onBack } = show();

    fireEvent.click(screen.getByRole("button", { name: "Open localhost:3000" }));
    fireEvent.click(screen.getByRole("button", { name: "Copy URL" }));
    fireEvent.click(screen.getByRole("button", { name: "Reveal folder" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop…" }));
    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(onCopy).toHaveBeenCalledWith("http://localhost:3000");
    expect(onReveal).toHaveBeenCalledTimes(1);
    expect(onStop).toHaveBeenCalledTimes(1);

    const page = screen.getByRole("region", { name: "acme-store" });
    fireEvent.keyDown(page, { key: "o" });
    fireEvent.keyDown(page, { key: "c" });
    fireEvent.keyDown(page, { key: "s" });
    fireEvent.keyDown(page, { key: "Escape" });
    expect(onOpen).toHaveBeenCalledTimes(2);
    expect(onCopy).toHaveBeenCalledTimes(2);
    expect(onStop).toHaveBeenCalledTimes(2);
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("says why a protected server can't be stopped from here", () => {
    const { onStop } = show({ ...server, protected: "database" });

    const stop = screen.getByRole("button", { name: "Stop…" }) as HTMLButtonElement;
    expect(stop.disabled).toBe(true);
    expect(screen.getByText(/runs a database/)).toBeTruthy();
    fireEvent.keyDown(screen.getByRole("region", { name: "acme-store" }), { key: "s" });
    expect(onStop).not.toHaveBeenCalled();
  });

  it("offers no stop, nor says why one is refused, where the platform can't stop", () => {
    render(
      <I18nProvider locale="en">
        <Detail service={{ ...server, protected: "database" }} now={NOW} onBack={vi.fn()} onOpen={vi.fn()} onCopy={vi.fn()} onReveal={vi.fn()} />
      </I18nProvider>,
    );

    expect(screen.queryByRole("button", { name: "Stop…" })).toBeNull();
    expect(screen.queryByText(/runs a database/)).toBeNull();
    // Nor does its key, which mustn't throw for want of a handler.
    fireEvent.keyDown(screen.getByRole("region", { name: "acme-store" }), { key: "s" });
  });

  it("offers neither to open nor to copy a link-local listener, which no URL reaches", () => {
    const { onOpen } = show({ ...server, ports: [{ port: 3000, address: "fe80::1" }] });

    expect(screen.queryByRole("button", { name: /Open/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Copy URL" })).toBeNull();
    fireEvent.keyDown(screen.getByRole("region", { name: "acme-store" }), { key: "o" });
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("says so when the server has stopped meanwhile, and Escape still goes back", () => {
    const { onBack } = show(null);

    expect(screen.getByText("This server isn't running anymore.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    fireEvent.keyDown(screen.getByRole("region"), { key: "Escape" });
    expect(onBack).toHaveBeenCalledTimes(2);
  });

  it("says copied only when this copy succeeded", async () => {
    let finish = () => {};
    const slow = vi.fn(() => new Promise<void>((resolve) => (finish = resolve)));
    const broken = vi.fn((): Promise<void> => {
      throw new Error("no clipboard");
    });
    const { rerender } = render(
      <I18nProvider locale="en">
        <Detail service={server} now={NOW} onBack={vi.fn()} onOpen={vi.fn()} onCopy={slow} onReveal={vi.fn()} onStop={vi.fn()} />
      </I18nProvider>,
    );

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copy URL" })));
    expect(screen.queryByText("Copied")).toBeNull();
    await act(async () => finish());
    expect(screen.getByRole("button", { name: "Copied" })).toBeTruthy();

    rerender(
      <I18nProvider locale="en">
        <Detail service={server} now={NOW} onBack={vi.fn()} onOpen={vi.fn()} onCopy={broken} onReveal={vi.fn()} onStop={vi.fn()} />
      </I18nProvider>,
    );
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copied" })));
    expect(screen.getByRole("button", { name: "Couldn't copy" })).toBeTruthy();
  });

  it("only says copied once the clipboard took it", async () => {
    const onCopy = vi.fn(async () => Promise.reject(new Error("denied")));
    render(
      <I18nProvider locale="en">
        <Detail service={server} now={NOW} onBack={vi.fn()} onOpen={vi.fn()} onCopy={onCopy} onReveal={vi.fn()} onStop={vi.fn()} />
      </I18nProvider>,
    );

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Copy URL" })));

    expect(screen.queryByText("Copied")).toBeNull();
    expect(screen.getByRole("button", { name: "Couldn't copy" })).toBeTruthy();
  });
});
