import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { urlOf } from "./address";
import { dragsAfterMoving } from "./drag";
import { useI18n } from "./i18n";
import { Chevron, Gear, Mark, OpenOut, PutBack, Search, Square, Sweep } from "./icons";
import { groupByProject, projectOf, totals } from "./model";
import { Plate } from "./Plate";
import { platform } from "./platform";
import { agentOf, displayName } from "./session";
import { trendOf } from "./store";
import { Trend } from "./Trend";
import type { AppUsage, Memory, Service, Thermal, Update } from "./types";
import styles from "./Panel.module.css";

/** A scan younger than this reads as now. */
const FRESH_MS = 10_000;

export interface PanelProps {
  /** Each project's color, by its folder. */
  tints: Map<string, number>;
  /** Kept, with its search and selection, while a server's page shows. */
  hidden?: boolean;
  update: Update | null;
  /** Milliseconds since the Unix epoch. */
  now: number;
  onShow: (service: Service) => void;
  onOpen: (service: Service) => void;
  /** Asks to stop a server; without it, no row offers to. */
  onStop?: (service: Service) => void;
  /** Asks to quit an app; without it, the breakdown only shows. */
  onQuit?: (app: AppUsage) => void;
  /** Opens the cleaner; its button stays off without it. */
  onClean?: () => void;
  /** Whether this platform has a cleaner at all; without one, no button shows. */
  cleans?: boolean;
  onSettings?: () => void;
  /** Drags the panel off its icon, from its header. */
  onDrag?: () => void;
  /** Whether the user put the panel elsewhere, which `onAnchor` undoes. */
  placed?: boolean;
  onAnchor?: () => void;
}

export function Panel({
  hidden = false,
  tints,
  update,
  now,
  onShow,
  onOpen,
  onStop,
  onQuit,
  onClean,
  cleans = true,
  onSettings,
  onDrag,
  placed = false,
  onAnchor,
}: PanelProps) {
  const header = <Header onClean={onClean} cleans={cleans} onSettings={onSettings} onDrag={onDrag} onAnchor={placed ? onAnchor : undefined} />;
  const i18n = useI18n();
  const [query, setQuery] = useState("");
  const [otherOpen, setOtherOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const search = useRef<HTMLInputElement>(null);
  const loading = update === null;
  // Typing goes to the search whenever the list shows.
  useEffect(() => {
    if (!hidden) search.current?.focus();
  }, [hidden, loading]);

  if (update === null) {
    return (
      <div className={styles.panel} hidden={hidden}>
        {header}
        <p className={styles.note}>{i18n.t("looking")}</p>
      </div>
    );
  }

  const dev = update.services.filter((service) => service.classification === "dev");
  const other = update.services.filter((service) => service.classification === "other");
  const searching = query.trim() !== "";
  const groups = groupByProject(dev.filter((service) => matches(service, query)));
  const others = other.filter((service) => matches(service, query));
  // A port held by something that isn't a dev server is still worth finding.
  const othersShown = otherOpen || searching;
  const order = [...groups.flatMap((group) => group.services), ...(othersShown ? others : [])];
  const selected = order.find((service) => service.id === selectedId) ?? null;

  const onKeyDown = (event: KeyboardEvent) => {
    // An input method's Enter picks a word, and WebKit may send it as 229
    // after the composition ends; a modifier means something else.
    const native = event.nativeEvent;
    if (native.isComposing || native.keyCode === 229 || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const at = selected ? order.indexOf(selected) : -1;
      const next = order[Math.min(order.length - 1, Math.max(0, at + (event.key === "ArrowDown" ? 1 : -1)))];
      if (next) setSelectedId(next.id);
    } else if (event.key === "Enter" && selected) {
      event.preventDefault();
      onShow(selected);
    } else if (event.key === "Escape" && searching) {
      event.preventDefault();
      event.stopPropagation();
      setQuery("");
    }
  };

  const row = (service: Service) => (
    <Row
      key={service.id}
      service={service}
      tint={tints.get(projectOf(service)) ?? 0}
      selected={service === selected}
      now={now}
      onShow={() => {
        setSelectedId(service.id);
        onShow(service);
      }}
      onOpen={() => onOpen(service)}
      onStop={onStop && (() => onStop(service))}
    />
  );

  let empty: string | null = null;
  if (order.length === 0) {
    empty = searching ? i18n.t("nothingMatches", { query: query.trim() }) : i18n.t("nothingRuns");
  }

  return (
    <div className={styles.panel} hidden={hidden}>
      {header}
      <Summary dev={dev} tints={tints} memory={update.memory} apps={update.apps} thermal={update.thermal} onQuit={onQuit} />
      <label className={styles.search}>
        <Search className={styles.searchIcon} />
        <input
          ref={search}
          type="search"
          role="combobox"
          value={query}
          placeholder={i18n.t("search")}
          aria-label={i18n.t("search")}
          aria-expanded={true}
          aria-autocomplete="list"
          aria-controls="servers"
          aria-activedescendant={selected ? `server-${selected.id}` : undefined}
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={onKeyDown}
        />
      </label>
      <div id="servers" role="listbox" aria-label={i18n.plural("servers", order.length)} className={styles.list}>
        {groups.map((group) => (
          <div key={group.id} role="group" aria-labelledby={`group-${encodeURIComponent(group.id)}`}>
            <h2 id={`group-${encodeURIComponent(group.id)}`} className={styles.group}>
              {group.name ?? i18n.t("noProject")}
              {group.framework ? <span className={styles.chip}>{group.framework}</span> : null}
            </h2>
            {group.services.map(row)}
          </div>
        ))}
        {othersShown && others.length > 0 ? (
          <div role="group" aria-label={i18n.t("otherListeners", { count: others.length })}>
            {others.map(row)}
          </div>
        ) : null}
      </div>
      {empty !== null ? <p className={styles.note}>{empty}</p> : null}
      <footer className={styles.footer}>
        {other.length > 0 ? (
          <button
            type="button"
            className={styles.more}
            aria-expanded={othersShown}
            onClick={() => setOtherOpen(!otherOpen)}
          >
            {i18n.t("otherListeners", { count: other.length })}
            <Chevron className={othersShown ? styles.open : undefined} />
          </button>
        ) : (
          <span />
        )}
        <span>
          {now - update.scannedAt < FRESH_MS
            ? i18n.t("updatedJustNow")
            : i18n.t("updatedAt", { time: i18n.time(update.scannedAt) })}
        </span>
      </footer>
    </div>
  );
}

interface HeaderProps {
  onClean?: () => void;
  cleans: boolean;
  onSettings?: () => void;
  onDrag?: () => void;
  /** Shown only while the panel sits where the user put it. */
  onAnchor?: () => void;
}

function Header({ onClean, cleans, onSettings, onDrag, onAnchor }: HeaderProps) {
  const i18n = useI18n();
  return (
    <header className={styles.header} onMouseDown={onDrag && dragsAfterMoving(onDrag)}>
      <Mark className={styles.mark} />
      Harbormaster
      <span className={styles.tools}>
        {onAnchor ? (
          <button type="button" className={styles.tool} aria-label={i18n.t("putBack")} title={i18n.t("putBack")} onClick={onAnchor}>
            <PutBack />
          </button>
        ) : null}
        {cleans ? (
          <button type="button" className={styles.tool} disabled={onClean === undefined} onClick={onClean}>
            <Sweep />
            {i18n.t("clean")}
          </button>
        ) : null}
        <button
          type="button"
          className={styles.tool}
          aria-label={i18n.t("settings")}
          title={i18n.t("settings")}
          disabled={onSettings === undefined}
          onClick={onSettings}
        >
          <Gear />
        </button>
      </span>
    </header>
  );
}

interface SummaryProps {
  dev: Service[];
  tints: Map<string, number>;
  memory: Memory | null;
  apps: AppUsage[] | null;
  thermal: Thermal;
  onQuit?: (app: AppUsage) => void;
}

function Summary({ dev, tints, memory, apps, thermal, onQuit }: SummaryProps) {
  const i18n = useI18n();
  const [open, setOpen] = useState(false);
  const sum = totals(dev);
  const servers = sum.memoryBytes ?? 0;
  const projects = groupByProject(dev).map((group) => ({
    id: group.id,
    tint: tints.get(group.id) ?? 0,
    bytes: totals(group.services).memoryBytes ?? 0,
  }));
  // Footprints can pass the memory in use; the meter can't show more than that.
  const fit = memory && servers > memory.used ? memory.used / servers : 1;
  const strong = (value: string): ReactNode => <strong>{value}</strong>;
  return (
    <>
      {memory ? (
        <div
          role="meter"
          aria-label={i18n.t("memoryInUse")}
          aria-valuemin={0}
          aria-valuemax={memory.total}
          aria-valuenow={memory.used}
          aria-valuetext={i18n.memoryOf(memory.used, memory.total)}
          className={styles.meter}
        >
          <Segments projects={projects} scale={fit} whole={memory.total} />
          <span style={{ width: share(memory.used - servers * fit, memory.total) }} />
        </div>
      ) : null}
      <div className={styles.stats}>
        <span>
          {i18n.marked("servers", { count: sum.count }, strong)}
          {sum.memoryBytes !== null ? <> · {strong(i18n.bytes(sum.memoryBytes))}</> : null}
          {sum.cpuPercent !== null ? (
            <> · {i18n.marked("cpu", { percent: i18n.percent(sum.cpuPercent) }, strong)}</>
          ) : null}
        </span>
        {memory ? (
          <button
            type="button"
            className={styles.more}
            aria-expanded={open}
            aria-controls={open ? "memory-by-app" : undefined}
            onClick={() => setOpen(!open)}
          >
            {i18n.memoryOf(memory.used, memory.total)}
            <Chevron className={open ? styles.open : undefined} />
          </button>
        ) : null}
      </div>
      {thermal === "serious" || thermal === "critical" ? <Heat dev={dev} /> : null}
      {memory && open ? (
        <Breakdown servers={servers} projects={projects} apps={apps} total={memory.total} onQuit={onQuit} />
      ) : null}
    </>
  );
}

/**
 * The dev servers and the apps using the most memory, on the meter's scale.
 * Nothing sums them up: each counts compressed memory at full size, as
 * Activity Monitor does, so together they can come to more than is in use.
 */
/** A project's share of the dev servers' memory, in its color. */
interface ProjectShare {
  id: string;
  tint: number;
  bytes: number;
}

/** The dev servers' memory, project by project, `scale`d and out of `whole`. */
function Segments({ projects, scale, whole }: { projects: ProjectShare[]; scale: number; whole: number }) {
  return projects.map((project) => (
    <span key={project.id} style={{ width: share(project.bytes * scale, whole), background: `var(--tint-${project.tint})` }} />
  ));
}

interface BreakdownProps {
  servers: number;
  projects: ProjectShare[];
  apps: AppUsage[] | null;
  total: number;
  onQuit?: (app: AppUsage) => void;
}

function Breakdown({ servers, projects, apps, total, onQuit }: BreakdownProps) {
  const i18n = useI18n();
  if (apps === null) {
    return (
      <p id="memory-by-app" className={styles.breakdown}>
        {i18n.t("measuring")}
      </p>
    );
  }
  const parts = [
    { key: "servers", name: i18n.t("devServers"), bytes: servers, app: null },
    ...apps.map((app) => ({ key: `app:${app.name}`, name: displayName(app.name), bytes: app.memoryBytes, app })),
  ].filter((part) => part.bytes > 0);
  return (
    <div id="memory-by-app" className={styles.breakdown}>
      <ul aria-label={i18n.t("memoryByApp")} className={styles.parts}>
        {parts.map(({ key, name, bytes, app }) => (
          <li key={key}>
            <span className={styles.partName}>{name}</span>
            <span className={styles.partSide}>
              <span className={styles.partAmount}>{i18n.bytes(bytes)}</span>
              {/* The dev servers stop from their own rows; the system is never quit. */}
              {app && !app.system && onQuit ? (
                <button
                  type="button"
                  className={styles.partQuit}
                  aria-label={i18n.t("quitApp", { name })}
                  onClick={() => onQuit(app)}
                >
                  {i18n.t("quit")}
                </button>
              ) : null}
            </span>
            <span className={styles.bar} aria-hidden="true">
              {app === null ? <Segments projects={projects} scale={1} whole={total} /> : <span style={{ width: share(bytes, total) }} />}
            </span>
          </li>
        ))}
      </ul>
      <p className={styles.figures}>{i18n.t(platform === "windows" ? "figuresWindows" : "figuresMac")}</p>
    </div>
  );
}

/** A width for `bytes` out of `whole`. */
function share(bytes: number, whole: number): string {
  return `${whole > 0 ? Math.min(100, (bytes / whole) * 100) : 0}%`;
}

interface RowProps {
  service: Service;
  tint: number;
  selected: boolean;
  now: number;
  onShow: () => void;
  onOpen: () => void;
  onStop?: () => void;
}

/** What a row is called: its branch, else what runs, else its project. */
function titleOf(service: Service): string {
  return service.project?.branch ?? service.processes[0]?.name ?? service.project?.name ?? "";
}

/** A dev server is named by the heat note once it uses this much of one core. */
const BUSY_CPU_PERCENT = 50;

/** The Mac slowing down to cool off, and the dev server using the most CPU meanwhile, which needn't be why. */
function Heat({ dev }: { dev: Service[] }) {
  const i18n = useI18n();
  let busiest: { service: Service; percent: number } | null = null;
  for (const service of dev) {
    const percent = service.cpuPercent;
    if (percent === null || !Number.isFinite(percent) || percent < BUSY_CPU_PERCENT) continue;
    if (busiest === null || percent > busiest.percent || (percent === busiest.percent && service.id < busiest.service.id)) {
      busiest = { service, percent };
    }
  }
  const port = busiest?.service.ports[0]?.port;
  return (
    <p className={styles.heat}>
      {i18n.t("slowedByHeat")}
      {busiest
        ? ` ${i18n.t("busiestServer", {
            name: port === undefined ? titleOf(busiest.service) : `${titleOf(busiest.service)} (:${port})`,
            percent: i18n.percent(busiest.percent),
          })}`
        : null}
    </p>
  );
}

function Row({ service, tint, selected, now, onShow, onOpen, onStop }: RowProps) {
  const i18n = useI18n();
  const port = service.ports[0]?.port;
  const openable = service.ports[0] !== undefined && urlOf(service.ports[0]) !== null;
  const project = service.project;
  const title = titleOf(service);
  const details = [
    project?.worktree ? i18n.t("worktree") : null,
    service.command,
    agentOf(service),
    i18n.uptime(Math.max(0, now / 1000 - service.startedAt)),
  ]
    .filter(Boolean)
    .join(" · ");
  const memory = service.memoryBytes === null ? null : i18n.bytes(service.memoryBytes);
  // What the row shows, so a server outside any project is told apart by what runs.
  const label = [...new Set([port === undefined ? null : i18n.t("port", { port }), project?.name, title, service.command, memory])]
    .filter(Boolean)
    .join(", ");
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (selected) ref.current?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  return (
    <div
      ref={ref}
      id={`server-${service.id}`}
      role="option"
      aria-selected={selected}
      aria-label={label}
      className={styles.row}
      onClick={onShow}
    >
      <Plate port={port} tint={tint} />
      <div className={styles.text}>
        <div className={styles.title}>{title}</div>
        <div className={styles.details}>{details}</div>
      </div>
      <Trend values={trendOf(service.id)} />
      <div className={styles.side}>
        <span className={styles.memory}>{memory}</span>
        {/* The mouse's shortcuts; the keyboard finds the same actions on the server's page. */}
        <span className={styles.actions} aria-hidden="true" onMouseDown={(event) => event.preventDefault()}>
          <button
            type="button"
            tabIndex={-1}
            className={styles.action}
            aria-label={i18n.t("openInBrowser")}
            title={i18n.t("openInBrowser")}
            disabled={!openable}
            onClick={(event) => {
              event.stopPropagation();
              onOpen();
            }}
          >
            <OpenOut />
          </button>
          {onStop ? (
            <button
              type="button"
              tabIndex={-1}
              className={`${styles.action} ${styles.danger}`}
              aria-label={i18n.t("stop")}
              title={i18n.t("stop")}
              disabled={service.protected !== null}
              onClick={(event) => {
                event.stopPropagation();
                onStop();
              }}
            >
              <Square />
            </button>
          ) : null}
        </span>
      </div>
    </div>
  );
}

/** Digits match a port by its start; anything else, the project, branch or command. */
function matches(service: Service, query: string): boolean {
  const wanted = query.trim().toLowerCase();
  if (wanted === "") return true;
  if (/^\d+$/.test(wanted)) return service.ports.some((port) => String(port.port).startsWith(wanted));
  const project = service.project;
  return [project?.name, project?.branch, project?.framework?.name, service.command, ...service.processes.map((member) => member.name)].some(
    (text) => text?.toLowerCase().includes(wanted),
  );
}
