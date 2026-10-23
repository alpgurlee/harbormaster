//! How much room a finding takes, counted the way the disk does.

use std::collections::HashSet;
use std::io;
#[cfg(unix)]
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::walk::{self, Dir};
use crate::{Candidate, Finding, Identity, Measured};
#[cfg(target_os = "macos")]
use crate::{Group, apps};

/// The most places measured at once.
const WORKERS: usize = 4;
/// Folders deeper than this aren't counted. A worker holds one open folder
/// per level it is down.
const DEEPEST: usize = 64;
/// Open files left for everything else the process does.
#[cfg(unix)]
const RESERVE: u64 = 64;

/// Measures every candidate under `home`, several at a time, telling
/// `measured` after each. A place that couldn't be measured in full is left
/// out rather than shown too small.
pub(crate) fn all(
    home: &Dir,
    candidates: Vec<Candidate>,
    stop: &AtomicBool,
    measured: &(dyn Fn(Measured) + Sync),
) -> Vec<Finding> {
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let measured: Vec<(usize, (Identity, u64))> = std::thread::scope(|scope| {
        let running: Vec<_> = (0..workers())
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    while !stop.load(Ordering::Relaxed) {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(candidate) = candidates.get(at) else {
                            break;
                        };
                        let sized = home.descend(&candidate.relative).and_then(|dir| {
                            #[cfg(target_os = "macos")]
                            if candidate.group == Group::Applications && !apps::eligible(&dir)? {
                                return Err(io::Error::other("no longer an app of the user's"));
                            }
                            Ok((Identity::of(&dir.metadata()?), size(&dir, stop)?))
                        });
                        let bytes = sized.as_ref().map_or(0, |(_, bytes)| *bytes);
                        if let Ok(sized) = sized {
                            mine.push((at, sized));
                        }
                        measured(Measured {
                            done: done.fetch_add(1, Ordering::Relaxed) + 1,
                            total: candidates.len(),
                            bytes,
                        });
                    }
                    mine
                })
            })
            .collect();
        running
            .into_iter()
            .flat_map(|worker| worker.join().unwrap_or_default())
            .collect()
    });
    let mut sizes = vec![(Identity::default(), 0); candidates.len()];
    for (at, measured) in measured {
        sizes[at] = measured;
    }
    candidates
        .into_iter()
        .zip(sizes)
        .map(|(candidate, (identity, bytes))| Finding {
            group: candidate.group,
            kind: candidate.kind,
            name: candidate.name,
            path: candidate.path,
            bytes,
            picked: candidate.picked,
            in_use: candidate.in_use,
            touched: candidate.touched,
            project: candidate.project,
            emptied: candidate.emptied,
            identity,
        })
        .collect()
}

/// As many workers as are worth it: Windows has no open-file limit to mind.
#[cfg(windows)]
pub(crate) fn workers() -> usize {
    WORKERS
}

/// As many workers as the open-file limit leaves room for.
#[cfg(unix)]
pub(crate) fn workers() -> usize {
    let mut limit = MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: `limit` has room for one rlimit, which getrlimit fills on success.
    let soft = if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) } == 0 {
        // SAFETY: getrlimit succeeded, so it filled `limit`.
        unsafe { limit.assume_init() }.rlim_cur
    } else {
        256
    };
    workers_for(soft)
}

#[cfg(unix)]
fn workers_for(open_files: u64) -> usize {
    let room = usize::try_from(open_files.saturating_sub(RESERVE)).unwrap_or(usize::MAX);
    (room / (DEEPEST + 2)).clamp(1, WORKERS)
}

/// The blocks a folder's tree takes: each hard-linked file once, never
/// following a link, and never onto another disk. Anything that would leave
/// the count short fails it instead: a stop, a tree deeper than `DEEPEST`, or
/// a folder it can't read.
pub(crate) fn size(dir: &Dir, stop: &AtomicBool) -> io::Result<u64> {
    let top = dir.metadata()?;
    Ok(top.bytes + below(dir, top.device, 0, &mut HashSet::new(), stop)?)
}

fn below(
    dir: &Dir,
    device: u64,
    depth: usize,
    linked: &mut HashSet<u128>,
    stop: &AtomicBool,
) -> io::Result<u64> {
    if stop.load(Ordering::Relaxed) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    if depth >= DEEPEST {
        return Err(io::Error::other("the folder goes deeper than is measured"));
    }
    let entries = dir.entries_while(|| !stop.load(Ordering::Relaxed))?;
    let mut total = 0;
    for (name, stat) in entries {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if walk::is_link(&stat) || stat.device != device {
            continue;
        }
        if walk::is_folder(&stat) {
            let Some(child) = dir.child_if_same(&name, &stat)? else {
                continue;
            };
            total += stat.bytes + below(&child, device, depth + 1, linked, stop)?;
        } else if stat.bytes == 0 || !stat.may_have_other_names() || linked.insert(stat.inode) {
            total += stat.bytes;
        }
    }
    if stop.load(Ordering::Relaxed) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::size;
    #[cfg(unix)]
    use super::workers_for;
    use crate::walk::Dir;

    #[test]
    fn never_passes_off_a_cancelled_count_as_whole() {
        let place = tempfile::tempdir().unwrap();
        let real = place.path().canonicalize().unwrap();
        std::fs::write(real.join("file"), vec![1; 4096]).unwrap();

        assert!(size(&Dir::open(&real).unwrap(), &AtomicBool::new(true)).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn runs_only_as_many_workers_as_open_files_allow() {
        assert_eq!(workers_for(64), 1);
        assert_eq!(workers_for(256), 2);
        assert_eq!(workers_for(10_240), 4);
    }
}
