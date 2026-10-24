//! A map of the home folder: what each folder holds, as the disk counts it,
//! so the window can show where the room goes.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::walk::{self, Dir, Stat};
use crate::{Area, Chart, Charting, Identity, Short};

/// The most places shown inside one folder; the others are counted together.
const SHOWN_INSIDE: usize = 50;
/// A place taking less room than this is counted with the others.
const SHOWN_FROM: u64 = 1024 * 1024;
/// The most places kept below any one folder, and so in the whole map,
/// whatever the disk holds.
const KEPT_AT_MOST: usize = 20_000;
/// Folders deeper than this aren't looked into.
const DEEPEST: usize = 64;
/// How often `told` hears how far the map has come.
const TELL_EVERY: Duration = Duration::from_millis(200);
/// Other apps' data, from the home folder: macOS may ask before it is read.
#[cfg(target_os = "macos")]
const OTHER_APPS: &[&str] = &["Library/Containers", "Library/Group Containers"];
/// Windows asks nothing before any of the home folder is read.
#[cfg(not(target_os = "macos"))]
const OTHER_APPS: &[&str] = &[];
/// The most records of an NTFS file table kept a byte for, many times what
/// a large disk holds; a file past them is kept by its whole id.
#[cfg(windows)]
const RECORDS: u64 = 1 << 26;

pub(crate) fn chart(
    chart: &Chart,
    stop: &AtomicBool,
    told: &mut dyn FnMut(Charting),
) -> io::Result<Area> {
    charted(chart, stop, told, KEPT_AT_MOST, TELL_EVERY)
}

/// The map `chart` asks for, with at most `kept` places below any folder,
/// telling how far it has come every `tell_every`.
fn charted(
    chart: &Chart,
    stop: &AtomicBool,
    told: &mut dyn FnMut(Charting),
    kept: usize,
    tell_every: Duration,
) -> io::Result<Area> {
    let home = Dir::open(chart.home)?;
    let stat = home.metadata()?;
    let mut walker = Walker {
        chart,
        stop,
        told,
        kept,
        tell_every,
        device: stat.device,
        linked: Linked::default(),
        bytes: 0,
        items: 0,
        last: Instant::now(),
    };
    let mut root = walker.folder(&home, Path::new(""), &stat, 0);
    numbered(&mut root, &mut 0);
    Ok(root)
}

struct Walker<'a> {
    chart: &'a Chart<'a>,
    stop: &'a AtomicBool,
    told: &'a mut dyn FnMut(Charting),
    kept: usize,
    tell_every: Duration,
    device: u64,
    /// Files with more than one name, so each is counted once.
    linked: Linked,
    bytes: u64,
    items: u64,
    last: Instant,
}

impl Walker<'_> {
    /// The folder `dir`, at `relative` from the home folder and stored as
    /// `stat`, with everything it holds. What is inside is taken in as it
    /// is read, folders as they are gone through, and each keeps no more
    /// places below it than the map may, so only what may be shown is held:
    /// never a whole listing, nor all of a wide folder's depths.
    fn folder(&mut self, dir: &Dir, relative: &Path, stat: &Stat, depth: usize) -> Area {
        let mut area = self.area(relative, stat, stat.bytes);
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
            if stat.device != self.device {
                return;
            }
            let below = relative.join(name);
            let each = if walk::is_folder(stat) {
                self.inner(dir, name, &below, stat, depth)
            } else {
                // A file with more names takes its room once, under the first one met.
                let again = stat.may_have_other_names() && !self.linked.first(stat.inode);
                let bytes = if again { 0 } else { stat.bytes };
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
        trimmed(&mut area, self.kept);
        area
    }

    /// The folder `name` inside `dir`, gone through unless it is other
    /// apps' data not asked for, or can't be.
    fn inner(&mut self, dir: &Dir, name: &OsStr, below: &Path, stat: &Stat, depth: usize) -> Area {
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
                Err(error) => unopened(&error),
            }
        };
        let bytes = stat.bytes;
        self.counted(bytes, below);
        Area {
            short: Some(short),
            ..self.area(below, stat, bytes)
        }
    }

    fn area(&self, relative: &Path, stat: &Stat, bytes: u64) -> Area {
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
            folded: false,
            path,
            identity: Identity::of(stat),
        }
    }

    /// Counts `bytes` more, at `relative`, telling how far it has come now and then.
    fn counted(&mut self, bytes: u64, relative: &Path) {
        self.bytes += bytes;
        self.items += 1;
        if self.last.elapsed() >= self.tell_every {
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

/// The files met so far that may have more names, by their ids on the one
/// disk mapped. A listing on Windows doesn't say how many names a file
/// has, so there every file is kept, each by a byte for its record in the
/// NTFS file table, which its id numbers: about a megabyte for a million
/// files.
#[derive(Debug, Default)]
struct Linked {
    /// For each record, a mark of the file met in it, or none: one taking
    /// the record over later is another file, and marked otherwise.
    #[cfg(windows)]
    records: Vec<u8>,
    others: HashSet<u128>,
}

impl Linked {
    /// Whether the file `inode` is met for the first time.
    fn first(&mut self, inode: u128) -> bool {
        #[cfg(windows)]
        if let Some((record, mark)) = record_of(inode) {
            let at = record as usize;
            if self.records.len() <= at {
                self.records.resize(at + 1, 0);
            }
            let met = self.records[at] == mark;
            self.records[at] = mark;
            return !met;
        }
        self.others.insert(inode)
    }
}

/// The record in the NTFS file table that `inode` names, the low 48 bits of
/// its id, and a mark, never zero, of how often the record has been used,
/// which the 16 bits above them count. An id of another kind, as ReFS's,
/// has neither.
#[cfg(windows)]
fn record_of(inode: u128) -> Option<(u64, u8)> {
    let reference = inode as u64;
    let record = reference & 0xFFFF_FFFF_FFFF;
    let mark = ((reference >> 48) % 255) as u8 + 1;
    (inode >> 64 == 0 && record < RECORDS).then_some((record, mark))
}

/// Why a folder listed a moment ago couldn't be opened: gone since, or made
/// something else, or kept from us.
fn unopened(error: &io::Error) -> Short {
    if walk::changed(error) {
        Short::Changed
    } else {
        Short::Denied
    }
}

/// Takes `each` into `area`: its room, why it may hold more, and itself if
/// it is among the largest worth showing. A place left out for a reason is
/// worth showing whatever its size, so the reason is seen. Of places as
/// large, the first by name stays first, whichever was read first.
fn offered(area: &mut Area, each: Area) {
    area.bytes += each.bytes;
    area.short = area.short.max(each.short);
    let worth = each.bytes >= SHOWN_FROM || each.short.is_some();
    if !worth {
        area.rest += each.bytes;
        return;
    }
    let at = area.inside.partition_point(|shown| {
        shown.bytes > each.bytes || (shown.bytes == each.bytes && shown.path <= each.path)
    });
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
        area.folded |= !area.inside.is_empty();
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
#[cfg(unix)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use super::{KEPT_AT_MOST, charted, offered, shown, trimmed, unopened};
    use crate::{Area, Chart, Charting, Identity, Short};

    const MIB: usize = 1024 * 1024;

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![1; bytes]).unwrap();
    }

    /// The map of `home` with at most `kept` places, telling `told` of every place counted.
    fn mapped(home: &Path, kept: usize, stop: &AtomicBool, told: &mut dyn FnMut(Charting)) -> Area {
        let chart = Chart {
            home,
            others: false,
        };
        charted(&chart, stop, told, kept, Duration::ZERO).unwrap()
    }

    fn at<'a>(area: &'a Area, names: &[&str]) -> &'a Area {
        names.iter().fold(area, |area, name| {
            area.inside
                .iter()
                .find(|each| each.name == *name)
                .unwrap_or_else(|| panic!("{name} in {:?}", area.inside))
        })
    }

    fn area(name: &str, bytes: u64, inside: Vec<Area>) -> Area {
        Area {
            id: 0,
            name: name.into(),
            bytes,
            folder: !inside.is_empty(),
            short: None,
            inside,
            rest: 0,
            folded: false,
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

        // The big folder stays in sight; what it holds is counted together,
        // and said to be for the map's sake rather than for being small.
        assert_eq!(root.inside.len(), 1);
        assert!(root.inside[0].inside.is_empty());
        assert_eq!(root.inside[0].rest, 100);
        assert!(root.inside[0].folded);
        assert!(!root.folded);
    }

    #[test]
    fn cuts_a_wide_folder_down_without_costing_a_narrow_one_its_depth() {
        let place = tempfile::tempdir().unwrap();
        let home = place.path().canonicalize().unwrap();
        for folder in 0..6 {
            for file in 0..2 {
                write(&home.join(format!("wide/d{folder}/f{file}")), 2 * MIB);
            }
        }
        write(&home.join("narrow/x/y/z/file"), 2 * MIB);

        let root = mapped(&home, 13, &AtomicBool::new(false), &mut |_| {});

        // Its twelve files would take the wide folder past the budget, so
        // what its folders hold is counted together...
        let wide = at(&root, &["wide"]);
        assert_eq!(wide.inside.len(), 6);
        assert!(
            wide.inside
                .iter()
                .all(|each| each.inside.is_empty() && each.rest >= 4 * MIB as u64),
            "{wide:?}"
        );
        // ...while the narrow one, which fits, is kept to the end.
        assert!(at(&root, &["narrow", "x", "y", "z", "file"]).bytes >= 2 * MIB as u64);
    }

    #[test]
    fn stops_while_reading_a_folder_and_keeps_what_it_has_counted() {
        let place = tempfile::tempdir().unwrap();
        let home = place.path().canonicalize().unwrap();
        for name in ["a.bin", "b.bin"] {
            write(&home.join(name), 2 * MIB);
        }
        let stop = AtomicBool::new(false);

        // The home folder is the first place counted, the first file in it the second.
        let root = mapped(&home, KEPT_AT_MOST, &stop, &mut |charting| {
            if charting.items == 2 {
                stop.store(true, Ordering::Relaxed);
            }
        });

        assert_eq!(root.short, Some(Short::Stopped));
        assert_eq!(root.inside.len(), 1);
    }

    #[test]
    fn shows_a_place_left_out_for_any_reason_however_small() {
        let mut home = area("home", 0, Vec::new());

        for short in [Short::Deep, Short::Changed] {
            let small = area("small", 4096, Vec::new());
            offered(
                &mut home,
                Area {
                    short: Some(short),
                    ..small
                },
            );
        }

        assert_eq!(home.inside.len(), 2);
        assert_eq!(home.rest, 0);
    }

    #[test]
    fn keeps_the_same_of_many_as_large_whatever_order_they_come_in() {
        let many: Vec<Area> = (0..51)
            .map(|index| area(&format!("f{index:02}"), 2 * MIB as u64, Vec::new()))
            .collect();
        let mut forward = area("home", 0, Vec::new());
        let mut backward = area("home", 0, Vec::new());

        many.iter()
            .cloned()
            .for_each(|each| offered(&mut forward, each));
        many.into_iter()
            .rev()
            .for_each(|each| offered(&mut backward, each));

        let names = |area: &Area| {
            area.inside
                .iter()
                .map(|each| each.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&forward), names(&backward));
        assert_eq!(forward.rest, backward.rest);
    }

    #[test]
    fn takes_a_folder_gone_or_made_something_else_before_it_was_opened_as_changed() {
        for code in [libc::ENOENT, libc::ELOOP, libc::ENOTDIR] {
            let error = io::Error::from_raw_os_error(code);
            assert_eq!(unopened(&error), Short::Changed, "{error}");
        }
        let denied = io::Error::from_raw_os_error(libc::EACCES);
        assert_eq!(unopened(&denied), Short::Denied);
    }

    #[test]
    fn shows_a_folder_whatever_bytes_its_name_holds() {
        let odd = Path::new(OsStr::from_bytes(b"Downloads/caf\xe9"));

        assert_eq!(shown(odd), "Downloads/caf\u{fffd}");
    }
}

#[cfg(test)]
mod linked {
    use super::Linked;

    #[test]
    fn counts_each_file_once_whatever_kind_its_id_is() {
        let mut linked = Linked::default();

        assert!(linked.first(42));
        assert!(!linked.first(42));
        assert!(linked.first(43));
        // Record 42 taken over by another file, its use counted one higher.
        assert!(linked.first(42 | 1 << 48));
        assert!(!linked.first(42 | 1 << 48));
        // An id past an NTFS file table's numbers, as ReFS gives.
        let wide = 1u128 << 100;
        assert!(linked.first(wide));
        assert!(!linked.first(wide));
    }
}
