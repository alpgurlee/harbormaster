import styles from "./Trend.module.css";

const WIDTH = 34;
const HEIGHT = 14;
/** A change smaller than this share of the peak draws almost flat; a bigger one fills the height. */
const CALM = 0.1;

/** A server's memory over the last minutes, as a line. */
export function Trend({ values }: { values: readonly number[] }) {
  if (values.length < 2) return <span className={styles.trend} aria-hidden="true" />;
  const low = Math.min(...values);
  const peak = Math.max(...values);
  const range = peak - low;
  const span = (range < peak * CALM ? peak : range) || 1;
  const points = values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * WIDTH;
      const y = HEIGHT - 1 - ((value - low) / span) * (HEIGHT - 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg className={styles.trend} viewBox={`0 0 ${WIDTH} ${HEIGHT}`} width={WIDTH} height={HEIGHT} aria-hidden="true">
      <polyline points={points} />
    </svg>
  );
}
