/** Which desktop the panel runs on, for its look and its wording. */
export const platform: "mac" | "windows" = navigator.userAgent.includes("Windows") ? "windows" : "mac";

/** Stopping servers, quitting apps and cleaning up are macOS's alone for now: on Windows the panel only shows what runs. */
export const acts = platform === "mac";
