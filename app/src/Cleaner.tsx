import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";

import { cleanRun, cleanScan, cleanStop } from "./api";
import { groupsOf, picked, plan, selectable, toggled, type GroupView, type Plan } from "./cleanup";
import { useI18n, type TextKey, type Translator } from "./i18n";
import { Chevron } from "./icons";
import { keepFocusInside } from "./sheet";
import type { CleanGroup, CleanRefusal, Cleaned, Finding, Removal, Why } from "./types";
import type { RunEvent, ScanEvent } from "./types";
import styles from "./Cleaner.module.css";
import sheet from "./Sheet.module.css";

/** The backend's cleaner calls, which tests hand in themselves. */
export interface CleanerBackend {
  scan: (onEvent: (event: ScanEvent) => void) => Promise<void>;
  run: (token: number, paths: string[], onEvent: (event: RunEvent) => void) => Promise<void>;
  stop: () => Promise<void>;
}

const BACKEND: CleanerBackend = { scan: cleanScan, run: cleanRun, stop: cleanStop };

const NAMES: Record<CleanGroup, TextKey> = {
  projects: "groupProjects",
  packages: "groupPackages",
  xcode: "groupXcode",
  apps: "groupApps",
  logs: "groupLogs",
  trash: "groupTrash",
};

const ABOUT: Record<CleanGroup, TextKey> = {
  projects: "aboutProjects",
  packages: "aboutPackages",
  xcode: "aboutXcode",
  apps: "aboutApps",
  logs: "aboutLogs",
  trash: "aboutTrash",
};

/** Each group's color, from the panel's project tints. */
const TINTS: Record<CleanGroup, number> = { projects: 0, packages: 3, xcode: 1, apps: 5, logs: 4, trash: 2 };

type Step =
  | { kind: "scanning"; done: number; total: number; found: number }
  | { kind: "found"; token: number; home: string; findings: Finding[] }
  | { kind: "removing"; home: string; chosen: Finding[]; removals: Removal[] }
  | { kind: "cleaned"; home: string; chosen: Finding[]; cleaned: Cleaned; removals: Removal[] }
  | { kind: "refused"; refusal: CleanRefusal; partial?: { chosen: Finding[]; removals: Removal[] } };

type Found = Extract<Step, { kind: "found" }>;

const SCANNING: Step = { kind: "scanning", done: 0, total: 0, found: 0 };

export interface CleanerProps {
  backend?: CleanerBackend;
  /** Milliseconds since the Unix epoch, for how long projects have sat. */
  now?: number;
}

/** Finds what this Mac can do without, says what removing it would do, and removes only what the user chose. */
export function Cleaner({ backend = BACKEND, now = Date.now() }: CleanerProps) {
  const i18n = useI18n();
  const [step, setStep] = useState<Step>(SCANNING);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Set<CleanGroup>>(new Set());
  const [confirming, setConfirming] = useState(false);
  // What had the focus before the sheet, to give it back when the sheet goes.
  const returnFocus = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (confirming) return;
    returnFocus.current?.focus();
    returnFocus.current = null;
  }, [confirming]);

  const scan = useCallback(() => {
    setStep(SCANNING);
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
          setOpen(new Set());
          setStep({ kind: "found", token: event.token, home: event.home, findings: event.findings });
        }
      })
      .catch((error: unknown) => setStep({ kind: "refused", refusal: asRefusal(error) }));
  }, [backend]);

  // One scan as the window opens, however often it renders.
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    scan();
  }, [scan]);

  const run = (found: Found) => {
    const going = found.findings.filter((finding) => chosen.has(finding.path));
    setConfirming(false);
    setStep({ kind: "removing", home: found.home, chosen: going, removals: [] });
    backend
      .run(
        found.token,
        going.map((finding) => finding.path),
        (event) => {
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
  return (
    <div className={styles.window}>
      <header className={styles.bar} data-tauri-drag-region>
        <h1 className={styles.title}>{i18n.t("clean")}</h1>
        {working ? (
          <button type="button" className={styles.action} onClick={() => backend.stop().catch(console.error)}>
            {i18n.t("stop")}
          </button>
        ) : (
          <button type="button" className={styles.action} onClick={scan}>
            {i18n.t("cleanScanAgain")}
          </button>
        )}
      </header>
      <div className={styles.page} inert={confirming}>
        {page(step, { chosen, open, now, i18n, setChosen, setOpen })}
      </div>
      {step.kind === "found" && step.findings.length > 0 ? (
        <Footer
          plan={plan(step.findings, chosen)}
          muted={confirming}
          onClean={() => {
            returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
            setConfirming(true);
          }}
        />
      ) : null}
      {confirming && step.kind === "found" ? (
        <Confirm plan={plan(step.findings, chosen)} onCancel={() => setConfirming(false)} onConfirm={() => run(step)} />
      ) : null}
    </div>
  );
}

interface Choice {
  chosen: Set<string>;
  open: Set<CleanGroup>;
  now: number;
  i18n: Translator;
  setChosen: (next: Set<string>) => void;
  setOpen: (next: Set<CleanGroup>) => void;
}

function page(step: Step, choice: Choice) {
  const { i18n } = choice;
  switch (step.kind) {
    case "scanning":
      return (
        <Progress
          label={step.total === 0 ? i18n.t("cleanLooking") : i18n.t("cleanMeasured", step)}
          detail={step.total === 0 ? null : i18n.t("cleanFoundSoFar", { bytes: i18n.bytes(step.found) })}
          done={step.done}
          total={step.total}
        />
      );
    case "found":
      return step.findings.length === 0 ? (
        <p className={styles.empty}>{i18n.t("cleanNothing")}</p>
      ) : (
        <Found step={step} choice={choice} />
      );
    case "removing":
      return (
        <>
          <Progress
            label={i18n.t("removing", { done: step.removals.length, total: step.chosen.length })}
            detail={null}
            done={step.removals.length}
            total={step.chosen.length}
          />
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

function Progress({ label, detail, done, total }: { label: string; detail: string | null; done: number; total: number }) {
  return (
    <div className={styles.progress} role="status">
      <p className={styles.phase}>{label}</p>
      <div className={styles.track} data-waiting={total === 0 ? "" : undefined}>
        <div className={styles.fill} style={{ width: total === 0 ? undefined : `${(100 * done) / total}%` }} />
      </div>
      {detail === null ? null : <p className={styles.detail}>{detail}</p>}
    </div>
  );
}

function Found({ step, choice }: { step: Found; choice: Choice }) {
  const { chosen, open, i18n } = choice;
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
      <ul className={styles.groups}>
        {groups.map((group) => (
          <Group
            key={group.group}
            view={group}
            home={step.home}
            expanded={open.has(group.group)}
            choice={choice}
          />
        ))}
      </ul>
    </>
  );
}

function Group({ view, home, expanded, choice }: { view: GroupView; home: string; expanded: boolean; choice: Choice }) {
  const { chosen, open, now, i18n, setChosen, setOpen } = choice;
  const name = i18n.t(NAMES[view.group]);
  const list = `items-${view.group}`;
  const flip = () => {
    const next = new Set(open);
    if (expanded) next.delete(view.group);
    else next.add(view.group);
    setOpen(next);
  };
  return (
    <li className={styles.group} style={{ ["--tint" as string]: `var(--tint-${TINTS[view.group]})` }}>
      <div className={styles.groupRow}>
        <Check
          state={view.state}
          label={name}
          disabled={!view.selectable}
          onChange={() => setChosen(toggled(chosen, view.findings))}
        />
        <button type="button" className={styles.opener} aria-expanded={expanded} aria-controls={list} onClick={flip}>
          <span className={styles.groupText}>
            <span className={styles.groupName}>
              <span className={styles.dot} />
              {name}
            </span>
            <span className={styles.about}>{i18n.t(ABOUT[view.group])}</span>
          </span>
          <span className={styles.size}>
            {i18n.bytes(view.bytes)}
            <span className={styles.count}>{i18n.t("groupChosen", { chosen: view.chosen, count: view.findings.length })}</span>
          </span>
          <Chevron className={styles.chevron} />
        </button>
      </div>
      {expanded ? (
        <ul id={list} className={styles.items} aria-label={name}>
          {view.findings.map((finding) => (
            <li key={finding.path} className={styles.item} data-held={finding.inUse === null ? undefined : ""}>
              <Check
                state={chosen.has(finding.path) ? "all" : "none"}
                label={finding.name}
                disabled={!selectable(finding)}
                onChange={() => setChosen(toggled(chosen, [finding]))}
              />
              <span className={styles.itemText}>
                <span className={styles.itemName}>{finding.name}</span>
                <span className={styles.itemNote}>{note(finding, home, now, i18n)}</span>
              </span>
              <span className={styles.itemSize}>{i18n.bytes(finding.bytes)}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </li>
  );
}

function Check({
  state,
  label,
  disabled,
  onChange,
}: {
  state: GroupView["state"];
  label: string;
  disabled: boolean;
  onChange: () => void;
}) {
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (box.current) box.current.indeterminate = state === "some";
  }, [state]);
  return (
    <input
      ref={box}
      type="checkbox"
      className={styles.check}
      aria-label={label}
      checked={state === "all"}
      disabled={disabled}
      onChange={onChange}
    />
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

/** Where it is, then why it is or isn't chosen. */
function note(finding: Finding, home: string, now: number, i18n: Translator): string {
  if (finding.inUse !== null) return i18n.t("inUseBy", { who: finding.inUse });
  const where = shown(finding.project ?? finding.path, home);
  const why =
    finding.touched !== null
      ? i18n.t("lastTouched", { ago: i18n.ago(now - finding.touched * 1000) })
      : finding.kind === "personal"
        ? i18n.t("goesToTrash")
        : finding.kind === "discarded"
          ? i18n.t("emptiedForGood")
          : finding.picked
            ? null
            : i18n.t("usedLast");
  return why === null ? where : `${where} · ${why}`;
}

/** `~/code/shop` for a path in the home folder. */
function shown(path: string, home: string): string {
  return path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path;
}

function outcomeText(removal: Removal, i18n: Translator): string {
  switch (removal.outcome) {
    case "deleted":
      return i18n.t("outcomeDeleted");
    case "trashed":
      return i18n.t("outcomeTrashed");
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

function refusalText(refusal: CleanRefusal, i18n: Translator): string {
  switch (refusal.code) {
    case "busy":
      return i18n.t("cleanBusy");
    case "closed":
      return i18n.t("cleanClosed");
    case "expired":
      return i18n.t("cleanExpired");
    case "unlisted":
      return i18n.t("cleanUnlisted");
    case "failed":
      return i18n.t("reasonFailed", { message: refusal.message });
  }
}

/** A rejected call carries the backend's refusal; anything else is a failure. */
function asRefusal(error: unknown): CleanRefusal {
  if (typeof error === "object" && error !== null && "code" in error && typeof error.code === "string") {
    return error as CleanRefusal;
  }
  return { code: "failed", message: String(error) };
}
