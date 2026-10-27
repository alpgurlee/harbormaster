import type { TextKey } from "./i18n";
import type { Area, Short } from "./types";

/** Something a mapped folder shows: one of its places, or what the others hold together. */
export type Piece = { kind: "area"; area: Area } | { kind: "rest"; bytes: number };

/** What `area` shows: each place large enough, largest first, then the rest. */
export function pieces(area: Area): Piece[] {
  const places: Piece[] = area.inside.map((each) => ({ kind: "area", area: each }));
  return area.rest > 0 ? [...places, { kind: "rest", bytes: area.rest }] : places;
}

export const bytesOf = (piece: Piece) => (piece.kind === "area" ? piece.area.bytes : piece.bytes);

/** Whether `area` is a folder never looked in, for a reason it has: its room is unknown rather than none. */
export const unmeasured = (area: Area) => area.folder && area.short !== null && area.inside.length === 0 && area.rest === 0;

/** What each reason a place may hold more than it says reads as. */
export const SHORT_TEXT: Record<Short, TextKey> = {
  deep: "shortDeep",
  changed: "shortChanged",
  denied: "shortDenied",
  withheld: "shortWithheld",
  skipped: "shortSkipped",
  stopped: "shortStopped",
};
