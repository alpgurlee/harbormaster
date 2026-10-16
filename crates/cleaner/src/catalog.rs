//! The places a developer's Mac fills with things its tools rebuild, written
//! from each tool's own documentation.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::walk::{self, Dir};
use crate::{Group, Kind};

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
}

const fn whole(group: Group, name: &'static str, path: &'static str) -> Place {
    Place {
        group,
        kind: Kind::Rebuildable,
        name,
        path,
        each: false,
        keep_latest: false,
    }
}

#[cfg(target_os = "macos")]
const PLACES: &[Place] = &[
    whole(Group::Packages, "npm", ".npm/_cacache"),
    whole(Group::Packages, "npx", ".npm/_npx"),
    whole(Group::Packages, "pnpm", "Library/pnpm/store"),
    whole(Group::Packages, "Yarn", "Library/Caches/Yarn"),
    whole(Group::Packages, "Bun", ".bun/install/cache"),
    whole(Group::Packages, "pip", "Library/Caches/pip"),
    whole(Group::Packages, "uv", ".cache/uv"),
    whole(Group::Packages, "Poetry", "Library/Caches/pypoetry"),
    whole(Group::Packages, "Cargo", ".cargo/registry/cache"),
    whole(Group::Packages, "Cargo sources", ".cargo/registry/src"),
    whole(Group::Packages, "Cargo git", ".cargo/git/checkouts"),
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

#[cfg(not(target_os = "macos"))]
const PLACES: &[Place] = &[];

/// Where apps keep caches, each in a folder named after the app.
#[cfg(target_os = "macos")]
const APP_CACHES: &str = "Library/Caches";

/// A finding before it is measured.
#[derive(Debug)]
pub(crate) struct Candidate {
    pub(crate) group: Group,
    pub(crate) kind: Kind,
    pub(crate) name: String,
    /// Where it is from the home folder, which it is reopened through.
    pub(crate) relative: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) picked: bool,
}

/// Everything the catalog points at under `home`, reached through real
/// folders only. `path` is where `home` is, for showing.
pub(crate) fn candidates(home: &Dir, path: &Path, stop: &AtomicBool) -> Vec<Candidate> {
    let mut found = Vec::new();
    let at = |relative: &Path, group, kind, name: String, picked| Candidate {
        group,
        kind,
        name,
        relative: relative.to_path_buf(),
        path: path.join(relative),
        picked,
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
        if !place.each {
            found.push(at(
                relative,
                place.group,
                place.kind,
                place.name.into(),
                rebuildable,
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
            ));
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(caches) = home.descend(Path::new(APP_CACHES)) {
        for (name, _) in folders(&caches, stop) {
            if is_app_folder(&name) {
                let relative = Path::new(APP_CACHES).join(&name);
                found.push(at(&relative, Group::Apps, Kind::Rebuildable, name, true));
            }
        }
    }
    found
}

/// The real folders inside `dir`, with when each last changed; links and
/// files are left out, as are names that aren't valid text.
fn folders(dir: &Dir, stop: &AtomicBool) -> Vec<(String, (i64, i64))> {
    dir.entries_while(|| !stop.load(Ordering::Relaxed))
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, stat)| walk::is_folder(stat))
        .filter_map(|(name, stat)| {
            Some((
                name.into_string().ok()?,
                (stat.st_mtime, stat.st_mtime_nsec),
            ))
        })
        .collect()
}

/// `com.spotify.client`: a folder named after an app's bundle, except
/// Apple's own, which the system relies on.
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
