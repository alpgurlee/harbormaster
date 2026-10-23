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

/** Cleaning up is macOS's alone for now. */
export const hasCleaner = platform === "mac";
