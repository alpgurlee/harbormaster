import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { App } from "./App";
import { dragPanel, hidePanel, resizePanel } from "./api";

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inApp: true,
  resizePanel: vi.fn(async () => {}),
  hidePanel: vi.fn(async () => {}),
  dragPanel: vi.fn(async () => {}),
  panelPlaced: vi.fn(async () => false),
  settingsSubscribe: vi.fn(async () => {}),
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
