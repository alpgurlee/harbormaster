//! What an app removed from here may have left behind: on Windows a folder
//! right inside AppData's Roaming, Local or LocalLow named like an app whose
//! uninstaller Harbormaster started and that the installed apps no longer
//! list; on a Mac an entry in one of the Library's folders carrying the
//! bundle identifier of an app moved to the Trash from here, or named like
//! it. Named like is all that can be told, so an entry named like something
//! still installed, running or on the PATH stays out of sight, none is ever
//! picked, and each goes to the Trash or the Recycle Bin.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::walk::{self, Dir};
use crate::{Candidate, Group, Kind, catalog};

/// An app as the system names it: what an entry it left would be named after.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub publisher: Option<String>,
    /// Where it was installed and what ran it, as its entry said.
    pub folders: Vec<PathBuf>,
    /// Its bundle identifiers, lowercase, as a Mac names an app's data.
    pub ids: Vec<String>,
}

/// What a scan knows of the PC's apps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Apps {
    /// Apps whose uninstaller was started from here, gone from the
    /// installed apps since.
    pub removed: Vec<App>,
    /// Every app the PC lists as installed, parts of Windows included.
    pub installed: Vec<App>,
    /// Other names apps go by: the programs App Paths lists, and the folders
    /// and shortcuts of Program Files and the Start Menu.
    pub names: Vec<String>,
}

/// The folders of the home folder apps keep their data in, on Windows.
#[cfg_attr(not(windows), allow(dead_code))]
const ROOTS: &[&[&str]] = &[
    &["AppData", "Roaming"],
    &["AppData", "Local"],
    &["AppData", "LocalLow"],
];
/// Where the packaged apps keep their data, each in a folder named after
/// its family, `Vendor.App_publisherid`, and where apps installed for this
/// user alone are kept.
#[cfg_attr(not(windows), allow(dead_code))]
const PACKAGES: &[&str] = &["AppData", "Local", "Packages"];
#[cfg_attr(not(windows), allow(dead_code))]
const PROGRAMS: &[&str] = &["AppData", "Local", "Programs"];
/// Folders in them that are Windows' own, or the user's, whatever they are
/// named like.
#[cfg_attr(not(windows), allow(dead_code))]
const NEVER: &[&str] = &[
    "Application Data",
    "Comms",
    "ConnectedDevicesPlatform",
    "Cookies",
    "CrashDumps",
    "D3DSCache",
    "History",
    "INetCache",
    "INetCookies",
    "IsolatedStorage",
    "Local Settings",
    "Microsoft",
    "Microsoft_Corporation",
    "Packages",
    "PeerDistRepub",
    "PlaceholderTileLogoFolder",
    "Programs",
    "PublisherCache",
    "Publishers",
    "Recent",
    "Saved Games",
    "Searches",
    "SendTo",
    "SquirrelTemp",
    "Start Menu",
    "Temp",
    "Templates",
    "TileDataLayer",
    "VirtualStore",
    "WebCache",
    "WindowsApps",
];
/// Tools that keep data in AppData without an uninstall entry, a shortcut
/// or a folder in Program Files to say so, and apps often run without being
/// installed, which nothing lists either.
const OWNERS: &[&str] = &[
    "npm",
    "nvm",
    "pnpm",
    "yarn",
    "bun",
    "pip",
    "uv",
    "poetry",
    "pypoetry",
    "cargo",
    "rustup",
    "gradle",
    "maven",
    "nuget",
    "composer",
    "deno",
    "jetbrains",
    "toolbox",
    "scoop",
    "githubcli",
    "gitcredentialmanager",
    "gnupg",
    "rclone",
    "gcloud",
    "helm",
    "terraform",
    "keepass",
    "putty",
    "filezilla",
    "winscp",
    "obsidian",
    "signal",
    "telegram",
];
/// Words that name no app on their own.
const STOP: &[&str] = &[
    "the",
    "and",
    "for",
    "app",
    "apps",
    "application",
    "applications",
    "data",
    "software",
    "update",
    "updates",
    "updater",
    "setup",
    "install",
    "installer",
    "uninstall",
    "launcher",
    "service",
    "services",
    "tool",
    "tools",
    "bin",
    "lib",
    "resources",
    "common",
    "shared",
    "local",
    "locallow",
    "roaming",
    "appdata",
    "program",
    "programs",
    "files",
    "windows",
    "microsoft",
    "system",
    "user",
    "users",
    "default",
    "temp",
    "cache",
    "caches",
    "config",
    "settings",
    "desktop",
    "client",
    "edition",
    "version",
    "x64",
    "x86",
    "64bit",
    "32bit",
    "win64",
    "bit",
    "win32",
    "inc",
    "llc",
    "ltd",
    "limited",
    "corp",
    "corporation",
    "gmbh",
    "co",
    "company",
    "technologies",
    "technology",
    "labs",
    "foundation",
    "team",
    "community",
    "enterprise",
    "professional",
    "pro",
    "free",
    "lite",
    "beta",
    "preview",
    "portable",
    "stable",
    "release",
    "current",
    "contents",
    "macos",
    "frameworks",
    "helpers",
];
/// Folders in a path below which an app's own folders begin.
const MARKERS: &[&str] = &[
    "program files",
    "program files (x86)",
    "programs",
    "local",
    "roaming",
    "locallow",
    "appdata",
    "applications",
    "application support",
    "library",
];
/// What ends a file's name rather than a folder's, in a path that can't be
/// looked at any more.
const FILES: &[&str] = &[
    "exe", "ico", "dll", "lnk", "msi", "bat", "cmd", "com", "url",
];

/// The folders right inside each AppData folder of `root`, the home folder
/// at `home`, named like one of `apps.removed` and like nothing still
/// installed, each as a candidate to go to the Recycle Bin.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn in_app_data(
    root: &Dir,
    home: &Path,
    apps: &Apps,
    busy: &[(PathBuf, String)],
    stop: &AtomicBool,
) -> Vec<Candidate> {
    if apps.removed.is_empty() {
        return Vec::new();
    }
    let owners = app_data_owners(root, apps, busy, stop);
    let removed = gone(&apps.removed, &owners);
    let mut found = Vec::new();
    for place in ROOTS {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let relative: PathBuf = place.iter().collect();
        let Ok(dir) = root.descend(&relative) else {
            continue;
        };
        let Ok(entries) = dir.entries_while(|| !stop.load(Ordering::Relaxed)) else {
            continue;
        };
        for (name, stat) in entries {
            if !walk::is_folder(&stat) || passed_over(&name, &stat) {
                continue;
            }
            let relative = relative.join(&name);
            if catalog::claimed(&relative) {
                continue;
            }
            let key = key_of(&name.to_string_lossy());
            if key.is_empty() || owners.contains(&key) {
                continue;
            }
            let Some(app) = removed.iter().find(|app| names_of(app).contains(&key)) else {
                continue;
            };
            found.push(Candidate {
                group: Group::Remnants,
                kind: Kind::Personal,
                name: format!("{} · {}", app.name, name.to_string_lossy()),
                path: home.join(&relative),
                relative,
                picked: false,
                in_use: None,
                project: None,
                touched: None,
                emptied: false,
            });
        }
    }
    found
}

/// What is never a candidate: a hidden or system folder, a link, or one of
/// Windows' own and the user's folders.
#[cfg_attr(not(windows), allow(dead_code))]
fn passed_over(name: &OsStr, stat: &walk::Stat) -> bool {
    #[cfg(windows)]
    if stat.hidden {
        return true;
    }
    walk::is_link(stat) || NEVER.iter().any(|never| walk::is_named(name, never))
}

/// The names everything still on the PC goes by: the installed apps and
/// what else names them, the programs running and where they are, what is
/// on the PATH, the packaged apps, the apps in the user's Programs folder,
/// and the tools known to leave no other trace.
#[cfg_attr(not(windows), allow(dead_code))]
fn app_data_owners(
    root: &Dir,
    apps: &Apps,
    busy: &[(PathBuf, String)],
    stop: &AtomicBool,
) -> HashSet<String> {
    let mut owners = owner_keys(apps, busy);
    for path in std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
    {
        owners.extend(names_along(&path));
    }
    for (folder, family) in [(PACKAGES, true), (PROGRAMS, false)] {
        let Ok(dir) = root.descend(&folder.iter().collect::<PathBuf>()) else {
            continue;
        };
        let Ok(entries) = dir.entries_while(|| !stop.load(Ordering::Relaxed)) else {
            continue;
        };
        for (name, _) in entries {
            let name = name.to_string_lossy();
            // A package's folder is its family name: `Vendor.App_publisherid`.
            let name = if family {
                name.split('_').next().unwrap_or_default()
            } else {
                &name
            };
            owners.extend(names_in(name));
        }
    }
    owners.remove("");
    owners
}

/// Of `removed`, the apps nothing in `owners` still goes by: one named
/// exactly like, or with the identifier of, something still installed or
/// running isn't gone, whatever its entry says.
fn gone<'a>(removed: &'a [App], owners: &HashSet<String>) -> Vec<&'a App> {
    removed
        .iter()
        .filter(|app| {
            !owners.contains(&key_of(&app.name)) && !app.ids.iter().any(|id| owners.contains(id))
        })
        .collect()
}

/// The names everything the scan was told of goes by: the installed apps
/// and what else names them, the programs running and where they are, and
/// the tools known to leave no other trace.
fn owner_keys(apps: &Apps, busy: &[(PathBuf, String)]) -> HashSet<String> {
    let mut owners: HashSet<String> = OWNERS.iter().map(|owner| (*owner).to_owned()).collect();
    for app in &apps.installed {
        owners.extend(names_of(app));
        owners.extend(app.ids.iter().map(|id| id.to_lowercase()));
    }
    for name in &apps.names {
        owners.extend(names_in(name));
    }
    for (path, who) in busy {
        owners.extend(names_in(who));
        owners.extend(names_along(path));
    }
    owners.remove("");
    owners
}

/// The names a folder `app` left would go by: its name, its publisher, and
/// the folders of its own it was installed in and ran from, each as a key,
/// and the first and last words of its name on their own.
pub(crate) fn names_of(app: &App) -> HashSet<String> {
    let mut names = names_in(&app.name);
    if let Some(publisher) = &app.publisher {
        names.insert(key_of(publisher));
    }
    for folder in &app.folders {
        names.extend(names_along(folder));
    }
    names.remove("");
    names
}

/// `name` as a key, and its first and last words when long enough to name
/// an app on their own.
fn names_in(name: &str) -> HashSet<String> {
    let words = words(name);
    let mut names = HashSet::new();
    names.insert(words.concat());
    for word in [words.first(), words.last()].into_iter().flatten() {
        if word.len() >= 4 {
            names.insert(word.clone());
        }
    }
    names.remove("");
    names
}

/// The keys of the folders of `path` below where an app's own begin, or of
/// its last folder; a file's name at the end isn't one.
fn names_along(path: &Path) -> HashSet<String> {
    let folders: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let folders = match folders.last() {
        Some(last) if is_file_name(last) => &folders[..folders.len() - 1],
        _ => &folders[..],
    };
    let marked = folders
        .iter()
        .rposition(|folder| MARKERS.contains(&folder.to_lowercase().as_str()));
    let own = match marked {
        Some(at) => &folders[at + 1..],
        None => folders.last().map(std::slice::from_ref).unwrap_or_default(),
    };
    own.iter()
        .map(|folder| key_of(folder))
        .filter(|key| !key.is_empty())
        .collect()
}

fn is_file_name(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        FILES
            .iter()
            .any(|file| extension.eq_ignore_ascii_case(file))
    })
}

/// `name` as the key folders and apps are matched by: its words run
/// together, lowercase, without what names no app.
pub(crate) fn key_of(name: &str) -> String {
    words(name).concat()
}

/// The words of `name` that may name an app, lowercase: split where its
/// letters stop and where a capital follows a lowercase letter, as in
/// `EpicGames`, leaving out numbers, single letters and stop words.
fn words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut last_lower = false;
    for character in name.chars() {
        if character.is_alphanumeric() {
            if character.is_uppercase() && last_lower {
                words.push(std::mem::take(&mut word));
            }
            word.extend(character.to_lowercase());
            last_lower = character.is_lowercase();
        } else {
            words.push(std::mem::take(&mut word));
            last_lower = false;
        }
    }
    words.push(word);
    words.retain(|word| {
        word.chars().count() > 1
            && !word.chars().all(|character| character.is_ascii_digit())
            && !STOP.contains(&word.as_str())
    });
    words
}

/// A folder of the Library apps keep a kind of data in, and how an entry
/// there is named after its app.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Shelf {
    folder: &'static [&'static str],
    /// What an entry's name ends in beyond the app's identifier, if anything.
    suffixes: &'static [&'static str],
    /// Whether an entry may be named after the app's name, not only its identifier.
    by_name: bool,
    /// Entries that are macOS's own, or the user's, whatever they are named like.
    never: &'static [&'static str],
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const LIBRARY: &[Shelf] = &[
    Shelf {
        folder: &["Library", "Application Support"],
        suffixes: &[],
        by_name: true,
        never: &[
            "AddressBook",
            "App Store",
            "Apple",
            "CallHistoryDB",
            "CallHistoryTransactions",
            "CloudDocs",
            "CrashReporter",
            "Dock",
            "FileProvider",
            "iCloud",
            "icdd",
            "Knowledge",
            "MobileSync",
            "Script Editor",
            "SyncServices",
        ],
    },
    Shelf {
        folder: &["Library", "Caches"],
        suffixes: &[],
        by_name: false,
        never: &["CloudKit", "FamilyCircle", "GeoServices", "Metadata"],
    },
    Shelf {
        folder: &["Library", "HTTPStorages"],
        suffixes: &[".binarycookies"],
        by_name: false,
        never: &[],
    },
    Shelf {
        folder: &["Library", "WebKit"],
        suffixes: &[],
        by_name: false,
        never: &[],
    },
    Shelf {
        folder: &["Library", "Cookies"],
        suffixes: &[".binarycookies"],
        by_name: false,
        never: &[],
    },
    Shelf {
        folder: &["Library", "Saved Application State"],
        suffixes: &[".savedState"],
        by_name: false,
        never: &[],
    },
    Shelf {
        folder: &["Library", "Preferences"],
        suffixes: &[".plist"],
        by_name: false,
        never: &[],
    },
    Shelf {
        folder: &["Library", "Logs"],
        suffixes: &[],
        by_name: true,
        never: &["DiagnosticReports", "CoreSimulator"],
    },
];
/// What Apple's own identifiers start with; nothing so named is ever listed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const APPLE: &str = "com.apple.";
/// Where apps are kept on a Mac, besides the user's own Applications folder.
#[cfg(target_os = "macos")]
const SHELVES: &[&str] = &["/Applications"];

/// The entries right inside each Library folder of `root`, the home folder
/// at `home`, carrying the identifier of one of `apps.removed` or, where an
/// app names its own folder, named exactly like one, and like nothing in
/// `owners`, each as a candidate to go to the Trash.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn in_library(
    root: &Dir,
    home: &Path,
    apps: &Apps,
    owners: &HashSet<String>,
    stop: &AtomicBool,
) -> Vec<Candidate> {
    let removed = gone(&apps.removed, owners);
    if removed.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for shelf in LIBRARY {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let relative: PathBuf = shelf.folder.iter().collect();
        let Ok(dir) = root.descend(&relative) else {
            continue;
        };
        let Ok(entries) = dir.entries_while(|| !stop.load(Ordering::Relaxed)) else {
            continue;
        };
        for (name, stat) in entries {
            if !(walk::is_folder(&stat) || walk::is_file(&stat)) {
                continue;
            }
            let text = name.to_string_lossy();
            if text.starts_with('.')
                || shelf
                    .never
                    .iter()
                    .any(|never| never.eq_ignore_ascii_case(&text))
            {
                continue;
            }
            let bare = shelf
                .suffixes
                .iter()
                .find_map(|suffix| strip_suffix_ignore_case(&text, suffix))
                .unwrap_or(&text);
            let id = bare.to_lowercase();
            if id.starts_with(APPLE) {
                continue;
            }
            let relative = relative.join(&name);
            if catalog::claimed(&relative) {
                continue;
            }
            let key = key_of(bare);
            if owners.contains(&id) || (!key.is_empty() && owners.contains(&key)) {
                continue;
            }
            let Some(app) = removed.iter().find(|app| {
                app.ids.contains(&id)
                    || (shelf.by_name && !key.is_empty() && key_of(&app.name) == key)
            }) else {
                continue;
            };
            found.push(Candidate {
                group: Group::Remnants,
                kind: Kind::Personal,
                name: format!("{} · {}", app.name, text),
                path: home.join(&relative),
                relative,
                picked: false,
                in_use: None,
                project: None,
                touched: None,
                emptied: false,
            });
        }
    }
    found
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let cut = text.len().checked_sub(suffix.len())?;
    text.is_char_boundary(cut)
        .then(|| text.split_at(cut))
        .filter(|(_, end)| end.eq_ignore_ascii_case(suffix))
        .map(|(start, _)| start)
}

/// The names everything still on this Mac goes by: what the scan was told
/// of, every app bundle in the Applications folders and one folder down,
/// by name and identifier, and the agents launched for this user.
#[cfg(target_os = "macos")]
pub(crate) fn library_owners(
    home: &Path,
    apps: &Apps,
    busy: &[(PathBuf, String)],
) -> HashSet<String> {
    let mut owners = owner_keys(apps, busy);
    let shelves = SHELVES
        .iter()
        .map(PathBuf::from)
        .chain([home.join("Applications")]);
    for shelf in shelves {
        for bundle in bundles_in(&shelf, 2) {
            if let Some(stem) = bundle.file_stem() {
                owners.extend(names_in(&stem.to_string_lossy()));
            }
            // One whose Info.plist can't be read counts by its name alone.
            let info = std::fs::read(bundle.join("Contents/Info.plist")).unwrap_or_default();
            owners.extend(bundle_identifier(&info));
        }
    }
    for agent in std::fs::read_dir(home.join("Library/LaunchAgents"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = agent.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "plist")
        {
            continue;
        }
        owners.extend(agent_names(&path));
    }
    owners.remove("");
    owners
}

/// What the launch agent at `path` goes by: its label and what it runs, or
/// its file's name when it can't be read.
#[cfg(target_os = "macos")]
fn agent_names(path: &Path) -> HashSet<String> {
    let mut names = HashSet::new();
    let Some(agent) = plist::Value::from_file(path)
        .ok()
        .and_then(|value| value.into_dictionary())
    else {
        names.extend(
            path.file_stem()
                .map(|stem| stem.to_string_lossy().to_lowercase()),
        );
        return names;
    };
    if let Some(label) = agent.get("Label").and_then(plist::Value::as_string) {
        names.insert(label.to_lowercase());
    }
    let program = agent
        .get("Program")
        .and_then(plist::Value::as_string)
        .or_else(|| {
            agent
                .get("ProgramArguments")
                .and_then(plist::Value::as_array)
                .and_then(|arguments| arguments.first())
                .and_then(plist::Value::as_string)
        });
    if let Some(program) = program.map(Path::new) {
        names.extend(names_along(program));
        if let Some(stem) = program.file_stem() {
            names.extend(names_in(&stem.to_string_lossy()));
        }
    }
    names
}

/// The app bundles right inside `folder`, and `levels` folders down.
#[cfg(target_os = "macos")]
fn bundles_in(folder: &Path, levels: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        // A link isn't followed: an alias to an app elsewhere isn't the app.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() {
            continue;
        }
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        {
            found.push(path);
        } else if levels > 1 {
            found.extend(bundles_in(&path, levels - 1));
        }
    }
    found
}

/// The bundle identifier in `info`, an Info.plist's bytes, lowercase.
#[cfg(target_os = "macos")]
fn bundle_identifier(info: &[u8]) -> Option<String> {
    plist::Value::from_reader(std::io::Cursor::new(info))
        .ok()?
        .into_dictionary()?
        .get("CFBundleIdentifier")?
        .as_string()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_lowercase)
}

/// `finding`, an app bundle listed by `reach`, as the app it is: its name
/// and its bundle identifier, read from its own Info.plist through real
/// folders; none when the bundle or its identifier can't be read.
#[cfg(target_os = "macos")]
pub(crate) fn identity(reach: &crate::Reach, finding: &crate::Finding) -> Option<App> {
    let at = reach
        .shared
        .filter(|shared| finding.path.starts_with(shared))
        .unwrap_or(reach.home);
    let relative = finding.path.strip_prefix(at).ok()?;
    let bundle = Dir::open(at).ok()?.descend(relative).ok()?;
    let info = bundle
        .child(OsStr::new("Contents"))
        .ok()?
        .read_start(OsStr::new("Info.plist"), 4 << 20)
        .ok()?;
    Some(App {
        name: finding.name.clone(),
        ids: vec![bundle_identifier(&info)?],
        ..App::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, publisher: Option<&str>, folders: &[&[&str]]) -> App {
        App {
            name: name.to_owned(),
            publisher: publisher.map(str::to_owned),
            folders: folders
                .iter()
                .map(|folder| folder.iter().collect())
                .collect(),
            ids: Vec::new(),
        }
    }

    fn mac_app(name: &str, id: &str) -> App {
        App {
            name: name.to_owned(),
            ids: vec![id.to_owned()],
            ..App::default()
        }
    }

    fn sorted(names: HashSet<String>) -> Vec<String> {
        let mut names: Vec<_> = names.into_iter().collect();
        names.sort();
        names
    }

    #[test]
    fn keys_a_name_by_its_words_without_what_names_no_app() {
        assert_eq!(key_of("Docker Desktop"), "docker");
        assert_eq!(key_of("Docker Inc."), "docker");
        assert_eq!(key_of("JetBrains s.r.o."), "jetbrains");
        assert_eq!(key_of("Python 3.12.1 (64-bit)"), "python");
        assert_eq!(key_of("EpicGamesLauncher"), "epicgames");
        assert_eq!(key_of("Epic Games"), "epicgames");
        assert_eq!(key_of("GitHub CLI"), "githubcli");
        assert_eq!(
            key_of("Microsoft Visual C++ 2015 Redistributable"),
            "visualredistributable"
        );
        assert_eq!(key_of("Temp"), "");
    }

    #[test]
    fn names_an_app_by_its_entry_and_the_folders_of_its_own() {
        let names = names_of(&app(
            "Google Chrome",
            Some("Google LLC"),
            &[
                &["C:", "Program Files", "Google", "Chrome", "Application"],
                &[
                    "C:",
                    "Program Files",
                    "Google",
                    "Chrome",
                    "Application",
                    "chrome.exe",
                ],
            ],
        ));

        assert_eq!(sorted(names), ["chrome", "google", "googlechrome"]);
    }

    #[test]
    fn takes_the_folders_below_the_program_folders_or_else_the_last() {
        let along = |path: &[&str]| sorted(names_along(&path.iter().collect::<PathBuf>()));

        assert_eq!(
            along(&[
                "C:",
                "Users",
                "dev",
                "AppData",
                "Local",
                "Programs",
                "cursor",
                "Cursor.exe"
            ]),
            ["cursor"]
        );
        assert_eq!(
            along(&[
                "C:",
                "Program Files (x86)",
                "Epic Games",
                "Launcher",
                "Portal",
                "Win64"
            ]),
            ["epicgames", "portal"]
        );
        assert_eq!(along(&["D:", "Tools", "Blender 4.3"]), ["blender"]);
        assert_eq!(along(&[]), Vec::<String>::new());
    }

    fn home_with(folders: &[&[&str]]) -> (tempfile::TempDir, PathBuf) {
        let place = tempfile::tempdir().unwrap();
        let home = walk::canonical(place.path()).unwrap();
        for folder in folders {
            std::fs::create_dir_all(home.join(folder.iter().collect::<PathBuf>())).unwrap();
        }
        (place, home)
    }

    #[test]
    fn lists_what_a_removed_app_may_have_left_and_nothing_still_owned() {
        let (_place, home) = home_with(&[
            &["AppData", "Roaming", "Docker"],
            &["AppData", "Local", "Docker Desktop"],
            &["AppData", "Roaming", "Google"],
            &["AppData", "Roaming", "Microsoft"],
            &["AppData", "Local", "Programs", "cursor"],
            &["AppData", "Local", "Packages", "Vendor.Other_8wekyb3d8bbwe"],
            &["AppData", "Local", "Other"],
            &["AppData", "Roaming", "npm"],
            &["AppData", "LocalLow", "Docker"],
        ]);
        let apps = Apps {
            removed: vec![
                app(
                    "Docker Desktop",
                    Some("Docker Inc."),
                    &[&["C:", "Program Files", "Docker", "Docker"]],
                ),
                app("Google Chrome", Some("Google LLC"), &[]),
                app("npm", None, &[]),
                app("Other", None, &[]),
                app("Cursor", Some("Anysphere"), &[]),
            ],
            installed: vec![app("Google Drive", Some("Google LLC"), &[])],
            names: vec!["Other.exe".to_owned()],
        };
        let root = Dir::open(&home).unwrap();

        let found = in_app_data(&root, &home, &apps, &[], &AtomicBool::new(false));

        let mut listed: Vec<(String, PathBuf)> = found
            .iter()
            .map(|candidate| (candidate.name.clone(), candidate.relative.clone()))
            .collect();
        listed.sort();
        let at = |folder: &[&str]| folder.iter().collect::<PathBuf>();
        assert_eq!(
            listed,
            [
                (
                    "Docker Desktop · Docker".to_owned(),
                    at(&["AppData", "LocalLow", "Docker"])
                ),
                (
                    "Docker Desktop · Docker".to_owned(),
                    at(&["AppData", "Roaming", "Docker"])
                ),
                (
                    "Docker Desktop · Docker Desktop".to_owned(),
                    at(&["AppData", "Local", "Docker Desktop"])
                ),
            ]
        );
        assert!(found.iter().all(|candidate| {
            candidate.group == Group::Remnants
                && candidate.kind == Kind::Personal
                && !candidate.picked
                && candidate.path == home.join(&candidate.relative)
        }));
    }

    #[test]
    fn leaves_out_what_a_running_program_goes_by() {
        let (_place, home) = home_with(&[
            &["AppData", "Roaming", "Slack"],
            &["AppData", "Roaming", "Zed"],
        ]);
        let apps = Apps {
            removed: vec![app("Slack", None, &[]), app("Zed", None, &[])],
            ..Apps::default()
        };
        let root = Dir::open(&home).unwrap();
        let busy = [(home.join("somewhere"), "slack".to_owned())];

        let found = in_app_data(&root, &home, &apps, &busy, &AtomicBool::new(false));

        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].relative,
            ["AppData", "Roaming", "Zed"].iter().collect::<PathBuf>()
        );
    }

    #[test]
    fn lists_what_a_trashed_app_left_in_the_library_by_identifier_and_exact_name() {
        let (_place, home) = home_with(&[
            &["Library", "Application Support", "Docker Desktop"],
            &["Library", "Application Support", "Google"],
            &["Library", "Application Support", "com.apple.TCC"],
            &["Library", "Application Support", "Slack"],
            &["Library", "Caches", "com.docker.docker"],
            &["Library", "Caches", "com.google.Chrome"],
            &["Library", "Caches", "com.tinyspeck.slackmacgap"],
            &[
                "Library",
                "Saved Application State",
                "com.docker.docker.savedState",
            ],
            &["Library", "HTTPStorages", "com.docker.docker"],
            &["Library", "Logs", "Docker Desktop"],
            &["Library", "Preferences"],
        ]);
        for file in ["com.docker.docker.plist", "com.google.Chrome.plist"] {
            std::fs::write(home.join("Library/Preferences").join(file), b"bplist").unwrap();
        }
        let apps = Apps {
            removed: vec![
                mac_app("Docker Desktop", "com.docker.docker"),
                mac_app("Google Chrome", "com.google.chrome"),
                mac_app("Slack", "com.tinyspeck.slackmacgap"),
            ],
            installed: vec![mac_app("Google Drive", "com.google.drivefs")],
            names: Vec::new(),
        };
        let root = Dir::open(&home).unwrap();
        // Slack runs on: whatever is named like it stays.
        let busy = [(home.join("somewhere"), "Slack".to_owned())];
        let owners = owner_keys(&apps, &busy);

        let found = in_library(&root, &home, &apps, &owners, &AtomicBool::new(false));

        let mut listed: Vec<(String, PathBuf)> = found
            .iter()
            .map(|candidate| (candidate.name.clone(), candidate.relative.clone()))
            .collect();
        listed.sort();
        let at = |folder: &[&str]| folder.iter().collect::<PathBuf>();
        assert_eq!(
            listed,
            [
                (
                    "Docker Desktop · Docker Desktop".to_owned(),
                    at(&["Library", "Application Support", "Docker Desktop"])
                ),
                (
                    "Docker Desktop · Docker Desktop".to_owned(),
                    at(&["Library", "Logs", "Docker Desktop"])
                ),
                (
                    "Docker Desktop · com.docker.docker".to_owned(),
                    at(&["Library", "Caches", "com.docker.docker"])
                ),
                (
                    "Docker Desktop · com.docker.docker".to_owned(),
                    at(&["Library", "HTTPStorages", "com.docker.docker"])
                ),
                (
                    "Docker Desktop · com.docker.docker.plist".to_owned(),
                    at(&["Library", "Preferences", "com.docker.docker.plist"])
                ),
                (
                    "Docker Desktop · com.docker.docker.savedState".to_owned(),
                    at(&[
                        "Library",
                        "Saved Application State",
                        "com.docker.docker.savedState"
                    ])
                ),
                (
                    "Google Chrome · com.google.Chrome".to_owned(),
                    at(&["Library", "Caches", "com.google.Chrome"])
                ),
                (
                    "Google Chrome · com.google.Chrome.plist".to_owned(),
                    at(&["Library", "Preferences", "com.google.Chrome.plist"])
                ),
            ]
        );
        assert!(found.iter().all(|candidate| {
            candidate.group == Group::Remnants
                && candidate.kind == Kind::Personal
                && !candidate.picked
        }));
    }

    #[test]
    fn cuts_only_the_suffix_an_entry_is_named_with() {
        assert_eq!(
            strip_suffix_ignore_case("com.docker.docker.savedState", ".savedState"),
            Some("com.docker.docker")
        );
        assert_eq!(
            strip_suffix_ignore_case("com.docker.docker.PLIST", ".plist"),
            Some("com.docker.docker")
        );
        assert_eq!(strip_suffix_ignore_case("Docker", ".plist"), None);
        assert_eq!(strip_suffix_ignore_case("", ".plist"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_an_apps_identifier_from_its_own_info_plist() {
        let (_place, home) = home_with(&[&["Applications", "Foo Bar.app", "Contents"]]);
        std::fs::write(
            home.join("Applications/Foo Bar.app/Contents/Info.plist"),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.Example.FooBar</string></dict></plist>"#,
        )
        .unwrap();
        let finding = crate::Finding {
            group: Group::Applications,
            kind: Kind::Personal,
            name: "Foo Bar".to_owned(),
            path: home.join("Applications/Foo Bar.app"),
            bytes: 1,
            picked: false,
            in_use: None,
            touched: None,
            project: None,
            emptied: false,
            identity: crate::Identity::default(),
        };
        let reach = crate::Reach {
            home: &home,
            shared: None,
        };

        let app = identity(&reach, &finding).expect("the app");

        assert_eq!(app.name, "Foo Bar");
        assert_eq!(app.ids, ["com.example.foobar"]);
        assert_eq!(bundle_identifier(b"not a plist"), None);
    }
}
