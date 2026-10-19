import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { I18nProvider } from "./i18n";
import { SettingsWindow, type SettingsBackend } from "./SettingsWindow";
import type { Settings } from "./types";

afterEach(cleanup);

const DEFAULTS: Settings = { language: "system", appearance: "system", hidden: [], roots: [], idleDays: 30 };

/** A backend that keeps what it is told, unless told to turn the next one down. */
function fake(first: Settings = DEFAULTS) {
  const kept: Settings[] = [];
  let refusal: unknown = null;
  const backend: SettingsBackend = {
    get: () => Promise.resolve(first),
    set: (settings) => {
      kept.push(settings);
      if (refusal === null) return Promise.resolve(settings);
      const refused = refusal;
      refusal = null;
      return Promise.reject(refused);
    },
  };
  return { backend, kept, refuse: (next: unknown) => (refusal = next) };
}

/** A backend that keeps each change only when the test lets it, one at a time. */
function slow(first: Settings = DEFAULTS) {
  const asked: Settings[] = [];
  const waiting: (() => void)[] = [];
  const backend: SettingsBackend = {
    get: () => Promise.resolve(first),
    set: (settings) => {
      asked.push(settings);
      return new Promise((resolve) => waiting.push(() => resolve(settings)));
    },
  };
  const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
  // Lets what was asked reach the backend, keeps the oldest change waiting, and lets what that sets off run.
  const keep = () =>
    act(async () => {
      await settle();
      waiting.shift()?.();
      await settle();
    });
  return { backend, asked, keep };
}

async function show(backend: SettingsBackend, cleans = true) {
  render(
    <I18nProvider locale="en">
      <SettingsWindow backend={backend} cleans={cleans} />
    </I18nProvider>,
  );
  await screen.findByRole("heading", { name: "General" });
}

const section = (name: string) => fireEvent.click(screen.getByRole("button", { name }));

describe("the settings window", () => {
  it("keeps a language and an appearance as soon as they are chosen", async () => {
    const driver = fake();
    await show(driver.backend);

    fireEvent.change(screen.getByRole("combobox", { name: "Language" }), { target: { value: "tr" } });
    await act(async () => {});
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    await act(async () => {});

    expect(driver.kept).toEqual([
      { ...DEFAULTS, language: "tr" },
      { ...DEFAULTS, language: "tr", appearance: "dark" },
    ]);
  });

  it("keeps both of two changes, the second made before the first was kept", async () => {
    const driver = slow();
    await show(driver.backend);

    fireEvent.change(screen.getByRole("combobox", { name: "Language" }), { target: { value: "tr" } });
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    await driver.keep();
    await driver.keep();

    expect(driver.asked).toEqual([
      { ...DEFAULTS, language: "tr" },
      { ...DEFAULTS, language: "tr", appearance: "dark" },
    ]);
  });

  it("is not set back by a first reading that comes back late", async () => {
    // Mounted twice, as in development, so the settings are read twice; the first reading comes back last.
    const readings: ((settings: Settings) => void)[] = [];
    const driver = fake();
    const backend: SettingsBackend = {
      get: () => new Promise((resolve) => readings.push(resolve)),
      set: driver.backend.set,
    };
    render(
      <StrictMode>
        <I18nProvider locale="en">
          <SettingsWindow backend={backend} />
        </I18nProvider>
      </StrictMode>,
    );
    expect(readings).toHaveLength(2);
    await act(async () => readings[1](DEFAULTS));

    fireEvent.change(screen.getByRole("combobox", { name: "Language" }), { target: { value: "tr" } });
    await act(async () => {});
    await act(async () => readings[0](DEFAULTS));
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    await act(async () => {});

    expect(driver.kept.at(-1)).toEqual({ ...DEFAULTS, language: "tr", appearance: "dark" });
  });

  it("adds a program never to list as a dev server, and takes it out again", async () => {
    const driver = fake();
    await show(driver.backend);
    section("Servers");

    fireEvent.change(screen.getByRole("textbox", { name: "Program name" }), { target: { value: "serena" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "Remove serena" }));
    await act(async () => {});

    expect(driver.kept.map((settings) => settings.hidden)).toEqual([["serena"], []]);
    expect(screen.getByRole<HTMLInputElement>("textbox", { name: "Program name" }).value).toBe("");
  });

  it("adds two programs, the second before the first was kept", async () => {
    const driver = slow();
    await show(driver.backend);
    section("Servers");
    const field = screen.getByRole<HTMLInputElement>("textbox", { name: "Program name" });

    fireEvent.change(field, { target: { value: "serena" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.change(field, { target: { value: "uvx" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await driver.keep();

    expect(field.value).toBe("uvx");

    await driver.keep();

    expect(driver.asked.map((settings) => settings.hidden)).toEqual([["serena"], ["serena", "uvx"]]);
    expect(field.value).toBe("");
  });

  it("says why a folder was turned down, and keeps what was typed", async () => {
    const driver = fake();
    await show(driver.backend);
    section("Clean up");
    driver.refuse({ code: "root", path: "/etc" });

    fireEvent.change(screen.getByRole("textbox", { name: "Folder, such as ~/code" }), { target: { value: "/etc" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    expect((await screen.findByRole("alert")).textContent).toBe("/etc isn't a folder inside your home folder.");
    expect(screen.getByRole<HTMLInputElement>("textbox", { name: "Folder, such as ~/code" }).value).toBe("/etc");
    expect(within(screen.getByRole("list", { name: "Folders with projects" })).queryAllByRole("listitem")).toHaveLength(0);
  });

  it("keeps how long a project sits before its build outputs are chosen", async () => {
    const driver = fake();
    await show(driver.backend);
    section("Clean up");

    fireEvent.change(screen.getByRole("spinbutton", { name: /Days a project sits/ }), { target: { value: "45" } });
    await act(async () => {});

    expect(driver.kept).toEqual([{ ...DEFAULTS, idleDays: 45 }]);
  });

  it("leaves out setting up a cleaner the platform doesn't have", async () => {
    await show(fake().backend, false);

    const sections = within(screen.getByRole("navigation", { name: "Settings" })).getAllByRole("button");
    expect(sections.map((button) => button.textContent)).toEqual(["General", "Servers", "About"]);
  });

  it("says what it is", async () => {
    await show(fake().backend);

    section("About");

    expect(screen.getByText("Version 0.1.0")).toBeTruthy();
    expect(screen.getByText("Free and open source, under the MIT License.")).toBeTruthy();
  });
});
