//! What the user has set, kept as JSON in the app's own folder.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
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
/// The longest path macOS takes (`PATH_MAX`).
const LONGEST_PATH: usize = 1024;
/// Far more than any settings take; a larger file is someone else's.
const LARGEST_FILE: u64 = 1024 * 1024;

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
    /// leading `~` stands for, each kept once.
    fn checked(mut self, home: &Path) -> Result<Self, Invalid> {
        if !(1..=MOST_IDLE_DAYS).contains(&self.idle_days) {
            return Err(Invalid::IdleDays);
        }
        // Counted as sent, so a long list is turned down before any work.
        if self.hidden.len() > MOST_ENTRIES || self.roots.len() > MOST_ENTRIES {
            return Err(Invalid::TooMany);
        }
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        for name in self.hidden.iter().map(|name| name.trim()) {
            if name.is_empty() {
                continue;
            }
            if name.chars().count() > LONGEST_NAME || name.chars().any(char::is_control) {
                return Err(Invalid::Name { name: name.into() });
            }
            if seen.insert(name) {
                names.push(name.to_owned());
            }
        }
        self.hidden = names;
        let mut seen = HashSet::new();
        let mut roots = Vec::new();
        for root in &self.roots {
            // `~/code` is the home folder's code folder.
            let root = match root.strip_prefix("~") {
                Ok(rest) => home.join(rest),
                Err(_) => root.clone(),
            };
            // `..` would lead out of the home folder whatever the path starts
            // with. A drive or share, on Windows, must still be the home's.
            let plain = root.components().all(|part| {
                matches!(
                    part,
                    Component::Prefix(_) | Component::RootDir | Component::Normal(_)
                )
            });
            if root.as_os_str().len() > LONGEST_PATH
                || !root.is_absolute()
                || !plain
                || !harbormaster_cleaner::within(&root, home)
                || harbormaster_cleaner::within(home, &root)
            {
                return Err(Invalid::Root { path: root });
            }
            // Spelled the same way however many slashes it was written with.
            let root: PathBuf = root.components().collect();
            if seen.insert(root.clone()) {
                roots.push(root);
            }
        }
        self.roots = roots;
        Ok(self)
    }
}

/// Tells a subscriber what is in force, and says whether it still can be told.
type Told = Box<dyn Fn(&Settings) -> bool + Send>;

/// The settings in force, the file they are kept in, and the subscribers
/// told of each change: the windows, and the menu bar icon's menu.
pub(crate) struct Store {
    file: PathBuf,
    home: PathBuf,
    state: Mutex<State>,
}

struct State {
    current: Settings,
    /// Each subscriber's way to be told, by its key.
    told: Vec<(String, Told)>,
}

impl Store {
    /// The settings kept in `file` for the user whose home folder is `home`;
    /// the defaults when there are none yet or none that would be kept now,
    /// in which case the file is left as it is.
    pub(crate) fn open(file: PathBuf, home: PathBuf) -> Self {
        let current = read(&file)
            .and_then(|text| serde_json::from_slice::<Settings>(&text).ok())
            .and_then(|settings| settings.checked(&home).ok())
            .unwrap_or_default();
        Self {
            file,
            home,
            state: Mutex::new(State {
                current,
                told: Vec::new(),
            }),
        }
    }

    pub(crate) fn get(&self) -> Settings {
        self.lock().current.clone()
    }

    /// Checks `settings`, writes them down, puts them in force and tells
    /// every subscriber, handing back what is in force now. Nothing changes
    /// if they can't be written.
    pub(crate) fn set(&self, settings: Settings) -> Result<Settings, Invalid> {
        let settings = settings.checked(&self.home)?;
        let mut state = self.lock();
        write(&self.file, &settings).map_err(|error| Invalid::Unsaved {
            message: error.to_string(),
        })?;
        state.told.retain(|(_, told)| told(&settings));
        state.current = settings.clone();
        Ok(settings)
    }

    /// Tells the subscriber identified by `key` what is in force now and
    /// then each change, for as long as `told` says it could be told.
    /// Told under the same lock as a change, so none falls between. One that
    /// subscribes again, as a window that reloaded does, is told the new way
    /// only.
    pub(crate) fn subscribe(&self, key: &str, told: impl Fn(&Settings) -> bool + Send + 'static) {
        let mut state = self.lock();
        state.told.retain(|(subscriber, _)| subscriber != key);
        if told(&state.current) {
            state.told.push((key.to_owned(), Box::new(told)));
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
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

/// What `file` holds, when it is no larger than settings ever are.
fn read(file: &Path) -> Option<Vec<u8>> {
    let mut text = Vec::new();
    fs::File::open(file)
        .ok()?
        .take(LARGEST_FILE + 1)
        .read_to_end(&mut text)
        .ok()?;
    u64::try_from(text.len())
        .is_ok_and(|length| length <= LARGEST_FILE)
        .then_some(text)
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
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{Appearance, Invalid, Language, Settings, Store};

    #[cfg(unix)]
    const HOME: &str = "/Users/dev";
    #[cfg(windows)]
    const HOME: &str = r"C:\Users\dev";

    fn home() -> PathBuf {
        PathBuf::from(HOME)
    }

    fn place() -> (tempfile::TempDir, PathBuf) {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("Harbormaster/settings.json");
        (folder, file)
    }

    fn open(file: PathBuf) -> Store {
        Store::open(file, PathBuf::from(HOME))
    }

    fn idle(days: u32) -> Settings {
        Settings {
            idle_days: days,
            ..Settings::default()
        }
    }

    #[test]
    fn starts_from_the_defaults_without_a_file() {
        let (_folder, file) = place();

        assert_eq!(open(file).get(), Settings::default());
        assert_eq!(Settings::default().idle_days, 30);
    }

    #[test]
    fn keeps_what_was_set_for_the_next_start() {
        let (_folder, file) = place();
        let chosen = Settings {
            language: Language::Tr,
            appearance: Appearance::Dark,
            hidden: vec!["serena".into()],
            roots: vec![home().join("work")],
            idle_days: 14,
        };

        let kept = open(file.clone()).set(chosen.clone());

        assert_eq!(kept.unwrap(), chosen);
        assert_eq!(open(file.clone()).get(), chosen);
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"idleDays\": 14"), "{text}");
        assert!(!file.with_extension("json.draft").exists());
    }

    #[test]
    fn falls_back_to_the_defaults_and_leaves_an_unreadable_file_alone() {
        let (_folder, file) = place();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"{ not json").unwrap();

        assert_eq!(open(file.clone()).get(), Settings::default());
        assert_eq!(fs::read(&file).unwrap(), b"{ not json");
    }

    #[test]
    fn reads_settings_from_another_version() {
        let (_folder, file) = place();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, br#"{"language": "en", "laterSetting": true}"#).unwrap();

        let read = open(file).get();

        assert_eq!(read.language, Language::En);
        assert_eq!(read.idle_days, 30);
    }

    #[test]
    fn reads_back_only_settings_it_would_keep() {
        for text in [
            r#"{"idleDays": 0}"#,
            r#"{"roots": ["/etc"]}"#,
            r#"{"hidden": ["a\u001bb"]}"#,
        ] {
            let (_folder, file) = place();
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, text).unwrap();

            assert_eq!(open(file.clone()).get(), Settings::default(), "{text}");
            assert_eq!(fs::read_to_string(&file).unwrap(), text);
        }
    }

    #[test]
    fn leaves_alone_a_file_larger_than_settings_ever_are() {
        let (_folder, file) = place();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut text = br#"{"idleDays": 14}"#.to_vec();
        text.resize(2 * 1024 * 1024, b' ');
        fs::write(&file, text).unwrap();

        assert_eq!(open(file).get(), Settings::default());
    }

    #[test]
    fn tidies_the_names_of_hidden_programs() {
        let (_folder, file) = place();
        let messy = Settings {
            hidden: vec![
                " serena ".into(),
                String::new(),
                "uvx".into(),
                "serena".into(),
            ],
            ..Settings::default()
        };

        let kept = open(file).set(messy).unwrap();

        assert_eq!(kept.hidden, ["serena", "uvx"]);
    }

    #[test]
    #[cfg(windows)]
    fn takes_a_project_folder_however_its_case_is_written_on_windows() {
        let (_folder, file) = place();
        let written = Settings {
            roots: vec![PathBuf::from(r"c:\users\DEV\code")],
            ..Settings::default()
        };

        let kept = open(file).set(written).unwrap();

        assert_eq!(kept.roots, [PathBuf::from(r"c:\users\DEV\code")]);
        let home_again = Settings {
            roots: vec![PathBuf::from(r"C:\USERS\dev")],
            ..Settings::default()
        };
        assert!(open(place().1).set(home_again).is_err());
    }

    #[test]
    fn takes_a_project_folder_written_from_the_home_folder() {
        let (_folder, file) = place();
        let written = Settings {
            roots: vec![PathBuf::from("~/work"), home().join("code")],
            ..Settings::default()
        };

        let kept = open(file).set(written).unwrap();

        assert_eq!(kept.roots, [home().join("work"), home().join("code")]);
    }

    #[test]
    fn keeps_a_project_folder_once_however_it_was_written() {
        let (_folder, file) = place();
        let written = Settings {
            roots: vec![
                PathBuf::from(format!("{HOME}/work/")),
                PathBuf::from("~/code"),
                PathBuf::from(format!("{HOME}//work")),
                home().join("code"),
            ],
            ..Settings::default()
        };

        let kept = open(file).set(written).unwrap();

        let spelled: Vec<_> = kept.roots.iter().map(|root| root.as_os_str()).collect();
        let (work, code) = (home().join("work"), home().join("code"));
        assert_eq!(spelled, [work.as_os_str(), code.as_os_str()]);
    }

    #[test]
    fn turns_down_what_cant_be_used_and_keeps_what_was_in_force() {
        let (_folder, file) = place();
        let store = open(file.clone());
        let with = |change: fn(&mut Settings)| {
            let mut settings = Settings::default();
            change(&mut settings);
            store.set(settings)
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
            with(|s| s.roots = vec![PathBuf::from(format!("{HOME}/../../etc"))]),
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
            with(|s| s.roots = vec![PathBuf::from(format!("{HOME}/{}", "a".repeat(1100)))]),
            Err(Invalid::Root { .. })
        ));
        // Another share, drive or way of writing the drive isn't the home's.
        #[cfg(windows)]
        for elsewhere in [
            r"\\server\share\code",
            r"\\?\C:\Users\dev\work",
            r"D:\Users\dev\work",
        ] {
            let settings = Settings {
                roots: vec![PathBuf::from(elsewhere)],
                ..Settings::default()
            };
            assert!(matches!(store.set(settings), Err(Invalid::Root { .. })));
        }
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
    fn counts_a_list_as_it_was_sent() {
        let (_folder, file) = place();
        let store = open(file);

        let names = store.set(Settings {
            hidden: vec!["serena".into(); 65],
            ..Settings::default()
        });
        let folders = store.set(Settings {
            roots: vec![PathBuf::from("~/code"); 65],
            ..Settings::default()
        });

        assert_eq!(names, Err(Invalid::TooMany));
        assert_eq!(folders, Err(Invalid::TooMany));
    }

    #[test]
    fn changes_nothing_when_the_settings_cant_be_written() {
        let (folder, _file) = place();
        // A file where the settings' folder would go.
        let blocked = folder.path().join("blocked");
        fs::write(&blocked, b"").unwrap();
        let store = open(blocked.join("settings.json"));

        let refused = store.set(idle(7));

        assert!(matches!(refused, Err(Invalid::Unsaved { .. })));
        assert_eq!(store.get(), Settings::default());
    }

    #[test]
    fn tells_a_window_what_is_in_force_and_then_each_change() {
        let (_folder, file) = place();
        let store = open(file);
        let heard = Arc::new(Mutex::new(Vec::new()));
        let ear = Arc::clone(&heard);
        store.subscribe("panel", move |settings| {
            ear.lock().unwrap().push(settings.idle_days);
            true
        });

        store.set(idle(14)).unwrap();
        let _ = store.set(idle(0));

        assert_eq!(*heard.lock().unwrap(), [30, 14]);
    }

    #[test]
    fn tells_a_window_that_subscribed_again_only_the_new_way() {
        let (_folder, file) = place();
        let store = open(file);
        let before = Arc::new(AtomicUsize::new(0));
        let after = Arc::new(AtomicUsize::new(0));
        let (old, new) = (Arc::clone(&before), Arc::clone(&after));
        store.subscribe("panel", move |_| {
            old.fetch_add(1, Ordering::SeqCst) < usize::MAX
        });
        store.subscribe("panel", move |_| {
            new.fetch_add(1, Ordering::SeqCst) < usize::MAX
        });

        store.set(idle(14)).unwrap();

        assert_eq!(before.load(Ordering::SeqCst), 1);
        assert_eq!(after.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn stops_telling_a_window_that_cant_be_told() {
        let (_folder, file) = place();
        let store = open(file);
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        // Told what is in force, then gone before the first change.
        store.subscribe("cleaner", move |_| {
            counted.fetch_add(1, Ordering::SeqCst) == 0
        });

        store.set(idle(14)).unwrap();
        store.set(idle(15)).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
