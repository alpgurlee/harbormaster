import { describe, expect, it } from "vitest";

import { bytesOf, pieces, unmeasured } from "./mapPieces";
import type { Area } from "./types";

function area(name: string, bytes: number, inside: Area[] = [], rest = 0): Area {
  return { id: 0, name, bytes, folder: inside.length > 0 || rest > 0, short: null, inside, rest, folded: false };
}

describe("what a mapped folder shows", () => {
  it("shows each place large enough, then what the others hold together", () => {
    const movies = area("Movies", 300, [area("trip.mov", 200), area("cut.mov", 60)], 40);

    const shown = pieces(movies);

    expect(shown.map((piece) => (piece.kind === "area" ? piece.area.name : "rest"))).toEqual(["trip.mov", "cut.mov", "rest"]);
    expect(shown.map(bytesOf)).toEqual([200, 60, 40]);
  });

  it("shows no rest when nothing else is held", () => {
    expect(pieces(area("Movies", 200, [area("trip.mov", 200)])).map((piece) => piece.kind)).toEqual(["area"]);
  });

  it("knows the room of a folder never looked in is unknown, not none", () => {
    expect(unmeasured({ ...area("Containers", 0), folder: true, short: "skipped" })).toBe(true);
    expect(unmeasured({ ...area("Library", 300, [area("Caches", 300)]), short: "skipped" })).toBe(false);
    expect(unmeasured({ ...area("Logs", 0), folder: true })).toBe(false);
  });
});
