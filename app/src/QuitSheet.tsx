import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { forceQuitAppPreview, quitApp, quitAppPreview, type QuitPreview, type Reason, type Stopped } from "./api";
import { useI18n, type Translator } from "./i18n";
import { closesWindows, stopsAtOnce } from "./platform";
import { protectionReason } from "./protection";
import { displayName } from "./session";
import { asReason, describe, explain, keepFocusInside } from "./sheet";
import type { AppUsage } from "./types";
import styles from "./Sheet.module.css";

/** The backend's quit calls, which tests hand in themselves. */
export interface Quitter {
  /** `id` names the app as the breakdown lists it. */
  preview: (id: string) => Promise<QuitPreview>;
  quit: (token: number) => Promise<Stopped>;
  /** What ending every process of the app at once would do; Windows only. */
  force: (id: string) => Promise<QuitPreview>;
}

const BACKEND: Quitter = { preview: quitAppPreview, quit: quitApp, force: forceQuitAppPreview };

/** Asking an app to go: on Windows, its windows are asked to close. */
const ASK = closesWindows
  ? ({ title: "closeTitle", how: "closeHowApp", go: "closeWindows", going: "closing" } as const)
  : ({ title: "quitTitle", how: "quitHowApp", go: "quit", going: "quitting" } as const);

/** Programs, which Windows ends at once and a Mac asks to exit. */
const END = stopsAtOnce
  ? ({ title: "forceTitlePlain", will: "willEnd", go: "forceQuit", going: "ending" } as const)
  : ({ title: "quitTitle", will: "willQuit", go: "quit", going: "quitting" } as const);

type Ready = Exclude<QuitPreview, { kind: "refused" }>;

type Step =
  | { kind: "asking" }
  | { kind: "ready"; preview: Ready }
  | { kind: "quitting"; preview: Ready }
  | { kind: "ended"; preview: Ready; stopped: Stopped }
  | { kind: "refused"; reason: Reason };

export interface QuitSheetProps {
  app: AppUsage;
  quitter?: Quitter;
  /** Nothing was quit, or the user has read why not. */
  onClose: () => void;
  /** Everything quit. */
  onDone: () => void;
}

/** Asks before quitting an app and warns before closing bare programs. A Mac
 * never forces either; Windows ends programs at once, and an app that stays
 * open only once the user has seen what that ends. */
export function QuitSheet({ app, quitter = BACKEND, onClose, onDone }: QuitSheetProps) {
  const i18n = useI18n();
  const [step, setStep] = useState<Step>({ kind: "asking" });
  const [forcing, setForcing] = useState(false);
  const dialog = useRef<HTMLDivElement>(null);
  const name = displayName(app.name);

  const show = (preview: QuitPreview) => setStep(preview.kind === "refused" ? preview : { kind: "ready", preview });

  useEffect(() => {
    let current = true;
    quitter
      .preview(app.id)
      .then((preview) => {
        if (current) setStep(preview.kind === "refused" ? preview : { kind: "ready", preview });
      })
      .catch((error: unknown) => {
        if (current) setStep({ kind: "refused", reason: asReason(error) });
      });
    return () => {
      current = false;
    };
  }, [app.id, quitter]);

  // The main button takes the focus at each step, or the sheet itself while none is usable.
  useEffect(() => {
    const sheet = dialog.current;
    (sheet?.querySelector<HTMLButtonElement>("button[data-main]:not(:disabled)") ?? sheet)?.focus();
  }, [step.kind]);

  const quit = (preview: Ready) => {
    setStep({ kind: "quitting", preview });
    quitter
      .quit(preview.token)
      .then((stopped) => {
        if (stopped.outcomes.every(({ outcome }) => outcome === "stopped" || outcome === "alreadyGone")) {
          onDone();
        } else {
          setStep({ kind: "ended", preview, stopped });
        }
      })
      .catch((error: unknown) => setStep({ kind: "refused", reason: asReason(error) }));
  };

  const force = () => {
    setForcing(true);
    setStep({ kind: "asking" });
    quitter
      .force(app.id)
      .then(show)
      .catch((error: unknown) => setStep({ kind: "refused", reason: asReason(error) }));
  };

  const busy = step.kind === "quitting";
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (!busy) onClose();
    } else if (event.key === "Tab") {
      keepFocusInside(event, dialog.current);
    }
  };

  const programs = forcing || ("preview" in step && step.preview.kind === "programs");
  const words = programs ? END : ASK;
  // What an app's closed windows left running may be ended next, once asked.
  const forceable = closesWindows && step.kind === "ended" && step.preview.kind === "app";

  return (
    <div className={styles.scrim}>
      <div
        ref={dialog}
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-labelledby="quit-title"
        className={styles.sheet}
        onKeyDown={onKeyDown}
      >
        <h2 id="quit-title" className={styles.title}>
          {i18n.t(words.title, { name })}
        </h2>
        {body(step, name, i18n)}
        <div className={styles.buttons}>
          {step.kind === "ready" || step.kind === "quitting" ? (
            <>
              <button type="button" className={styles.quiet} disabled={busy} onClick={onClose}>
                {i18n.t("cancel")}
              </button>
              <button
                type="button"
                data-main
                className={styles.danger}
                disabled={busy}
                onClick={() => step.kind === "ready" && quit(step.preview)}
              >
                {busy ? i18n.t(words.going) : i18n.t(words.go)}
              </button>
            </>
          ) : forceable ? (
            <>
              <button type="button" className={styles.quiet} onClick={onClose}>
                {i18n.t("close")}
              </button>
              <button type="button" data-main className={styles.danger} onClick={force}>
                {i18n.t("forceQuitEllipsis")}
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

function body(step: Step, name: string, i18n: Translator) {
  switch (step.kind) {
    case "asking":
      return <p className={styles.note}>{i18n.t("checking")}</p>;
    case "ready":
    case "quitting":
      if (step.preview.kind === "app") return <p className={styles.note}>{i18n.t(ASK.how, { name })}</p>;
      return (
        <>
          <h3 className={styles.section}>{i18n.t(END.will)}</h3>
          <ul className={styles.stops}>
            {step.preview.quits.map((process) => (
              <li key={process.pid}>
                <span className={styles.name}>{process.name}</span>
                <span className={styles.command}>{process.command}</span>
                <span className={styles.pid}>PID {process.pid}</span>
              </li>
            ))}
          </ul>
          {step.preview.staysOpen.length > 0 ? (
            <>
              <h3 className={styles.section}>{i18n.t("staysOpen")}</h3>
              <ul className={styles.stays}>
                {[...new Set(step.preview.staysOpen.map((process) => displayName(process.name)))].map((label) => (
                  <li key={label}>{label}</li>
                ))}
              </ul>
            </>
          ) : null}
          {stopsAtOnce ? (
            <p className={styles.warning}>{i18n.t("howItEnds")}</p>
          ) : (
            <>
              <p className={styles.warning}>{i18n.t("quitWarning")}</p>
              <p className={styles.note}>{i18n.t("quitHowPrograms")}</p>
            </>
          )}
        </>
      );
    case "ended": {
      const left = step.stopped.outcomes.filter(({ outcome }) => outcome !== "stopped" && outcome !== "alreadyGone");
      if (step.preview.kind === "app") {
        // An app's helpers are many; only why a window wasn't asked is worth naming.
        const refused = left.filter(({ outcome }) => typeof outcome === "object");
        return (
          <>
            {step.stopped.stillRunning ? <p className={styles.note}>{i18n.t("stillOpen", { name })}</p> : null}
            {refused.length > 0 ? outcomes(refused, i18n) : null}
          </>
        );
      }
      return (
        <>
          {step.stopped.stillRunning ? <p className={styles.note}>{i18n.t("stillRunningAfter")}</p> : null}
          {outcomes(left, i18n)}
        </>
      );
    }
    case "refused":
      return <p className={styles.note}>{why(step.reason, name, i18n)}</p>;
  }
}

function outcomes(processes: Stopped["outcomes"], i18n: Translator) {
  return (
    <ul className={styles.stops}>
      {processes.map((process) => (
        <li key={process.pid}>
          <span className={styles.name}>{process.name}</span>
          <span className={styles.command}>{describe(process.outcome, i18n)}</span>
          <span className={styles.pid}>PID {process.pid}</span>
        </li>
      ))}
    </ul>
  );
}

/** A stop's reasons, said of an app. */
function why(reason: Reason, name: string, i18n: Translator): string {
  switch (reason.code) {
    case "notFound":
      return i18n.t("quitGone", { name });
    case "protected":
      return i18n.t("wontQuit", { reason: protectionReason(i18n, reason.protection) });
    case "changed":
      return i18n.t("quitChanged");
    default:
      return explain(reason, i18n);
  }
}
