import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { useI18n } from "./i18n";
import { Chevron, Mark, OpenOut, Search, Square } from "./icons";
import { groupByProject, totals } from "./model";
import { Plate } from "./Plate";
import { platform } from "./platform";
import { agentOf, displayName } from "./session";
import type { AppUsage, Memory, Service, Update } from "./types";
import styles from "./Panel.module.css";

/** A scan younger than this reads as now. */
const FRESH_MS = 10_000;

export interface PanelProps {
  /** Kept, with its search and selection, while a server's page shows. */
  hidden?: boolean;
  update: Update | null;
  /** Milliseconds since the Unix epoch. */
  now: number;
  onShow: (service: Service) => void;
  onOpen: (service: Service) => void;
  onStop: (service: Service) => void;
}

export function Panel({ hidden = false, update, now, onShow, onOpen, onStop }: PanelProps) {
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
        <Header />
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
      selected={service === selected}
      now={now}
      onShow={() => {
        setSelectedId(service.id);
        onShow(service);
      }}
      onOpen={() => onOpen(service)}
      onStop={() => onStop(service)}
    />
  );

  let empty: string | null = null;
  if (order.length === 0) {
    empty = searching ? i18n.t("nothingMatches", { query: query.trim() }) : i18n.t("nothingRuns");
  }

  return (
    <div className={styles.panel} hidden={hidden}>
      <Header />
      <Summary dev={dev} memory={update.memory} apps={update.apps} />
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

function Header() {
  return (
    <header className={styles.header}>
      <Mark className={styles.mark} />
      Harbormaster
    </header>
  );
}

function Summary({ dev, memory, apps }: { dev: Service[]; memory: Memory | null; apps: AppUsage[] | null }) {
  const i18n = useI18n();
  const [open, setOpen] = useState(false);
  const sum = totals(dev);
  const servers = sum.memoryBytes ?? 0;
  // Footprints can pass the memory in use; the meter can't show more than that.
  const serversInUse = Math.min(servers, memory?.used ?? 0);
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
          <span className={styles.servers} style={{ width: share(serversInUse, memory.total) }} />
          <span style={{ width: share(memory.used - serversInUse, memory.total) }} />
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
      {memory && open ? <Breakdown servers={servers} apps={apps} total={memory.total} /> : null}
    </>
  );
}

/**
 * The dev servers and the apps using the most memory, on the meter's scale.
 * Nothing sums them up: each counts compressed memory at full size, as
 * Activity Monitor does, so together they can come to more than is in use.
 */
function Breakdown({ servers, apps, total }: { servers: number; apps: AppUsage[] | null; total: number }) {
  const i18n = useI18n();
  if (apps === null) {
    return (
      <p id="memory-by-app" className={styles.breakdown}>
        {i18n.t("measuring")}
      </p>
    );
  }
  const parts = [
    { key: "servers", name: i18n.t("devServers"), bytes: servers },
    ...apps.map((app) => ({ key: `app:${app.name}`, name: displayName(app.name), bytes: app.memoryBytes })),
  ].filter((part) => part.bytes > 0);
  return (
    <div id="memory-by-app" className={styles.breakdown}>
      <ul aria-label={i18n.t("memoryByApp")} className={styles.parts}>
        {parts.map((part) => (
          <li key={part.key}>
            <span className={styles.partName}>{part.name}</span>
            <span className={styles.partAmount}>{i18n.bytes(part.bytes)}</span>
            <span className={styles.bar} aria-hidden="true">
              <span className={part.key === "servers" ? styles.servers : undefined} style={{ width: share(part.bytes, total) }} />
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
  selected: boolean;
  now: number;
  onShow: () => void;
  onOpen: () => void;
  onStop: () => void;
}

function Row({ service, selected, now, onShow, onOpen, onStop }: RowProps) {
  const i18n = useI18n();
  const port = service.ports[0]?.port;
  const project = service.project;
  const title = project?.branch ?? service.processes[0]?.name ?? project?.name ?? "";
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
      <Plate port={port} />
      <div className={styles.text}>
        <div className={styles.title}>
          <span className={styles.dot} aria-hidden="true" />
          {title}
        </div>
        <div className={styles.details}>{details}</div>
      </div>
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
            onClick={(event) => {
              event.stopPropagation();
              onOpen();
            }}
          >
            <OpenOut />
          </button>
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
