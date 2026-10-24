//! What apps put aside in this user's temporary folder while they ran, and
//! left behind. Only what nothing has touched for a week goes, one entry of
//! the folder at a time, never the folder itself. What a process works in
//! or runs from stays, as do the locks NuGet coordinates its restores
//! through. A program may still use something none of this sees; Windows
//! then won't let it go, and the removal says how much stayed.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::walk::{self, Dir, Stat};
use crate::{Finding, Group, Identity, Kind, Outcome, Survey, within};

/// How long an entry goes untouched before it may go, as long as Disk
/// Cleanup waits for temporary files.
const AGE: Duration = Duration::from_secs(7 * 86_400);
/// Folders deeper than this aren't looked into, and what holds one stays.
const DEEPEST: usize = 32;
/// What stays whatever its age, by name.
const KEPT: &[&str] = &["NuGetScratch"];

/// What of the temporary folder at `temp`, inside `survey.home`, could go,
/// as one finding, if the folder is reached through real folders.
pub(crate) fn finding(
    root: &Dir,
    survey: &Survey,
    temp: &Path,
    stop: &AtomicBool,
) -> Option<Finding> {
    if !within(temp, survey.home) {
        return None;
    }
    let relative: PathBuf = temp
        .components()
        .skip(survey.home.components().count())
        .collect();
    let dir = root.descend(&relative).ok()?;
    let identity = Identity::of(&dir.metadata().ok()?);
    let bytes = dir
        .entries_while(|| !stop.load(Ordering::Relaxed))
        .ok()?
        .iter()
        .filter_map(|(name, stat)| {
            room_if_old(&dir, temp, name, stat, survey.busy, survey.now, stop)
        })
        .sum();
    Some(Finding {
        group: Group::Temp,
        kind: Kind::Rebuildable,
        name: "Temporary files".into(),
        path: temp.to_path_buf(),
        bytes,
        picked: true,
        in_use: None,
        touched: None,
        project: None,
        emptied: true,
        identity,
    })
}

/// Removes from `dir`, the temporary folder at `temp`, each entry that may
/// go as things stand now, `busy` holding what processes hold. An entry
/// Windows won't let go is left, and counted, and the rest go on.
pub(crate) fn removed(
    dir: &Dir,
    temp: &Path,
    busy: &[(PathBuf, String)],
    stop: &AtomicBool,
) -> Outcome {
    let now = SystemTime::now();
    let listed = dir.metadata().and_then(|own| {
        dir.entries_while(|| !stop.load(Ordering::Relaxed))
            .map(|entries| (own.device, entries))
    });
    let (device, entries) = match listed {
        Ok(listed) => listed,
        Err(error) => {
            return Outcome::Failed {
                why: error.to_string(),
            };
        }
    };
    let mut left = 0;
    for (name, stat) in entries {
        if stop.load(Ordering::Relaxed) {
            return Outcome::Interrupted;
        }
        if room_if_old(dir, temp, &name, &stat, busy, now, stop).is_none() {
            continue;
        }
        let gone = if walk::is_folder(&stat) {
            match dir.child_if_same(&name, &stat) {
                // A folder goes once it is empty, which it no longer is if
                // something came into it since.
                Ok(Some(child)) => {
                    emptied(&child, device, 1, now, stop).and_then(|()| dir.unlink(&name, true))
                }
                // Something else is in its place now.
                Ok(None) => continue,
                Err(error) => Err(error),
            }
        } else {
            dir.unlink_file_if(&name, &|own| old(own.touched, now))
        };
        match gone {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                return Outcome::Interrupted;
            }
            Err(_) => left += 1,
        }
    }
    if left > 0 {
        Outcome::Partly { left }
    } else {
        Outcome::Deleted
    }
}

/// The room the entry `name` of `dir`, which is at `folder`, takes, if it
/// may go: it isn't kept, holds nothing in `busy`, and neither it nor
/// anything in it was touched within `AGE` before `now`.
fn room_if_old(
    dir: &Dir,
    folder: &Path,
    name: &OsStr,
    stat: &Stat,
    busy: &[(PathBuf, String)],
    now: SystemTime,
    stop: &AtomicBool,
) -> Option<u64> {
    if KEPT.iter().any(|kept| walk::is_named(name, kept)) {
        return None;
    }
    let path = folder.join(name);
    if busy.iter().any(|(held, _)| within(held, &path)) {
        return None;
    }
    let (bytes, touched) = aged(dir, name, stat, 0, stop)?;
    old(touched, now).then_some(bytes)
}

/// Whether what was last touched at `touched` was left `AGE` before `now`;
/// a time ahead of now says nothing of how old it is.
fn old(touched: SystemTime, now: SystemTime) -> bool {
    touched <= now && touched.checked_add(AGE).is_some_and(|until| until <= now)
}

/// Removes what `dir`, on `device`, holds, each file only if it is still
/// old before `now` as the handle that removes it reads it, and each folder
/// once it is empty. Anything young, out of the ordinary or that Windows
/// won't let go stops it, with what went before gone.
fn emptied(
    dir: &Dir,
    device: u64,
    depth: usize,
    now: SystemTime,
    stop: &AtomicBool,
) -> io::Result<()> {
    if depth > DEEPEST {
        return Err(io::Error::other("the folder goes deeper than is removed"));
    }
    for (name, stat) in dir.entries_while(|| !stop.load(Ordering::Relaxed))? {
        if stop.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if stat.device != device {
            return Err(io::Error::other("reaches onto another disk"));
        }
        let gone = if walk::is_folder(&stat) {
            match dir.child_if_same(&name, &stat) {
                Ok(Some(child)) => {
                    emptied(&child, device, depth + 1, now, stop)?;
                    dir.unlink(&name, true)
                }
                Ok(None) => return Err(io::Error::other("changed while being removed")),
                Err(error) => Err(error),
            }
        } else {
            dir.unlink_file_if(&name, &|own| old(own.touched, now))
        };
        match gone {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// The room `name` in `dir`, listed as `listed`, takes, and the latest it
/// or anything in it was touched, each read through a handle of its own,
/// as a listing may lag behind a file being written. Nothing if any of it
/// can't be read, isn't what was listed, or goes deeper than is looked.
fn aged(
    dir: &Dir,
    name: &OsStr,
    listed: &Stat,
    depth: usize,
    stop: &AtomicBool,
) -> Option<(u64, SystemTime)> {
    if depth > DEEPEST || stop.load(Ordering::Relaxed) {
        return None;
    }
    if !walk::is_folder(listed) {
        let own = dir.item(name).ok()?.metadata().ok()?;
        let same = own.device == listed.device && own.inode == listed.inode;
        return same.then_some((own.bytes, own.touched));
    }
    let child = dir.child_if_same(name, listed).ok()??;
    let mut found = (0u64, child.metadata().ok()?.touched);
    for (inner, stat) in child.entries_while(|| !stop.load(Ordering::Relaxed)).ok()? {
        let (bytes, touched) = aged(&child, &inner, &stat, depth + 1, stop)?;
        found = (found.0.saturating_add(bytes), found.1.max(touched));
    }
    Some(found)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime};

    use super::emptied;
    use crate::walk::{self, Dir};

    #[test]
    fn empties_nothing_that_is_young_by_its_own_account_when_removed() {
        let place = tempfile::tempdir().unwrap();
        let home = walk::canonical(place.path()).unwrap();
        std::fs::create_dir_all(home.join("entry").join("inner")).unwrap();
        std::fs::write(home.join("entry").join("inner").join("fresh.log"), b"x").unwrap();
        let root = Dir::open(&home).unwrap();
        let entry = root.child(OsStr::new("entry")).unwrap();
        let device = entry.metadata().unwrap().device;
        let stop = AtomicBool::new(false);

        assert!(emptied(&entry, device, 1, SystemTime::now(), &stop).is_err());
        assert!(home.join("entry").join("inner").join("fresh.log").exists());

        let later = SystemTime::now() + Duration::from_secs(8 * 86_400);
        emptied(&entry, device, 1, later, &stop).unwrap();
        assert!(!home.join("entry").join("inner").exists());
    }
}
