import styles from "./Plate.module.css";

/** A port as a berth number on the quay. */
export function Plate({ port, large = false }: { port: number | undefined; large?: boolean }) {
  return <span className={large ? `${styles.plate} ${styles.large}` : styles.plate}>{port ?? "—"}</span>;
}
