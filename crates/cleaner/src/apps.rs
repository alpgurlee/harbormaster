//! The apps the user may remove: bundles of their own in the shared app
//! folder and in their home folder's, each with the room it takes.

use std::cmp::Reverse;
use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::walk::{self, Dir, Stat};
use crate::{Candidate, Finding, Group, Kind, Measured, Reach, in_use, measure};

/// Flags that lock a file against being moved or changed.
const LOCKED: u32 = libc::UF_IMMUTABLE
    | libc::UF_APPEND
    | libc::SF_IMMUTABLE
    | libc::SF_APPEND
    | SF_RESTRICTED
    | SF_NOUNLINK;
/// Writing needs an entitlement: macOS's own protected files. Not in libc.
const SF_RESTRICTED: u32 = 0x0008_0000;
/// It may not be removed or renamed. Not in libc.
const SF_NOUNLINK: u32 = 0x0010_0000;

pub(crate) fn listed(
    reach: &Reach,
    busy: &[(PathBuf, String)],
    stop: &AtomicBool,
    measured: &(dyn Fn(Measured) + Sync),
) -> io::Result<Vec<Finding>> {
    let mut shelves = Vec::new();
    let shared = reach.shared.map(Path::to_path_buf);
    for folder in shared.into_iter().chain([reach.home.join("Applications")]) {
        // A Mac may lack either folder.
        let Ok(root) = Dir::open(&folder) else {
            continue;
        };
        let candidates = shelved(&root, &folder, busy, stop)?;
        shelves.push((root, candidates));
    }
    let total = shelves.iter().map(|(_, candidates)| candidates.len()).sum();
    let mut before = 0;
    let mut found = Vec::new();
    for (root, candidates) in shelves {
        let count = candidates.len();
        found.extend(measure::all(&root, candidates, stop, &|progress| {
            measured(Measured {
                done: before + progress.done,
                total,
                ..progress
            });
        }));
        before += count;
    }
    // One that couldn't be measured in full is left out rather than shown too small.
    found.retain(|app| app.bytes > 0);
    found.sort_by_key(|app| Reverse(app.bytes));
    Ok(found)
}

/// The user's own app bundles in `root`, which is at `folder`, and in the
/// folders right inside it, as Utilities or a maker's own folder.
fn shelved(
    root: &Dir,
    folder: &Path,
    busy: &[(PathBuf, String)],
    stop: &AtomicBool,
) -> io::Result<Vec<Candidate>> {
    let keep_going = || !stop.load(Ordering::Relaxed);
    let device = root.metadata()?.device;
    let here = |stat: &Stat| walk::is_folder(stat) && stat.device == device;
    let mut found = Vec::new();
    for (name, stat) in root.entries_while(keep_going)? {
        if !here(&stat) {
            continue;
        }
        if is_bundle(&name) {
            found.extend(candidate(root, Path::new(&name), &stat, folder, busy));
            continue;
        }
        let Ok(Some(inner)) = root.child_if_same(&name, &stat) else {
            continue;
        };
        for (inner_name, inner_stat) in inner.entries_while(keep_going).unwrap_or_default() {
            if here(&inner_stat) && is_bundle(&inner_name) {
                let relative = Path::new(&name).join(&inner_name);
                found.extend(candidate(&inner, &relative, &inner_stat, folder, busy));
            }
        }
    }
    Ok(found)
}

/// The bundle at `relative` from `folder`, the last of it named in `dir`
/// and stored as `stat`, if it is an app of the user's own.
fn candidate(
    dir: &Dir,
    relative: &Path,
    stat: &Stat,
    folder: &Path,
    busy: &[(PathBuf, String)],
) -> Option<Candidate> {
    let name = relative.file_name()?;
    let bundle = dir.child_if_same(name, stat).ok()??;
    if !eligible(&bundle).ok()? {
        return None;
    }
    let path = folder.join(relative);
    Some(Candidate {
        group: Group::Applications,
        kind: Kind::Personal,
        name: stem(name),
        in_use: in_use(&path, None, busy, &[]),
        relative: relative.to_path_buf(),
        path,
        picked: false,
        project: None,
        touched: None,
        emptied: false,
    })
}

/// Whether the open `bundle` is an app of the user's own: theirs, not
/// locked, with its Info.plist in its Contents, a file of its own rather
/// than a link. Asked of the very folder measured too, so what is offered
/// is what was asked about.
pub(crate) fn eligible(bundle: &Dir) -> io::Result<bool> {
    let info = bundle
        .child(OsStr::new("Contents"))?
        .item(OsStr::new("Info.plist"))?
        .metadata()?;
    Ok(owned(&bundle.metadata()?) && walk::is_file(&info))
}

/// Whether the user may move what `stat` describes: it is theirs, and not
/// locked against change.
fn owned(stat: &Stat) -> bool {
    // SAFETY: getuid has no preconditions.
    let user = unsafe { libc::getuid() };
    stat.owner == user && stat.flags & LOCKED == 0
}

fn is_bundle(name: &OsStr) -> bool {
    let name = name.as_bytes();
    name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(b".app")
}

/// An app's name as Finder shows it: its bundle's, without `.app`.
fn stem(name: &OsStr) -> String {
    let name = name.as_bytes();
    String::from_utf8_lossy(&name[..name.len() - 4]).into_owned()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::sync::atomic::AtomicBool;

    use std::time::UNIX_EPOCH;

    use super::{SF_NOUNLINK, SF_RESTRICTED, is_bundle, owned, shelved};
    use crate::measure;
    use crate::walk::{Dir, FileType, Stat};

    fn stat(owner: libc::uid_t, flags: u32) -> Stat {
        Stat {
            kind: FileType::Folder,
            device: 1,
            inode: 1,
            links: Some(1),
            bytes: 0,
            modified: UNIX_EPOCH,
            owner,
            flags,
        }
    }

    #[test]
    fn offers_only_what_is_the_users_and_unlocked() {
        // SAFETY: getuid has no preconditions.
        let user = unsafe { libc::getuid() };

        assert!(owned(&stat(user, 0)));
        assert!(!owned(&stat(user + 1, 0)));
        for flag in [
            libc::UF_IMMUTABLE,
            libc::SF_IMMUTABLE,
            SF_RESTRICTED,
            SF_NOUNLINK,
        ] {
            assert!(!owned(&stat(user, flag)), "{flag:#x}");
        }
    }

    #[test]
    fn measures_a_bundle_only_while_it_is_still_the_users_app() {
        let place = tempfile::tempdir().unwrap();
        let folder = place.path().canonicalize().unwrap();
        fs::create_dir_all(folder.join("Big.app/Contents")).unwrap();
        fs::write(folder.join("Big.app/Contents/Info.plist"), b"<plist/>").unwrap();
        let root = Dir::open(&folder).unwrap();
        let stop = AtomicBool::new(false);
        let candidates = shelved(&root, &folder, &[], &stop).unwrap();
        assert_eq!(candidates.len(), 1);

        // Swapped, once seen, for a folder that isn't an app.
        fs::rename(folder.join("Big.app"), folder.join("Old.app")).unwrap();
        fs::create_dir_all(folder.join("Big.app/Contents/MacOS")).unwrap();
        fs::write(folder.join("Big.app/Contents/MacOS/app"), vec![1; 1 << 20]).unwrap();

        let found = measure::all(&root, candidates, &stop, &|_| {});

        assert!(found.iter().all(|app| app.bytes == 0), "{found:?}");
    }

    #[test]
    fn knows_a_bundle_by_its_name() {
        assert!(is_bundle(OsStr::new("Docker.app")));
        assert!(is_bundle(OsStr::new("Old.APP")));
        assert!(!is_bundle(OsStr::new(".app")));
        assert!(!is_bundle(OsStr::new("Utilities")));
    }
}
