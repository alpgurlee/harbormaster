//! Taking away what the user confirmed, one finding at a time, through
//! opened folders only, with every safety rule checked again right before.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::walk::{self, Dir, Item};
use crate::{Cleaned, Finding, Group, Identity, Kind, Outcome, Reach, Removal, Usage, Why, in_use};

/// Folders deeper than this aren't emptied; nothing measured goes deeper.
const DEEPEST: usize = 64;

/// The folders removing starts from, each opened once with where it is: the
/// home folder, and the shared app folder when the Mac has one.
pub(crate) struct Roots<'a> {
    home: (Dir, &'a Path),
    shared: Option<(Dir, &'a Path)>,
}

impl<'a> Roots<'a> {
    pub(crate) fn open(reach: &Reach<'a>) -> io::Result<Self> {
        Ok(Self {
            home: (Dir::open(reach.home)?, reach.home),
            shared: reach
                .shared
                .and_then(|at| Dir::open(at).ok().map(|dir| (dir, at))),
        })
    }

    /// The folder `finding` is reached from, and where it is: the home
    /// folder, or for an app of the user's the shared app folder, from which
    /// nothing else is ever taken.
    fn of(&self, finding: &Finding) -> Option<(&Dir, &Path)> {
        let (home, at) = &self.home;
        if finding.path.starts_with(at) {
            return Some((home, at));
        }
        let app = finding.group == Group::Applications && finding.kind == Kind::Personal;
        self.shared
            .as_ref()
            .filter(|(_, at)| app && finding.path.starts_with(at))
            .map(|(shared, at)| (shared, *at))
    }

    /// Whether `finding` is still the folder or file that was measured,
    /// reached through real folders from its root.
    pub(crate) fn still_there(&self, finding: &Finding) -> bool {
        self.of(finding)
            .is_some_and(|(root, at)| still_there(root, at, &finding.path, finding.identity))
    }
}

/// `free` reads how much room the disk has, before and after.
pub(crate) fn all(
    reach: &Reach,
    findings: &[Finding],
    usage: &dyn Fn() -> io::Result<Usage>,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
    free: &dyn Fn() -> io::Result<u64>,
    stop: &AtomicBool,
    told: &mut dyn FnMut(&Removal),
) -> io::Result<Cleaned> {
    let roots = Roots::open(reach)?;
    let before = free()?;
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
                Ok(usage) => one(&roots, finding, &usage, trash, stop),
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
    let after = free()?;
    Ok(Cleaned {
        removals,
        freed_bytes: after.saturating_sub(before),
    })
}

/// One finding: looked at again, then deleted or moved to the Trash.
fn one(
    roots: &Roots,
    finding: &Finding,
    usage: &Usage,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
    stop: &AtomicBool,
) -> Outcome {
    let Some((root, at)) = roots.of(finding) else {
        return refused(Why::OutsideHome);
    };
    let (parent, opened, name) = match reached(root, at, &finding.path, finding.identity) {
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
        (Opened::Folder(dir), Kind::Personal) => trashed(&|| dir.path(), &finding.path, at, trash),
        (Opened::Folder(dir), Kind::Rebuildable | Kind::Discarded) => {
            deleted(&parent, dir, name, finding.emptied, stop)
        }
        // A file only ever goes to the Trash.
        (Opened::File(file), _) => trashed(&|| file.path(), &finding.path, at, trash),
    };
    match done {
        Ok(outcome) => outcome,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Outcome::Interrupted,
        Err(error) => Outcome::Failed {
            why: error.to_string(),
        },
    }
}

/// Whether `path` is still the folder or file with `identity`, reached
/// through real folders from `root`, which is at `at`.
pub(crate) fn still_there(root: &Dir, at: &Path, path: &Path, identity: Identity) -> bool {
    reached(root, at, path, identity).is_ok()
}

/// What a finding names, opened.
enum Opened {
    Folder(Dir),
    File(Item),
}

/// What `path` names, opened through real folders from `root`, which is at
/// `at`, with the folder above it and its name there, if it is still the
/// one with `identity`.
fn reached<'a>(
    root: &Dir,
    at: &Path,
    path: &'a Path,
    identity: Identity,
) -> Result<(Dir, Opened, &'a OsStr), Why> {
    let relative = path.strip_prefix(at).map_err(|_| Why::OutsideHome)?;
    let (above, name) = relative
        .parent()
        .zip(relative.file_name())
        .ok_or(Why::OutsideHome)?;
    let opened = root.descend(above).and_then(|parent| {
        let (opened, stat) = if identity.folder {
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
    if seen != identity {
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
    empty(dir, own.device, 0, stop)?;
    if !emptied {
        parent.unlink(name, true)?;
    }
    Ok(Outcome::Deleted)
}

/// Removes everything inside `dir`, staying on `device` and never following
/// a link: a link goes, what it points to stays. A folder goes once it is
/// empty. Anything out of the ordinary stops this finding.
fn empty(dir: &Dir, device: u64, depth: usize, stop: &AtomicBool) -> io::Result<()> {
    if depth >= DEEPEST {
        return Err(io::Error::other("the folder goes deeper than is removed"));
    }
    for (name, stat) in dir.entries_while(|| !stop.load(Ordering::Relaxed))? {
        if stop.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if stat.device != device {
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
/// which must be inside `at`, the folder it was reached from, and be
/// `chosen`, the path the user chose: a file with other links may be named
/// by any of them. Afterwards it must be what the Trash took.
fn trashed(
    now: &dyn Fn() -> io::Result<PathBuf>,
    chosen: &Path,
    at: &Path,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
) -> io::Result<Outcome> {
    let before = now()?;
    if !before.starts_with(at) {
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
    use super::all;
    use crate::walk::Dir;
    use crate::{Outcome, Reach, Usage};
    use std::fs;
    use std::sync::atomic::AtomicBool;

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
    #[cfg(target_os = "macos")]
    fn moves_nothing_to_the_trash_once_told_to_stop() {
        use std::time::{Duration, SystemTime};

        use super::{Roots, one};
        use crate::{Survey, scan};

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

        let reach = Reach {
            home: &home,
            shared: None,
        };
        let outcome = one(
            &Roots::open(&reach).unwrap(),
            &findings[0],
            &Usage::default(),
            &|_| panic!("nothing may go to the Trash"),
            &AtomicBool::new(true),
        );

        assert_eq!(outcome, Outcome::Stopped);
        assert!(archive.join("a").exists());
    }

    #[test]
    fn says_how_much_more_room_the_disk_has_after_than_before() {
        let place = tempfile::tempdir().unwrap();
        let home = place.path().canonicalize().unwrap();
        let reach = Reach {
            home: &home,
            shared: None,
        };
        let freed = |before: u64, after: u64| {
            let reads = std::cell::Cell::new(0);
            let free = || {
                reads.set(reads.get() + 1);
                Ok(if reads.get() == 1 { before } else { after })
            };
            all(
                &reach,
                &[],
                &|| Ok(Usage::default()),
                &|_| panic!("nothing may go to the Trash"),
                &free,
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .unwrap()
            .freed_bytes
        };

        assert_eq!(freed(100, 160), 60);
        // Others wrote more meanwhile than the removal gave back.
        assert_eq!(freed(160, 100), 0);
    }
}
