import { useEffect, useState } from "react";

import { openInBrowser } from "./api";
import { I18nProvider, localeOf } from "./i18n";
import { Panel } from "./Panel";
import { useUpdate } from "./store";
import type { Service } from "./types";

/** Re-renders now and then, so ages and uptimes move without a new scan. */
function useNow(every: number): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), every);
    return () => clearInterval(timer);
  }, [every]);
  return now;
}

function open(service: Service) {
  const port = service.ports[0]?.port;
  if (port !== undefined) openInBrowser(service.id, port).catch(console.error);
}

export function App() {
  const update = useUpdate();
  const now = useNow(5_000);
  const [, setShown] = useState<string | null>(null);

  return (
    <I18nProvider locale={localeOf(navigator.language)}>
      <Panel
        update={update}
        now={Math.max(now, update?.scannedAt ?? 0)}
        onShow={(service) => setShown(service.id)}
        onOpen={open}
        onStop={(service) => setShown(service.id)}
      />
    </I18nProvider>
  );
}
