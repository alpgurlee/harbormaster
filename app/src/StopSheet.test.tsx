import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Preview, Stopped } from "./api";
import { I18nProvider } from "./i18n";
import { StopSheet, type Stopper } from "./StopSheet";
import type { Service } from "./types";

afterEach(cleanup);

const service = {
  id: "20-1",
  ports: [{ port: 3000, address: "127.0.0.1" }],
  project: { name: "acme-store" },
  processes: [{ pid: 20, name: "node" }],
} as Service;

const ready: Preview = {
  kind: "ready",
  token: 7,
  stops: [
    { pid: 18244, name: "node", command: "npm run dev" },
    { pid: 18310, name: "node", command: "next dev" },
  ],
  frees: [3000],
  staysOpen: [
    { pid: 900, name: "-zsh" },
    { pid: 899, name: "claude" },
  ],
};

const clean: Stopped = {
  outcomes: [
    { pid: 18310, name: "node", outcome: "stopped" },
    { pid: 18244, name: "node", outcome: "alreadyGone" },
  ],
  stillRunning: false,
};

async function open(stopper: Partial<Stopper>) {
  const handlers = { onClose: vi.fn(), onDone: vi.fn() };
  const full: Stopper = {
    preview: vi.fn(async () => ready),
    stop: vi.fn(async () => clean),
    force: vi.fn(async () => clean),
    ...stopper,
  };
  await act(async () => {
    render(
      <I18nProvider locale="en">
        <StopSheet service={service} stopper={full} {...handlers} />
      </I18nProvider>,
    );
  });
  return { ...handlers, stopper: full };
}

describe("StopSheet", () => {
  it("shows what stops and what stays open before anything is sent", async () => {
    const { stopper, onDone } = await open({});

    expect(screen.getByRole("dialog", { name: "Stop acme-store on :3000?" })).toBeTruthy();
    expect(screen.getByText("PID 18244")).toBeTruthy();
    expect(screen.getByText("next dev")).toBeTruthy();
    expect(screen.getByText("zsh")).toBeTruthy();
    expect(screen.getByText("Claude Code")).toBeTruthy();
    expect(stopper.stop).not.toHaveBeenCalled();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop server" })));

    expect(stopper.stop).toHaveBeenCalledWith(7);
    expect(onDone).toHaveBeenCalled();
  });

  it("says why when the server can't be stopped", async () => {
    const { onClose } = await open({
      preview: vi.fn(async () => ({ kind: "refused", reason: { code: "protected", protection: "database" } }) as Preview),
    });

    expect(screen.getByText(/runs a database/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Stop server" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("offers to force what is still running, and only then", async () => {
    const stuck: Stopped = { outcomes: [{ pid: 18310, name: "node", outcome: "stillRunning" }], stillRunning: true };
    const { stopper, onDone } = await open({ stop: vi.fn(async () => stuck) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop server" })));
    expect(screen.getByText(/Still running after 5 seconds/)).toBeTruthy();
    expect(stopper.force).not.toHaveBeenCalled();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Force stop" })));
    expect(stopper.force).toHaveBeenCalledWith(7);
    expect(onDone).toHaveBeenCalled();
  });

  it("offers to force once, not again after a force left something running", async () => {
    const stuck: Stopped = { outcomes: [{ pid: 18310, name: "node", outcome: "stillRunning" }], stillRunning: true };
    const { stopper, onDone } = await open({ stop: vi.fn(async () => stuck), force: vi.fn(async () => stuck) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop server" })));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Force stop" })));

    expect(screen.getByText("still running")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Force stop" })).toBeNull();
    expect(stopper.force).toHaveBeenCalledTimes(1);
    expect(onDone).not.toHaveBeenCalled();
  });

  it("stops nothing when the server changed after it was approved", async () => {
    const { onDone } = await open({ stop: vi.fn(async () => Promise.reject({ code: "changed" })) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop server" })));

    expect(screen.getByText(/changed since you looked/)).toBeTruthy();
    expect(onDone).not.toHaveBeenCalled();
  });

  it("says what happened to every process a stop didn't end", async () => {
    const mixed: Stopped = {
      outcomes: [
        { pid: 18310, name: "node", outcome: "stillRunning" },
        { pid: 18244, name: "node", outcome: "accessDenied" },
      ],
      stillRunning: true,
    };
    await open({ stop: vi.fn(async () => mixed) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop server" })));

    expect(screen.getByText("still running")).toBeTruthy();
    expect(screen.getByText("not allowed to stop it")).toBeTruthy();
  });

  it("holds the focus while it waits, and keeps a stop in flight from being cancelled", async () => {
    let finish: (stopped: Stopped) => void = () => {};
    const { onClose } = await open({ stop: vi.fn(() => new Promise<Stopped>((resolve) => (finish = resolve))) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop server" })));
    const dialog = screen.getByRole("dialog");
    expect(document.activeElement).toBe(dialog);
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();

    await act(async () => finish(clean));
  });

  it("reports processes it couldn't stop", async () => {
    const denied: Stopped = { outcomes: [{ pid: 18310, name: "node", outcome: "accessDenied" }], stillRunning: false };
    const { onDone } = await open({ stop: vi.fn(async () => denied) });

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop server" })));

    expect(screen.getByText(/not allowed/)).toBeTruthy();
    expect(onDone).not.toHaveBeenCalled();
  });

  it("cancels on Escape", async () => {
    const { onClose } = await open({});

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    expect(onClose).toHaveBeenCalled();
  });
});
