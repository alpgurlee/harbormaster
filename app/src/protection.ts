import type { TextKey, Translator } from "./i18n";
import type { Protection } from "./types";

const REASONS: Record<Protection, TextKey> = {
  system: "protectedSystem",
  otherUser: "protectedOtherUser",
  otherSession: "protectedOtherSession",
  session: "protectedSession",
  unverified: "protectedUnverified",
  database: "protectedDatabase",
  sharedSocket: "protectedSharedSocket",
};

/** `it runs a database; …`: the reason alone, for a sentence of its own. */
export function protectionReason(i18n: Translator, protection: Protection): string {
  return i18n.t(REASONS[protection]);
}

export function whyProtected(i18n: Translator, protection: Protection): string {
  return i18n.t("wontStop", { reason: protectionReason(i18n, protection) });
}
