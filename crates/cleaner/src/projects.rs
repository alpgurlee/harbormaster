//! Build outputs inside the user's projects: what `npm install`, `next dev`
//! or `cargo build` made, which a project left alone for a while can do
//! without, since its tools make it again.

use std::cmp::Reverse;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use crate::walk::{self, Dir, Stat};
use crate::{Candidate, Group, Kind, Survey};

/// How many folder levels below a root are looked at. Projects sit near the
/// top; what is deeper is their own files.
const DEEPEST: usize = 6;
/// What a Node project's tools make beside its `package.json`.
const NODE_OUTPUTS: &[&str] = &[
    "node_modules",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
];
/// Outputs so large that looking inside them for projects would take longer
/// than everything else.
const NEVER_INSIDE: &[&str] = &["node_modules", "target"];
/// How a `CACHEDIR.TAG` file starts, by which a folder says its contents can
/// be made again (the Cache Directory Tagging Specification).
const TAG: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// Every build output under the survey's roots, each listed once, picked
/// when its project has been left alone for the survey's `idle_after`.
pub(crate) fn candidates(home: &Dir, survey: &Survey, stop: &AtomicBool) -> Vec<Candidate> {
    let now = survey
        .now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut walk = Walk {
        home: survey.home,
        idle_before: now.saturating_sub(survey.idle_after.as_secs()),
        stop,
        walked: HashSet::new(),
        found: Vec::new(),
    };
    // Spelled as the disk spells them, which is how processes report their
    // paths; a root that is a link resolves outside `home` and is left alone.
    let mut roots: Vec<PathBuf> = survey
        .roots
        .iter()
        .filter_map(|root| walk::canonical(root).ok())
        .collect();
    // A folder is walked once, so the deeper of two nested roots goes first:
    // from it, more of what lies below is within the depth limit.
    roots.sort_by_key(|root| Reverse(root.components().count()));
    for root in roots {
        let Ok(relative) = root.strip_prefix(survey.home) else {
            continue;
        };
        if let Ok(dir) = home.descend(relative) {
            walk.visit(&dir, relative, 0);
        }
    }
    walk.found
}

struct Walk<'a> {
    home: &'a Path,
    /// A project last touched at or before this second counts as left alone.
    idle_before: u64,
    stop: &'a AtomicBool,
    /// Folders already looked at, by disk and inode, so one reached twice,
    /// under nested roots or two spellings, is listed once.
    walked: HashSet<(u64, u128)>,
    found: Vec<Candidate>,
}

impl Walk<'_> {
    /// Takes the outputs directly inside `dir`, then looks inside its other
    /// folders, staying on the same disk.
    fn visit(&mut self, dir: &Dir, relative: &Path, depth: usize) {
        let Ok(own) = dir.metadata() else {
            return;
        };
        if !self.walked.insert((own.device, own.inode)) {
            return;
        }
        let Ok(entries) = dir.entries_while(|| !self.stopped()) else {
            return;
        };
        let is_node = entries
            .iter()
            .any(|(name, stat)| name == "package.json" && walk::is_file(stat));
        let mut touched = None;
        for (name, stat) in &entries {
            if self.stopped() {
                return;
            }
            if !walk::is_folder(stat) || stat.device != own.device {
                continue;
            }
            let Ok(Some(child)) = dir.child_if_same(name, stat) else {
                continue;
            };
            if (is_node && named(name, NODE_OUTPUTS)) || tagged(&child) {
                let touched =
                    *touched.get_or_insert_with(|| self.last_touched(dir, &own, &entries));
                self.found.push(self.output(relative, name, touched));
            } else if depth + 1 < DEEPEST && !hidden(name) && !named(name, NEVER_INSIDE) {
                self.visit(&child, &relative.join(name), depth + 1);
            }
        }
    }

    /// When the project at `dir`, stored as `own`, was last worked on: the
    /// newest change to the folder, to anything directly inside it, or inside
    /// its `.git` folder, which every commit and checkout changes. Unknown
    /// when any of that can't be read.
    fn last_touched(&self, dir: &Dir, own: &Stat, entries: &[(OsString, Stat)]) -> Option<u64> {
        let mut newest = modified(own);
        for (_, stat) in entries {
            newest = newest.max(modified(stat));
        }
        let git = entries
            .iter()
            .find(|(name, stat)| name == ".git" && walk::is_folder(stat));
        if let Some((name, stat)) = git {
            let Ok(Some(git)) = dir.child_if_same(name, stat) else {
                return None;
            };
            for (_, stat) in git.entries_while(|| !self.stopped()).ok()? {
                newest = newest.max(modified(&stat));
            }
        }
        Some(newest)
    }

    fn output(&self, project: &Path, name: &OsStr, touched: Option<u64>) -> Candidate {
        let relative = project.join(name);
        let root = self.home.join(project);
        let title = root.file_name().unwrap_or_default().to_string_lossy();
        Candidate {
            group: Group::Projects,
            kind: Kind::Rebuildable,
            name: format!("{title} · {}", name.to_string_lossy()),
            path: self.home.join(&relative),
            relative,
            picked: touched.is_some_and(|touched| touched <= self.idle_before),
            in_use: None,
            project: Some(root),
            touched,
            emptied: false,
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// Whether `dir` carries a valid `CACHEDIR.TAG`.
pub(crate) fn tagged(dir: &Dir) -> bool {
    dir.read_start(OsStr::new("CACHEDIR.TAG"), TAG.len() as u64)
        .is_ok_and(|start| start == TAG)
}

fn named(name: &OsStr, any: &[&str]) -> bool {
    any.iter().any(|one| name == *one)
}

fn hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

fn modified(stat: &Stat) -> u64 {
    stat.modified
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
