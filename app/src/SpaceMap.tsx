import { useEffect, useRef, useState } from "react";

import { asRefusal, refusalText } from "./cleanerText";
import { useI18n } from "./i18n";
import { SHORT_TEXT, bytesOf, pieces, unmeasured, type Piece } from "./mapPieces";
import { squarify } from "./treemap";
import type { Area, CleanRefusal } from "./types";
import styles from "./SpaceMap.module.css";

/** The room left around each tile, so tiles side by side stay apart. */
const GAP = 1;
/** The least room a tile needs to say what it is. */
const LABEL_WIDTH = 72;
const LABEL_HEIGHT = 34;
/** How many tints the theme has; places take them in turn, so neighbours differ. */
const TINTS = 6;

export interface SpaceMapProps {
  root: Area;
  /** Shows the place numbered `id` in Finder; rejects with a `CleanRefusal`. */
  onReveal: (id: number) => Promise<void>;
}

/** A map of the home folder to go through: what each folder holds, as a list and as tiles sized by their room. */
export function SpaceMap({ root, onReveal }: SpaceMapProps) {
  const i18n = useI18n();
  const [trail, setTrail] = useState<Area[]>([root]);
  const [chosen, setChosen] = useState<Area | null>(null);
  // Why Finder wasn't shown a place, kept with the place it was for.
  const [refused, setRefused] = useState<{ id: number; refusal: CleanRefusal } | null>(null);
  const here = trail[trail.length - 1];
  const shown = pieces(here);
  const focus = chosen ?? here;
  const nameOf = (area: Area) => (area === root ? i18n.t("mapHome") : area.name);

  const pick = (piece: Piece) => {
    if (piece.kind === "rest") return;
    if (piece.area.inside.length > 0) {
      setTrail([...trail, piece.area]);
      setChosen(null);
    } else {
      setChosen(piece.area);
    }
  };
  const reveal = () => {
    const { id } = focus;
    setRefused(null);
    onReveal(id).catch((error: unknown) => setRefused({ id, refusal: asRefusal(error) }));
  };

  return (
    <div className={styles.map}>
      <nav className={styles.trail} aria-label={i18n.t("mapTrail")}>
        <ol>
          {trail.map((area, depth) =>
            depth === trail.length - 1 ? (
              <li key={area.id} aria-current="location">
                {nameOf(area)}
              </li>
            ) : (
              <li key={area.id}>
                <button
                  type="button"
                  className={styles.crumb}
                  onClick={() => {
                    setTrail(trail.slice(0, depth + 1));
                    setChosen(null);
                  }}
                >
                  {nameOf(area)}
                </button>
              </li>
            ),
          )}
        </ol>
      </nav>
      {shown.length === 0 ? (
        <p className={styles.nothing}>{i18n.t("mapNothing")}</p>
      ) : (
        <div className={styles.body}>
          <ul className={styles.list} aria-label={i18n.t("mapHolds", { name: nameOf(here) })}>
            {shown.map((piece, index) => (
              <li key={piece.kind === "rest" ? "rest" : piece.area.id} style={tintOf(piece, index)}>
                <Row piece={piece} share={here.bytes === 0 ? 0 : bytesOf(piece) / here.bytes} chosen={chosen} onPick={pick} />
              </li>
            ))}
          </ul>
          <Tiles shown={shown} chosen={chosen} onPick={pick} />
        </div>
      )}
      <section className={styles.detail} aria-label={nameOf(focus)}>
        <p className={styles.about}>
          <strong className={styles.detailName}>{nameOf(focus)}</strong>
          <span className={styles.facts}>
            {i18n.t(focus.folder ? "mapFolder" : "mapFile")}
            {unmeasured(focus) ? null : ` · ${i18n.bytes(focus.bytes)}`}
          </span>
          {focus.short === null ? null : <span className={styles.note}>{i18n.t(SHORT_TEXT[focus.short])}</span>}
          {focus.folded ? <span className={styles.note}>{i18n.t("mapFolded")}</span> : null}
          {refused?.id === focus.id ? (
            <span className={styles.refusal} role="alert">
              {refusalText(refused.refusal, i18n)}
            </span>
          ) : null}
        </p>
        {focus === root ? null : (
          <button type="button" className={styles.reveal} onClick={reveal}>
            {i18n.t("revealInFinder")}
          </button>
        )}
      </section>
    </div>
  );
}

/** The tint of the piece at `index`, the same in the list and on its tile; what the others hold together has none. */
const tintOf = (piece: Piece, index: number) =>
  piece.kind === "rest" ? undefined : { ["--tint" as string]: `var(--tint-${index % TINTS})` };

/** One line of the list, the accessible twin of the tiles. */
function Row({ piece, share, chosen, onPick }: { piece: Piece; share: number; chosen: Area | null; onPick: (piece: Piece) => void }) {
  const i18n = useI18n();
  const facts = (
    <>
      <span className={styles.swatch} aria-hidden="true" />
      <span className={styles.rowName}>{piece.kind === "rest" ? i18n.t("mapRest") : piece.area.name}</span>
      <span className={styles.rowSize}>{piece.kind === "area" && unmeasured(piece.area) ? "–" : i18n.bytes(bytesOf(piece))}</span>
      <span className={styles.share} aria-hidden="true">
        <span style={{ inlineSize: `${Math.min(1, share) * 100}%` }} />
      </span>
    </>
  );
  if (piece.kind === "rest") {
    return (
      <div className={styles.row} data-rest="">
        {facts}
      </div>
    );
  }
  const { area } = piece;
  return (
    <button
      type="button"
      className={styles.row}
      aria-current={area === chosen ? "true" : undefined}
      onClick={() => onPick(piece)}
    >
      {facts}
      {area.short === null ? null : (
        <>
          <span className={styles.flag} aria-hidden="true" />
          <span className={styles.hidden}>{i18n.t(SHORT_TEXT[area.short])}</span>
        </>
      )}
      {area.inside.length > 0 ? (
        <span className={styles.chevron} aria-hidden="true">
          ›
        </span>
      ) : null}
    </button>
  );
}

/** The pieces as tiles, each as large as its share of the room; the list says the same to a screen reader. */
function Tiles({ shown, chosen, onPick }: { shown: Piece[]; chosen: Area | null; onPick: (piece: Piece) => void }) {
  const i18n = useI18n();
  const box = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => {
    const element = box.current;
    if (element === null) return;
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (rect !== undefined) setSize({ width: rect.width, height: rect.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const boxes = squarify(shown.map(bytesOf), { x: 0, y: 0, ...size });

  return (
    <div ref={box} className={styles.tiles} aria-hidden="true">
      {shown.map((piece, index) => {
        const { x, y, width, height } = boxes[index];
        if (width <= 2 * GAP || height <= 2 * GAP) return null;
        const area = piece.kind === "area" ? piece.area : null;
        return (
          <div
            key={area?.id ?? "rest"}
            className={styles.tile}
            data-tile=""
            data-rest={area === null ? "" : undefined}
            data-chosen={area !== null && area === chosen ? "" : undefined}
            style={{ ...tintOf(piece, index), left: x + GAP, top: y + GAP, width: width - 2 * GAP, height: height - 2 * GAP }}
            onClick={() => onPick(piece)}
          >
            {width >= LABEL_WIDTH && height >= LABEL_HEIGHT ? (
              <>
                <span className={styles.tileName}>{area?.name ?? i18n.t("mapRest")}</span>
                <span className={styles.tileSize}>{i18n.bytes(bytesOf(piece))}</span>
              </>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
