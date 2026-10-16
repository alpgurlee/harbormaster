/** Which desktop the panel runs on, for its look and its wording. */
export const platform: "mac" | "windows" = navigator.userAgent.includes("Windows") ? "windows" : "mac";
