import { useEffect, useRef, useState, type RefObject } from "react";

import { hidePanel, inApp, openInBrowser, resizePanel, revealFolder } from "./api";
import { Detail } from "./Detail";
import { I18nProvider, localeOf } from "./i18n";
import { Panel } from "./Panel";
import { StopSheet } from "./StopSheet";
import { useUpdate } from "./store";
import type { Service } from "./types";
import styles from "./App.module.css";

/** Re-renders now and then, so ages and uptimes move without a new scan. */
function useNow(every: number): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), every);
    return () => clearInterval(timer);
  }, [every]);
  return now;
}

/** In the app, the window is exactly as tall as the panel. */
function useWindowFit(frame: RefObject<HTMLDivElement | null>) {
  useEffect(() => {
    const element = frame.current;
    if (!inApp || element === null) return;
    const observer = new ResizeObserver(() => {
      resizePanel(Math.ceil(element.getBoundingClientRect().height)).catch(console.error);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [frame]);
}

/** An Escape that nothing in the panel wanted closes it. */
function useEscapeHides() {
  useEffect(() => {
    if (!inApp) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) hidePanel().catch(console.error);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}

function open(service: Service) {
  const port = service.ports[0]?.port;
  if (port !== undefined) openInBrowser(service.id, port).catch(console.error);
}

function copy(url: string): Promise<void> {
  return navigator.clipboard.writeText(url);
}

function reveal(service: Service) {
  revealFolder(service.id).catch(console.error);
}

export function App() {
  const update = useUpdate();
  const now = Math.max(useNow(5_000), update?.scannedAt ?? 0);
  const [shown, setShown] = useState<string | null>(null);
  // The server as it was when asked about, so the question stays put while scans go on.
  const [stopping, setStopping] = useState<Service | null>(null);
  const frame = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  // Where the focus goes back to when the sheet closes; the page may have changed under it.
  const returnFocus = useRef<HTMLElement | null>(null);
  useWindowFit(frame);
  useEscapeHides();
  const service = shown === null ? null : (update?.services.find((each) => each.id === shown) ?? null);

  const askStop = (chosen: Service) => {
    returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setStopping(chosen);
  };
  useEffect(() => {
    if (stopping !== null) return;
    const previous = returnFocus.current;
    returnFocus.current = null;
    if (previous === null) return;
    // It may be gone, or still there but disabled.
    previous.focus();
    if (document.activeElement !== previous) {
      const fallback =
        content.current?.querySelector<HTMLElement>("section button") ??
        content.current?.querySelector<HTMLElement>('[role="combobox"]');
      fallback?.focus();
    }
  }, [stopping]);

  return (
    <I18nProvider locale={localeOf(navigator.language)}>
      <div ref={frame} className={styles.frame}>
        <div ref={content} className={styles.content} inert={stopping !== null}>
          <Panel
            hidden={shown !== null}
            update={update}
            now={now}
            onShow={(chosen) => setShown(chosen.id)}
            onOpen={open}
            onStop={askStop}
          />
          {shown === null ? null : (
            <Detail
              service={service}
              now={now}
              onBack={() => setShown(null)}
              onOpen={open}
              onCopy={copy}
              onReveal={reveal}
              onStop={askStop}
            />
          )}
        </div>
        {stopping === null ? null : (
          <StopSheet
            service={stopping}
            onClose={() => setStopping(null)}
            onDone={() => {
              setStopping(null);
              setShown(null);
            }}
          />
        )}
      </div>
    </I18nProvider>
  );
}
