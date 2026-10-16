import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { cleanDisk, cleanReveal, cleanRun, cleanScan, cleanStop } from "./api";
import { MODULES, groupsOf, picked, plan, type CleanModule, type Plan } from "./cleanup";
import { TINTS, asRefusal, outcomeText, refusalText } from "./cleanerText";
import { useI18n, type Translator } from "./i18n";
import { Review } from "./Review";
import { keepFocusInside } from "./sheet";
import type { CleanRefusal, Cleaned, Disk, Finding, Removal, RunEvent, ScanEvent } from "./types";
import styles from "./Cleaner.module.css";
import sheet from "./Sheet.module.css";

/** The backend's cleaner calls, which tests hand in themselves. */
export interface CleanerBackend {
  scan: (onEvent: (event: ScanEvent) => void) => Promise<void>;
  run: (token: number, paths: string[], onEvent: (event: RunEvent) => void) => Promise<void>;
  stop: () => Promise<void>;
  disk: () => Promise<Disk>;
  reveal: (path: string) => Promise<void>;
}

const BACKEND: CleanerBackend = { scan: cleanScan, run: cleanRun, stop: cleanStop, disk: cleanDisk, reveal: cleanReveal };

const SECTIONS: CleanModule[] = ["developer", "system"];
const SECTION_NAMES = { developer: "moduleDeveloper", system: "moduleSystem" } as const;

type View = "overview" | CleanModule;

type Step =
  | { kind: "ready" }
  | { kind: "scanning"; done: number; total: number; found: number }
  | { kind: "found"; token: number; home: string; findings: Finding[] }
  | { kind: "removing"; home: string; chosen: Finding[]; removals: Removal[] }
  | { kind: "cleaned"; home: string; chosen: Finding[]; cleaned: Cleaned; removals: Removal[] }
  | { kind: "refused"; refusal: CleanRefusal; partial?: { chosen: Finding[]; removals: Removal[] } };

type Found = Extract<Step, { kind: "found" }>;

export interface CleanerProps {
  backend?: CleanerBackend;
  /** Milliseconds since the Unix epoch, for how long projects have sat. */
  now?: number;
}

/** Finds what this Mac can do without, says what removing it would do, and removes only what the user chose. */
export function Cleaner({ backend = BACKEND, now = Date.now() }: CleanerProps) {
  const i18n = useI18n();
  const [step, setStep] = useState<Step>({ kind: "ready" });
  const [view, setView] = useState<View>("overview");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [disk, setDisk] = useState<Disk | null>(null);
  const [confirming, setConfirming] = useState(false);
  // What had the focus before the sheet, to give it back when the sheet goes.
  const returnFocus = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (confirming) return;
    returnFocus.current?.focus();
    returnFocus.current = null;
  }, [confirming]);

  const measureDisk = useCallback(() => {
    backend
      .disk()
      .then(setDisk)
      .catch(() => setDisk(null));
  }, [backend]);
  useEffect(measureDisk, [measureDisk]);

  const scan = useCallback(() => {
    setStep({ kind: "scanning", done: 0, total: 0, found: 0 });
    setView("overview");
    setConfirming(false);
    backend
      .scan((event) => {
        if (event.kind === "measured") {
          setStep((current) =>
            current.kind === "scanning"
              ? {
                  kind: "scanning",
                  done: Math.max(current.done, event.done),
                  total: event.total,
                  found: current.found + event.bytes,
                }
              : current,
          );
        } else {
          setChosen(picked(event.findings));
          setStep({ kind: "found", token: event.token, home: event.home, findings: event.findings });
        }
      })
      .catch((error: unknown) => setStep({ kind: "refused", refusal: asRefusal(error) }));
  }, [backend]);

  const run = (found: Found) => {
    const going = found.findings.filter((finding) => chosen.has(finding.path));
    setConfirming(false);
    setView("overview");
    setStep({ kind: "removing", home: found.home, chosen: going, removals: [] });
    backend
      .run(
        found.token,
        going.map((finding) => finding.path),
        (event) => {
          if (event.kind === "done") measureDisk();
          setStep((current) => {
            // Events may still come after the app turned the rest down.
            if (event.kind === "removed" && current.kind === "refused" && current.partial !== undefined) {
              const { kind: _, ...removal } = event;
              return { ...current, partial: { ...current.partial, removals: [...current.partial.removals, removal] } };
            }
            if (current.kind !== "removing") return current;
            if (event.kind === "removed") {
              const { kind: _, ...removal } = event;
              return { ...current, removals: [...current.removals, removal] };
            }
            return {
              kind: "cleaned",
              home: current.home,
              chosen: current.chosen,
              cleaned: { removals: event.removals, freedBytes: event.freedBytes },
              removals: current.removals,
            };
          });
        },
      )
      .catch((error: unknown) =>
        // What was removed before the app turned the rest down stays in sight.
        setStep((current) => ({
          kind: "refused",
          refusal: asRefusal(error),
          partial: current.kind === "removing" ? { chosen: current.chosen, removals: current.removals } : undefined,
        })),
      );
  };

  const working = step.kind === "scanning" || step.kind === "removing";
  const found = step.kind === "found" ? step : null;
  const sectionBytes = (section: CleanModule) =>
    found?.findings.filter((finding) => MODULES[section].includes(finding.group)).reduce((total, each) => total + each.bytes, 0) ?? 0;
  const title = view === "overview" ? i18n.t("overview") : i18n.t(SECTION_NAMES[view]);

  return (
    <div className={styles.window}>
      <nav className={styles.side} aria-label={i18n.t("clean")} inert={confirming}>
        <div className={styles.sideTop} data-tauri-drag-region />
        <button
          type="button"
          className={styles.section}
          aria-current={view === "overview" ? "page" : undefined}
          onClick={() => setView("overview")}
        >
          {i18n.t("overview")}
        </button>
        {SECTIONS.map((section) => (
          <button
            key={section}
            type="button"
            className={styles.section}
            aria-current={view === section ? "page" : undefined}
            disabled={found === null}
            onClick={() => setView(section)}
          >
            {i18n.t(SECTION_NAMES[section])}
            {found === null ? null : <span className={styles.sectionSize}>{i18n.bytes(sectionBytes(section))}</span>}
          </button>
        ))}
        {disk === null ? null : (
          <p className={styles.diskNote}>
            {i18n.t("diskFree", { free: i18n.bytes(disk.free), total: i18n.bytes(disk.total) })}
          </p>
        )}
      </nav>
      <div className={styles.main} inert={confirming}>
        <header className={styles.bar} data-tauri-drag-region>
          <h1 className={styles.title}>{title}</h1>
          {working ? (
            <button type="button" className={styles.action} onClick={() => backend.stop().catch(console.error)}>
              {i18n.t("stop")}
            </button>
          ) : step.kind === "ready" ? null : (
            <button type="button" className={styles.action} onClick={scan}>
              {i18n.t("cleanScanAgain")}
            </button>
          )}
        </header>
        <div className={styles.page}>
          {view !== "overview" && found !== null ? (
            <Review
              key={view}
              findings={found.findings.filter((finding) => MODULES[view].includes(finding.group))}
              home={found.home}
              chosen={chosen}
              now={now}
              onChoose={setChosen}
              onReveal={(finding) => backend.reveal(finding.path).catch(console.error)}
            />
          ) : (
            overview(step, { disk, chosen, i18n, onScan: scan, onReview: setView })
          )}
        </div>
        {found !== null && found.findings.length > 0 ? (
          <Footer
            plan={plan(found.findings, chosen)}
            muted={confirming}
            onClean={() => {
              returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
              setConfirming(true);
            }}
          />
        ) : null}
      </div>
      {confirming && found !== null ? (
        <Confirm plan={plan(found.findings, chosen)} onCancel={() => setConfirming(false)} onConfirm={() => run(found)} />
      ) : null}
    </div>
  );
}

interface Overview {
  disk: Disk | null;
  chosen: Set<string>;
  i18n: Translator;
  onScan: () => void;
  onReview: (section: CleanModule) => void;
}

function overview(step: Step, { disk, chosen, i18n, onScan, onReview }: Overview) {
  switch (step.kind) {
    case "ready":
      return (
        <div className={styles.start}>
          <Ring fraction={disk === null ? 0 : 1 - disk.free / disk.total}>
            {disk === null ? null : (
              <>
                <strong>{i18n.bytes(disk.free)}</strong>
                <span>{i18n.t("diskOf", { total: i18n.bytes(disk.total) })}</span>
              </>
            )}
          </Ring>
          <button type="button" className={styles.scan} onClick={onScan}>
            {i18n.t("scan")}
          </button>
          <p className={styles.hint}>{i18n.t("scanHint")}</p>
        </div>
      );
    case "scanning":
      return (
        <div className={styles.start} role="status">
          <Ring fraction={step.total === 0 ? 0 : step.done / step.total} waiting={step.total === 0}>
            <strong>{i18n.bytes(step.found)}</strong>
          </Ring>
          <p className={styles.phase}>{step.total === 0 ? i18n.t("cleanLooking") : i18n.t("cleanMeasured", step)}</p>
        </div>
      );
    case "found":
      return step.findings.length === 0 ? (
        <p className={styles.empty}>{i18n.t("cleanNothing")}</p>
      ) : (
        <Summary step={step} chosen={chosen} i18n={i18n} onReview={onReview} />
      );
    case "removing":
      return (
        <>
          <div className={styles.start} role="status">
            <Ring fraction={step.chosen.length === 0 ? 0 : step.removals.length / step.chosen.length}>
              <strong>
                {step.removals.length}/{step.chosen.length}
              </strong>
            </Ring>
            <p className={styles.phase}>{i18n.t("removing", { done: step.removals.length, total: step.chosen.length })}</p>
          </div>
          <Outcomes removals={step.removals} chosen={step.chosen} i18n={i18n} />
        </>
      );
    case "cleaned":
      return <Done step={step} i18n={i18n} />;
    case "refused":
      return (
        <>
          <p className={styles.empty}>{refusalText(step.refusal, i18n)}</p>
          {step.partial === undefined ? null : (
            <Outcomes removals={step.partial.removals} chosen={step.partial.chosen} i18n={i18n} />
          )}
        </>
      );
  }
}

/** A share of a whole, drawn as a ring with something to say in its middle. */
function Ring({ fraction, waiting = false, children }: { fraction: number; waiting?: boolean; children: ReactNode }) {
  const circumference = 2 * Math.PI * 52;
  return (
    <div className={styles.ring} data-waiting={waiting ? "" : undefined}>
      <svg viewBox="0 0 120 120" aria-hidden="true">
        <circle className={styles.ringTrack} cx="60" cy="60" r="52" />
        <circle
          className={styles.ringFill}
          cx="60"
          cy="60"
          r="52"
          strokeDasharray={circumference}
          strokeDashoffset={circumference * (1 - Math.min(1, Math.max(0, fraction)))}
          transform="rotate(-90 60 60)"
        />
      </svg>
      <div className={styles.ringText}>{children}</div>
    </div>
  );
}

function Summary({
  step,
  chosen,
  i18n,
  onReview,
}: {
  step: Found;
  chosen: Set<string>;
  i18n: Translator;
  onReview: (section: CleanModule) => void;
}) {
  const groups = groupsOf(step.findings, chosen);
  const all = groups.reduce((total, group) => total + group.bytes, 0);
  const chosenBytes = groups.reduce((total, group) => total + group.chosenBytes, 0);
  return (
    <>
      <section className={styles.hero} data-testid="hero">
        <p className={styles.figure}>{i18n.bytes(chosenBytes)}</p>
        <p className={styles.caption}>{i18n.t("cleanChosen", { total: i18n.bytes(all) })}</p>
      </section>
      <div className={styles.spread} aria-hidden="true">
        {groups.map((group) => (
          <span
            key={group.group}
            className={styles.share}
            style={{ flexGrow: group.bytes, ["--tint" as string]: `var(--tint-${TINTS[group.group]})` }}
          >
            <span className={styles.shareChosen} style={{ width: `${(100 * group.chosenBytes) / group.bytes}%` }} />
          </span>
        ))}
      </div>
      <ul className={styles.cards}>
        {SECTIONS.map((section) => {
          const members = groups.filter((group) => MODULES[section].includes(group.group));
          if (members.length === 0) return null;
          const found = members.reduce((total, group) => total + group.bytes, 0);
          const picks = members.reduce((total, group) => total + group.chosenBytes, 0);
          return (
            <li key={section} className={styles.card}>
              <span className={styles.cardName}>{i18n.t(SECTION_NAMES[section])}</span>
              <span className={styles.cardFigure}>{i18n.bytes(found)}</span>
              <span className={styles.cardNote}>
                {i18n.t("moduleSummary", { found: i18n.bytes(found), chosen: i18n.bytes(picks) })}
              </span>
              <button type="button" className={styles.cardButton} onClick={() => onReview(section)}>
                {i18n.t("review")}
              </button>
            </li>
          );
        })}
      </ul>
    </>
  );
}

function Footer({ plan, muted, onClean }: { plan: Plan; muted: boolean; onClean: () => void }) {
  const i18n = useI18n();
  return (
    <footer className={styles.foot} inert={muted}>
      <span className={styles.total}>
        <strong>{i18n.plural("items", plan.count)}</strong> · {i18n.bytes(plan.bytes)}
      </span>
      <button type="button" className={styles.primary} disabled={plan.count === 0} onClick={onClean}>
        {i18n.t("cleanEllipsis")}
      </button>
    </footer>
  );
}

/** Asks once more, saying plainly what is gone for good and what can be put back. */
function Confirm({ plan, onCancel, onConfirm }: { plan: Plan; onCancel: () => void; onConfirm: () => void }) {
  const i18n = useI18n();
  const dialog = useRef<HTMLDivElement>(null);
  useEffect(() => {
    dialog.current?.querySelector<HTMLButtonElement>("button[data-main]")?.focus();
  }, []);
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    } else if (event.key === "Tab") {
      keepFocusInside(event, dialog.current);
    }
  };
  return (
    <div className={`${sheet.scrim} ${styles.scrim}`}>
      <div
        ref={dialog}
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-labelledby="clean-title"
        className={`${sheet.sheet} ${styles.sheet}`}
        onKeyDown={onKeyDown}
      >
        <h2 id="clean-title" className={sheet.title}>
          {i18n.t("confirmClean", { bytes: i18n.bytes(plan.bytes) })}
        </h2>
        <ul className={styles.consequences}>
          {plan.deleted.count > 0 ? <li>{i18n.plural("confirmDeleted", plan.deleted.count)}</li> : null}
          {plan.trashed.count > 0 ? <li>{i18n.plural("confirmTrashed", plan.trashed.count)}</li> : null}
          {plan.emptied ? <li>{i18n.t("confirmEmptied")}</li> : null}
        </ul>
        <p className={sheet.note}>{i18n.t("confirmChecked")}</p>
        <div className={sheet.buttons}>
          <button type="button" className={sheet.quiet} onClick={onCancel}>
            {i18n.t("cancel")}
          </button>
          <button type="button" data-main className={sheet.danger} onClick={onConfirm}>
            {i18n.t("clean")}
          </button>
        </div>
      </div>
    </div>
  );
}

function Done({ step, i18n }: { step: Extract<Step, { kind: "cleaned" }>; i18n: Translator }) {
  const removals = step.cleaned.removals.length > 0 ? step.cleaned.removals : step.removals;
  const cleaned = removals.filter(({ outcome }) => outcome === "deleted" || outcome === "trashed").length;
  const left = removals.length - cleaned;
  return (
    <>
      <section className={styles.hero} data-testid="hero">
        <p className={styles.figure}>{i18n.t("freed", { bytes: i18n.bytes(step.cleaned.freedBytes) })}</p>
        <p className={styles.caption}>
          {i18n.plural("cleanedItems", cleaned)}
          {left > 0 ? ` · ${i18n.plural("leftItems", left)}` : ""}
        </p>
      </section>
      <Outcomes removals={removals} chosen={step.chosen} i18n={i18n} />
    </>
  );
}

/** Each removal as it went, what didn't go first. */
function Outcomes({ removals, chosen, i18n }: { removals: Removal[]; chosen: Finding[]; i18n: Translator }) {
  const gone = (removal: Removal) => removal.outcome === "deleted" || removal.outcome === "trashed";
  const ordered = [...removals.filter((removal) => !gone(removal)), ...removals.filter(gone)];
  return (
    <ul className={styles.outcomes}>
      {ordered.map((removal) => (
        <li key={removal.path} className={styles.outcome} data-gone={gone(removal) ? "" : undefined}>
          <span className={styles.itemName}>{chosen.find((finding) => finding.path === removal.path)?.name ?? removal.path}</span>
          <span className={styles.itemNote}>{outcomeText(removal, i18n)}</span>
        </li>
      ))}
    </ul>
  );
}
