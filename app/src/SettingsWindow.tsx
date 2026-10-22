import { useEffect, useRef, useState, type FormEvent } from "react";

import { version } from "../package.json";
import { settingsGet, settingsSet } from "./api";
import { useI18n, type TextKey, type Translator } from "./i18n";
import { hasCleaner, platform } from "./platform";
import type { Appearance, Settings, SettingsRefusal } from "./types";
import styles from "./SettingsWindow.module.css";

/** Where the window reads and keeps the settings, which tests hand in themselves. */
export interface SettingsBackend {
  get: () => Promise<Settings>;
  /** Keeps `settings`, handing back what is in force; rejects with a `SettingsRefusal`. */
  set: (settings: Settings) => Promise<Settings>;
}

const BACKEND: SettingsBackend = { get: settingsGet, set: settingsSet };

type Section = "general" | "servers" | "cleanup" | "about";
const SECTIONS: Section[] = ["general", "servers", "cleanup", "about"];
const SECTION_NAMES: Record<Section, TextKey> = {
  general: "settingsGeneral",
  servers: "settingsServers",
  cleanup: "settingsCleanup",
  about: "settingsAbout",
};
const APPEARANCES: [Appearance, TextKey][] = [
  ["system", "sameAsSystem"],
  ["light", "appearanceLight"],
  ["dark", "appearanceDark"],
];

/** What the user can set, each kept as soon as it is changed. */
/** `cleans` says whether this platform has the cleaner its section sets up. */
export function SettingsWindow({ backend = BACKEND, cleans = hasCleaner }: { backend?: SettingsBackend; cleans?: boolean }) {
  const i18n = useI18n();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [section, setSection] = useState<Section>("general");
  const [refusal, setRefusal] = useState<SettingsRefusal | null>(null);
  // Changes are kept one at a time, each made to what the one before left in force.
  const queue = useRef(Promise.resolve(true));
  const latest = useRef<Settings | null>(null);
  useEffect(() => {
    // Only the latest reading counts: one made before the window was mounted again can come back last.
    let live = true;
    backend
      .get()
      .then((first) => {
        if (!live) return;
        latest.current = first;
        setSettings(first);
      })
      .catch(console.error);
    return () => {
      live = false;
    };
  }, [backend]);
  if (settings === null) return null;

  // Whether the change was kept, so what was typed for it can go.
  const change = (edit: (current: Settings) => Settings) => {
    const kept = queue.current.then(() => {
      setRefusal(null);
      return backend.set(edit(latest.current ?? settings)).then(
        (now) => {
          latest.current = now;
          setSettings(now);
          return true;
        },
        (error: unknown) => {
          setRefusal(asRefusal(error));
          return false;
        },
      );
    });
    queue.current = kept;
    return kept;
  };

  return (
    <div className={styles.window}>
      <nav className={styles.side} aria-label={i18n.t("settings")}>
        <div className={styles.sideTop} data-tauri-drag-region />
        {SECTIONS.filter((each) => cleans || each !== "cleanup").map((each) => (
          <button
            key={each}
            type="button"
            className={styles.section}
            aria-current={section === each ? "page" : undefined}
            onClick={() => {
              setSection(each);
              setRefusal(null);
            }}
          >
            {i18n.t(SECTION_NAMES[each])}
          </button>
        ))}
      </nav>
      <main className={styles.main}>
        <header className={styles.bar} data-tauri-drag-region>
          <h1 className={styles.title}>{i18n.t(SECTION_NAMES[section])}</h1>
        </header>
        <div className={styles.page}>
          {section === "general" ? (
            <>
              <label className={styles.field}>
                <span className={styles.label}>{i18n.t("language")}</span>
                <select
                  className={styles.select}
                  value={settings.language}
                  onChange={(event) => {
                    const language = event.target.value as Settings["language"];
                    void change((current) => ({ ...current, language }));
                  }}
                >
                  <option value="system">{i18n.t("sameAsSystem")}</option>
                  <option value="en">English</option>
                  <option value="tr">Türkçe</option>
                </select>
              </label>
              <fieldset className={styles.field}>
                <legend className={styles.label}>{i18n.t("appearance")}</legend>
                <div className={styles.choices}>
                  {APPEARANCES.map(([appearance, name]) => (
                    <label key={appearance} className={styles.choice}>
                      <input
                        type="radio"
                        name="appearance"
                        checked={settings.appearance === appearance}
                        onChange={() => void change((current) => ({ ...current, appearance }))}
                      />
                      {i18n.t(name)}
                    </label>
                  ))}
                </div>
              </fieldset>
            </>
          ) : section === "servers" ? (
            <Entries
              title={i18n.t("hiddenTitle")}
              hint={i18n.t("hiddenHint")}
              placeholder={i18n.t("programName")}
              entries={settings.hidden}
              onChange={(edit) => change((current) => ({ ...current, hidden: edit(current.hidden) }))}
            />
          ) : section === "cleanup" ? (
            <>
              <Entries
                title={i18n.t("rootsTitle")}
                hint={i18n.t("rootsHint")}
                placeholder={i18n.t("folderPath")}
                entries={settings.roots}
                onChange={(edit) => change((current) => ({ ...current, roots: edit(current.roots) }))}
              />
              <label className={styles.field}>
                <span className={styles.label}>{i18n.t("idleTitle")}</span>
                <input
                  className={styles.days}
                  type="number"
                  min={1}
                  max={365}
                  defaultValue={settings.idleDays}
                  onChange={(event) => {
                    const days = Number(event.target.value);
                    if (Number.isInteger(days) && days >= 1 && days <= 365) void change((current) => ({ ...current, idleDays: days }));
                  }}
                />
              </label>
            </>
          ) : (
            <div className={styles.about}>
              <p className={styles.name}>Harbormaster</p>
              <p className={styles.hint}>{i18n.t("version", { version })}</p>
              <p>{i18n.t(platform === "windows" ? "aboutTextWindows" : "aboutText")}</p>
              <p className={styles.hint}>{i18n.t("aboutLicense")}</p>
              <p className={styles.hint}>github.com/alpgurlee/harbormaster</p>
            </div>
          )}
          {refusal === null ? null : (
            <p className={styles.refusal} role="alert">
              {refusalText(refusal, i18n)}
            </p>
          )}
        </div>
      </main>
    </div>
  );
}

interface EntriesProps {
  title: string;
  hint: string;
  placeholder: string;
  entries: string[];
  /** Changes the list as it stands when the change is kept; resolves to whether it was. */
  onChange: (edit: (entries: string[]) => string[]) => Promise<boolean>;
}

/** A list of names or folders, each to take out, and a field to add one. */
function Entries({ title, hint, placeholder, entries, onChange }: EntriesProps) {
  const i18n = useI18n();
  const [draft, setDraft] = useState("");
  const add = (event: FormEvent) => {
    event.preventDefault();
    const entry = draft.trim();
    if (entry === "") return;
    // What was typed stays until it is kept, so a refused entry can be put right,
    // and what was typed after it stays too.
    const typed = draft;
    void onChange((list) => [...list, entry]).then((kept) => {
      if (kept) setDraft((now) => (now === typed ? "" : now));
    });
  };
  return (
    <section className={styles.field}>
      <h2 className={styles.label}>{title}</h2>
      <p className={styles.hint}>{hint}</p>
      <ul className={styles.entries} aria-label={title}>
        {entries.map((entry) => (
          <li key={entry} className={styles.entry}>
            <span className={styles.entryName}>{entry}</span>
            <button
              type="button"
              className={styles.remove}
              aria-label={i18n.t("removeEntry", { name: entry })}
              onClick={() => void onChange((list) => list.filter((each) => each !== entry))}
            >
              ×
            </button>
          </li>
        ))}
      </ul>
      <form className={styles.adding} onSubmit={add}>
        <input
          className={styles.input}
          value={draft}
          placeholder={placeholder}
          aria-label={placeholder}
          onChange={(event) => setDraft(event.target.value)}
        />
        <button type="submit" className={styles.button}>
          {i18n.t("add")}
        </button>
      </form>
    </section>
  );
}

function refusalText(refusal: SettingsRefusal, i18n: Translator): string {
  switch (refusal.code) {
    case "idleDays":
      return i18n.t("invalidIdleDays");
    case "tooMany":
      return i18n.t("invalidTooMany");
    case "root":
      return i18n.t("invalidRoot", { path: refusal.path });
    case "name":
      return i18n.t("invalidName", { name: refusal.name });
    case "unsaved":
      return i18n.t("invalidUnsaved", { message: refusal.message });
  }
}

/** A refusal from the app, or anything else thrown made into one. */
function asRefusal(error: unknown): SettingsRefusal {
  if (typeof error === "object" && error !== null && "code" in error && typeof error.code === "string") {
    return error as SettingsRefusal;
  }
  return { code: "unsaved", message: String(error) };
}
