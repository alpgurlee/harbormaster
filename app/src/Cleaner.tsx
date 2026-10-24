import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import {
  cleanApps,
  cleanDaemonPreview,
  cleanDaemonStop,
  cleanDaemons,
  cleanDisk,
  cleanHunt,
  cleanMap,
  cleanRecheck,
  cleanReveal,
  cleanRevealArea,
  cleanRun,
  cleanScan,
  cleanStop,
} from "./api";
import { MODULES, SEARCHES, groupsOf, plan, rechosen, stillChosen, type CleanModule, type Plan, type Search } from "./cleanup";
import { TINTS, asRefusal, outcomeText, refusalText } from "./cleanerText";
import { DaemonSheet, type DaemonStopper } from "./DaemonSheet";
import { useI18n, type TextKey, type Translator } from "./i18n";
import { Review } from "./Review";
import { bin, cleanerSections, mapsHome, platform } from "./platform";
import { keepFocusInside } from "./sheet";
import { SpaceMap } from "./SpaceMap";
import type { Area, CleanRefusal, Cleaned, Disk, Finding, MapEvent, Removal, RunEvent, ScanEvent } from "./types";
import styles from "./Cleaner.module.css";
import sheet from "./Sheet.module.css";

/** The backend's cleaner calls, which tests hand in themselves. */
export interface CleanerBackend extends DaemonStopper {
  scan: (onEvent: (event: ScanEvent) => void) => Promise<void>;
  hunt: (onEvent: (event: ScanEvent) => void) => Promise<void>;
  apps: (onEvent: (event: ScanEvent) => void) => Promise<void>;
  run: (token: number, paths: string[], onEvent: (event: RunEvent) => void) => Promise<void>;
  stop: () => Promise<void>;
  disk: () => Promise<Disk>;
  reveal: (path: string) => Promise<void>;
  recheck: (token: number) => Promise<Finding[]>;
  map: (others: boolean, onEvent: (event: MapEvent) => void) => Promise<void>;
  revealArea: (token: number, id: number) => Promise<void>;
  /** Of the items listed under `token`, those only known dev daemons keep in use. */
  daemons: (token: number) => Promise<string[]>;
}

const BACKEND: CleanerBackend = {
  scan: cleanScan,
  hunt: cleanHunt,
  apps: cleanApps,
  run: cleanRun,
  stop: cleanStop,
  disk: cleanDisk,
  reveal: cleanReveal,
  recheck: cleanRecheck,
  map: cleanMap,
  revealArea: cleanRevealArea,
  daemons: cleanDaemons,
  daemonPreview: cleanDaemonPreview,
  daemonStop: cleanDaemonStop,
};

/** Coming back to the window more often than this doesn't look at the list again. */
const RECHECK_EVERY_MS = 3_000;

/** What the app said can be stopped, about the list it was asked for. */
interface Told {
  listed: Finding[];
  paths: ReadonlySet<string>;
}

const NONE: ReadonlySet<string> = new Set();

const SECTION_NAMES = {
  developer: "moduleDeveloper",
  system: "moduleSystem",
  large: "moduleLarge",
  apps: "moduleApps",
} as const;

/** What each search says before it starts, its buttons, what it says while it looks, and once it has measured some. */
const HINTS: Record<Search, TextKey> = {
  scan: platform === "windows" ? "scanHintWindows" : "scanHint",
  hunt: platform === "windows" ? "huntHintWindows" : "huntHint",
  apps: "appsHint",
};
const STARTS: Record<Search, TextKey> = { scan: "scan", hunt: "scan", apps: "appsStart" };
const AGAINS: Record<Search, TextKey> = { scan: "cleanScanAgain", hunt: "cleanScanAgain", apps: "appsAgain" };
const LOOKING: Record<Search, TextKey> = { scan: "cleanLooking", hunt: "huntLooking", apps: "appsLooking" };
const MEASURED: Record<Search, TextKey> = { scan: "cleanMeasured", hunt: "huntMeasured", apps: "appsMeasured" };
/** What a section searched for on its own says when its search found nothing. */
const NOTHING: Partial<Record<CleanModule, TextKey>> = { large: "noLargeFiles", apps: "noApps" };

type View = "overview" | CleanModule | "map";

type Step =
  | { kind: "ready" }
  | { kind: "scanning"; search: Search; done: number; total: number; found: number }
  | {
      kind: "found";
      token: number;
      home: string;
      findings: Finding[];
      scanned: boolean;
      hunted: boolean;
      appsListed: boolean;
    }
  | { kind: "removing"; home: string; chosen: Finding[]; removals: Removal[] }
  | { kind: "cleaned"; home: string; chosen: Finding[]; cleaned: Cleaned; removals: Removal[]; next: Found }
  | {
      kind: "refused";
      refusal: CleanRefusal;
      partial?: { chosen: Finding[]; removals: Removal[] };
      /** What is left to go back to, when the app could say. */
      next?: Found;
    };

type Found = Extract<Step, { kind: "found" }>;

/** Where the space map stands: it is made apart from scans, though never at the same time as one. */
type Mapping =
  | { kind: "ready"; refusal?: CleanRefusal }
  | { kind: "mapping"; bytes: number; items: number; folder: string }
  | { kind: "mapped"; token: number; root: Area };

export interface CleanerProps {
  backend?: CleanerBackend;
  /** Milliseconds since the Unix epoch, for how long projects have sat. */
  now?: number;
  /** The sections past the overview this platform has. */
  sections?: CleanModule[];
  /** Whether the window maps the home folder. */
  maps?: boolean;
}

/** Finds what this Mac can do without, says what removing it would do, and removes only what the user chose. */
export function Cleaner({ backend = BACKEND, now = Date.now(), sections = cleanerSections, maps = mapsHome }: CleanerProps) {
  const i18n = useI18n();
  const [step, setStep] = useState<Step>({ kind: "ready" });
  const [view, setView] = useState<View>("overview");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  // How long a project had to sit for the last scan to choose its build outputs.
  const [idleDays, setIdleDays] = useState<number | null>(null);
  const [disk, setDisk] = useState<Disk | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [mapping, setMapping] = useState<Mapping>({ kind: "ready" });
  // Whether the map looks in other apps' data too, which macOS asks about.
  const [others, setOthers] = useState(false);
  // What had the focus before the sheet, to give it back when the sheet goes.
  const returnFocus = useRef<HTMLElement | null>(null);
  // Where a clean-up began, to go back to once it is done.
  const cleanedFrom = useRef<View>("overview");
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
  // The window hides rather than closes, so coming back to it measures the
  // disk again: room may have gone or come meanwhile.
  useEffect(() => {
    window.addEventListener("focus", measureDisk);
    return () => window.removeEventListener("focus", measureDisk);
  }, [measureDisk]);

  // A scan looks for what tools and apps leave; a hunt, for the user's large files; a listing, for their apps.
  const search = useCallback(
    (kind: Search) => {
      setStep({ kind: "scanning", search: kind, done: 0, total: 0, found: 0 });
      setView(kind === "hunt" ? "large" : kind === "apps" ? "apps" : "overview");
      setConfirming(false);
      const look = { scan: backend.scan, hunt: backend.hunt, apps: backend.apps }[kind];
      look((event) => {
        if (event.kind === "measured") {
          setStep((current) =>
            current.kind === "scanning"
              ? {
                  ...current,
                  done: Math.max(current.done, event.done),
                  total: event.total,
                  found: current.found + event.bytes,
                }
              : current,
          );
        } else {
          const { kind: _, idleDays: days, ...found } = event;
          setChosen((current) => rechosen(current, found.findings, kind));
          if (days !== null) setIdleDays(days);
          setStep({ kind: "found", ...found });
        }
      }).catch((error: unknown) => setStep({ kind: "refused", refusal: asRefusal(error) }));
    },
    [backend],
  );

  // Coming back to the window, the list is looked at again: what was removed
  // meanwhile drops out, and what is in use is read anew.
  const token = step.kind === "found" ? step.token : null;
  // Looks at the list again at once, as after stopping what kept an item in use.
  const lookAgain = useRef(() => {});
  useEffect(() => {
    if (token === null) return;
    let live = true;
    let asking = false;
    let again = false;
    let last = 0;
    let later: ReturnType<typeof setTimeout> | undefined;
    const recheck = () => {
      // Coming back while it looks, or too soon after, looks once more later.
      if (asking) {
        again = true;
        return;
      }
      clearTimeout(later);
      const wait = last + RECHECK_EVERY_MS - Date.now();
      if (wait > 0) {
        later = setTimeout(recheck, wait);
        return;
      }
      asking = true;
      last = Date.now();
      backend
        .recheck(token)
        .then((findings) => {
          if (!live) return;
          setStep((current) => (current.kind === "found" ? { ...current, findings } : current));
          setChosen((current) => stillChosen(current, findings));
        })
        // The list stays as it was.
        .catch(console.error)
        .finally(() => {
          asking = false;
          if (again && live) {
            again = false;
            recheck();
          }
        });
    };
    lookAgain.current = recheck;
    window.addEventListener("focus", recheck);
    return () => {
      live = false;
      clearTimeout(later);
      window.removeEventListener("focus", recheck);
    };
  }, [backend, token]);

  // Which items in use only known dev daemons keep so, which can be stopped from
  // here. Each answer is about the list it was asked for; none yet offers no stop.
  const [told, setTold] = useState<Told | null>(null);
  const [stopping, setStopping] = useState<Finding | null>(null);
  const listedNow = step.kind === "found" ? step.findings : null;
  useEffect(() => {
    if (token === null || listedNow === null || !listedNow.some((finding) => finding.inUse !== null)) return;
    let live = true;
    backend
      .daemons(token)
      .then((paths) => {
        if (live) setTold({ listed: listedNow, paths: new Set(paths) });
      })
      .catch(console.error);
    return () => {
      live = false;
    };
  }, [backend, token, listedNow]);
  const stoppable = told !== null && told.listed === listedNow ? told.paths : NONE;

  const chart = (withOthers: boolean) => {
    setMapping({ kind: "mapping", bytes: 0, items: 0, folder: "" });
    backend
      .map(withOthers, (event) => {
        if (event.kind === "charting") {
          const { kind: _, ...charting } = event;
          setMapping((current) => (current.kind === "mapping" ? { kind: "mapping", ...charting } : current));
        } else {
          setMapping({ kind: "mapped", token: event.token, root: event.area });
        }
      })
      .catch((error: unknown) => setMapping({ kind: "ready", refusal: asRefusal(error) }));
  };

  const run = (found: Found) => {
    const going = found.findings.filter((finding) => chosen.has(finding.path));
    setConfirming(false);
    // Looking again may have left nothing chosen; the scan stays for later.
    if (going.length === 0) return;
    cleanedFrom.current = view;
    setView("overview");
    setStep({ kind: "removing", home: found.home, chosen: going, removals: [] });
    backend
      .run(
        found.token,
        going.map((finding) => finding.path),
        (event) => {
          if (event.kind !== "removed") measureDisk();
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
            if (event.kind === "failed") {
              const { kind: _, refusal, ...next } = event;
              return {
                kind: "refused",
                refusal,
                partial: { chosen: current.chosen, removals: current.removals },
                next: { kind: "found", home: current.home, ...next },
              };
            }
            const { removals, freedBytes, kind: _, ...next } = event;
            return {
              kind: "cleaned",
              home: current.home,
              chosen: current.chosen,
              cleaned: { removals, freedBytes },
              removals: current.removals,
              next: { kind: "found", home: current.home, ...next },
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
  // A map takes the same turn as a scan or a removal, so neither starts while the other runs.
  const charting = mapping.kind === "mapping";
  const found = step.kind === "found" ? step : null;
  // What a clean-up left, to go back to.
  const next = step.kind === "cleaned" || step.kind === "refused" ? (step.next ?? null) : null;
  const back = () => {
    if (next === null) return;
    setChosen(new Set());
    setStep(next);
    setView(cleanedFrom.current);
  };
  const sectionBytes = (section: CleanModule) =>
    list?.findings.filter((finding) => MODULES[section].includes(finding.group)).reduce((total, each) => total + each.bytes, 0) ?? 0;
  const title = i18n.t(view === "overview" ? "overview" : view === "map" ? "moduleMap" : SECTION_NAMES[view]);
  // A section shows what its search found: the scan's, the hunt's for large files, or the listed apps.
  const list = found ?? next;
  const listed = (section: CleanModule) => list !== null && searched(list, SEARCHES[section]);
  // Each is started from where it shows, whether or not another came first.
  const first: Search | null =
    view === "overview"
      ? found !== null && !found.scanned
        ? "scan"
        : null
      : view !== "map" && SEARCHES[view] !== "scan" && (step.kind === "ready" || (found !== null && !searched(found, SEARCHES[view])))
        ? SEARCHES[view]
        : null;
  // A section searched for on its own says so when its search found nothing.
  const nothing =
    view !== "overview" && view !== "map" && found !== null && !found.findings.some((finding) => MODULES[view].includes(finding.group))
      ? NOTHING[view]
      : undefined;

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
        {sections.map((section) => (
          <button
            key={section}
            type="button"
            className={styles.section}
            aria-current={view === section ? "page" : undefined}
            disabled={SEARCHES[section] === "scan" && !listed(section)}
            onClick={() => {
              back();
              setView(section);
            }}
          >
            {i18n.t(SECTION_NAMES[section])}
            {listed(section) ? <span className={styles.sectionSize}>{i18n.bytes(sectionBytes(section))}</span> : null}
          </button>
        ))}
        {maps ? (
          <button
            type="button"
            className={styles.section}
            aria-current={view === "map" ? "page" : undefined}
            onClick={() => setView("map")}
          >
            {i18n.t("moduleMap")}
            {mapping.kind === "mapped" ? <span className={styles.sectionSize}>{i18n.bytes(mapping.root.bytes)}</span> : null}
          </button>
        ) : null}
        {disk === null ? null : (
          <p className={styles.diskNote}>
            {i18n.t("diskFree", { free: i18n.bytes(disk.free), total: i18n.bytes(disk.total) })}
          </p>
        )}
      </nav>
      <div className={styles.main} inert={confirming}>
        <header className={styles.bar} data-tauri-drag-region>
          <h1 className={styles.title}>{title}</h1>
          {view === "map" ? (
            charting ? (
              <button type="button" className={styles.action} onClick={() => backend.stop().catch(console.error)}>
                {i18n.t("stop")}
              </button>
            ) : mapping.kind === "mapped" ? (
              <button type="button" className={styles.action} disabled={working} onClick={() => chart(others)}>
                {i18n.t("mapAgain")}
              </button>
            ) : null
          ) : working ? (
            <button type="button" className={styles.action} onClick={() => backend.stop().catch(console.error)}>
              {i18n.t("stop")}
            </button>
          ) : step.kind === "ready" || first !== null ? null : (
            <button
              type="button"
              className={styles.action}
              disabled={charting}
              onClick={() => search(view === "overview" ? "scan" : SEARCHES[view])}
            >
              {i18n.t(AGAINS[view === "overview" ? "scan" : SEARCHES[view]])}
            </button>
          )}
        </header>
        <div className={styles.page}>
          {view === "map" ? (
            mapPage(mapping, {
              disk,
              others,
              working,
              i18n,
              onOthers: setOthers,
              onMap: () => chart(others),
              onMapOthers: () => {
                setOthers(true);
                chart(true);
              },
              onReveal: (token, id) => backend.revealArea(token, id),
            })
          ) : first !== null ? (
            <Start
              disk={disk}
              hint={i18n.t(charting ? "waitForMap" : HINTS[first])}
              start={i18n.t(STARTS[first])}
              disabled={charting}
              onStart={() => search(first)}
            />
          ) : nothing !== undefined ? (
            <p className={styles.empty}>{i18n.t(nothing)}</p>
          ) : view !== "overview" && found !== null ? (
            <Review
              key={view}
              findings={found.findings.filter((finding) => MODULES[view].includes(finding.group))}
              home={found.home}
              chosen={chosen}
              idleDays={idleDays}
              now={now}
              stoppable={stoppable}
              onChoose={setChosen}
              onReveal={(finding) => backend.reveal(finding.path).catch(console.error)}
              onStopDaemons={setStopping}
            />
          ) : (
            overview(step, { disk, chosen, charting, sections, i18n, onScan: () => search("scan"), onReview: setView, onBack: back })
          )}
        </div>
        {view !== "map" && found !== null && found.findings.length > 0 ? (
          <Footer
            plan={plan(found.findings, chosen)}
            muted={confirming}
            disabled={charting}
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
      {stopping !== null && token !== null ? (
        <DaemonSheet
          token={token}
          finding={stopping}
          stopper={backend}
          onClose={() => setStopping(null)}
          onDone={() => {
            setStopping(null);
            lookAgain.current();
          }}
        />
      ) : null}
    </div>
  );
}

/** Whether `list` shows what `search` found, so that none shown means none found. */
function searched(list: { scanned: boolean; hunted: boolean; appsListed: boolean }, search: Search): boolean {
  return { scan: list.scanned, hunt: list.hunted, apps: list.appsListed }[search];
}

interface Overview {
  disk: Disk | null;
  chosen: Set<string>;
  /** Whether a map is being made, which no scan may start beside. */
  charting: boolean;
  sections: CleanModule[];
  i18n: Translator;
  onScan: () => void;
  onReview: (section: CleanModule) => void;
  onBack: () => void;
}

function overview(step: Step, { disk, chosen, charting, sections, i18n, onScan, onReview, onBack }: Overview) {
  switch (step.kind) {
    case "ready":
      return (
        <Start
          disk={disk}
          hint={i18n.t(charting ? "waitForMap" : HINTS.scan)}
          start={i18n.t("scan")}
          disabled={charting}
          onStart={onScan}
        />
      );
    case "scanning":
      return (
        <div className={styles.start} role="status">
          <Ring fraction={step.total === 0 ? 0 : step.done / step.total} waiting={step.total === 0}>
            <strong>{i18n.bytes(step.found)}</strong>
          </Ring>
          <p className={styles.phase}>
            {step.total === 0
              ? i18n.t(LOOKING[step.search])
              : i18n.t(MEASURED[step.search], { done: step.done, total: step.total })}
          </p>
        </div>
      );
    case "found":
      return step.findings.length === 0 ? (
        <p className={styles.empty}>{i18n.t("cleanNothing")}</p>
      ) : (
        <Summary step={step} chosen={chosen} sections={sections} i18n={i18n} onReview={onReview} />
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
      return <Done step={step} i18n={i18n} onBack={onBack} />;
    case "refused":
      return (
        <>
          <p className={styles.empty}>{refusalText(step.refusal, i18n)}</p>
          {step.next === undefined ? null : (
            <button type="button" className={styles.cardButton} onClick={onBack}>
              {i18n.t("backToList")}
            </button>
          )}
          {step.partial === undefined ? null : (
            <Outcomes removals={step.partial.removals} chosen={step.partial.chosen} i18n={i18n} />
          )}
        </>
      );
  }
}

interface MapPage {
  disk: Disk | null;
  others: boolean;
  /** Whether a scan or a removal runs, which no map may start beside. */
  working: boolean;
  i18n: Translator;
  onOthers: (others: boolean) => void;
  onMap: () => void;
  /** Maps again with other apps' data too. */
  onMapOthers: () => void;
  onReveal: (token: number, id: number) => Promise<void>;
}

function mapPage(mapping: Mapping, { disk, others, working, i18n, onOthers, onMap, onMapOthers, onReveal }: MapPage) {
  switch (mapping.kind) {
    case "ready":
      return (
        <>
          {mapping.refusal === undefined ? null : <p className={styles.refused}>{refusalText(mapping.refusal, i18n)}</p>}
          <Start
            disk={disk}
            hint={i18n.t(working ? "waitForCleaning" : "mapHint")}
            start={i18n.t("mapStart")}
            disabled={working}
            onStart={onMap}
          >
            <label className={styles.others}>
              <input type="checkbox" checked={others} onChange={(event) => onOthers(event.target.checked)} />
              {i18n.t("mapOthers")}
            </label>
            {others ? <p className={styles.hint}>{i18n.t("mapOthersHint")}</p> : null}
          </Start>
        </>
      );
    case "mapping":
      return (
        <div className={styles.start} role="status">
          <Ring fraction={0} waiting>
            <strong>{i18n.bytes(mapping.bytes)}</strong>
          </Ring>
          <p className={styles.phase}>{i18n.t("mapLooking")}</p>
          {mapping.items === 0 ? null : (
            <p className={styles.hint}>{i18n.plural("mapCounted", mapping.items, { items: i18n.number(mapping.items) })}</p>
          )}
          <p className={styles.where} aria-hidden="true">
            {mapping.folder}
          </p>
        </div>
      );
    case "mapped":
      return (
        <SpaceMap
          key={mapping.token}
          root={mapping.root}
          onReveal={(id) => onReveal(mapping.token, id)}
          onMapOthers={others || working ? undefined : onMapOthers}
        />
      );
  }
}

interface StartProps {
  disk: Disk | null;
  hint: string;
  /** What the button that starts says. */
  start: string;
  disabled?: boolean;
  onStart: () => void;
  /** What may be asked before starting. */
  children?: ReactNode;
}

/** How full the disk is, and a button to look into it. */
function Start({ disk, hint, start, disabled = false, onStart, children }: StartProps) {
  const i18n = useI18n();
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
      <button type="button" className={styles.scan} disabled={disabled} onClick={onStart}>
        {start}
      </button>
      {children}
      <p className={styles.hint}>{hint}</p>
    </div>
  );
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
  sections,
  i18n,
  onReview,
}: {
  step: Found;
  chosen: Set<string>;
  sections: CleanModule[];
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
        {sections.map((section) => {
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

function Footer({ plan, muted, disabled, onClean }: { plan: Plan; muted: boolean; disabled: boolean; onClean: () => void }) {
  const i18n = useI18n();
  return (
    <footer className={styles.foot} inert={muted}>
      <span className={styles.total}>
        <strong>{i18n.plural("items", plan.count)}</strong> · {i18n.bytes(plan.bytes)}
      </span>
      <button type="button" className={styles.primary} disabled={plan.count === 0 || disabled} onClick={onClean}>
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
          {plan.trashed.count > 0 ? <li>{i18n.plural(bin.confirm, plan.trashed.count)}</li> : null}
          {plan.emptied ? <li>{i18n.t("confirmEmptied")}</li> : null}
        </ul>
        <p className={sheet.note}>{i18n.t("confirmChecked")}</p>
        <div className={sheet.buttons}>
          <button type="button" className={sheet.quiet} onClick={onCancel}>
            {i18n.t("cancel")}
          </button>
          <button type="button" data-main className={sheet.danger} disabled={plan.count === 0} onClick={onConfirm}>
            {i18n.t("clean")}
          </button>
        </div>
      </div>
    </div>
  );
}

function Done({
  step,
  i18n,
  onBack,
}: {
  step: Extract<Step, { kind: "cleaned" }>;
  i18n: Translator;
  onBack: () => void;
}) {
  const removals = step.cleaned.removals.length > 0 ? step.cleaned.removals : step.removals;
  const cleaned = removals.filter(({ outcome }) => outcome === "deleted" || outcome === "trashed").length;
  const left = removals.length - cleaned;
  // What went to the Trash is still on the disk; its room comes once the Trash is emptied.
  const trashed = removals
    .filter(({ outcome }) => outcome === "trashed")
    .reduce((total, { path }) => total + (step.chosen.find((finding) => finding.path === path)?.bytes ?? 0), 0);
  const freed = i18n.bytes(step.cleaned.freedBytes);
  const caption = [
    i18n.plural("cleanedItems", cleaned),
    left > 0 ? i18n.plural("leftItems", left) : null,
    trashed > 0 ? i18n.t("freedSoFar", { bytes: freed }) : null,
  ];
  return (
    <>
      <section className={styles.hero} data-testid="hero">
        <p className={styles.figure}>
          {trashed > 0 ? i18n.t(bin.moved, { bytes: i18n.bytes(trashed) }) : i18n.t("freed", { bytes: freed })}
        </p>
        <p className={styles.caption}>{caption.filter((part) => part !== null).join(" · ")}</p>
      </section>
      {trashed > 0 ? <p className={styles.hint}>{i18n.t(bin.hint)}</p> : null}
      <button type="button" className={styles.cardButton} onClick={onBack}>
        {i18n.t("backToList")}
      </button>
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
