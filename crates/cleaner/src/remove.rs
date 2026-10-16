//! Taking away what the user confirmed, one finding at a time, through
//! opened folders only, with every safety rule checked again right before.

use std::ffi::{CString, OsStr};
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::walk::{self, Dir};
use crate::{Cleaned, Finding, Identity, Kind, Outcome, Removal, Survey, in_use};

/// Folders deeper than this aren't emptied; nothing measured goes deeper.
const DEEPEST: usize = 64;

pub(crate) fn all(
    survey: &Survey,
    findings: &[Finding],
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
    stop: &AtomicBool,
    told: &mut dyn FnMut(&Removal),
) -> io::Result<Cleaned> {
    let home = Dir::open(survey.home)?;
    let before = free_space(survey.home)?;
    // Emptying the Trash comes first, so what goes into it now stays there.
    let mut order: Vec<&Finding> = findings.iter().collect();
    order.sort_by_key(|finding| finding.kind != Kind::Discarded);
    let mut removals = Vec::with_capacity(findings.len());
    for finding in order {
        let outcome = if stop.load(Ordering::Relaxed) {
            Outcome::Stopped
        } else {
            one(&home, survey, finding, trash, stop)
        };
        let removal = Removal {
            path: finding.path.clone(),
            outcome,
        };
        told(&removal);
        removals.push(removal);
    }
    let after = free_space(survey.home)?;
    Ok(Cleaned {
        removals,
        freed_bytes: after.saturating_sub(before),
    })
}

/// One finding: looked at again, then deleted or moved to the Trash.
fn one(
    home: &Dir,
    survey: &Survey,
    finding: &Finding,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
    stop: &AtomicBool,
) -> Outcome {
    let Ok(relative) = finding.path.strip_prefix(survey.home) else {
        return refused("outside the home folder");
    };
    let Some((above, name)) = relative.parent().zip(relative.file_name()) else {
        return refused("the home folder itself");
    };
    let opened = home.descend(above).and_then(|parent| {
        let dir = parent.child(name)?;
        let seen = Identity::of(&dir.metadata()?);
        Ok((parent, dir, seen))
    });
    let (parent, dir, seen) = match opened {
        Ok(opened) => opened,
        Err(error) => return refused(format!("not reached through real folders ({error})")),
    };
    if seen != finding.identity {
        return refused("no longer the folder the scan measured");
    }
    if let Some(who) = in_use(&finding.path, finding.project.as_deref(), survey) {
        return refused(format!("in use by {who}"));
    }
    if stop.load(Ordering::Relaxed) {
        return Outcome::Stopped;
    }
    let done = match finding.kind {
        Kind::Personal => trashed(&dir, survey.home, trash),
        Kind::Rebuildable | Kind::Discarded => deleted(&parent, &dir, name, finding.emptied, stop),
    };
    match done {
        Ok(outcome) => outcome,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Outcome::Stopped,
        Err(error) => Outcome::Failed {
            why: error.to_string(),
        },
    }
}

fn refused(why: impl Into<String>) -> Outcome {
    Outcome::Refused { why: why.into() }
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

/// Moves the folder `dir` to the Trash whole. The Trash takes a path, so it
/// gets the one the kernel gives for this very folder right now, which must
/// be inside `home`; afterwards this folder must be what the Trash took.
fn trashed(
    dir: &Dir,
    home: &Path,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
) -> io::Result<Outcome> {
    let now = dir.path()?;
    if !now.starts_with(home) {
        return Ok(refused("moved out of the home folder"));
    }
    let landed = trash(&now)?;
    if dir.path()? != landed {
        return Ok(Outcome::Failed {
            why: format!(
                "the Trash took {} instead, which the scan never saw",
                landed.display()
            ),
        });
    }
    Ok(Outcome::Trashed)
}

/// The room free on the disk holding `path`, as the disk reports it.
fn free_space(path: &Path) -> io::Result<u64> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    let mut stats = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `path` is a valid C string and `stats` has room for one statfs.
    if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statfs succeeded, so it filled `stats`.
    let stats = unsafe { stats.assume_init() };
    Ok(stats.f_bavail * u64::from(stats.f_bsize))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime};

    use super::one;
    use crate::walk::Dir;
    use crate::{Outcome, Survey, scan};

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
            &survey,
            &findings[0],
            &|_| panic!("nothing may go to the Trash"),
            &AtomicBool::new(true),
        );

        assert_eq!(outcome, Outcome::Stopped);
        assert!(archive.join("a").exists());
    }
}
