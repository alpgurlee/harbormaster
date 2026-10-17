import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import type { DaemonPreview, Reason, Stopped } from "./api";
import { useI18n, type Translator } from "./i18n";
import { asReason, describe, explain, keepFocusInside } from "./sheet";
import type { Finding } from "./types";
import styles from "./Sheet.module.css";

/** The backend's calls for stopping the daemons that keep an item in use. */
export interface DaemonStopper {
  daemonPreview: (token: number, path: string) => Promise<DaemonPreview>;
  daemonStop: (approval: number) => Promise<Stopped>;
}

type Ready = Extract<DaemonPreview, { kind: "ready" }>;

type Step =
  | { kind: "asking" }
  | { kind: "ready"; preview: Ready }
  | { kind: "stopping"; preview: Ready }
  | { kind: "ended"; stopped: Stopped }
  | { kind: "refused"; reason: Reason };

export interface DaemonSheetProps {
  /** The list the item is shown in. */
  token: number;
  finding: Finding;
  stopper: DaemonStopper;
  /** Nothing was stopped, or the user has read why not. */
  onClose: () => void;
  /** Every daemon stopped, so the list can look again. */
  onDone: () => void;
}

/** Asks before stopping the dev daemons that keep an item in use, and never forces them. */
export function DaemonSheet({ token, finding, stopper, onClose, onDone }: DaemonSheetProps) {
  const i18n = useI18n();
  const [step, setStep] = useState<Step>({ kind: "asking" });
  const dialog = useRef<HTMLDivElement>(null);
  const name = finding.inUse ?? finding.name;

  useEffect(() => {
    let current = true;
    stopper
      .daemonPreview(token, finding.path)
      .then((preview) => {
        if (current) setStep(preview.kind === "refused" ? preview : { kind: "ready", preview });
      })
      .catch((error: unknown) => {
        if (current) setStep({ kind: "refused", reason: asReason(error) });
      });
    return () => {
      current = false;
    };
  }, [token, finding.path, stopper]);

  // The main button takes the focus at each step, or the sheet itself while none is usable.
  useEffect(() => {
    const sheet = dialog.current;
    (sheet?.querySelector<HTMLButtonElement>("button[data-main]:not(:disabled)") ?? sheet)?.focus();
  }, [step.kind]);

  const stop = (preview: Ready) => {
    setStep({ kind: "stopping", preview });
    stopper
      .daemonStop(preview.approval)
      .then((stopped) => {
        if (stopped.outcomes.every(({ outcome }) => outcome === "stopped" || outcome === "alreadyGone")) {
          onDone();
        } else {
          setStep({ kind: "ended", stopped });
        }
      })
      .catch((error: unknown) => setStep({ kind: "refused", reason: asReason(error) }));
  };

  const busy = step.kind === "stopping";
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (!busy) onClose();
    } else if (event.key === "Tab") {
      keepFocusInside(event, dialog.current);
    }
  };

  return (
    <div className={styles.scrim}>
      <div
        ref={dialog}
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-labelledby="daemon-title"
        className={styles.sheet}
        onKeyDown={onKeyDown}
      >
        <h2 id="daemon-title" className={styles.title}>
          {i18n.t("stopTitlePlain", { name })}
        </h2>
        {body(step, i18n)}
        <div className={styles.buttons}>
          {step.kind === "ready" || step.kind === "stopping" ? (
            <>
              <button type="button" className={styles.quiet} disabled={busy} onClick={onClose}>
                {i18n.t("cancel")}
              </button>
              <button
                type="button"
                data-main
                className={styles.danger}
                disabled={busy}
                onClick={() => step.kind === "ready" && stop(step.preview)}
              >
                {busy ? i18n.t("stopping") : i18n.t("stop")}
              </button>
            </>
          ) : step.kind === "asking" ? null : (
            <button type="button" data-main className={styles.quiet} onClick={onClose}>
              {i18n.t("close")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function body(step: Step, i18n: Translator) {
  switch (step.kind) {
    case "asking":
      return <p className={styles.note}>{i18n.t("checking")}</p>;
    case "ready":
    case "stopping":
      return (
        <>
          <h3 className={styles.section}>{i18n.t("willStop")}</h3>
          <ul className={styles.stops}>
            {step.preview.stops.map((daemon) => (
              <li key={daemon.pid}>
                <span className={styles.name}>{daemon.name}</span>
                <span className={styles.command}>{daemon.command}</span>
                <span className={styles.pid}>PID {daemon.pid}</span>
              </li>
            ))}
          </ul>
          <p className={styles.warning}>{i18n.t("daemonWarning")}</p>
          <p className={styles.note}>{i18n.t("daemonHow")}</p>
        </>
      );
    case "ended":
      return (
        <>
          {step.stopped.stillRunning ? <p className={styles.note}>{i18n.t("stillRunningAfter")}</p> : null}
          <ul className={styles.stops}>
            {step.stopped.outcomes
              .filter(({ outcome }) => outcome !== "stopped" && outcome !== "alreadyGone")
              .map((daemon) => (
                <li key={daemon.pid}>
                  <span className={styles.name}>{daemon.name}</span>
                  <span className={styles.command}>{describe(daemon.outcome, i18n)}</span>
                  <span className={styles.pid}>PID {daemon.pid}</span>
                </li>
              ))}
          </ul>
        </>
      );
    case "refused":
      return <p className={styles.note}>{why(step.reason, i18n)}</p>;
  }
}

/** A stop's reasons, said of the daemons that keep an item in use. */
function why(reason: Reason, i18n: Translator): string {
  switch (reason.code) {
    case "notFound":
      return i18n.t("daemonGone");
    case "changed":
      return i18n.t("daemonChanged");
    default:
      return explain(reason, i18n);
  }
}
