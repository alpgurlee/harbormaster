import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { forceStop, stopPreview, stopServer, type Outcome, type Preview, type Reason, type Stopped } from "./api";
import { useI18n, type Translator } from "./i18n";
import { whyProtected } from "./protection";
import { displayName } from "./session";
import type { Service } from "./types";
import styles from "./StopSheet.module.css";

/** The backend's stop calls, which tests hand in themselves. */
export interface Stopper {
  preview: (id: string) => Promise<Preview>;
  stop: (token: number) => Promise<Stopped>;
  force: (token: number) => Promise<Stopped>;
}

const BACKEND: Stopper = { preview: stopPreview, stop: stopServer, force: forceStop };

type Ready = Extract<Preview, { kind: "ready" }>;

type Step =
  | { kind: "asking" }
  | { kind: "ready"; preview: Ready }
  | { kind: "stopping"; preview: Ready }
  | { kind: "stuck"; preview: Ready; stopped: Stopped }
  | { kind: "forcing"; preview: Ready; stopped: Stopped }
  | { kind: "ended"; stopped: Stopped }
  | { kind: "refused"; reason: Reason };

export interface StopSheetProps {
  service: Service;
  stopper?: Stopper;
  /** Nothing was stopped, or the user has read why not. */
  onClose: () => void;
  /** Everything stopped. */
  onDone: () => void;
}

/** Asks before stopping a server, says what happened, and offers force only after a stop. */
export function StopSheet({ service, stopper = BACKEND, onClose, onDone }: StopSheetProps) {
  const i18n = useI18n();
  const [step, setStep] = useState<Step>({ kind: "asking" });
  const dialog = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let current = true;
    stopper
      .preview(service.id)
      .then((preview) => {
        if (current) setStep(preview.kind === "ready" ? { kind: "ready", preview } : preview);
      })
      .catch((error: unknown) => {
        if (current) setStep({ kind: "refused", reason: asReason(error) });
      });
    return () => {
      current = false;
    };
  }, [service.id, stopper]);

  // The main button takes the focus at each step, or the sheet itself while none is usable.
  useEffect(() => {
    const sheet = dialog.current;
    (sheet?.querySelector<HTMLButtonElement>("button[data-main]:not(:disabled)") ?? sheet)?.focus();
  }, [step.kind]);

  const settle = (preview: Ready, stopped: Stopped) => {
    if (stopped.stillRunning) {
      setStep({ kind: "stuck", preview, stopped });
    } else if (stopped.outcomes.every(({ outcome }) => outcome === "stopped" || outcome === "alreadyGone")) {
      onDone();
    } else {
      setStep({ kind: "ended", stopped });
    }
  };
  const stop = (preview: Ready) => {
    setStep({ kind: "stopping", preview });
    stopper
      .stop(preview.token)
      .then((stopped) => settle(preview, stopped))
      .catch((error: unknown) => setStep({ kind: "refused", reason: asReason(error) }));
  };
  const force = (preview: Ready, stopped: Stopped) => {
    setStep({ kind: "forcing", preview, stopped });
    stopper
      .force(preview.token)
      .then((after) => settle(preview, after))
      .catch((error: unknown) => setStep({ kind: "refused", reason: asReason(error) }));
  };

  const busy = step.kind === "stopping" || step.kind === "forcing";
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (!busy) onClose();
    } else if (event.key === "Tab") {
      keepFocusInside(event, dialog.current);
    }
  };

  const name = service.project?.name ?? service.processes[0]?.name ?? "";
  const ports = "preview" in step ? step.preview.frees.map((port) => `:${port}`).join(", ") : "";

  return (
    <div className={styles.scrim}>
      <div
        ref={dialog}
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-labelledby="stop-title"
        className={styles.sheet}
        onKeyDown={onKeyDown}
      >
        <h2 id="stop-title" className={styles.title}>
          {ports ? i18n.t("stopTitle", { name, ports }) : i18n.t("stopTitlePlain", { name })}
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
                {busy ? i18n.t("stopping") : i18n.t("stopServer")}
              </button>
            </>
          ) : step.kind === "stuck" || step.kind === "forcing" ? (
            <>
              <button type="button" className={styles.quiet} disabled={busy} onClick={onClose}>
                {i18n.t("cancel")}
              </button>
              <button
                type="button"
                data-main
                className={styles.danger}
                disabled={busy}
                onClick={() => step.kind === "stuck" && force(step.preview, step.stopped)}
              >
                {busy ? i18n.t("forcing") : i18n.t("forceStop")}
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
            {step.preview.stops.map((process) => (
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
                {[...new Set(step.preview.staysOpen.map((holder) => displayName(holder.name)))].map((label) => (
                  <li key={label}>{label}</li>
                ))}
              </ul>
            </>
          ) : null}
          <p className={styles.note}>{i18n.t("howItStops")}</p>
        </>
      );
    case "stuck":
    case "forcing":
      return (
        <>
          <p className={styles.note}>{i18n.t("stillRunningAfter")}</p>
          <ul className={styles.stops}>
            {step.stopped.outcomes
              .filter(({ outcome }) => outcome !== "stopped" && outcome !== "alreadyGone")
              .map((process) => (
                <li key={process.pid}>
                  <span className={styles.name}>{process.name}</span>
                  <span className={styles.command}>{describe(process.outcome, i18n)}</span>
                  <span className={styles.pid}>PID {process.pid}</span>
                </li>
              ))}
          </ul>
        </>
      );
    case "ended":
      return (
        <ul className={styles.stops}>
          {step.stopped.outcomes.map((process) => (
            <li key={process.pid}>
              <span className={styles.name}>{process.name}</span>
              <span className={styles.command}>{describe(process.outcome, i18n)}</span>
              <span className={styles.pid}>PID {process.pid}</span>
            </li>
          ))}
        </ul>
      );
    case "refused":
      return <p className={styles.note}>{explain(step.reason, i18n)}</p>;
  }
}

function describe(outcome: Outcome, i18n: Translator): string {
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

function explain(reason: Reason, i18n: Translator): string {
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
    case "failed":
      return i18n.t("reasonFailed", { message: reason.message });
  }
}

/** A rejected stop carries the backend's reason; anything else is a failure. */
function asReason(error: unknown): Reason {
  if (typeof error === "object" && error !== null && "code" in error && typeof error.code === "string") {
    return error as Reason;
  }
  return { code: "failed", message: String(error) };
}

/** Tab goes round the sheet's buttons, never behind it. */
function keepFocusInside(event: KeyboardEvent, dialog: HTMLElement | null) {
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
