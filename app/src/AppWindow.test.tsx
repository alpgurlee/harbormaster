import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { App } from "./App";
import { resizePanel } from "./api";

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  inApp: true,
  resizePanel: vi.fn(async () => {}),
  hidePanel: vi.fn(async () => {}),
}));

afterEach(cleanup);
beforeEach(() => {
  vi.mocked(resizePanel).mockClear();
  // jsdom lays nothing out, so nothing would ever be resized.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});

it("fits the window to the panel again each time it is focused", () => {
  render(<App />);
  expect(resizePanel).not.toHaveBeenCalled();

  act(() => {
    window.dispatchEvent(new Event("focus"));
  });

  expect(resizePanel).toHaveBeenCalledTimes(1);
});
