import type { CSSProperties } from "react";

import styles from "./Plate.module.css";

/** A port as a berth number on the quay, in its project's color. */
export function Plate({ port, tint = 0, large = false }: { port: number | undefined; tint?: number; large?: boolean }) {
  return (
    <span
      className={large ? `${styles.plate} ${styles.large}` : styles.plate}
      style={{ "--tint": `var(--tint-${tint})` } as CSSProperties}
    >
      {port ?? "—"}
    </span>
  );
}
