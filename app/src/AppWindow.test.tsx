import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { App } from "./App";
import { dragPanel, hidePanel, resizePanel, updateInstall, updateOpenAppManagement, updateSubscribe, updateWhatsNew } from "./api";
import type { UpdateStatus } from "./types";

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
  vi.restoreAllMocks();
});
beforeEach(() => {
  vi.mocked(resizePanel).mockClear();
  vi.mocked(hidePanel).mockClear();
  vi.mocked(dragPanel).mockClear();
  vi.mocked(updateSubscribe).mockClear();
  vi.mocked(updateInstall).mockClear();
  vi.mocked(updateWhatsNew).mockClear();
  vi.mocked(updateOpenAppManagement).mockClear();
  // jsdom lays nothing out, so nothing would ever be resized.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ height: 245.2 } as DOMRect);
});

const focus = () =>
  act(() => {
    window.dispatchEvent(new Event("focus"));
  });

it("fits the window to the panel again each time it is focused, until the panel goes", () => {
  const { unmount } = render(<App />);
  expect(resizePanel).not.toHaveBeenCalled();

  focus();
  focus();
  expect(vi.mocked(resizePanel).mock.calls).toEqual([[246], [246]]);

  unmount();
  focus();
  expect(resizePanel).toHaveBeenCalledTimes(2);
});

it("hides the panel on an Escape nothing in it wanted", () => {
  render(<App />);

  fireEvent.keyDown(window, { key: "Escape" });

  expect(hidePanel).toHaveBeenCalledTimes(1);
});

it("is dragged off its icon by its header", () => {
  const { container } = render(<App />);

  fireEvent.mouseDown(container.querySelector("header")!, { button: 0, screenX: 10, screenY: 10 });
  fireEvent.mouseMove(window, { buttons: 1, screenX: 30, screenY: 10 });

  expect(dragPanel).toHaveBeenCalledTimes(1);
});

/** What the app says of updates, as it would over the subscription. */
function told(status: UpdateStatus) {
  const subscriber = vi.mocked(updateSubscribe).mock.calls.at(-1)?.[0];
  if (subscriber === undefined) throw new Error("nothing subscribed to updates");
  act(() => subscriber(status));
}

it("says a newer version is available, and installs it when asked", () => {
  render(<App />);
  expect(screen.queryByText("Harbormaster 0.1.1 is available")).toBeNull();

  told({ kind: "available", version: "0.1.1" });
  screen.getByText("Harbormaster 0.1.1 is available");
  fireEvent.click(screen.getByRole("button", { name: "What's new" }));
  fireEvent.click(screen.getByRole("button", { name: "Update" }));

  expect(updateWhatsNew).toHaveBeenCalledTimes(1);
  expect(updateInstall).toHaveBeenCalledTimes(1);
});

it("puts the notice away for this run on Later, until a newer version comes", () => {
  render(<App />);
  told({ kind: "available", version: "0.1.1" });

  fireEvent.click(screen.getByRole("button", { name: "Later" }));
  expect(screen.queryByText("Harbormaster 0.1.1 is available")).toBeNull();
  told({ kind: "available", version: "0.1.1" });
  expect(screen.queryByText("Harbormaster 0.1.1 is available")).toBeNull();

  told({ kind: "available", version: "0.1.2" });
  screen.getByText("Harbormaster 0.1.2 is available");
});

it("says how far the download is, then what went wrong, with the way to allow it", () => {
  render(<App />);

  told({ kind: "downloading", version: "0.1.1", percent: 42 });
  screen.getByText("Downloading 0.1.1… 42%");
  expect(screen.queryByRole("button", { name: "Update" })).toBeNull();

  told({ kind: "installing", version: "0.1.1" });
  screen.getByText("Installing 0.1.1…");

  told({ kind: "failed", version: "0.1.1", why: "permission", message: "Operation not permitted (os error 1)" });
  screen.getByText("macOS didn't let Harbormaster replace itself. Allow it under App Management, then try again.");
  fireEvent.click(screen.getByRole("button", { name: "Open App Management settings" }));
  expect(updateOpenAppManagement).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(updateInstall).toHaveBeenCalledTimes(1);

  told({ kind: "failed", version: "0.1.1", why: "busy", message: "" });
  screen.getByText("Finish what Harbormaster is doing first, then try again.");
  told({ kind: "failed", version: "0.1.1", why: "cancelled", message: "" });
  screen.getByText("The update was cancelled.");
  told({ kind: "failed", version: "0.1.1", why: "install", message: "the disk is full" });
  screen.getByText("The update couldn't be installed: the disk is full");
  // A failed check is the About section's to tell; the panel stays quiet.
  told({ kind: "failed", version: null, why: "check", message: "no route to host" });
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
});
