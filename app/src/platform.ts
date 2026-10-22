/** Which desktop the panel runs on, for its look and its wording. */
export const platform: "mac" | "windows" = navigator.userAgent.includes("Windows") ? "windows" : "mac";

/** Windows has no request to quit that every server heeds, so a stop there
 * ends the processes at once, and is asked about once. A Mac asks them to
 * quit and forces only what outlives that, after asking again. */
export const stopsAtOnce = platform === "windows";

/** Quitting apps and cleaning up are macOS's alone for now. */
export const quitsApps = platform === "mac";
export const hasCleaner = platform === "mac";
