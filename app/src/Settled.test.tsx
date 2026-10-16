import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useI18n } from "./i18n";
import { Settled, type SettingsSource } from "./Settled";
import type { Settings } from "./types";

afterEach(() => {
  cleanup();
  delete document.documentElement.dataset.theme;
});

const DEFAULTS: Settings = { language: "system", appearance: "system", hidden: [], roots: [], idleDays: 30 };

/** Settings the test hands out, first read when `first` settles, and changes it tells of. */
function source(first: Promise<Settings>) {
  let told: (settings: Settings) => void = () => {};
  let stops = 0;
  const settings: SettingsSource = {
    get: () => first,
    listen: (listener) => {
      told = listener;
      return Promise.resolve(() => {
        stops += 1;
      });
    },
  };
  return { settings, change: (next: Settings) => act(() => told(next)), stops: () => stops };
}

function Word() {
  return <p>{useI18n().t("overview")}</p>;
}

describe("a window's settings", () => {
  it("speaks the language and wears the appearance chosen, and follows a change", async () => {
    const { settings, change } = source(Promise.resolve({ ...DEFAULTS, language: "tr", appearance: "dark" }));
    render(
      <Settled source={settings}>
        <Word />
      </Settled>,
    );

    expect(await screen.findByText("Genel Bakış")).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBe("dark");

    change({ ...DEFAULTS, language: "en" });

    expect(screen.getByText("Overview")).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("keeps a change heard before the first reading came back", async () => {
    let answer: (settings: Settings) => void = () => {};
    const { settings, change } = source(new Promise((resolve) => (answer = resolve)));
    render(
      <Settled source={settings}>
        <Word />
      </Settled>,
    );

    change({ ...DEFAULTS, language: "tr" });
    await act(async () => answer({ ...DEFAULTS, language: "en" }));

    expect(screen.getByText("Genel Bakış")).toBeTruthy();
  });

  it("stops hearing of changes once it is gone", async () => {
    const { settings, stops } = source(Promise.resolve(DEFAULTS));
    const { unmount } = render(
      <Settled source={settings}>
        <Word />
      </Settled>,
    );

    unmount();
    await act(async () => {});

    expect(stops()).toBe(1);
  });
});
