import type { CleanModule } from "./cleanup";

/** Which desktop the panel runs on, for its look and its wording. */
export const platform: "mac" | "windows" = navigator.userAgent.includes("Windows") ? "windows" : "mac";

/** Windows has no request to quit that every server heeds, so a stop there
 * ends the processes at once, and is asked about once. A Mac asks them to
 * quit and forces only what outlives that, after asking again. */
export const stopsAtOnce = platform === "windows";

/** Windows asks an app to close its windows, and ends what outlives that
 * only after asking the user again. A Mac asks the app to quit, and never
 * forces it. */
export const closesWindows = platform === "windows";

/** The cleaner's sections: on Windows all but the user's apps, which wait for
 * a way to uninstall them there. */
export const cleanerSections: CleanModule[] = platform === "windows" ? ["developer", "system", "large"] : ["developer", "system", "large", "apps"];

/** Whether the cleaner maps where the room in the home folder goes. */
export const mapsHome = platform === "mac";

/** What shows a file where it is. */
export const revealsIn = platform === "windows" ? "revealInExplorer" : "revealInFinder";

/** How the window speaks of where personal things go: the Trash, or on
 * Windows the Recycle Bin. */
export const bin =
  platform === "windows"
    ? ({
        goes: "goesToRecycleBin",
        confirm: "confirmRecycled",
        moved: "movedToRecycleBin",
        hint: "recycleBinHint",
        outcome: "outcomeRecycled",
        emptied: "confirmEmptiedRecycleBin",
      } as const)
    : ({
        goes: "goesToTrash",
        confirm: "confirmTrashed",
        moved: "movedToTrash",
        hint: "trashHint",
        outcome: "outcomeTrashed",
        emptied: "confirmEmptied",
      } as const);
