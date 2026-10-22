import type { KeyboardEvent } from "react";

import type { Outcome, Reason } from "./api";
import type { Translator } from "./i18n";
import { whyProtected } from "./protection";

export function describe(outcome: Outcome, i18n: Translator): string {
  if (typeof outcome === "object") return i18n.t("outcomeFailed", { message: outcome.failed });
  switch (outcome) {
    case "stopped":
      return i18n.t("outcomeStopped");
    case "alreadyGone":
      return i18n.t("outcomeAlreadyGone");
    case "replaced":
      return i18n.t("outcomeReplaced");
    case "accessDenied":
      return i18n.t("outcomeAccessDenied");
    case "stillRunning":
      return i18n.t("outcomeStillRunning");
  }
}

/** Why a stop was turned down. */
export function explain(reason: Reason, i18n: Translator): string {
  switch (reason.code) {
    case "notFound":
      return i18n.t("gone");
    case "protected":
      return whyProtected(i18n, reason.protection);
    case "runsHarbormaster":
      return i18n.t("reasonRunsHarbormaster");
    case "changed":
      return i18n.t("reasonChanged");
    case "expired":
      return i18n.t("reasonExpired");
    case "untrackedExec":
      return i18n.t("reasonUntrackedExec");
    case "unknownLineage":
      return i18n.t("reasonUnknownLineage");
    case "notAllowed":
      return i18n.t("reasonNotAllowed");
    case "strays":
      return i18n.t("reasonStrays");
    case "failed":
      return i18n.t("reasonFailed", { message: reason.message });
  }
}

/** A rejected call carries the backend's reason; anything else is a failure. */
export function asReason(error: unknown): Reason {
  if (typeof error === "object" && error !== null && "code" in error && typeof error.code === "string") {
    return error as Reason;
  }
  return { code: "failed", message: String(error) };
}

/** Tab goes round the sheet's buttons, never behind it. */
export function keepFocusInside(event: KeyboardEvent, dialog: HTMLElement | null) {
  const buttons = [...(dialog?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
  if (buttons.length === 0) {
    event.preventDefault();
    return;
  }
  const first = buttons[0];
  const last = buttons[buttons.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}
