import type { TextKey, Translator } from "./i18n";
import type { Protection } from "./types";

const REASONS: Record<Protection, TextKey> = {
  system: "protectedSystem",
  otherUser: "protectedOtherUser",
  session: "protectedSession",
  unverified: "protectedUnverified",
  database: "protectedDatabase",
  sharedSocket: "protectedSharedSocket",
};

export function whyProtected(i18n: Translator, protection: Protection): string {
  return i18n.t("wontStop", { reason: i18n.t(REASONS[protection]) });
}
