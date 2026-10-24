import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { useI18n, type Translator } from "./i18n";
import { keepFocusInside } from "./sheet";
import type { Program, ProgramList, UninstallRefusal } from "./types";
import styles from "./Programs.module.css";
import list from "./Review.module.css";
import confirm from "./Cleaner.module.css";
import sheet from "./Sheet.module.css";

/** The backend's calls for the programs installed on Windows, which tests hand in themselves. */
export interface Uninstaller {
  programs: () => Promise<ProgramList>;
  uninstall: (token: number, id: number) => Promise<void>;
}

type Order = "name" | "size";

const ORDERS: { order: Order; label: "orderName" | "orderSize" }[] = [
  { order: "name", label: "orderName" },
  { order: "size", label: "orderSize" },
];

export interface ProgramsProps {
  programs: Program[];
  onUninstall: (program: Program) => void;
}

/** The programs installed on this PC, each removed only by its own uninstaller. */
export function Programs({ programs, onUninstall }: ProgramsProps) {
  const i18n = useI18n();
  const [query, setQuery] = useState("");
  const [order, setOrder] = useState<Order>("name");
  const shown = ordered(matching(programs, query), order, i18n.locale);
  return (
    <section className={`${list.detail} ${styles.programs}`} aria-label={i18n.t("moduleApps")}>
      <div className={list.intro}>
        <p className={list.about}>{i18n.t("aboutPrograms")}</p>
      </div>
      <div className={list.toolbar}>
        <input
          type="search"
          className={list.search}
          placeholder={i18n.t("searchItems")}
          aria-label={i18n.t("searchItems")}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <select
          className={list.order}
          aria-label={i18n.t("orderBy")}
          value={order}
          onChange={(event) => setOrder(event.target.value as Order)}
        >
          {ORDERS.map(({ order: each, label }) => (
            <option key={each} value={each}>
              {i18n.t(label)}
            </option>
          ))}
        </select>
      </div>
      {shown.length === 0 ? (
        <p className={list.empty}>{i18n.t("noItemsMatch", { query: query.trim() })}</p>
      ) : (
        <ul className={list.items} aria-label={i18n.t("moduleApps")}>
          {shown.map((program) => (
            <li key={program.id} className={list.item}>
              <span className={list.itemText}>
                <span className={list.itemName}>{program.name}</span>
                <span className={list.itemNote}>{note(program, i18n)}</span>
              </span>
              {program.estimated === null ? null : (
                <span className={list.itemSize}>{i18n.t("programsSize", { bytes: i18n.bytes(program.estimated) })}</span>
              )}
              <button
                type="button"
                className={styles.uninstall}
                aria-label={i18n.t("programsUninstallNamed", { name: program.name })}
                onClick={() => onUninstall(program)}
              >
                {i18n.t("programsUninstall")}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/** Who made it, which version it is, and whether it is everyone's on this PC. */
function note(program: Program, i18n: Translator): string {
  return [program.publisher, program.version, program.everyone ? i18n.t("programsEveryone") : null]
    .filter((part) => part !== null)
    .join(" · ");
}

/** The programs whose name or publisher holds `query`, whatever the case. */
function matching(programs: Program[], query: string): Program[] {
  const wanted = query.trim().toLocaleLowerCase();
  if (wanted === "") return programs;
  return programs.filter(
    (program) =>
      program.name.toLocaleLowerCase().includes(wanted) || (program.publisher?.toLocaleLowerCase().includes(wanted) ?? false),
  );
}

/** By name, or the largest first, those whose size isn't known last. */
function ordered(programs: Program[], order: Order, locale: string): Program[] {
  const byName = (a: Program, b: Program) => a.name.localeCompare(b.name, locale);
  const copy = [...programs];
  switch (order) {
    case "name":
      return copy.sort(byName);
    case "size":
      return copy.sort((a, b) => (b.estimated ?? -1) - (a.estimated ?? -1) || byName(a, b));
  }
}

type Step = { kind: "asking" } | { kind: "starting" } | { kind: "started" } | { kind: "refused"; refusal: UninstallRefusal };

export interface UninstallSheetProps {
  /** The listing the program is shown in. */
  token: number;
  program: Program;
  uninstaller: Uninstaller;
  onClose: () => void;
  /** Closes the sheet and lists the programs again. */
  onListAgain: () => void;
}

/** Asks before starting a program's own uninstaller, saying what runs and what Windows may ask. */
export function UninstallSheet({ token, program, uninstaller, onClose, onListAgain }: UninstallSheetProps) {
  const i18n = useI18n();
  // What Windows can't be sure of is never started, so it isn't offered.
  const [step, setStep] = useState<Step>(
    program.uninstaller === null ? { kind: "refused", refusal: { code: "unsure" } } : { kind: "asking" },
  );
  const dialog = useRef<HTMLDivElement>(null);

  // The main button takes the focus at each step, or the sheet itself while none is usable.
  useEffect(() => {
    const own = dialog.current;
    (own?.querySelector<HTMLButtonElement>("button[data-main]:not(:disabled)") ?? own)?.focus();
  }, [step.kind]);

  const start = () => {
    setStep({ kind: "starting" });
    uninstaller
      .uninstall(token, program.id)
      .then(() => setStep({ kind: "started" }))
      .catch((error: unknown) => setStep({ kind: "refused", refusal: asUninstallRefusal(error) }));
  };

  // Windows may be asking for an administrator meanwhile, which the sheet waits for.
  const busy = step.kind === "starting";
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (!busy) onClose();
    } else if (event.key === "Tab") {
      keepFocusInside(event, dialog.current);
    }
  };
  const made = [program.publisher, program.version].filter((part) => part !== null).join(" · ");

  return (
    <div className={`${sheet.scrim} ${confirm.scrim}`}>
      <div
        ref={dialog}
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-labelledby="uninstall-title"
        className={`${sheet.sheet} ${confirm.sheet}`}
        onKeyDown={onKeyDown}
      >
        <h2 id="uninstall-title" className={sheet.title}>
          {i18n.t(step.kind === "started" ? "uninstallStartedTitle" : "uninstallTitle", { name: program.name })}
        </h2>
        {made === "" ? null : <p className={styles.made}>{made}</p>}
        {step.kind === "asking" || step.kind === "starting" ? (
          <>
            <ul className={confirm.consequences}>
              <li>
                {i18n.t("uninstallRuns")}
                <span className={styles.program}>{program.uninstaller}</span>
              </li>
              <li>{i18n.t(program.everyone ? "uninstallEveryone" : "uninstallYours")}</li>
            </ul>
            <p className={sheet.note}>{i18n.t("uninstallFollow")}</p>
          </>
        ) : (
          <p className={sheet.note} role="status">
            {step.kind === "started" ? i18n.t("uninstallStarted") : why(step.refusal, program, i18n)}
          </p>
        )}
        <div className={sheet.buttons}>
          {step.kind === "asking" || step.kind === "starting" ? (
            <>
              <button type="button" className={sheet.quiet} disabled={busy} onClick={onClose}>
                {i18n.t("cancel")}
              </button>
              <button type="button" data-main className={sheet.danger} disabled={busy} onClick={start}>
                {busy ? i18n.t("uninstallStarting") : i18n.t("uninstallStart")}
              </button>
            </>
          ) : step.kind === "refused" && stale(step.refusal) ? (
            <>
              <button type="button" className={sheet.quiet} onClick={onClose}>
                {i18n.t("close")}
              </button>
              <button type="button" data-main className={sheet.quiet} onClick={onListAgain}>
                {i18n.t("appsAgain")}
              </button>
            </>
          ) : (
            <button type="button" data-main className={sheet.quiet} onClick={onClose}>
              {i18n.t("close")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/** Whether the list was out of date, which listing again puts right. */
function stale(refusal: UninstallRefusal): boolean {
  return refusal.code === "expired" || refusal.code === "unlisted" || refusal.code === "gone" || refusal.code === "changed";
}

function why(refusal: UninstallRefusal, program: Program, i18n: Translator): string {
  switch (refusal.code) {
    case "expired":
    case "unlisted":
      return i18n.t("uninstallExpired");
    case "gone":
      return i18n.t("uninstallGone", { name: program.name });
    case "changed":
      return i18n.t("uninstallChanged", { name: program.name });
    case "unsure":
      return i18n.t("uninstallUnsure", { name: program.name });
    case "wantsAdministrator":
      return i18n.t("uninstallWantsAdministrator");
    case "unprotected":
      return i18n.t("uninstallUnprotected");
    case "declined":
      return i18n.t("uninstallDeclined");
    case "failed":
      return i18n.t("uninstallFailed", { message: refusal.message });
  }
}

/** A rejected call carries the backend's refusal; anything else is a failure. */
function asUninstallRefusal(error: unknown): UninstallRefusal {
  if (typeof error === "object" && error !== null && "code" in error && typeof error.code === "string") {
    return error as UninstallRefusal;
  }
  return { code: "failed", message: String(error) };
}
