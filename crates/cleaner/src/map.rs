//! A map of the home folder: what each folder holds, as the disk counts it,
//! so the window can show where the room goes.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::walk::{self, Dir};
use crate::{Area, Chart, Charting, Identity, Short, measure};

/// The most places shown inside one folder; the others are counted together.
const SHOWN_INSIDE: usize = 50;
/// A place taking less room than this is counted with the others.
const SHOWN_FROM: u64 = 1024 * 1024;
/// The most places a map keeps, whatever the disk holds.
const KEPT_AT_MOST: usize = 20_000;
/// Folders deeper than this aren't looked into.
const DEEPEST: usize = 64;
/// How often `told` hears how far the map has come.
const TELL_EVERY: Duration = Duration::from_millis(200);
/// Other apps' data, from the home folder: macOS may ask before it is read.
const OTHER_APPS: &[&str] = &["Library/Containers", "Library/Group Containers"];

pub(crate) fn chart(
    chart: &Chart,
    stop: &AtomicBool,
    told: &mut dyn FnMut(Charting),
) -> io::Result<Area> {
    let home = Dir::open(chart.home)?;
    let stat = home.metadata()?;
    let mut walker = Walker {
        chart,
        stop,
        told,
        device: stat.st_dev,
        linked: HashSet::new(),
        bytes: 0,
        items: 0,
        last: Instant::now(),
    };
    let mut root = walker.folder(&home, Path::new(""), &stat, 0);
    trimmed(&mut root, KEPT_AT_MOST);
    numbered(&mut root, &mut 0);
    Ok(root)
}

struct Walker<'a> {
    chart: &'a Chart<'a>,
    stop: &'a AtomicBool,
    told: &'a mut dyn FnMut(Charting),
    device: libc::dev_t,
    /// Files with more than one name, so each is counted once.
    linked: HashSet<(libc::dev_t, libc::ino_t)>,
    bytes: u64,
    items: u64,
    last: Instant,
}

impl Walker<'_> {
    /// The folder `dir`, at `relative` from the home folder and stored as
    /// `stat`, with everything it holds. What is inside is taken in as it
    /// is read, folders as they are gone through, so only what may be shown
    /// is held, never the whole listing.
    fn folder(&mut self, dir: &Dir, relative: &Path, stat: &libc::stat, depth: usize) -> Area {
        let mut area = self.area(relative, stat, measure::blocks(stat));
        self.counted(area.bytes, relative);
        if self.stopped() {
            area.short = Some(Short::Stopped);
            return area;
        }
        if depth >= DEEPEST {
            area.short = Some(Short::Deep);
            return area;
        }
        let stop = self.stop;
        let listed = dir.each_seen(|| !stop.load(Ordering::Relaxed), &mut |name, stat| {
            if stat.st_dev != self.device {
                return;
            }
            let below = relative.join(name);
            let each = if walk::is_folder(stat) {
                self.inner(dir, name, &below, stat, depth)
            } else {
                // A file with more names takes its room once, under the first one met.
                let again = stat.st_nlink > 1 && !self.linked.insert((stat.st_dev, stat.st_ino));
                let bytes = if again { 0 } else { measure::blocks(stat) };
                self.counted(bytes, &below);
                self.area(&below, stat, bytes)
            };
            offered(&mut area, each);
        });
        if !listed.is_ok_and(|missed| !missed) {
            area.short = area.short.max(Some(Short::Denied));
        }
        // A stop while reading leaves the listing short without failing it.
        if self.stopped() {
            area.short = area.short.max(Some(Short::Stopped));
        }
        area
    }

    /// The folder `name` inside `dir`, gone through unless it is other
    /// apps' data not asked for, or can't be.
    fn inner(
        &mut self,
        dir: &Dir,
        name: &OsStr,
        below: &Path,
        stat: &libc::stat,
        depth: usize,
    ) -> Area {
        let other_apps =
            !self.chart.others && OTHER_APPS.iter().any(|path| below == Path::new(path));
        let short = if other_apps {
            Short::Skipped
        } else if self.stopped() {
            Short::Stopped
        } else {
            match dir.child_if_same(name, stat) {
                Ok(Some(child)) => return self.folder(&child, below, stat, depth + 1),
                Ok(None) => Short::Changed,
                Err(_) => Short::Denied,
            }
        };
        let bytes = measure::blocks(stat);
        self.counted(bytes, below);
        Area {
            short: Some(short),
            ..self.area(below, stat, bytes)
        }
    }

    fn area(&self, relative: &Path, stat: &libc::stat, bytes: u64) -> Area {
        // Joining nothing would add a slash to the home folder's path.
        let path = if relative.as_os_str().is_empty() {
            self.chart.home.to_path_buf()
        } else {
            self.chart.home.join(relative)
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Area {
            id: 0,
            name,
            bytes,
            folder: walk::is_folder(stat),
            short: None,
            inside: Vec::new(),
            rest: 0,
            path,
            identity: Identity::of(stat),
        }
    }

    /// Counts `bytes` more, at `relative`, telling how far it has come now and then.
    fn counted(&mut self, bytes: u64, relative: &Path) {
        self.bytes += bytes;
        self.items += 1;
        if self.last.elapsed() >= TELL_EVERY {
            self.last = Instant::now();
            (self.told)(Charting {
                bytes: self.bytes,
                items: self.items,
                folder: shown(relative.parent().unwrap_or(relative)),
            });
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// Takes `each` into `area`: its room, why it may hold more, and itself if
/// it is among the largest worth showing. A place left out for a reason is
/// worth showing whatever its size, so the reason is seen.
fn offered(area: &mut Area, each: Area) {
    area.bytes += each.bytes;
    area.short = area.short.max(each.short);
    let worth = each.bytes >= SHOWN_FROM || each.short.is_some_and(|short| short > Short::Changed);
    if !worth {
        area.rest += each.bytes;
        return;
    }
    let at = area
        .inside
        .partition_point(|shown| shown.bytes >= each.bytes);
    area.inside.insert(at, each);
    if area.inside.len() > SHOWN_INSIDE
        && let Some(last) = area.inside.pop()
    {
        area.rest += last.bytes;
    }
}

/// Keeps at most `budget` places in `area`, the levels nearest it whole:
/// the deepest level that would go past the budget is counted together.
fn trimmed(area: &mut Area, budget: usize) {
    let mut levels = Vec::new();
    tallied(area, 0, &mut levels);
    let mut kept = 0;
    let deepest = levels
        .iter()
        .take_while(|count| {
            kept += **count;
            kept <= budget
        })
        .count();
    cut(area, 0, deepest.saturating_sub(1));
}

/// Counts the places at each level down from `area`.
fn tallied(area: &Area, depth: usize, levels: &mut Vec<usize>) {
    if levels.len() <= depth {
        levels.push(0);
    }
    levels[depth] += 1;
    for each in &area.inside {
        tallied(each, depth + 1, levels);
    }
}

/// Counts together all that is shown below the level `last`.
fn cut(area: &mut Area, depth: usize, last: usize) {
    if depth == last {
        area.rest += area.inside.drain(..).map(|each| each.bytes).sum::<u64>();
        return;
    }
    for each in &mut area.inside {
        cut(each, depth + 1, last);
    }
}

/// A folder's path as the window shows it, whatever bytes its names hold.
fn shown(relative: &Path) -> String {
    relative.to_string_lossy().into_owned()
}

/// Numbers `area` and what it holds in the order they are shown, from `next`.
fn numbered(area: &mut Area, next: &mut u32) {
    area.id = *next;
    *next += 1;
    for each in &mut area.inside {
        numbered(each, next);
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use super::{shown, trimmed};
    use crate::{Area, Identity};

    fn area(name: &str, bytes: u64, inside: Vec<Area>) -> Area {
        Area {
            id: 0,
            name: name.into(),
            bytes,
            folder: !inside.is_empty(),
            short: None,
            inside,
            rest: 0,
            path: PathBuf::from(name),
            identity: Identity::default(),
        }
    }

    #[test]
    fn keeps_the_levels_nearest_the_top_whole_when_trimming() {
        let files = (0..10)
            .map(|index| area(&format!("f{index}"), 10, Vec::new()))
            .collect();
        let mut root = area("home", 100, vec![area("big", 100, files)]);

        trimmed(&mut root, 5);

        // The big folder stays in sight; what it holds is counted together.
        assert_eq!(root.inside.len(), 1);
        assert!(root.inside[0].inside.is_empty());
        assert_eq!(root.inside[0].rest, 100);
    }

    #[test]
    fn shows_a_folder_whatever_bytes_its_name_holds() {
        let odd = Path::new(OsStr::from_bytes(b"Downloads/caf\xe9"));

        assert_eq!(shown(odd), "Downloads/caf\u{fffd}");
    }
}
