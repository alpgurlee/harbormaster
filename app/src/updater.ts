import { useEffect, useState } from "react";

import { updateCheck, updateInstall, updateOpenAppManagement, updateSubscribe, updateWhatsNew } from "./api";
import type { Translator } from "./i18n";
import type { UpdateStatus } from "./types";

/** The app's updater calls, which tests hand in themselves. */
export interface Updater {
  subscribe: (told: (status: UpdateStatus) => void) => Promise<void>;
  check: () => Promise<void>;
  install: () => Promise<void>;
  whatsNew: () => Promise<void>;
  openAppManagement: () => Promise<void>;
}

export const UPDATER: Updater = {
  subscribe: updateSubscribe,
  check: updateCheck,
  install: updateInstall,
  whatsNew: updateWhatsNew,
  openAppManagement: updateOpenAppManagement,
};

/** What the app knows of updates, as it tells it; nothing without an updater or before it has spoken. */
export function useUpdateStatus(updater: Updater | null): UpdateStatus | null {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  useEffect(() => {
    if (updater === null) return;
    let live = true;
    updater
      .subscribe((next) => {
        if (live) setStatus(next);
      })
      .catch(console.error);
    return () => {
      live = false;
    };
  }, [updater]);
  return status;
}

/** How far an update has come, as a line; nothing for a failed check, which only the About section tells. */
export function progressText(status: UpdateStatus, i18n: Translator): string | null {
  switch (status.kind) {
    case "available":
      return i18n.t("updateAvailable", { version: status.version });
    case "downloading":
      return status.percent === null
        ? i18n.t("updateDownloadingSome", { version: status.version })
        : i18n.t("updateDownloading", { version: status.version, percent: String(status.percent) });
    case "installing":
      return i18n.t("updateInstalling", { version: status.version });
    default:
      return null;
  }
}

/** Why an install didn't come through, as a line; `mac` names the macOS setting that may let it. */
export function failureText(status: UpdateStatus & { kind: "failed" }, i18n: Translator, mac: boolean): string {
  switch (status.why) {
    case "busy":
      return i18n.t("updateBusy");
    case "cancelled":
      return i18n.t("updateCancelled");
    case "permission":
      return mac ? i18n.t("updatePermission") : i18n.t("updateFailed", { message: status.message });
    default:
      return i18n.t("updateFailed", { message: status.message });
  }
}
