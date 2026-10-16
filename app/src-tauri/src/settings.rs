//! What the user has set, kept as JSON in the app's own folder.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "settings";

/// The longest a project may sit before its build outputs are chosen.
const MOST_IDLE_DAYS: u32 = 365;
/// Enough names and folders for anyone; more is a mistake.
const MOST_ENTRIES: usize = 64;
const LONGEST_NAME: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Settings {
    pub(crate) language: Language,
    pub(crate) appearance: Appearance,
    /// Programs never listed as dev servers, by process name.
    pub(crate) hidden: Vec<String>,
    /// Where the cleaner looks for projects; none means the usual folders.
    pub(crate) roots: Vec<PathBuf>,
    /// How long a project sits untouched before its build outputs are chosen.
    pub(crate) idle_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::System,
            appearance: Appearance::System,
            hidden: Vec::new(),
            roots: Vec::new(),
            idle_days: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Language {
    #[default]
    System,
    En,
    Tr,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// Why settings were turned down.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Invalid {
    IdleDays,
    /// A project folder must be a plain full path inside the home folder.
    Root {
        path: PathBuf,
    },
    TooMany,
    Name {
        name: String,
    },
    /// They couldn't be written down.
    Unsaved {
        message: String,
    },
}

impl Settings {
    /// These settings made tidy, or why they can't be used: names trimmed,
    /// empty ones and repeats dropped, folders inside `home` only, which a
    /// leading `~` stands for.
    fn checked(mut self, home: &Path) -> Result<Self, Invalid> {
        if !(1..=MOST_IDLE_DAYS).contains(&self.idle_days) {
            return Err(Invalid::IdleDays);
        }
        let mut names: Vec<String> = Vec::new();
        for name in self.hidden.iter().map(|name| name.trim()) {
            if name.is_empty() || names.iter().any(|kept| kept == name) {
                continue;
            }
            if name.chars().count() > LONGEST_NAME || name.chars().any(char::is_control) {
                return Err(Invalid::Name { name: name.into() });
            }
            names.push(name.into());
        }
        self.hidden = names;
        // `~/code` is the home folder's code folder.
        self.roots = self
            .roots
            .into_iter()
            .map(|root| match root.strip_prefix("~") {
                Ok(rest) => home.join(rest),
                Err(_) => root,
            })
            .collect();
        self.roots.dedup();
        // `..` would lead out of the home folder whatever the path starts with.
        let plain = |root: &Path| {
            root.components()
                .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
        };
        if let Some(path) = self.roots.iter().find(|root| {
            !root.is_absolute() || !plain(root) || !root.starts_with(home) || *root == home
        }) {
            return Err(Invalid::Root { path: path.clone() });
        }
        if self.hidden.len() > MOST_ENTRIES || self.roots.len() > MOST_ENTRIES {
            return Err(Invalid::TooMany);
        }
        Ok(self)
    }
}

/// The settings in force, and the file they are kept in.
#[derive(Debug)]
pub(crate) struct Store {
    file: PathBuf,
    current: Mutex<Settings>,
}

impl Store {
    /// The settings kept in `file`; the defaults when there are none yet or
    /// they can't be read, in which case the file is left as it is.
    pub(crate) fn open(file: PathBuf) -> Self {
        let current = fs::read(&file)
            .ok()
            .and_then(|text| serde_json::from_slice(&text).ok())
            .unwrap_or_default();
        Self {
            file,
            current: Mutex::new(current),
        }
    }

    pub(crate) fn get(&self) -> Settings {
        self.lock().clone()
    }

    /// Checks `settings`, writes them down and puts them in force, handing
    /// back what is in force now. Nothing changes if they can't be written.
    pub(crate) fn set(&self, settings: Settings, home: &Path) -> Result<Settings, Invalid> {
        let settings = settings.checked(home)?;
        let mut current = self.lock();
        write(&self.file, &settings).map_err(|error| Invalid::Unsaved {
            message: error.to_string(),
        })?;
        *current = settings.clone();
        Ok(settings)
    }

    fn lock(&self) -> MutexGuard<'_, Settings> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Brings the settings window up, making it the first time.
pub(crate) fn show(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::default())
        .title("Harbormaster")
        .inner_size(640.0, 460.0)
        .min_inner_size(560.0, 400.0)
        .center();
    // The page runs up under the title bar, as a Mac app's own content does.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder.build()?.set_focus()
}

/// Writes `settings` to `file` whole or not at all: a copy beside it first,
/// then moved over it.
fn write(file: &Path, settings: &Settings) -> io::Result<()> {
    let folder = file
        .parent()
        .ok_or_else(|| io::Error::other("the settings file has no folder"))?;
    fs::create_dir_all(folder)?;
    let draft = file.with_extension("json.draft");
    fs::write(&draft, serde_json::to_vec_pretty(settings)?)?;
    fs::rename(&draft, file)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{Appearance, Invalid, Language, Settings, Store};

    const HOME: &str = "/Users/dev";

    fn place() -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("Harbormaster/settings.json");
        (folder, file)
    }

    #[test]
    fn starts_from_the_defaults_without_a_file() {
        let (_folder, file) = place();

        assert_eq!(Store::open(file).get(), Settings::default());
        assert_eq!(Settings::default().idle_days, 30);
    }

    #[test]
    fn keeps_what_was_set_for_the_next_start() {
        let (_folder, file) = place();
        let chosen = Settings {
            language: Language::Tr,
            appearance: Appearance::Dark,
            hidden: vec!["serena".into()],
            roots: vec![PathBuf::from("/Users/dev/work")],
            idle_days: 14,
        };

        let kept = Store::open(file.clone()).set(chosen.clone(), Path::new(HOME));

        assert_eq!(kept.unwrap(), chosen);
        assert_eq!(Store::open(file.clone()).get(), chosen);
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"idleDays\": 14"), "{text}");
        assert!(!file.with_extension("json.draft").exists());
    }

    #[test]
    fn falls_back_to_the_defaults_and_leaves_an_unreadable_file_alone() {
        let (_folder, file) = place();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"{ not json").unwrap();

        assert_eq!(Store::open(file.clone()).get(), Settings::default());
        assert_eq!(fs::read(&file).unwrap(), b"{ not json");
    }

    #[test]
    fn reads_settings_from_another_version() {
        let (_folder, file) = place();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, br#"{"language": "en", "laterSetting": true}"#).unwrap();

        let read = Store::open(file).get();

        assert_eq!(read.language, Language::En);
        assert_eq!(read.idle_days, 30);
    }

    #[test]
    fn tidies_the_names_of_hidden_programs() {
        let (_folder, file) = place();
        let messy = Settings {
            hidden: vec![
                " serena ".into(),
                String::new(),
                "serena".into(),
                "uvx".into(),
            ],
            ..Settings::default()
        };

        let kept = Store::open(file).set(messy, Path::new(HOME)).unwrap();

        assert_eq!(kept.hidden, ["serena", "uvx"]);
    }

    #[test]
    fn takes_a_project_folder_written_from_the_home_folder() {
        let (_folder, file) = place();
        let written = Settings {
            roots: vec![PathBuf::from("~/work"), PathBuf::from("/Users/dev/code")],
            ..Settings::default()
        };

        let kept = Store::open(file).set(written, Path::new(HOME)).unwrap();

        assert_eq!(
            kept.roots,
            [
                PathBuf::from("/Users/dev/work"),
                PathBuf::from("/Users/dev/code")
            ]
        );
    }

    #[test]
    fn turns_down_what_cant_be_used_and_keeps_what_was_in_force() {
        let (_folder, file) = place();
        let store = Store::open(file.clone());
        let with = |change: fn(&mut Settings)| {
            let mut settings = Settings::default();
            change(&mut settings);
            store.set(settings, Path::new(HOME))
        };

        assert_eq!(with(|s| s.idle_days = 0), Err(Invalid::IdleDays));
        assert_eq!(with(|s| s.idle_days = 400), Err(Invalid::IdleDays));
        assert!(matches!(
            with(|s| s.roots = vec![PathBuf::from("/etc")]),
            Err(Invalid::Root { .. })
        ));
        assert!(matches!(
            with(|s| s.roots = vec![PathBuf::from("code")]),
            Err(Invalid::Root { .. })
        ));
        assert!(matches!(
            with(|s| s.roots = vec![PathBuf::from(HOME)]),
            Err(Invalid::Root { .. })
        ));
        assert!(matches!(
            with(|s| s.roots = vec![PathBuf::from("/Users/dev/../../etc")]),
            Err(Invalid::Root { .. })
        ));
        assert!(matches!(
            with(|s| s.roots = vec![PathBuf::from("~")]),
            Err(Invalid::Root { .. })
        ));
        assert!(matches!(
            with(|s| s.roots = vec![PathBuf::from("~other/code")]),
            Err(Invalid::Root { .. })
        ));
        assert!(matches!(
            with(|s| s.hidden = vec!["a\u{1b}b".into()]),
            Err(Invalid::Name { .. })
        ));
        assert_eq!(
            with(|s| s.hidden = (0..65).map(|n| n.to_string()).collect()),
            Err(Invalid::TooMany)
        );
        assert_eq!(store.get(), Settings::default());
        assert!(!file.exists());
    }

    #[test]
    fn changes_nothing_when_the_settings_cant_be_written() {
        let (folder, _file) = place();
        // A file where the settings' folder would go.
        let blocked = folder.path().join("blocked");
        fs::write(&blocked, b"").unwrap();
        let store = Store::open(blocked.join("settings.json"));

        let refused = store.set(
            Settings {
                idle_days: 7,
                ..Settings::default()
            },
            Path::new(HOME),
        );

        assert!(matches!(refused, Err(Invalid::Unsaved { .. })));
        assert_eq!(store.get(), Settings::default());
    }
}
