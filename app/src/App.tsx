import { useEffect, useRef, useState, type RefObject } from "react";

import { anchorPanel, dragPanel, hidePanel, inApp, openCleaner, openInBrowser, openSettings, panelPlaced, resizePanel, revealFolder } from "./api";
import { Detail } from "./Detail";
import { projectOf, tintsOf } from "./model";
import { Panel } from "./Panel";
import { QuitSheet } from "./QuitSheet";
import { Settled } from "./Settled";
import { StopSheet } from "./StopSheet";
import { useUpdate } from "./store";
import type { AppUsage, Service } from "./types";
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
    const fit = () => {
      resizePanel(Math.ceil(element.getBoundingClientRect().height)).catch(console.error);
    };
    const observer = new ResizeObserver(fit);
    observer.observe(element);
    // A resize that went astray would otherwise stand until the panel's height changes.
    window.addEventListener("focus", fit);
    return () => {
      observer.disconnect();
      window.removeEventListener("focus", fit);
    };
  }, [frame]);
}

/** Whether the panel sits where the user put it. The app has the last word:
 * it hangs the panel from its icon again if the screen it was on is gone. */
function usePlaced(): [boolean, (placed: boolean) => void] {
  const [placed, setPlaced] = useState(false);
  useEffect(() => {
    if (!inApp) return;
    const ask = () => {
      panelPlaced().then(setPlaced).catch(console.error);
    };
    ask();
    window.addEventListener("focus", ask);
    return () => window.removeEventListener("focus", ask);
  }, []);
  return [placed, setPlaced];
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

function clean() {
  openCleaner().catch(console.error);
}

function settle() {
  openSettings().catch(console.error);
}

function reveal(service: Service) {
  revealFolder(service.id).catch(console.error);
}

export function App() {
  const update = useUpdate();
  // Dragged off its icon, the panel stays where it was put until put back.
  const [placed, setPlaced] = usePlaced();
  const drag = () => {
    setPlaced(true);
    dragPanel().catch(console.error);
  };
  const anchor = () => {
    setPlaced(false);
    anchorPanel().catch(console.error);
  };
  const now = Math.max(useNow(5_000), update?.scannedAt ?? 0);
  const [shown, setShown] = useState<string | null>(null);
  // The server as it was when asked about, so the question stays put while scans go on.
  const [stopping, setStopping] = useState<Service | null>(null);
  const [quitting, setQuitting] = useState<AppUsage | null>(null);
  const asking = stopping !== null || quitting !== null;
  const frame = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  // Where the focus goes back to when the sheet closes; the page may have changed under it.
  const returnFocus = useRef<HTMLElement | null>(null);
  useWindowFit(frame);
  useEscapeHides();
  const service = shown === null ? null : (update?.services.find((each) => each.id === shown) ?? null);
  const tints = tintsOf(update?.services ?? []);

  const keepFocus = () => {
    returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  };
  const askStop = (chosen: Service) => {
    keepFocus();
    setStopping(chosen);
  };
  const askQuit = (chosen: AppUsage) => {
    keepFocus();
    setQuitting(chosen);
  };
  useEffect(() => {
    if (asking) return;
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
  }, [asking]);

  return (
    <Settled>
      <div ref={frame} className={styles.frame}>
        <div ref={content} className={styles.content} inert={asking}>
          <Panel
            hidden={shown !== null}
            tints={tints}
            update={update}
            now={now}
            onShow={(chosen) => setShown(chosen.id)}
            onOpen={open}
            onStop={askStop}
            onQuit={askQuit}
            onClean={inApp ? clean : undefined}
            onSettings={inApp ? settle : undefined}
            onDrag={inApp ? drag : undefined}
            placed={placed}
            onAnchor={anchor}
          />
          {shown === null ? null : (
            <Detail
              service={service}
              tint={service === null ? 0 : (tints.get(projectOf(service)) ?? 0)}
              now={now}
              onBack={() => setShown(null)}
              onOpen={open}
              onCopy={copy}
              onReveal={reveal}
              onStop={askStop}
              onDrag={inApp ? drag : undefined}
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
        {quitting === null ? null : (
          <QuitSheet
            app={quitting}
            onClose={() => setQuitting(null)}
            onDone={() => {
              // The app's row goes with the app, so the focus waits on the breakdown's toggle.
              returnFocus.current = content.current?.querySelector<HTMLElement>('[aria-controls="memory-by-app"]') ?? null;
              setQuitting(null);
            }}
          />
        )}
      </div>
    </Settled>
  );
}
