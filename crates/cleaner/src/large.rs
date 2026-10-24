//! The user's own large files: films, archives, disk images and installers
//! kept around the home folder, found so the user can decide on each.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

use crate::walk::{self, Dir, Stat};
use crate::{Finding, Group, Hunt, Identity, Kind, Measured, in_use, measure, projects};

/// Folders deeper than this aren't looked into.
const DEEPEST: usize = 32;

/// Where apps keep their data in the home folder, which isn't looked in.
#[cfg(not(windows))]
const APP_DATA: &str = "Library";
#[cfg(windows)]
const APP_DATA: &str = "AppData";

/// Folders macOS shows as one thing, such as an app, a photo library or a
/// virtual machine, by their extension. They are left whole, never looked
/// into: a file taken from inside one would break it.
const PACKAGES: &[&str] = &[
    "app",
    "appex",
    "band",
    "bundle",
    "fcpbundle",
    "framework",
    "imovielibrary",
    "key",
    "kext",
    "logicx",
    "lrlibrary",
    "musiclibrary",
    "numbers",
    "pages",
    "photoslibrary",
    "playground",
    "plugin",
    "pvm",
    "rtfd",
    "sparsebundle",
    "tvlibrary",
    "utm",
    "vmwarevm",
    "xcarchive",
    "xcodeproj",
    "xcworkspace",
];

/// Each kind of large file, by the extensions that name it.
const KINDS: &[(Group, &[&str])] = &[
    (
        Group::Videos,
        &[
            "3gp", "avi", "flv", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "mts", "webm", "wmv",
        ],
    ),
    (
        Group::Archives,
        &[
            "7z",
            "bz2",
            "dmg",
            "gz",
            "img",
            "iso",
            "rar",
            "sparseimage",
            "tar",
            "tgz",
            "xip",
            "xz",
            "zip",
            "zst",
        ],
    ),
    (
        Group::Installers,
        &[
            "aab", "apk", "appimage", "deb", "exe", "ipa", "mpkg", "msi", "pkg", "rpm",
        ],
    ),
    (
        Group::Audio,
        &[
            "aac", "aif", "aiff", "caf", "flac", "m4a", "mp3", "ogg", "wav",
        ],
    ),
    (
        Group::Images,
        &[
            "arw", "cr2", "dng", "gif", "heic", "jpeg", "jpg", "nef", "png", "psd", "raw", "tif",
            "tiff", "webp",
        ],
    ),
    (
        Group::Documents,
        &[
            "csv", "doc", "docx", "epub", "key", "numbers", "pages", "pdf", "ppt", "pptx", "xls",
            "xlsx",
        ],
    ),
];

pub(crate) fn hunt(
    hunt: &Hunt,
    stop: &AtomicBool,
    measured: &(dyn Fn(Measured) + Sync),
) -> io::Result<Vec<Finding>> {
    let home = Dir::open(hunt.home)?;
    let device = home.metadata()?.device;
    // Files right in the home folder are looked at here; each folder in it
    // is looked through on its own.
    let mut found = Vec::new();
    let mut places = Vec::new();
    for (name, stat) in home.entries_while(|| !stop.load(Ordering::Relaxed))? {
        if passed_over(&name, &stat, device) || is_app_data(&name) {
            continue;
        }
        if walk::is_folder(&stat) {
            if entered(&name) {
                places.push((name, stat));
            }
        } else {
            found.extend(finding(hunt, &home, &name, Path::new(""), &stat));
        }
    }
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..measure::workers().min(places.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    while !stop.load(Ordering::Relaxed) {
                        let Some((name, stat)) = places.get(next.fetch_add(1, Ordering::Relaxed))
                        else {
                            break;
                        };
                        let before = mine.len();
                        if let Ok(Some(dir)) = home.child_if_same(name, stat) {
                            looked(&dir, Path::new(name), device, 1, hunt, stop, &mut mine);
                        }
                        measured(Measured {
                            done: done.fetch_add(1, Ordering::Relaxed) + 1,
                            total: places.len(),
                            bytes: mine[before..].iter().map(|finding| finding.bytes).sum(),
                        });
                    }
                    mine
                })
            })
            .collect();
        for worker in workers {
            found.extend(worker.join().unwrap_or_default());
        }
    });
    found.sort_by(|a, b| a.group.cmp(&b.group).then(b.bytes.cmp(&a.bytes)));
    Ok(found)
}

/// Looks through `dir`, which is `relative` from the home folder, adding the
/// large files in it and below to `found`. A folder that can't be read is
/// passed over, as is a cache: what a tool keeps for itself isn't the user's.
fn looked(
    dir: &Dir,
    relative: &Path,
    device: u64,
    depth: usize,
    hunt: &Hunt,
    stop: &AtomicBool,
    found: &mut Vec<Finding>,
) {
    if depth > DEEPEST || projects::tagged(dir) {
        return;
    }
    let Ok(entries) = dir.entries_while(|| !stop.load(Ordering::Relaxed)) else {
        return;
    };
    for (name, stat) in entries {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if passed_over(&name, &stat, device) {
            continue;
        }
        if walk::is_folder(&stat) {
            if !entered(&name) {
                continue;
            }
            if let Ok(Some(child)) = dir.child_if_same(&name, &stat) {
                looked(
                    &child,
                    &relative.join(&name),
                    device,
                    depth + 1,
                    hunt,
                    stop,
                    found,
                );
            }
        } else if walk::is_file(&stat) {
            found.extend(finding(hunt, dir, &name, relative, &stat));
        }
    }
}

/// What is never looked at: a hidden name, a link, or another disk, and on
/// Windows what File Explorer hides.
fn passed_over(name: &OsStr, stat: &Stat, device: u64) -> bool {
    let hidden = name.as_encoded_bytes().starts_with(b".");
    #[cfg(windows)]
    let hidden = hidden || stat.hidden;
    hidden || walk::is_link(stat) || stat.device != device
}

/// Whether `name`, in the home folder, is where apps keep their data.
fn is_app_data(name: &OsStr) -> bool {
    #[cfg(windows)]
    {
        walk::same_name(name, OsStr::new(APP_DATA))
    }
    #[cfg(not(windows))]
    {
        name == APP_DATA
    }
}

/// Whether a folder called `name` is looked into: not a package, nor the
/// packages a Node project downloaded.
fn entered(name: &OsStr) -> bool {
    let package = extension(name).is_some_and(|extension| PACKAGES.contains(&extension.as_str()));
    !package && name != "node_modules"
}

/// The file `name` in `dir`, which is `folder` from the home folder, listed
/// as `stat`, if it is a regular file large enough to list. A file with
/// other links is left out: trashing one of its names would free nothing.
fn finding(hunt: &Hunt, dir: &Dir, name: &OsStr, folder: &Path, stat: &Stat) -> Option<Finding> {
    let bytes = stat.bytes;
    if !walk::is_file(stat) || bytes < hunt.smallest {
        return None;
    }
    let stat = &with_links(dir, name, stat)?;
    if stat.may_have_other_names() {
        return None;
    }
    let path = hunt.home.join(folder.join(name));
    Some(Finding {
        group: group_of(name),
        kind: Kind::Personal,
        name: name.to_string_lossy().into_owned(),
        in_use: in_use(&path, None, hunt.busy, &[]),
        path,
        bytes,
        picked: false,
        touched: stat
            .modified
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|since| since.as_secs()),
        project: None,
        emptied: false,
        identity: Identity::of(stat),
    })
}

/// `stat`, listed for `name` in `dir`, with how many names the file has:
/// a listing on Windows doesn't say, so the file is looked at for it.
fn with_links(dir: &Dir, name: &OsStr, stat: &Stat) -> Option<Stat> {
    if stat.links.is_some() {
        return Some(*stat);
    }
    let now = dir.item(name).ok()?.metadata().ok()?;
    (now.device == stat.device && now.inode == stat.inode).then_some(now)
}

fn group_of(name: &OsStr) -> Group {
    extension(name)
        .and_then(|extension| {
            KINDS
                .iter()
                .find(|(_, extensions)| extensions.contains(&extension.as_str()))
        })
        .map_or(Group::OtherFiles, |(group, _)| *group)
}

/// The extension of `name`, in lower case.
fn extension(name: &OsStr) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
}
