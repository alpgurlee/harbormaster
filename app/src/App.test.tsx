import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { App } from "./App";
import { sample } from "./demo";
import { publish } from "./store";

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  quitAppPreview: vi.fn(async () => ({ kind: "app", token: 1 })),
  quitApp: vi.fn(async () => ({ outcomes: [{ pid: 1, name: "Docker", outcome: "stopped" }], stillRunning: false })),
}));

afterEach(cleanup);
beforeEach(() => {
  Element.prototype.scrollIntoView = () => {};
});

it("brings the focus back to the page when the server vanished behind the stop sheet", async () => {
  const now = Date.now();
  const update = sample(now);
  const [first] = update.services;
  act(() => publish(update));
  render(<App />);

  fireEvent.click(screen.getByRole("option", { name: /Port 3000/ }));
  const stop = screen.getByRole("button", { name: /Stop…|Durdur…/ });
  stop.focus();
  await act(async () => fireEvent.click(stop));
  act(() => publish({ ...update, services: update.services.filter((service) => service !== first) }));
  expect(stop.isConnected).toBe(false);
  const close = screen.getByRole("button", { name: /Close|Kapat/ });
  close.focus();
  await act(async () => fireEvent.click(close));

  const back = screen.getByRole("button", { name: /Back|Geri/ });
  expect(document.activeElement).toBe(back);
  fireEvent.keyDown(back, { key: "Escape" });
  expect(screen.getByRole("combobox")).toBeTruthy();
});

it("brings the focus back to the page when the server became protected behind the stop sheet", async () => {
  const update = sample(Date.now());
  const [first] = update.services;
  act(() => publish(update));
  render(<App />);

  fireEvent.click(screen.getByRole("option", { name: /Port 3000/ }));
  const stop = screen.getByRole("button", { name: /Stop…|Durdur…/ });
  stop.focus();
  await act(async () => fireEvent.click(stop));
  act(() => publish({ ...update, services: update.services.map((service) => (service === first ? { ...service, protected: "database" } : service)) }));
  const close = screen.getByRole("button", { name: /Close|Kapat/ });
  close.focus();
  await act(async () => fireEvent.click(close));

  expect(document.activeElement).toBe(screen.getByRole("button", { name: /Back|Geri/ }));
});

async function askToQuitDocker() {
  act(() => publish(sample(Date.now())));
  render(<App />);
  const toggle = screen.getByRole("button", { name: /14\.9 of 32 GB|14,9 \/ 32 GB/ });
  fireEvent.click(toggle);
  const quit = screen.getByRole("button", { name: /Quit Docker|Kapat: Docker/ });
  quit.focus();
  await act(async () => fireEvent.click(quit));
  expect(screen.getByRole("dialog", { name: /Quit Docker\?|Docker kapatılsın mı\?/ })).toBeTruthy();
  return { toggle, quit };
}

it("asks before quitting an app from the breakdown, and brings the focus back when that's called off", async () => {
  const { quit } = await askToQuitDocker();

  await act(async () => fireEvent.click(screen.getByRole("button", { name: /Cancel|Vazgeç/ })));

  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(quit);
});

it("keeps the focus on the breakdown once the app it quit is gone", async () => {
  const { toggle } = await askToQuitDocker();

  await act(async () => fireEvent.click(screen.getByRole("button", { name: /^(Quit|Kapat)$/ })));

  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(toggle);
});
