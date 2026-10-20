import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

const line = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} as const;

/** The quay and its three bollards, as on the app icon. */
export function Mark(props: IconProps) {
  return (
    <svg viewBox="0 0 432 300" width="16" height="11" fill="currentColor" aria-hidden="true" {...props}>
      <rect x="0" y="240" width="432" height="60" rx="30" />
      <rect x="56" y="80" width="64" height="196" rx="32" />
      <rect x="184" y="0" width="64" height="276" rx="32" />
      <rect x="312" y="80" width="64" height="196" rx="32" />
    </svg>
  );
}

export function Gear(props: IconProps) {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" {...line} {...props}>
      <circle cx="8" cy="8" r="2" />
      <path d="M8 1.75v1.5M8 12.75v1.5M14.25 8h-1.5M3.25 8h-1.5M12.42 3.58l-1.06 1.06M4.64 11.36l-1.06 1.06M12.42 12.42l-1.06-1.06M4.64 4.64 3.58 3.58" />
    </svg>
  );
}

/** A broom, for clearing out what a Mac no longer needs. */
export function Sweep(props: IconProps) {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" {...line} {...props}>
      <path d="M12.75 2.25 8.5 6.5M6.25 6.75l3 3M5.5 7.5 2.75 13.25h3.5l.75-1.5.5 1.5h2.25l.5-3.5" />
    </svg>
  );
}

export function Search(props: IconProps) {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" {...line} {...props}>
      <circle cx="7" cy="7" r="4.25" />
      <path d="m10.25 10.25 3.5 3.5" />
    </svg>
  );
}

export function OpenOut(props: IconProps) {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" {...line} {...props}>
      <path d="M9.5 2.75h3.75v3.75M13.25 2.75 7.5 8.5M11.75 9.5v3a.75.75 0 0 1-.75.75H3.5a.75.75 0 0 1-.75-.75V5a.75.75 0 0 1 .75-.75h3" />
    </svg>
  );
}

export function Square(props: IconProps) {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" {...line} {...props}>
      <rect x="4.25" y="4.25" width="7.5" height="7.5" rx="1.25" />
    </svg>
  );
}

export function Chevron(props: IconProps) {
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" {...line} {...props}>
      <path d="m6 3.5 4.5 4.5L6 12.5" />
    </svg>
  );
}

/** An arrow going back, for a panel to return by its icon. */
export function PutBack(props: IconProps) {
  return (
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" {...line} {...props}>
      <path d="M6 3.5 3 6.5l3 3" />
      <path d="M3 6.5h6.25a3.75 3.75 0 0 1 0 7.5H7.5" />
    </svg>
  );
}
