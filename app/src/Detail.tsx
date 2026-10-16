import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { hostOf, urlOf } from "./address";
import { useI18n } from "./i18n";
import { Chevron, OpenOut } from "./icons";
import { Plate } from "./Plate";
import { platform } from "./platform";
import { whyProtected } from "./protection";
import { agentOf, terminalOf } from "./session";
import type { Service } from "./types";
import styles from "./Detail.module.css";

export interface DetailProps {
  /** Null once the server has stopped. */
  service: Service | null;
  /** Milliseconds since the Unix epoch. */
  now: number;
  onBack: () => void;
  onOpen: (service: Service) => void;
  /** Settles once the URL is on the clipboard. */
  onCopy: (url: string) => Promise<void>;
  onReveal: (service: Service) => void;
  onStop: (service: Service) => void;
}

/** One server's page: what runs, where, from what, and what it uses. */
export function Detail({ service, now, onBack, onOpen, onCopy, onReveal, onStop }: DetailProps) {
  const i18n = useI18n();
  const first = useRef<HTMLButtonElement>(null);
  const [copied, setCopied] = useState<"copied" | "copyFailed" | null>(null);

  useEffect(() => {
    first.current?.focus();
  }, [service?.id]);
  useEffect(() => {
    if (copied === null) return;
    const timer = setTimeout(() => setCopied(null), 1500);
    return () => clearTimeout(timer);
  }, [copied]);

  const back = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return false;
    event.preventDefault();
    event.stopPropagation();
    onBack();
    return true;
  };

  if (service === null) {
    return (
      <section className={styles.page} aria-label={i18n.t("gone")} onKeyDown={back}>
        <header className={styles.header}>
          <button ref={first} type="button" className={styles.back} aria-label={i18n.t("back")} onClick={onBack}>
            <Chevron />
          </button>
        </header>
        <p className={styles.note}>{i18n.t("gone")}</p>
      </section>
    );
  }

  const project = service.project;
  const title = project?.name ?? service.processes[0]?.name ?? service.command ?? "";
  const port = service.ports[0];
  const url = port ? urlOf(port) : null;
  const stoppable = service.protected === null;
  const facts = [
    [i18n.t("branch"), project?.branch],
    [i18n.t("folder"), project?.root],
    [i18n.t("command"), service.command],
    [i18n.t("terminal"), terminalOf(service)],
    [i18n.t("session"), agentOf(service)],
  ].filter((fact): fact is [string, string] => Boolean(fact[1]));

  const copy = async () => {
    if (url === null) return;
    setCopied(null);
    try {
      await onCopy(url);
      setCopied("copied");
    } catch {
      setCopied("copyFailed");
    }
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.nativeEvent.isComposing || event.altKey || event.ctrlKey || event.metaKey || back(event)) return;
    if (event.key === "o" && port) {
      onOpen(service);
    } else if (event.key === "c") {
      void copy();
    } else if (event.key === "s" && stoppable) {
      onStop(service);
    }
  };

  return (
    <section className={styles.page} aria-label={title} onKeyDown={onKeyDown}>
      <header className={styles.header}>
        <button ref={first} type="button" className={styles.back} aria-label={i18n.t("back")} onClick={onBack}>
          <Chevron />
        </button>
        <h1 className={styles.title}>{title}</h1>
      </header>

      <div className={styles.hero}>
        <Plate port={port?.port} large />
        <div className={styles.status}>
          <div>
            <span className={styles.dot} aria-hidden="true" />
            {i18n.t("running", { uptime: i18n.uptime(Math.max(0, now / 1000 - service.startedAt)) })}
          </div>
          <div className={styles.muted}>{i18n.list([...new Set(service.ports.map((each) => each.address))])}</div>
        </div>
      </div>
      {port ? (
        <div className={styles.actions}>
          <button type="button" className={styles.primary} onClick={() => onOpen(service)}>
            <OpenOut />
            {i18n.t("openAt", { host: hostOf(port.address), port: port.port })}
          </button>
          <button type="button" className={styles.secondary} onClick={copy}>
            {i18n.t(copied ?? "copyUrl")}
          </button>
        </div>
      ) : null}

      <ul className={styles.facts} aria-label={i18n.t("about")}>
        {facts.map(([label, value]) => (
          <li key={label}>
            <span className={styles.label}>{label}</span>
            <span className={styles.value}>{value}</span>
          </li>
        ))}
      </ul>

      <div className={styles.usage}>
        <Figure
          label={i18n.t("memory")}
          value={service.memoryBytes === null ? "—" : i18n.bytes(service.memoryBytes)}
          note={i18n.t(platform === "windows" ? "memoryWindows" : "memoryMac")}
        />
        <Figure label="CPU" value={service.cpuPercent === null ? "—" : i18n.percent(service.cpuPercent, 1)} />
        <Figure label={i18n.t("processes")} value={String(service.processes.length)} />
      </div>
      <ul className={styles.processes} aria-label={i18n.t("processes")}>
        {service.processes.map((member) => (
          <li key={member.pid}>
            <span className={styles.name}>{member.name}</span>
            <span className={styles.command}>{member.command}</span>
            <span className={styles.memory}>{member.memoryBytes === null ? "" : i18n.bytes(member.memoryBytes)}</span>
          </li>
        ))}
      </ul>

      <footer className={styles.footer}>
        <button type="button" className={styles.stop} disabled={!stoppable} onClick={() => onStop(service)}>
          {i18n.t("stopEllipsis")}
        </button>
        {project ? (
          <button type="button" className={styles.link} onClick={() => onReveal(service)}>
            {i18n.t("revealFolder")}
          </button>
        ) : null}
      </footer>
      {service.protected === null ? null : <p className={styles.why}>{whyProtected(i18n, service.protected)}</p>}
    </section>
  );
}

function Figure({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className={styles.figure}>
      <span className={styles.label}>{label}</span>
      <strong>{value}</strong>
      {note ? <span className={styles.label}>{note}</span> : null}
    </div>
  );
}

