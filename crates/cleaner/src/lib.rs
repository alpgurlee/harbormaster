//! What a developer's machine can do without: caches its tools rebuild, the
//! build outputs of projects left alone, logs, and the Trash. Finding is
//! read-only; nothing here removes anything.

mod catalog;
mod measure;
mod projects;
mod walk;

use std::ffi::{CStr, OsStr};
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use serde::Serialize;

/// Less than this isn't worth the list's room.
const SMALLEST: u64 = 1024 * 1024;

/// Where people usually keep their projects, from the home folder.
const USUAL_ROOTS: &[&str] = &[
    "Developer",
    "Projects",
    "code",
    "src",
    "dev",
    "workspace",
    "repos",
    "work",
];

/// How the cleaner window groups what it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    /// Build outputs inside the user's projects.
    Projects,
    /// Package managers' downloads.
    Packages,
    /// Xcode's build data, device symbols and archives.
    Xcode,
    /// Apps' own caches.
    Apps,
    Logs,
    Trash,
}

/// How a finding would go, which decides whether it is picked unasked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Its tool rebuilds it: deleted, and picked.
    Rebuildable,
    /// The user's own work, such as an Xcode archive: moved to the Trash, never picked unasked.
    Personal,
    /// Already thrown away: emptying it is for good, so never picked unasked.
    Discarded,
}

/// Something that could go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub group: Group,
    pub kind: Kind,
    /// What the window calls it: `npm`, `17.5 (21F79)`, `shop · node_modules`.
    pub name: String,
    pub path: PathBuf,
    /// Space it takes on disk.
    pub bytes: u64,
    /// Chosen unless the user says otherwise.
    pub picked: bool,
    /// Who is using it, when something is, which keeps it from being picked.
    pub in_use: Option<String>,
    /// For a project's build output: when the project was last worked on,
    /// in seconds since the Unix epoch.
    pub touched: Option<u64>,
}

/// What a scan looks through, and what it knows about the machine.
#[derive(Debug, Clone, Copy)]
pub struct Survey<'a> {
    /// The account's home folder as the disk spells it, which `account_home`
    /// gives: an absolute path with a real folder at every step.
    pub home: &'a Path,
    /// Where projects are looked for; a root outside `home` is ignored.
    pub roots: &'a [PathBuf],
    /// Paths processes hold, such as working folders, programs and open
    /// files, each with its process's name.
    pub busy: &'a [(PathBuf, String)],
    /// Project roots with a dev server running, each with what to call the server.
    pub running: &'a [(PathBuf, String)],
    /// How long a project goes untouched before its build outputs are picked.
    pub idle_after: Duration,
    pub now: SystemTime,
}

/// A finding before it is measured.
#[derive(Debug)]
pub(crate) struct Candidate {
    pub(crate) group: Group,
    pub(crate) kind: Kind,
    pub(crate) name: String,
    /// Where it is from the home folder, which it is reopened through.
    pub(crate) relative: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) picked: bool,
    pub(crate) in_use: Option<String>,
    /// The project this is a build output of, whose use is its use.
    pub(crate) project: Option<PathBuf>,
    pub(crate) touched: Option<u64>,
}

/// What could go on the machine `survey` describes, measured, by group and
/// largest first. `stop` cuts a long scan short, keeping what was measured.
pub fn scan(survey: &Survey, stop: &AtomicBool) -> io::Result<Vec<Finding>> {
    let root = walk::Dir::open(survey.home)?;
    let mut candidates = catalog::candidates(&root, survey.home, stop);
    candidates.extend(projects::candidates(&root, survey, stop));
    for candidate in &mut candidates {
        candidate.in_use = in_use(candidate, survey);
        candidate.picked &= candidate.in_use.is_none();
    }
    let mut findings = measure::all(&root, candidates, stop);
    findings.retain(|finding| finding.bytes >= SMALLEST);
    findings.sort_by(|a, b| a.group.cmp(&b.group).then(b.bytes.cmp(&a.bytes)));
    Ok(findings)
}

/// Who is using `candidate`: a dev server running in its project, above it
/// or below it, or else a process holding a path inside it or its project.
fn in_use(candidate: &Candidate, survey: &Survey) -> Option<String> {
    let project = candidate.project.as_deref();
    let server = survey.running.iter().find(|(root, _)| {
        project.is_some_and(|project| root.starts_with(project) || project.starts_with(root))
    });
    let holder = || {
        survey.busy.iter().find(|(path, _)| {
            path.starts_with(&candidate.path)
                || project.is_some_and(|project| path.starts_with(project))
        })
    };
    server.or_else(holder).map(|(_, who)| who.clone())
}

/// The usual project folders that exist under `home`.
pub fn usual_roots(home: &Path) -> Vec<PathBuf> {
    USUAL_ROOTS
        .iter()
        .map(|name| home.join(name))
        .filter(|root| root.is_dir())
        .collect()
}

/// The home folder the user's account names, which the cleaner trusts over
/// whatever `$HOME` says, spelled as the disk spells it and with any link
/// resolved, so that it reads like the paths processes report.
pub fn account_home() -> io::Result<PathBuf> {
    let mut entry = MaybeUninit::<libc::passwd>::uninit();
    let mut text = vec![0; 16 * 1024];
    let mut found = std::ptr::null_mut();
    // SAFETY: `entry` and `text` are the sizes passed, and `found` is where
    // getpwuid_r reports the entry it filled.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            entry.as_mut_ptr(),
            text.as_mut_ptr(),
            text.len(),
            &raw mut found,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if found.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "this account has no entry in the user database",
        ));
    }
    // SAFETY: `found` points at `entry`, whose strings live in `text`.
    let folder = unsafe { CStr::from_ptr((*found).pw_dir) };
    std::fs::canonicalize(OsStr::from_bytes(folder.to_bytes()))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    use super::{Candidate, Group, Kind, Survey, in_use};

    fn output(project: &str) -> Candidate {
        Candidate {
            group: Group::Projects,
            kind: Kind::Rebuildable,
            name: "web · node_modules".into(),
            relative: PathBuf::from("code/mono/apps/web/node_modules"),
            path: PathBuf::from(project).join("node_modules"),
            picked: true,
            in_use: None,
            project: Some(PathBuf::from(project)),
            touched: None,
        }
    }

    fn survey<'a>(running: &'a [(PathBuf, String)]) -> Survey<'a> {
        Survey {
            home: Path::new("/Users/dev"),
            roots: &[],
            busy: &[],
            running,
            idle_after: Duration::ZERO,
            now: SystemTime::now(),
        }
    }

    #[test]
    fn a_dev_server_above_or_below_a_project_is_using_it() {
        let server = |root: &str| vec![(PathBuf::from(root), "the server".to_string())];
        let web = output("/Users/dev/code/mono/apps/web");

        assert!(in_use(&web, &survey(&server("/Users/dev/code/mono"))).is_some());
        assert!(in_use(&web, &survey(&server("/Users/dev/code/mono/apps/web/src"))).is_some());
        assert!(in_use(&web, &survey(&server("/Users/dev/code/mono/apps/api"))).is_none());
        assert!(in_use(&web, &survey(&server("/Users/dev/code/mono/apps/webb"))).is_none());
    }
}
