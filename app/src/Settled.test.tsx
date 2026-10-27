import { act, cleanup, render, screen } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { useI18n } from "./i18n";
import { Settled, type SettingsSource } from "./Settled";
import type { Settings } from "./types";

afterEach(() => {
  cleanup();
  delete document.documentElement.dataset.theme;
});

const DEFAULTS: Settings = { language: "system", appearance: "system", hidden: [], roots: [], idleDays: 30, checkUpdates: true };

/** Settings the test tells the window of, as the app does: what is in force first, then each change. */
function source() {
  const listeners: ((settings: Settings) => void)[] = [];
  const settings: SettingsSource = {
    subscribe: (listener) => {
      listeners.push(listener);
      return Promise.resolve();
    },
  };
  // Through the latest subscription unless told which.
  const tell = (next: Settings, through = listeners.length - 1) => act(() => listeners[through](next));
  return { settings, tell, subscriptions: () => listeners.length };
}

function Word() {
  return <p>{useI18n().t("overview")}</p>;
}

describe("a window's settings", () => {
  it("speaks the system's language until it is told the settings", () => {
    const { settings } = source();
    render(
      <Settled source={settings}>
        <Word />
      </Settled>,
    );

    expect(screen.getByText("Overview")).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("speaks the language and wears the appearance chosen, and follows a change", () => {
    const { settings, tell } = source();
    render(
      <Settled source={settings}>
        <Word />
      </Settled>,
    );

    tell({ ...DEFAULTS, language: "tr", appearance: "dark" });

    expect(screen.getByText("Genel Bakış")).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBe("dark");

    tell({ ...DEFAULTS, language: "en" });

    expect(screen.getByText("Overview")).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("ignores what still reaches a subscription it gave up", () => {
    const { settings, tell, subscriptions } = source();
    // Mounted twice, as in development: the first subscription is given up for the second.
    render(
      <StrictMode>
        <Settled source={settings}>
          <Word />
        </Settled>
      </StrictMode>,
    );
    expect(subscriptions()).toBe(2);

    tell({ ...DEFAULTS, language: "tr" }, 1);
    tell({ ...DEFAULTS, language: "en" }, 0);

    expect(screen.getByText("Genel Bakış")).toBeTruthy();
  });
});
