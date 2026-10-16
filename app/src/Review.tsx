import { useEffect, useRef, useState } from "react";

import { choiceOf, groupsOf, matching, ordered, selectable, toggled, type GroupView, type Order } from "./cleanup";
import { ABOUT, NAMES, TINTS, note } from "./cleanerText";
import { useI18n } from "./i18n";
import { OpenOut } from "./icons";
import type { CleanGroup, Finding } from "./types";
import styles from "./Review.module.css";

const ORDERS: { order: Order; label: "orderSize" | "orderName" | "orderAge" }[] = [
  { order: "size", label: "orderSize" },
  { order: "name", label: "orderName" },
  { order: "age", label: "orderAge" },
];

export interface ReviewProps {
  /** The findings of one section of the window, all of its groups. */
  findings: Finding[];
  home: string;
  chosen: Set<string>;
  /** Milliseconds since the Unix epoch, for how long projects have sat. */
  now: number;
  onChoose: (next: Set<string>) => void;
  onReveal: (finding: Finding) => void;
}

/** A section's groups on the left, the one looked at on the right, item by item. */
export function Review({ findings, home, chosen, now, onChoose, onReveal }: ReviewProps) {
  const i18n = useI18n();
  const groups = groupsOf(findings, chosen);
  const [current, setCurrent] = useState<CleanGroup | null>(null);
  const [query, setQuery] = useState("");
  const [order, setOrder] = useState<Order>("size");
  const view = groups.find((group) => group.group === current) ?? groups[0];
  if (view === undefined) return <p className={styles.empty}>{i18n.t("cleanNothing")}</p>;
  const items = ordered(matching(view.findings, query), order);

  return (
    <div className={styles.review}>
      <ul className={styles.groups}>
        {groups.map((group) => (
          <li
            key={group.group}
            className={styles.group}
            data-current={group === view ? "" : undefined}
            style={{ ["--tint" as string]: `var(--tint-${TINTS[group.group]})` }}
          >
            <Check
              state={group.state}
              label={i18n.t(NAMES[group.group])}
              disabled={!group.selectable}
              onChange={() => onChoose(toggled(chosen, group.findings))}
            />
            <button
              type="button"
              className={styles.groupButton}
              aria-current={group === view ? "true" : undefined}
              onClick={() => setCurrent(group.group)}
            >
              <span className={styles.groupName}>
                <span className={styles.dot} />
                <span className={styles.groupLabel}>{i18n.t(NAMES[group.group])}</span>
              </span>
              <span className={styles.groupSize}>{i18n.bytes(group.bytes)}</span>
            </button>
          </li>
        ))}
      </ul>
      <section className={styles.detail} aria-label={i18n.t(NAMES[view.group])}>
        <p className={styles.about}>{i18n.t(ABOUT[view.group])}</p>
        <div className={styles.toolbar}>
          <Check
            state={choiceOf(items, chosen)}
            label={i18n.t("chooseShown")}
            disabled={!items.some(selectable)}
            onChange={() => onChoose(toggled(chosen, items))}
          />
          <input
            type="search"
            className={styles.search}
            placeholder={i18n.t("searchItems")}
            aria-label={i18n.t("searchItems")}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          <select
            className={styles.order}
            aria-label={i18n.t("orderBy")}
            value={order}
            onChange={(event) => setOrder(event.target.value as Order)}
          >
            {ORDERS.map(({ order: each, label }) => (
              <option key={each} value={each}>
                {i18n.t(label)}
              </option>
            ))}
          </select>
        </div>
        {items.length === 0 ? (
          <p className={styles.empty}>{i18n.t("noItemsMatch", { query: query.trim() })}</p>
        ) : (
          <ul className={styles.items} aria-label={i18n.t(NAMES[view.group])}>
            {items.map((finding) => (
              <li key={finding.path} className={styles.item} data-held={finding.inUse === null ? undefined : ""}>
                <Check
                  state={chosen.has(finding.path) ? "all" : "none"}
                  label={finding.name}
                  disabled={!selectable(finding)}
                  onChange={() => onChoose(toggled(chosen, [finding]))}
                />
                <span className={styles.itemText}>
                  <span className={styles.itemName}>{finding.name}</span>
                  <span className={styles.itemNote}>{note(finding, home, now, i18n)}</span>
                </span>
                <span className={styles.itemSize}>{i18n.bytes(finding.bytes)}</span>
                <button
                  type="button"
                  className={styles.reveal}
                  aria-label={`${i18n.t("revealInFinder")}: ${finding.name}`}
                  title={i18n.t("revealInFinder")}
                  onClick={() => onReveal(finding)}
                >
                  <OpenOut />
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

export function Check({
  state,
  label,
  disabled,
  onChange,
}: {
  state: GroupView["state"];
  label: string;
  disabled: boolean;
  onChange: () => void;
}) {
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (box.current) box.current.indeterminate = state === "some";
  }, [state]);
  return (
    <input
      ref={box}
      type="checkbox"
      className={styles.check}
      aria-label={label}
      checked={state === "all"}
      disabled={disabled}
      onChange={onChange}
    />
  );
}
