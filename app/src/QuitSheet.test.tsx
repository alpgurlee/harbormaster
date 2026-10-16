import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { QuitPreview, Stopped } from "./api";
import { I18nProvider } from "./i18n";
import { QuitSheet, type Quitter } from "./QuitSheet";
import type { AppUsage } from "./types";

afterEach(cleanup);

const chrome: AppUsage = { name: "Google Chrome", memoryBytes: 3 * 1024 * 1024 * 1024, system: false };

const programs: QuitPreview = {
  kind: "programs",
  token: 4,
  quits: [{ pid: 20, name: "node", command: "node server.js" }],
  staysOpen: [{ pid: 21, name: "postgres" }],
};

const quit: Stopped = { outcomes: [{ pid: 10, name: "Google Chrome", outcome: "stopped" }], stillRunning: false };

async function open(quitter: Partial<Quitter>, app: AppUsage = chrome) {
  const handlers = { onClose: vi.fn(), onDone: vi.fn() };
  const full: Quitter = {
    preview: vi.fn(async (): Promise<QuitPreview> => ({ kind: "app", token: 3 })),
    quit: vi.fn(async () => quit),
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

describe("QuitSheet", () => {
  it("asks an app to quit as ⌘Q would, once the user says so", async () => {
    const { quitter, onDone } = await open({});

    expect(screen.getByRole("dialog", { name: "Quit Google Chrome?" })).toBeTruthy();
    expect(screen.getByText(/as ⌘Q would/)).toBeTruthy();
    expect(quitter.preview).toHaveBeenCalledWith("Google Chrome");
    expect(quitter.quit).not.toHaveBeenCalled();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Quit" })));

    expect(quitter.quit).toHaveBeenCalledWith(3);
    expect(onDone).toHaveBeenCalled();
  });

  it("warns before closing command-line programs, and says what closes and what stays", async () => {
    await open({ preview: vi.fn(async () => programs) }, { name: "node", memoryBytes: 1, system: false });

    expect(screen.getByText(/a coding agent's session, a language server/)).toBeTruthy();
    expect(screen.getByText("node server.js")).toBeTruthy();
    expect(screen.getByText("PID 20")).toBeTruthy();
    expect(screen.getByText("postgres")).toBeTruthy();
    expect(screen.getByText(/Nothing is forced/)).toBeTruthy();
  });

  it("names a coding agent as the rest of the panel does", async () => {
    await open({}, { name: "claude", memoryBytes: 1, system: false });

    expect(screen.getByRole("dialog", { name: "Quit Claude Code?" })).toBeTruthy();
  });

  it("says so when the app is still open, and never offers to force it", async () => {
    const open_: Stopped = { outcomes: [{ pid: 10, name: "Google Chrome", outcome: "stillRunning" }], stillRunning: true };
    const { onDone } = await open({ quit: vi.fn(async () => open_) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Quit" })));

    expect(screen.getByText(/Google Chrome is still open/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Force/ })).toBeNull();
    expect(onDone).not.toHaveBeenCalled();
  });

  it("says what happened to every program that didn't close", async () => {
    const stuck: Stopped = {
      outcomes: [
        { pid: 20, name: "node", outcome: "stillRunning" },
        { pid: 22, name: "node", outcome: "accessDenied" },
      ],
      stillRunning: true,
    };
    await open({ preview: vi.fn(async () => programs), quit: vi.fn(async () => stuck) }, { name: "node", memoryBytes: 1, system: false });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Quit" })));

    expect(screen.getByText("still running")).toBeTruthy();
    expect(screen.getByText("not allowed to stop it")).toBeTruthy();
  });

  it("says why it won't quit something", async () => {
    const { onClose } = await open({
      preview: vi.fn(async (): Promise<QuitPreview> => ({ kind: "refused", reason: { code: "protected", protection: "system" } })),
    });

    expect(screen.getByText("Harbormaster won't quit this: it is part of the operating system.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Quit" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("says so when the app is gone, or changed since it was approved", async () => {
    await open({ preview: vi.fn(async (): Promise<QuitPreview> => ({ kind: "refused", reason: { code: "notFound" } })) });
    expect(screen.getByText("Google Chrome isn't running anymore.")).toBeTruthy();

    cleanup();
    await open({ quit: vi.fn(async () => Promise.reject({ code: "changed" })) });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Quit" })));
    expect(screen.getByText(/changed since you looked/)).toBeTruthy();
  });

  it("holds the focus while it waits, and keeps a quit in flight from being cancelled", async () => {
    let finish: (stopped: Stopped) => void = () => {};
    const { onClose } = await open({ quit: vi.fn(() => new Promise<Stopped>((resolve) => (finish = resolve))) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Quit" })));
    const dialog = screen.getByRole("dialog");
    expect(document.activeElement).toBe(dialog);
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();

    await act(async () => finish(quit));
  });

  it("cancels on Escape", async () => {
    const { onClose } = await open({});

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    expect(onClose).toHaveBeenCalled();
  });
});
