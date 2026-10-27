import { useI18n } from "./i18n";
import { failureText, progressText } from "./updater";
import type { UpdateStatus } from "./types";
import styles from "./UpdateBar.module.css";

export interface UpdateBarProps {
  status: UpdateStatus;
  onUpdate: () => void;
  onLater: () => void;
  onWhatsNew: () => void;
  /** Opens the macOS setting that lets Harbormaster replace itself; a Mac alone has one. */
  onAllow?: () => void;
}

/** A line under the panel's header about a newer version: offered, coming, or not come through. */
export function UpdateBar({ status, onUpdate, onLater, onWhatsNew, onAllow }: UpdateBarProps) {
  const i18n = useI18n();
  if (status.kind === "failed") {
    // A failed check is the About section's to tell.
    if (status.why === "check" || status.version === null) return null;
    return (
      <div className={styles.bar} role="status">
        <span className={styles.text}>{failureText(status, i18n, onAllow !== undefined)}</span>
        <span className={styles.actions}>
          {status.why === "permission" && onAllow !== undefined ? (
            <button type="button" className={styles.button} onClick={onAllow}>
              {i18n.t("openAppManagement")}
            </button>
          ) : null}
          <button type="button" className={styles.button} onClick={onUpdate}>
            {i18n.t("updateTryAgain")}
          </button>
        </span>
      </div>
    );
  }
  const text = progressText(status, i18n);
  if (text === null) return null;
  return (
    <div className={styles.bar} role="status">
      <span className={styles.text}>{text}</span>
      {status.kind === "available" ? (
        <span className={styles.actions}>
          <button type="button" className={styles.button} onClick={onWhatsNew}>
            {i18n.t("updateWhatsNew")}
          </button>
          <button type="button" className={`${styles.button} ${styles.main}`} onClick={onUpdate}>
            {i18n.t("updateNow")}
          </button>
          <button type="button" className={styles.button} onClick={onLater}>
            {i18n.t("updateLater")}
          </button>
        </span>
      ) : null}
    </div>
  );
}
