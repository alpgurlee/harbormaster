//! The places a developer's machine fills with things its tools rebuild,
//! written from each tool's own documentation.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use crate::walk::{self, Dir};
use crate::{Candidate, Group, Kind};

/// A place the cleaner knows, from the home folder.
struct Place {
    group: Group,
    kind: Kind,
    name: &'static str,
    path: &'static str,
    /// Each entry inside is a finding of its own, as each archive is.
    each: bool,
    /// Of entries found one by one, the one changed last stays unpicked:
    /// the device symbols Xcode used most recently.
    keep_latest: bool,
    /// The programs that keep it in use while any of them runs, by name.
    /// Windows doesn't tell what a program has open, as macOS does, so
    /// there a tool's running is taken as its using its cache.
    used_by: &'static [&'static str],
    /// The place holds an app's profiles, each keeping a cache of its own
    /// at this name, which is all that is listed of it: a browser's.
    in_each: Option<&'static str>,
}

const fn whole(group: Group, name: &'static str, path: &'static str) -> Place {
    Place {
        group,
        kind: Kind::Rebuildable,
        name,
        path,
        each: false,
        keep_latest: false,
        used_by: &[],
        in_each: None,
    }
}

impl Place {
    #[cfg(windows)]
    const fn used_by(self, programs: &'static [&'static str]) -> Self {
        Self {
            used_by: programs,
            ..self
        }
    }

    #[cfg(windows)]
    const fn in_each(self, cache: &'static str) -> Self {
        Self {
            in_each: Some(cache),
            ..self
        }
    }
}

#[cfg(target_os = "macos")]
const PLACES: &[Place] = &[
    whole(Group::Packages, "npm", ".npm/_cacache"),
    whole(Group::Packages, "npx", ".npm/_npx"),
    whole(Group::Packages, "pnpm", "Library/pnpm/store"),
    whole(Group::Packages, "pnpm metadata", "Library/Caches/pnpm"),
    whole(Group::Packages, "Yarn", "Library/Caches/Yarn"),
    whole(Group::Packages, "Bun", ".bun/install/cache"),
    whole(Group::Packages, "pip", "Library/Caches/pip"),
    whole(Group::Packages, "uv", ".cache/uv"),
    whole(Group::Packages, "Poetry", "Library/Caches/pypoetry"),
    whole(Group::Packages, "Cargo", ".cargo/registry/cache"),
    whole(Group::Packages, "Cargo sources", ".cargo/registry/src"),
    whole(Group::Packages, "Cargo git", ".cargo/git/checkouts"),
    whole(Group::Packages, "Go build cache", "Library/Caches/go-build"),
    whole(Group::Packages, "Composer", "Library/Caches/composer"),
    // Only what pub downloaded: its bin and global_packages are tools the user activated.
    whole(Group::Packages, "Dart packages", ".pub-cache/hosted"),
    whole(Group::Packages, "Dart git packages", ".pub-cache/git"),
    // Only what Deno downloaded or compiled: location_data holds what programs stored.
    whole(
        Group::Packages,
        "Deno modules",
        "Library/Caches/deno/remote",
    ),
    whole(
        Group::Packages,
        "Deno npm packages",
        "Library/Caches/deno/npm",
    ),
    whole(
        Group::Packages,
        "Deno compiled code",
        "Library/Caches/deno/gen",
    ),
    whole(
        Group::Packages,
        "SwiftPM",
        "Library/Caches/org.swift.swiftpm",
    ),
    whole(
        Group::Packages,
        "Carthage",
        "Library/Caches/org.carthage.CarthageKit",
    ),
    // Jars `mvn install` put here can't be downloaded again; never .m2 itself, which holds settings.
    Place {
        kind: Kind::Personal,
        ..whole(Group::Packages, "Maven", ".m2/repository")
    },
    // Local .NET tools live here too, and builds that skip restoring need what it holds.
    Place {
        kind: Kind::Personal,
        ..whole(Group::Packages, "NuGet", ".nuget/packages")
    },
    whole(Group::Packages, "Gradle", ".gradle/caches"),
    whole(
        Group::Packages,
        "Gradle distributions",
        ".gradle/wrapper/dists",
    ),
    whole(Group::Packages, "CocoaPods", "Library/Caches/CocoaPods"),
    whole(Group::Packages, "Homebrew", "Library/Caches/Homebrew"),
    whole(Group::Packages, "node-gyp", "Library/Caches/node-gyp"),
    whole(Group::Packages, "Next.js SWC", "Library/Caches/next-swc"),
    whole(Group::Packages, "TypeScript", "Library/Caches/typescript"),
    whole(
        Group::Packages,
        "Playwright browsers",
        "Library/Caches/ms-playwright",
    ),
    whole(
        Group::Xcode,
        "DerivedData",
        "Library/Developer/Xcode/DerivedData",
    ),
    whole(
        Group::Xcode,
        "Simulator caches",
        "Library/Developer/CoreSimulator/Caches",
    ),
    Place {
        each: true,
        keep_latest: true,
        ..whole(
            Group::Xcode,
            "iOS",
            "Library/Developer/Xcode/iOS DeviceSupport",
        )
    },
    Place {
        each: true,
        keep_latest: true,
        ..whole(
            Group::Xcode,
            "watchOS",
            "Library/Developer/Xcode/watchOS DeviceSupport",
        )
    },
    Place {
        kind: Kind::Personal,
        each: true,
        ..whole(Group::Xcode, "Archives", "Library/Developer/Xcode/Archives")
    },
    whole(Group::Apps, "Google Chrome", "Library/Caches/Google/Chrome"),
    // Local History, which can bring back uncommitted work, lives here too.
    Place {
        kind: Kind::Personal,
        ..whole(Group::Apps, "JetBrains IDEs", "Library/Caches/JetBrains")
    },
    whole(Group::Logs, "Logs", "Library/Logs"),
    Place {
        kind: Kind::Discarded,
        ..whole(Group::Trash, "Trash", ".Trash")
    },
];

/// What each tool downloads or builds for itself, and what Maven and NuGet
/// keep that may not be had again, which goes to the Recycle Bin; apps'
/// caches, taken to be in use while the app runs, as Windows won't remove
/// what an app has open; and the dumps apps leave when they crash.
#[cfg(windows)]
const PLACES: &[Place] = &[
    // npm, npx, Yarn and node-gyp run as node, which says nothing of what it runs.
    whole(Group::Packages, "npm", r"AppData\Local\npm-cache\_cacache"),
    whole(Group::Packages, "npx", r"AppData\Local\npm-cache\_npx"),
    whole(Group::Packages, "pnpm", r"AppData\Local\pnpm\store").used_by(&["pnpm"]),
    whole(
        Group::Packages,
        "pnpm metadata",
        r"AppData\Local\pnpm-cache",
    )
    .used_by(&["pnpm"]),
    whole(Group::Packages, "Yarn", r"AppData\Local\Yarn\Cache"),
    whole(
        Group::Packages,
        "Yarn Berry",
        r"AppData\Local\Yarn\Berry\cache",
    ),
    whole(Group::Packages, "Bun", r".bun\install\cache").used_by(&["bun"]),
    whole(Group::Packages, "pip", r"AppData\Local\pip\Cache").used_by(&["pip", "pip3"]),
    whole(Group::Packages, "uv", r"AppData\Local\uv\cache").used_by(&["uv", "uvx"]),
    whole(Group::Packages, "Poetry", r"AppData\Local\pypoetry\Cache").used_by(&["poetry"]),
    whole(Group::Packages, "Cargo", r".cargo\registry\cache").used_by(CARGO),
    whole(Group::Packages, "Cargo sources", r".cargo\registry\src").used_by(CARGO),
    whole(Group::Packages, "Cargo git", r".cargo\git\checkouts").used_by(CARGO),
    whole(Group::Packages, "Go build cache", r"AppData\Local\go-build").used_by(&["go"]),
    whole(Group::Packages, "Composer", r"AppData\Local\Composer").used_by(&["composer"]),
    // Only what pub downloaded: its bin and global_packages are tools the user activated.
    whole(
        Group::Packages,
        "Dart packages",
        r"AppData\Local\Pub\Cache\hosted",
    )
    .used_by(DART),
    whole(
        Group::Packages,
        "Dart git packages",
        r"AppData\Local\Pub\Cache\git",
    )
    .used_by(DART),
    // Only what Deno downloaded or compiled: location_data holds what programs stored.
    whole(
        Group::Packages,
        "Deno modules",
        r"AppData\Local\deno\remote",
    )
    .used_by(DENO),
    whole(
        Group::Packages,
        "Deno npm packages",
        r"AppData\Local\deno\npm",
    )
    .used_by(DENO),
    whole(
        Group::Packages,
        "Deno compiled code",
        r"AppData\Local\deno\gen",
    )
    .used_by(DENO),
    // Jars `mvn install` put here can't be downloaded again; never .m2 itself, which holds settings.
    Place {
        kind: Kind::Personal,
        ..whole(Group::Packages, "Maven", r".m2\repository")
    },
    // Local .NET tools live here too, and builds that skip restoring need what it holds.
    Place {
        kind: Kind::Personal,
        ..whole(Group::Packages, "NuGet", r".nuget\packages")
    }
    .used_by(NUGET),
    // What NuGet fetched over HTTP.
    whole(
        Group::Packages,
        "NuGet downloads",
        r"AppData\Local\NuGet\v3-cache",
    )
    .used_by(NUGET),
    whole(Group::Packages, "Gradle", r".gradle\caches").used_by(GRADLE),
    whole(
        Group::Packages,
        "Gradle distributions",
        r".gradle\wrapper\dists",
    )
    .used_by(GRADLE),
    whole(Group::Packages, "node-gyp", r"AppData\Local\node-gyp\Cache"),
    whole(Group::Packages, "Next.js SWC", r"AppData\Local\next-swc"),
    whole(
        Group::Packages,
        "TypeScript",
        r"AppData\Local\Microsoft\TypeScript",
    ),
    // A browser Playwright runs is a program inside it, which is seen anyway.
    whole(
        Group::Packages,
        "Playwright browsers",
        r"AppData\Local\ms-playwright",
    ),
    // Only what each profile caches of the web; its pages, logins and history stay.
    whole(
        Group::Apps,
        "Google Chrome",
        r"AppData\Local\Google\Chrome\User Data",
    )
    .in_each("Cache")
    .used_by(&["chrome"]),
    whole(
        Group::Apps,
        "Microsoft Edge",
        r"AppData\Local\Microsoft\Edge\User Data",
    )
    .in_each("Cache")
    .used_by(&["msedge"]),
    whole(
        Group::Apps,
        "Brave",
        r"AppData\Local\BraveSoftware\Brave-Browser\User Data",
    )
    .in_each("Cache")
    .used_by(&["brave"]),
    whole(
        Group::Apps,
        "Firefox",
        r"AppData\Local\Mozilla\Firefox\Profiles",
    )
    .in_each("cache2")
    .used_by(&["firefox"]),
    whole(
        Group::Apps,
        "Visual Studio Code",
        r"AppData\Roaming\Code\Cache",
    )
    .used_by(CODE),
    whole(
        Group::Apps,
        "Visual Studio Code compiled code",
        r"AppData\Roaming\Code\CachedData",
    )
    .used_by(CODE),
    whole(Group::Apps, "Discord", r"AppData\Roaming\discord\Cache").used_by(&["Discord"]),
    whole(Group::Apps, "Slack", r"AppData\Roaming\Slack\Cache").used_by(&["slack"]),
    // Kept by Windows Error Reporting when it is set to, for finding out why an app crashed.
    Place {
        kind: Kind::Personal,
        ..whole(Group::Logs, "Crash dumps", r"AppData\Local\CrashDumps")
    },
];

#[cfg(windows)]
const CARGO: &[&str] = &["cargo", "rustc"];
#[cfg(windows)]
const CODE: &[&str] = &["Code"];
#[cfg(windows)]
const DART: &[&str] = &["dart"];
#[cfg(windows)]
const DENO: &[&str] = &["deno"];
#[cfg(windows)]
const NUGET: &[&str] = &["dotnet", "msbuild", "nuget"];
/// The daemons that run Gradle's builds, as the engine names them.
#[cfg(windows)]
const GRADLE: &[&str] = &["Gradle daemon", "Kotlin daemon"];

/// Where apps keep caches, each in a folder named after the app.
#[cfg(target_os = "macos")]
const APP_CACHES: &str = "Library/Caches";

/// Everything the catalog points at under `home`, reached through real
/// folders only. `path` is where `home` is, for showing.
pub(crate) fn candidates(home: &Dir, path: &Path, stop: &AtomicBool) -> Vec<Candidate> {
    let mut found = Vec::new();
    let at = |relative: &Path, group, kind, name: String, picked, emptied| Candidate {
        group,
        kind,
        name,
        relative: relative.to_path_buf(),
        path: path.join(relative),
        picked,
        in_use: None,
        project: None,
        touched: None,
        emptied,
    };
    for place in PLACES {
        if stop.load(Ordering::Relaxed) {
            return found;
        }
        let relative = Path::new(place.path);
        let Ok(dir) = home.descend(relative) else {
            continue;
        };
        let rebuildable = place.kind == Kind::Rebuildable;
        if let Some(cache) = place.in_each {
            for (profile, _) in folders(&dir, stop) {
                let relative = relative.join(&profile).join(cache);
                if home.descend(&relative).is_ok() {
                    let name = profile_name(place.name, &profile);
                    found.push(at(
                        &relative,
                        place.group,
                        place.kind,
                        name,
                        rebuildable,
                        true,
                    ));
                }
            }
            continue;
        }
        if !place.each {
            found.push(at(
                relative,
                place.group,
                place.kind,
                place.name.into(),
                rebuildable,
                place.kind != Kind::Personal,
            ));
            continue;
        }
        let mut folders = folders(&dir, stop);
        folders.sort_by_key(|(_, changed)| *changed);
        let latest = folders.len().checked_sub(1).filter(|_| place.keep_latest);
        for (index, (name, _)) in folders.into_iter().enumerate() {
            let picked = rebuildable && Some(index) != latest;
            found.push(at(
                &relative.join(&name),
                place.group,
                place.kind,
                name,
                picked,
                false,
            ));
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(caches) = home.descend(Path::new(APP_CACHES)) {
        for (name, _) in folders(&caches, stop) {
            let relative = Path::new(APP_CACHES).join(&name);
            // A tool's cache the catalog names is listed as what it is, once.
            let named = PLACES.iter().any(|place| Path::new(place.path) == relative);
            if is_app_folder(&name) && !named {
                found.push(at(
                    &relative,
                    Group::Apps,
                    Kind::Rebuildable,
                    name,
                    true,
                    true,
                ));
            }
        }
    }
    found
}

/// The programs that keep the place at `relative` from the home folder, or
/// one of its entries, in use while they run.
/// Whether the catalog reaches into the folder at `relative` from the home
/// folder: a place of its is it, or is inside it.
pub(crate) fn claimed(relative: &Path) -> bool {
    PLACES
        .iter()
        .any(|place| crate::within(Path::new(place.path), relative))
}

pub(crate) fn used_by(relative: &Path) -> &'static [&'static str] {
    PLACES
        .iter()
        .find(|place| {
            let at = Path::new(place.path);
            let profiles = place.in_each.is_some_and(|cache| {
                relative.file_name() == Some(OsStr::new(cache))
                    && relative.parent().and_then(Path::parent) == Some(at)
            });
            relative == at || (place.each && relative.parent() == Some(at)) || profiles
        })
        .map_or(&[], |place| place.used_by)
}

/// What the cache of an app's profile in `folder` is called: by the app's
/// name alone for its first profile, and with the profile's otherwise.
/// Firefox's profile folders start with letters of their own, left out.
fn profile_name(app: &str, folder: &str) -> String {
    if folder == "Default" {
        return app.to_owned();
    }
    let profile = folder.split_once('.').map_or(folder, |(_, name)| name);
    format!("{app} · {profile}")
}

/// The real folders inside `dir`, with when each last changed; links and
/// files are left out, as are names that aren't valid text.
fn folders(dir: &Dir, stop: &AtomicBool) -> Vec<(String, SystemTime)> {
    dir.entries_while(|| !stop.load(Ordering::Relaxed))
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, stat)| walk::is_folder(stat))
        .filter_map(|(name, stat)| Some((name.into_string().ok()?, stat.modified)))
        .collect()
}

/// `com.spotify.client`: a folder named after an app's bundle, except
/// Apple's own, which the system relies on.
#[cfg(target_os = "macos")]
fn is_app_folder(name: &str) -> bool {
    let labels: Vec<&str> = name.split('.').collect();
    labels.len() >= 3
        && labels.iter().all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
        && !name.to_ascii_lowercase().starts_with("com.apple.")
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::is_app_folder;

    #[test]
    fn tells_an_apps_folder_from_others() {
        assert!(is_app_folder("com.spotify.client"));
        assert!(is_app_folder("com.microsoft.VSCode.ShipIt"));
        assert!(is_app_folder("com.Breakpad.crash_report_sender"));
        assert!(!is_app_folder("com.apple.Safari"));
        assert!(!is_app_folder("COM.APPLE.helpd"));
        assert!(!is_app_folder("CloudKit"));
        assert!(!is_app_folder("io.sentry"));
        assert!(!is_app_folder("com..client"));
        assert!(!is_app_folder("../x.y.z"));
    }
}
