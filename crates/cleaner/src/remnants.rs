//! What an app removed from here may have left in AppData: a folder right
//! inside Roaming, Local or LocalLow named like an app whose uninstaller
//! Harbormaster started and that the installed apps no longer list. Named
//! like is all that can be told, so a folder named like something still
//! installed, running or on the PATH stays out of sight, none is ever
//! picked, and each goes to the Recycle Bin.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::walk::{self, Dir};
use crate::{Candidate, Group, Kind, catalog};

/// An app as its uninstall entry names it: what a folder it left would be
/// named after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub publisher: Option<String>,
    /// Where it was installed and what ran it, as its entry said.
    pub folders: Vec<PathBuf>,
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

/// The folders of the home folder apps keep their data in.
const ROOTS: &[&[&str]] = &[
    &["AppData", "Roaming"],
    &["AppData", "Local"],
    &["AppData", "LocalLow"],
];
/// Where the packaged apps keep their data, each in a folder named after
/// its family, `Vendor.App_publisherid`, and where apps installed for this
/// user alone are kept.
const PACKAGES: &[&str] = &["AppData", "Local", "Packages"];
const PROGRAMS: &[&str] = &["AppData", "Local", "Programs"];
/// Folders in them that are Windows' own, or the user's, whatever they are
/// named like.
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
];
/// What ends a file's name rather than a folder's, in a path that can't be
/// looked at any more.
const FILES: &[&str] = &[
    "exe", "ico", "dll", "lnk", "msi", "bat", "cmd", "com", "url",
];

/// The folders right inside each AppData folder of `root`, the home folder
/// at `home`, named like one of `apps.removed` and like nothing still
/// installed, each as a candidate to go to the Recycle Bin.
pub(crate) fn candidates(
    root: &Dir,
    home: &Path,
    apps: &Apps,
    busy: &[(PathBuf, String)],
    stop: &AtomicBool,
) -> Vec<Candidate> {
    if apps.removed.is_empty() {
        return Vec::new();
    }
    let owners = owners(root, apps, busy, stop);
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
            let Some(app) = apps.removed.iter().find(|app| names_of(app).contains(&key)) else {
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
fn owners(
    root: &Dir,
    apps: &Apps,
    busy: &[(PathBuf, String)],
    stop: &AtomicBool,
) -> HashSet<String> {
    let mut owners: HashSet<String> = OWNERS.iter().map(|owner| (*owner).to_owned()).collect();
    for app in &apps.installed {
        owners.extend(names_of(app));
    }
    for name in &apps.names {
        owners.extend(names_in(name));
    }
    for (path, who) in busy {
        owners.extend(names_in(who));
        owners.extend(names_along(path));
    }
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

        let found = candidates(&root, &home, &apps, &[], &AtomicBool::new(false));

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

        let found = candidates(&root, &home, &apps, &busy, &AtomicBool::new(false));

        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].relative,
            ["AppData", "Roaming", "Zed"].iter().collect::<PathBuf>()
        );
    }
}
