//! How much room a finding takes, counted the way the disk does.

use std::collections::HashSet;
use std::io;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::Finding;
use crate::catalog::Candidate;
use crate::walk::{self, Dir};

/// The most places measured at once.
const WORKERS: usize = 4;
/// Folders deeper than this aren't counted. A worker holds one open folder
/// per level it is down.
const DEEPEST: usize = 64;
/// Open files left for everything else the process does.
const RESERVE: u64 = 64;

/// Measures every candidate under `home`, several at a time. A place that
/// couldn't be measured in full is left out rather than shown too small.
pub(crate) fn all(home: &Dir, candidates: Vec<Candidate>, stop: &AtomicBool) -> Vec<Finding> {
    let next = AtomicUsize::new(0);
    let measured: Vec<(usize, u64)> = std::thread::scope(|scope| {
        let running: Vec<_> = (0..workers())
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    while !stop.load(Ordering::Relaxed) {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(candidate) = candidates.get(at) else {
                            break;
                        };
                        let measured = home
                            .descend(&candidate.relative)
                            .and_then(|dir| size(&dir, stop));
                        if let Ok(bytes) = measured {
                            mine.push((at, bytes));
                        }
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
    let mut sizes = vec![0; candidates.len()];
    for (at, bytes) in measured {
        sizes[at] = bytes;
    }
    candidates
        .into_iter()
        .zip(sizes)
        .map(|(candidate, bytes)| Finding {
            group: candidate.group,
            kind: candidate.kind,
            name: candidate.name,
            path: candidate.path,
            bytes,
            picked: candidate.picked,
        })
        .collect()
}

/// As many workers as the open-file limit leaves room for.
fn workers() -> usize {
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
    Ok(blocks(&top) + below(dir, top.st_dev, 0, &mut HashSet::new(), stop)?)
}

fn below(
    dir: &Dir,
    device: libc::dev_t,
    depth: usize,
    linked: &mut HashSet<u64>,
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
        if walk::is_link(&stat) || stat.st_dev != device {
            continue;
        }
        if walk::is_folder(&stat) {
            let Some(child) = dir.child_if_same(&name, &stat)? else {
                continue;
            };
            total += blocks(&stat) + below(&child, device, depth + 1, linked, stop)?;
        } else if stat.st_nlink <= 1 || linked.insert(stat.st_ino) {
            total += blocks(&stat);
        }
    }
    if stop.load(Ordering::Relaxed) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    Ok(total)
}

fn blocks(stat: &libc::stat) -> u64 {
    u64::try_from(stat.st_blocks).unwrap_or(0) * 512
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::{size, workers_for};
    use crate::walk::Dir;

    #[test]
    fn never_passes_off_a_cancelled_count_as_whole() {
        let place = tempfile::tempdir().unwrap();
        let real = place.path().canonicalize().unwrap();
        std::fs::write(real.join("file"), vec![1; 4096]).unwrap();

        assert!(size(&Dir::open(&real).unwrap(), &AtomicBool::new(true)).is_err());
    }

    #[test]
    fn runs_only_as_many_workers_as_open_files_allow() {
        assert_eq!(workers_for(64), 1);
        assert_eq!(workers_for(256), 2);
        assert_eq!(workers_for(10_240), 4);
    }
}
