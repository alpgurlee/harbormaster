//! Taking away what the user confirmed, one finding at a time, through
//! opened folders only, with every safety rule checked again right before.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::walk::{self, Dir, Item};
use crate::{Cleaned, Finding, Identity, Kind, Outcome, Removal, Usage, Why, in_use, space};

/// Folders deeper than this aren't emptied; nothing measured goes deeper.
const DEEPEST: usize = 64;

pub(crate) fn all(
    home: &Path,
    findings: &[Finding],
    usage: &dyn Fn() -> io::Result<Usage>,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
    stop: &AtomicBool,
    told: &mut dyn FnMut(&Removal),
) -> io::Result<Cleaned> {
    let root = Dir::open(home)?;
    let before = space::disk(home)?.free;
    // Emptying the Trash comes first, so what goes into it now stays there.
    let mut order: Vec<&Finding> = findings.iter().collect();
    order.sort_by_key(|finding| finding.kind != Kind::Discarded);
    let mut removals = Vec::with_capacity(findings.len());
    for finding in order {
        let outcome = if stop.load(Ordering::Relaxed) {
            Outcome::Stopped
        } else {
            // Removing the one before may have taken a while, so what uses
            // what is read again for each.
            match usage() {
                Ok(usage) => one(&root, home, finding, &usage, trash, stop),
                Err(error) => refused(Why::Unchecked {
                    error: error.to_string(),
                }),
            }
        };
        let removal = Removal {
            path: finding.path.clone(),
            outcome,
        };
        told(&removal);
        removals.push(removal);
    }
    let after = space::disk(home)?.free;
    Ok(Cleaned {
        removals,
        freed_bytes: after.saturating_sub(before),
    })
}

/// One finding: looked at again, then deleted or moved to the Trash.
fn one(
    root: &Dir,
    home: &Path,
    finding: &Finding,
    usage: &Usage,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
    stop: &AtomicBool,
) -> Outcome {
    let (parent, opened, name) = match reached(root, home, finding) {
        Ok(reached) => reached,
        Err(why) => return refused(why),
    };
    if let Some(who) = in_use(
        &finding.path,
        finding.project.as_deref(),
        &usage.busy,
        &usage.running,
    ) {
        return refused(Why::InUse { by: who });
    }
    if stop.load(Ordering::Relaxed) {
        return Outcome::Stopped;
    }
    let done = match (&opened, finding.kind) {
        (Opened::Folder(dir), Kind::Personal) => {
            trashed(&|| dir.path(), &finding.path, home, trash)
        }
        (Opened::Folder(dir), Kind::Rebuildable | Kind::Discarded) => {
            deleted(&parent, dir, name, finding.emptied, stop)
        }
        // A file only ever goes to the Trash.
        (Opened::File(file), _) => trashed(&|| file.path(), &finding.path, home, trash),
    };
    match done {
        Ok(outcome) => outcome,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Outcome::Interrupted,
        Err(error) => Outcome::Failed {
            why: error.to_string(),
        },
    }
}

/// Whether `finding` is still the folder or file the scan measured, reached
/// through real folders inside `home`.
pub(crate) fn still_there(root: &Dir, home: &Path, finding: &Finding) -> bool {
    reached(root, home, finding).is_ok()
}

/// What a finding names, opened.
enum Opened {
    Folder(Dir),
    File(Item),
}

/// What `finding` names, opened through real folders from `root`, with the
/// folder above it and its name there, if it is still what the scan measured.
fn reached<'a>(
    root: &Dir,
    home: &Path,
    finding: &'a Finding,
) -> Result<(Dir, Opened, &'a OsStr), Why> {
    let relative = finding
        .path
        .strip_prefix(home)
        .map_err(|_| Why::OutsideHome)?;
    let (above, name) = relative
        .parent()
        .zip(relative.file_name())
        .ok_or(Why::OutsideHome)?;
    let opened = root.descend(above).and_then(|parent| {
        let (opened, stat) = if finding.identity.folder {
            let dir = parent.child(name)?;
            let stat = dir.metadata()?;
            (Opened::Folder(dir), stat)
        } else {
            let file = parent.item(name)?;
            let stat = file.metadata()?;
            (Opened::File(file), stat)
        };
        Ok((parent, opened, Identity::of(&stat)))
    });
    let (parent, opened, seen) = opened.map_err(|error| Why::NotReached {
        error: error.to_string(),
    })?;
    if seen != finding.identity {
        return Err(Why::Changed);
    }
    Ok((parent, opened, name))
}

fn refused(why: Why) -> Outcome {
    Outcome::Refused { why }
}

/// Empties `dir`, then, unless `emptied` is all that was asked, removes it
/// from `parent`, where it is called `name`.
fn deleted(
    parent: &Dir,
    dir: &Dir,
    name: &OsStr,
    emptied: bool,
    stop: &AtomicBool,
) -> io::Result<Outcome> {
    let own = dir.metadata()?;
    empty(dir, own.st_dev, 0, stop)?;
    if !emptied {
        parent.unlink(name, true)?;
    }
    Ok(Outcome::Deleted)
}

/// Removes everything inside `dir`, staying on `device` and never following
/// a link: a link goes, what it points to stays. A folder goes once it is
/// empty. Anything out of the ordinary stops this finding.
fn empty(dir: &Dir, device: libc::dev_t, depth: usize, stop: &AtomicBool) -> io::Result<()> {
    if depth >= DEEPEST {
        return Err(io::Error::other("the folder goes deeper than is removed"));
    }
    for (name, stat) in dir.entries_while(|| !stop.load(Ordering::Relaxed))? {
        if stop.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if stat.st_dev != device {
            return Err(io::Error::other("reaches onto another disk"));
        }
        let folder = walk::is_folder(&stat);
        if folder {
            match dir.child_if_same(&name, &stat) {
                Ok(Some(child)) => empty(&child, device, depth + 1, stop)?,
                Ok(None) => return Err(io::Error::other("changed while being removed")),
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            }
        }
        match dir.unlink(&name, folder) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if stop.load(Ordering::Relaxed) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    Ok(())
}

/// Moves a folder or file to the Trash whole. The Trash takes a path, so it
/// gets the one `now` reads from the kernel for this very folder or file,
/// which must be inside `home` and be `chosen`, the path the user chose: a
/// file with other links may be named by any of them. Afterwards it must be
/// what the Trash took.
fn trashed(
    now: &dyn Fn() -> io::Result<PathBuf>,
    chosen: &Path,
    home: &Path,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
) -> io::Result<Outcome> {
    let before = now()?;
    if !before.starts_with(home) {
        return Ok(refused(Why::OutsideHome));
    }
    if before != chosen {
        return Ok(refused(Why::Changed));
    }
    let landed = trash(&before)?;
    if now()? != landed {
        return Ok(Outcome::Failed {
            why: format!(
                "the Trash took {} instead, which the scan never saw",
                landed.display()
            ),
        });
    }
    Ok(Outcome::Trashed)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime};

    use super::one;
    use crate::walk::Dir;
    use crate::{Outcome, Survey, Usage, scan};

    #[test]
    fn stops_before_deleting_anything_when_told_while_listing() {
        let place = tempfile::tempdir().unwrap();
        let home = place.path().canonicalize().unwrap();
        fs::create_dir_all(home.join("cache/inner")).unwrap();
        fs::write(home.join("cache/inner/a"), b"x").unwrap();
        let root = Dir::open(&home).unwrap();
        let cache = root.child(std::ffi::OsStr::new("cache")).unwrap();

        let done = super::deleted(
            &root,
            &cache,
            std::ffi::OsStr::new("cache"),
            false,
            &AtomicBool::new(true),
        );

        // So a stop during removal may have deleted nothing yet.
        assert_eq!(done.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        assert!(home.join("cache/inner/a").exists());
    }

    #[test]
    fn moves_nothing_the_kernel_names_by_another_path() {
        let place = tempfile::tempdir().unwrap();
        let home = place.path().canonicalize().unwrap();
        let chosen = home.join("Downloads/clip.mov");
        let other = home.join("Big.app/Contents/Resources/clip.mov");

        let outcome = super::trashed(&|| Ok(other.clone()), &chosen, &home, &|_| {
            panic!("nothing may go to the Trash")
        });

        assert_eq!(
            outcome.unwrap(),
            Outcome::Refused {
                why: crate::Why::Changed
            }
        );
    }

    #[test]
    fn moves_nothing_to_the_trash_once_told_to_stop() {
        let place = tempfile::tempdir().unwrap();
        let home = place.path().canonicalize().unwrap();
        let archive = home.join("Library/Developer/Xcode/Archives/2026-09-20");
        fs::create_dir_all(&archive).unwrap();
        fs::write(archive.join("a"), vec![1; 2 * 1024 * 1024]).unwrap();
        let survey = Survey {
            home: &home,
            roots: &[],
            busy: &[],
            running: &[],
            idle_after: Duration::ZERO,
            now: SystemTime::now(),
        };
        let findings = scan(&survey, &AtomicBool::new(false)).unwrap();

        let outcome = one(
            &Dir::open(&home).unwrap(),
            &home,
            &findings[0],
            &Usage::default(),
            &|_| panic!("nothing may go to the Trash"),
            &AtomicBool::new(true),
        );

        assert_eq!(outcome, Outcome::Stopped);
        assert!(archive.join("a").exists());
    }
}
