import { largeFile } from "./cleanup";
import type { TextKey, Translator } from "./i18n";
import { bin, platform } from "./platform";
import type { CleanGroup, CleanRefusal, Finding, Removal, Why } from "./types";

export const NAMES: Record<CleanGroup, TextKey> = {
  projects: "groupProjects",
  packages: "groupPackages",
  xcode: "groupXcode",
  apps: "groupApps",
  remnants: "groupRemnants",
  temp: "groupTemp",
  logs: "groupLogs",
  trash: platform === "windows" ? "groupTrashWindows" : "groupTrash",
  videos: "groupVideos",
  archives: "groupArchives",
  installers: "groupInstallers",
  audio: "groupAudio",
  images: "groupImages",
  documents: "groupDocuments",
  otherFiles: "groupOtherFiles",
  applications: "groupApplications",
};

export const ABOUT: Record<CleanGroup, TextKey> = {
  projects: "aboutProjects",
  packages: "aboutPackages",
  xcode: "aboutXcode",
  apps: "aboutApps",
  remnants: "aboutRemnants",
  temp: "aboutTemp",
  logs: platform === "windows" ? "aboutLogsWindows" : "aboutLogs",
  trash: "aboutTrash",
  videos: platform === "windows" ? "aboutVideosWindows" : "aboutVideos",
  archives: "aboutArchives",
  installers: platform === "windows" ? "aboutInstallersWindows" : "aboutInstallers",
  audio: "aboutAudio",
  images: "aboutImages",
  documents: "aboutDocuments",
  otherFiles: "aboutOtherFiles",
  applications: "aboutApplications",
};

/** Each group's color, from the panel's project tints. */
export const TINTS: Record<CleanGroup, number> = {
  projects: 0,
  packages: 3,
  xcode: 1,
  apps: 5,
  remnants: 1,
  temp: 3,
  logs: 4,
  trash: 2,
  videos: 2,
  archives: 0,
  installers: 1,
  audio: 5,
  images: 4,
  documents: 3,
  otherFiles: 0,
  applications: 1,
};

/** Where it is, then why it is or isn't chosen. */
export function note(finding: Finding, home: string, now: number, i18n: Translator): string {
  if (finding.inUse !== null) return i18n.t("inUseBy", { who: finding.inUse });
  // A large file's or an app's name is shown already; the folder it is in is what's left to say.
  const named = largeFile(finding) || finding.group === "applications";
  const where = shown(named ? folderOf(finding.path) : (finding.project ?? finding.path), home);
  const why =
    finding.touched !== null
      ? i18n.t(largeFile(finding) ? "changedAgo" : "lastTouched", { ago: i18n.ago(now - finding.touched * 1000) })
      : finding.kind === "personal"
        ? i18n.t(bin.goes)
        : finding.kind === "discarded"
          ? i18n.t("emptiedForGood")
          : finding.picked
            ? null
            : i18n.t("usedLast");
  return why === null ? where : `${where} · ${why}`;
}

/** What separates the names in a path: `/`, or on Windows `\` too. */
const SEPARATORS = ["/", "\\"];

function folderOf(path: string): string {
  const last = Math.max(...SEPARATORS.map((separator) => path.lastIndexOf(separator)));
  return path.slice(0, Math.max(0, last)) || "/";
}

/** `~/code/shop`, or `~\code\shop` on Windows, for a path in the home folder. */
export function shown(path: string, home: string): string {
  const inside = SEPARATORS.some((separator) => path.startsWith(home + separator));
  return inside ? `~${path.slice(home.length)}` : path;
}

export function outcomeText(removal: Removal, i18n: Translator): string {
  switch (removal.outcome) {
    case "deleted":
      return i18n.t("outcomeDeleted");
    case "trashed":
      return i18n.t(bin.outcome);
    case "partly":
      return i18n.plural("outcomePartly", removal.left);
    case "stopped":
      return i18n.t("outcomeNotReached");
    case "interrupted":
      return i18n.t("outcomeInterrupted");
    case "failed":
      return i18n.t("outcomeFailedWith", { why: removal.why });
    case "refused":
      return i18n.t("outcomeLeft", { why: whyText(removal.why, i18n) });
  }
}

function whyText(why: Why, i18n: Translator): string {
  switch (why.code) {
    case "outsideHome":
      return i18n.t("whyOutsideHome");
    case "changed":
      return i18n.t("whyChanged");
    case "notReached":
      return i18n.t("whyNotReached");
    case "inUse":
      return i18n.t("whyInUse", { who: why.by });
    case "unchecked":
      return i18n.t("whyUnchecked");
  }
}

export function refusalText(refusal: CleanRefusal, i18n: Translator): string {
  switch (refusal.code) {
    case "busy":
      return i18n.t("cleanBusy");
    case "expired":
      return i18n.t("cleanExpired");
    case "unlisted":
      return i18n.t("cleanUnlisted");
    case "changed":
      return i18n.t("mapChanged");
    case "failed":
      return i18n.t("reasonFailed", { message: refusal.message });
  }
}

/** A rejected call carries the backend's refusal; anything else is a failure. */
export function asRefusal(error: unknown): CleanRefusal {
  if (typeof error === "object" && error !== null && "code" in error && typeof error.code === "string") {
    return error as CleanRefusal;
  }
  return { code: "failed", message: String(error) };
}
