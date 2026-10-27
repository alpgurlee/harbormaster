import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { App } from "./App";
import { dragPanel, hidePanel, updateSubscribe } from "./api";
import type { UpdateStatus } from "./types";

// On Windows the panel is a window of its own, with a title bar.
vi.mock("./platform", () => ({
  platform: "windows",
  stopsAtOnce: true,
  closesWindows: true,
  panelIsWindow: true,
  cleanerSections: ["developer", "system", "large", "apps"],
  listsPrograms: true,
  mapsHome: true,
  revealsIn: "revealInExplorer",
  bin: {
    goes: "goesToRecycleBin",
    confirm: "confirmRecycled",
    moved: "movedToRecycleBin",
    hint: "recycleBinHint",
    outcome: "outcomeRecycled",
    emptied: "confirmEmptiedRecycleBin",
  },
}));

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inApp: true,
  resizePanel: vi.fn(async () => {}),
  hidePanel: vi.fn(async () => {}),
  dragPanel: vi.fn(async () => {}),
  panelPlaced: vi.fn(async () => false),
  settingsSubscribe: vi.fn(async () => {}),
  updateSubscribe: vi.fn(async () => {}),
  updateInstall: vi.fn(async () => {}),
  updateWhatsNew: vi.fn(async () => {}),
  updateOpenAppManagement: vi.fn(async () => {}),
}));

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
beforeEach(() => {
  vi.mocked(hidePanel).mockClear();
  vi.mocked(dragPanel).mockClear();
  vi.mocked(updateSubscribe).mockClear();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});

it("stays open on an Escape, as a window of its own does", () => {
  render(<App />);

  fireEvent.keyDown(window, { key: "Escape" });

  expect(hidePanel).not.toHaveBeenCalled();
});

it("leaves moving the window to its title bar", () => {
  const { container } = render(<App />);

  fireEvent.mouseDown(container.querySelector("header")!, { button: 0, screenX: 10, screenY: 10 });
  fireEvent.mouseMove(window, { buttons: 1, screenX: 30, screenY: 10 });

  expect(dragPanel).not.toHaveBeenCalled();
});

it("names no macOS setting when Windows refused the update", () => {
  render(<App />);
  const subscriber = vi.mocked(updateSubscribe).mock.calls.at(-1)?.[0];
  if (subscriber === undefined) throw new Error("nothing subscribed to updates");
  const status: UpdateStatus = { kind: "failed", version: "0.1.1", why: "permission", message: "Access is denied. (os error 5)" };

  act(() => subscriber(status));

  screen.getByText("The update couldn't be installed: Access is denied. (os error 5)");
  expect(screen.queryByRole("button", { name: "Open App Management settings" })).toBeNull();
  screen.getByRole("button", { name: "Try again" });
});
