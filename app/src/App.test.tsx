import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";

import { App } from "./App";
import { sample } from "./demo";
import { publish } from "./store";

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
