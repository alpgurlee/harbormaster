import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SpaceMap } from "./SpaceMap";
import type { Area } from "./types";

// The tiles are laid out once their room is known; without a size there are none.
beforeEach(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const MB = 1024 ** 2;
const GB = 1024 ** 3;

let next = 0;
function area(name: string, bytes: number, extra: Partial<Area> = {}): Area {
  next += 1;
  return { id: next, name, bytes, folder: false, short: null, inside: [], rest: 0, folded: false, ...extra };
}

function folder(name: string, inside: Area[], rest = 0, extra: Partial<Area> = {}): Area {
  const bytes = inside.reduce((total, each) => total + each.bytes, rest);
  return area(name, bytes, { folder: true, inside, rest, ...extra });
}

const caches = folder("Caches", [area("blob", 3 * GB)]);
const containers = area("Containers", 4096, { folder: true, short: "skipped" });
const library = folder("Library", [caches, containers], 2 * GB, { short: "skipped" });
const notes = area("notes.txt", 2 * MB);
const root = { ...folder("dev", [library, notes], MB, { short: "skipped" }), id: 0 };

const rows = () =>
  within(screen.getByRole("list", { name: "What Home holds" }))
    .getAllByRole("listitem")
    .map((row) => row.textContent);

describe("the space map", () => {
  it("lists what the folder holds, largest first, with the smaller items together last", () => {
    render(<SpaceMap root={root} onReveal={vi.fn()} />);

    expect(rows()).toEqual([
      expect.stringContaining("Library"),
      expect.stringContaining("notes.txt"),
      expect.stringContaining("Smaller items"),
    ]);
    expect(rows()[0]).toContain("5.0 GB");
  });

  it("goes into a folder, and back up by the trail", () => {
    render(<SpaceMap root={root} onReveal={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /Library/ }));

    const trail = screen.getByRole("navigation", { name: "Where you are" });
    expect(within(trail).getByText("Library").getAttribute("aria-current")).toBe("location");
    screen.getByRole("button", { name: /Caches/ });

    fireEvent.click(within(trail).getByRole("button", { name: "Home" }));

    screen.getByRole("button", { name: /notes\.txt/ });
  });

  it("shows a chosen place in Finder by its number, never the home folder itself", async () => {
    const onReveal = vi.fn(() => Promise.resolve());
    render(<SpaceMap root={root} onReveal={onReveal} />);
    expect(screen.queryByRole("button", { name: "Show in Finder" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /notes\.txt/ }));
    fireEvent.click(screen.getByRole("button", { name: "Show in Finder" }));

    expect(onReveal).toHaveBeenCalledWith(notes.id);
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("says why a place may hold more than it shows", () => {
    render(<SpaceMap root={root} onReveal={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Library/ }));

    fireEvent.click(screen.getByRole("button", { name: /Containers/ }));

    screen.getByRole("button", { name: /^Containers.*Holds other apps' data, not looked in$/ });
    expect(screen.getByRole("region", { name: "Containers" }).textContent).toContain("Holds other apps' data, not looked in");
  });

  it("offers to map other apps' data too where it was left out, when it can", () => {
    const onMapOthers = vi.fn();
    const { rerender } = render(<SpaceMap root={root} onReveal={vi.fn()} onMapOthers={onMapOthers} />);
    fireEvent.click(screen.getByRole("button", { name: /Library/ }));
    fireEvent.click(screen.getByRole("button", { name: /Containers/ }));

    fireEvent.click(screen.getByRole("button", { name: "Map with other apps' data" }));

    expect(onMapOthers).toHaveBeenCalledTimes(1);
    // A place left out for another reason, or a map that can't start, offers nothing.
    rerender(<SpaceMap root={root} onReveal={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Map with other apps' data" })).toBeNull();
  });

  it("offers Full Disk Access where the system kept something from being read, where it has the setting", () => {
    const mail = area("Mail", 4096, { folder: true, short: "withheld" });
    const locked = area("locked", 4096, { folder: true, short: "denied" });
    const home = { ...folder("dev", [mail, locked, notes], 0, { short: "withheld" }), id: 0 };
    const onOpenFullDiskAccess = vi.fn();
    const { rerender } = render(<SpaceMap root={home} onReveal={vi.fn()} onOpenFullDiskAccess={onOpenFullDiskAccess} />);
    // Full Disk Access opens nothing a file's own permissions keep closed.
    fireEvent.click(screen.getByRole("button", { name: /locked/ }));
    expect(screen.queryByRole("button", { name: "Open Full Disk Access settings" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Mail/ }));
    expect(screen.getByRole("region", { name: "Mail" }).textContent).toContain(
      "The system kept some of it from being read; Full Disk Access may let Harbormaster read it",
    );
    fireEvent.click(screen.getByRole("button", { name: "Open Full Disk Access settings" }));

    expect(onOpenFullDiskAccess).toHaveBeenCalledTimes(1);
    rerender(<SpaceMap root={home} onReveal={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Open Full Disk Access settings" })).toBeNull();
  });

  it("gives no size for a place it never looked in", () => {
    render(<SpaceMap root={root} onReveal={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /Library/ }));

    const row = screen.getByRole("button", { name: /Containers/ });
    fireEvent.click(row);

    expect(row.textContent).not.toContain("MB");
    expect(screen.getByRole("region", { name: "Containers" }).textContent).not.toContain("MB");
    // What was looked in keeps its size, even when something in it wasn't.
    expect(screen.getByRole("navigation", { name: "Where you are" }).textContent).toContain("Library");
    expect(screen.getByRole("button", { name: /Caches/ }).textContent).toContain("3.0 GB");
  });

  it("keeps the keyboard where it went: on the folder opened, and on the one gone back to", () => {
    render(<SpaceMap root={root} onReveal={vi.fn()} />);
    const library = screen.getByRole("button", { name: /Library/ });
    library.focus();

    fireEvent.click(library);

    expect(document.activeElement?.getAttribute("aria-current")).toBe("location");
    expect(document.activeElement?.textContent).toBe("Library");

    fireEvent.click(screen.getByRole("button", { name: "Home" }));

    expect(document.activeElement?.getAttribute("aria-current")).toBe("location");
    expect(document.activeElement?.textContent).toBe("Home");
  });

  /** A reveal the test answers when it likes. */
  function pending() {
    const answers: { resolve: () => void; reject: (reason: unknown) => void }[] = [];
    const onReveal = vi.fn(() => new Promise<void>((resolve, reject) => answers.push({ resolve, reject })));
    return { answers, onReveal };
  }

  it("says only what the latest showing in Finder met, of the same place", async () => {
    const { answers, onReveal } = pending();
    render(<SpaceMap root={root} onReveal={onReveal} />);
    fireEvent.click(screen.getByRole("button", { name: /notes\.txt/ }));
    const show = screen.getByRole("button", { name: "Show in Finder" });

    fireEvent.click(show);
    fireEvent.click(show);
    await act(async () => answers[1].resolve());
    await act(async () => answers[0].reject({ code: "changed" }));

    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says only what the latest showing in Finder met, of another place", async () => {
    const { answers, onReveal } = pending();
    render(<SpaceMap root={root} onReveal={onReveal} />);
    fireEvent.click(screen.getByRole("button", { name: /notes\.txt/ }));
    fireEvent.click(screen.getByRole("button", { name: "Show in Finder" }));
    fireEvent.click(screen.getByRole("button", { name: /Library/ }));
    fireEvent.click(screen.getByRole("button", { name: /Containers/ }));
    fireEvent.click(screen.getByRole("button", { name: "Show in Finder" }));

    await act(async () => answers[1].reject({ code: "failed", message: "no Finder" }));
    await act(async () => answers[0].reject({ code: "changed" }));

    expect(screen.getByRole("alert").textContent).toContain("no Finder");
  });

  it("says when a place changed since it was mapped", async () => {
    render(<SpaceMap root={root} onReveal={() => Promise.reject({ code: "changed" })} />);
    fireEvent.click(screen.getByRole("button", { name: /notes\.txt/ }));

    fireEvent.click(screen.getByRole("button", { name: "Show in Finder" }));

    expect((await screen.findByRole("alert")).textContent).toContain("It has changed since the map was made.");
  });

  it("lays out a tile for each thing shown, and opens a folder from its tile", () => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(private readonly told: ResizeObserverCallback) {}
        observe(target: Element) {
          this.told([{ target, contentRect: { width: 600, height: 400 } } as ResizeObserverEntry], this as never);
        }
        disconnect() {}
      },
    );
    const movies = folder("Movies", [area("trip.mov", 3 * GB)]);
    const even = { ...folder("dev", [movies, area("disk.iso", 2 * GB)], GB), id: 0 };
    const { container } = render(<SpaceMap root={even} onReveal={vi.fn()} />);
    const tiles = () => [...container.querySelectorAll<HTMLElement>("[data-tile]")];

    expect(tiles().map((tile) => tile.textContent)).toEqual(["Movies3.0 GB", "disk.iso2.0 GB", "Smaller items1.0 GB"]);
    fireEvent.click(tiles()[0]);

    expect(within(screen.getByRole("navigation", { name: "Where you are" })).getByText("Movies").getAttribute("aria-current")).toBe(
      "location",
    );
  });
});
