//! What a developer's machine can do without: caches its tools rebuild, the
//! build outputs of projects left alone, logs, and the Trash. A scan only
//! looks; removal takes what the user confirmed, checking each item again.

mod catalog;
mod large;
mod measure;
mod projects;
mod remove;
mod space;
mod walk;

use std::ffi::{CStr, OsStr};
use std::fmt;
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
    /// The user's large files, by kind: films and screen recordings.
    Videos,
    /// Compressed archives and disk images.
    Archives,
    /// Installers and app packages for this or another system.
    Installers,
    Audio,
    Images,
    Documents,
    /// Large files of any other kind.
    OtherFiles,
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
    /// For a project's build output: the project it belongs to.
    pub project: Option<PathBuf>,
    /// Only what is inside goes; the folder itself stays, as the system or
    /// the app that owns it expects it to be there. Never so for personal
    /// things, which go to the Trash whole.
    pub emptied: bool,
    /// Which folder or file the scan measured, so that removal takes only that one.
    #[serde(skip)]
    pub identity: Identity,
}

/// A folder or file as the disk knows it, whatever it is called: its disk,
/// its inode, and which of the two it is. The default is nothing at all,
/// which nothing on disk matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Identity {
    device: libc::dev_t,
    inode: libc::ino_t,
    pub(crate) folder: bool,
}

impl Identity {
    pub(crate) fn of(stat: &libc::stat) -> Self {
        Self {
            device: stat.st_dev,
            inode: stat.st_ino,
            folder: walk::is_folder(stat),
        }
    }
}

/// How a finding went when its removal was asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum Outcome {
    /// Gone for good.
    Deleted,
    /// In the Trash.
    Trashed,
    /// Left alone: it was in use, or no longer what the scan saw.
    Refused { why: Why },
    /// Something went wrong partway; `why` says what.
    Failed { why: String },
    /// Not reached before the stop, so left as it was.
    Stopped,
    /// Stopped while being removed: some of it may be gone.
    Interrupted,
}

/// Why a finding was left alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Why {
    /// It isn't inside the home folder, or no longer is.
    OutsideHome,
    /// A link or something else stands where a folder on its way was.
    NotReached {
        error: String,
    },
    /// Another folder is in its place now.
    Changed,
    InUse {
        by: String,
    },
    /// What is using what couldn't be read, so it isn't known to be free.
    Unchecked {
        error: String,
    },
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideHome => f.write_str("outside the home folder"),
            Self::NotReached { error } => write!(f, "not reached through real folders ({error})"),
            Self::Changed => f.write_str("no longer the folder the scan measured"),
            Self::InUse { by } => write!(f, "in use by {by}"),
            Self::Unchecked { error } => write!(f, "couldn't tell what is using it ({error})"),
        }
    }
}

/// One finding's removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Removal {
    pub path: PathBuf,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// What a removal did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cleaned {
    pub removals: Vec<Removal>,
    /// Room the disk got back, read from the disk before and after.
    pub freed_bytes: u64,
}

/// How far measuring has come, told after each place is measured. Places
/// are measured several at a time, so `done` can arrive out of order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    /// Places measured so far, this one included.
    pub done: usize,
    /// Places there are to measure.
    pub total: usize,
    /// What this place adds to the list: none when it is too small or
    /// couldn't be measured in full.
    pub bytes: u64,
}

/// How large a disk is, and how much of it is free to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Disk {
    pub total: u64,
    pub free: u64,
}

/// The disk holding `path`, as the disk reports it.
pub fn disk(path: &Path) -> io::Result<Disk> {
    space::disk(path)
}

/// What is using what on the machine, as removal reads it before each item.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    /// Paths processes hold, such as working folders, programs and open
    /// files, each with its process's name.
    pub busy: Vec<(PathBuf, String)>,
    /// Project roots with a dev server running, each with what to call the server.
    pub running: Vec<(PathBuf, String)>,
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

/// What a hunt for large files looks through.
#[derive(Debug, Clone, Copy)]
pub struct Hunt<'a> {
    /// The account's home folder as the disk spells it, which `account_home`
    /// gives: an absolute path with a real folder at every step.
    pub home: &'a Path,
    /// Files taking less room than this are left out.
    pub smallest: u64,
    /// Paths processes hold, each with its process's name.
    pub busy: &'a [(PathBuf, String)],
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
    pub(crate) emptied: bool,
}

/// What could go on the machine `survey` describes, measured, by group and
/// largest first. `stop` cuts a long scan short, keeping what was measured.
pub fn scan(survey: &Survey, stop: &AtomicBool) -> io::Result<Vec<Finding>> {
    scan_with(survey, stop, &|_| {})
}

/// `scan`, telling `measured` after each place is measured.
pub fn scan_with(
    survey: &Survey,
    stop: &AtomicBool,
    measured: &(dyn Fn(Measured) + Sync),
) -> io::Result<Vec<Finding>> {
    let root = walk::Dir::open(survey.home)?;
    let mut candidates = catalog::candidates(&root, survey.home, stop);
    candidates.extend(projects::candidates(&root, survey, stop));
    for candidate in &mut candidates {
        candidate.in_use = in_use(
            &candidate.path,
            candidate.project.as_deref(),
            survey.busy,
            survey.running,
        );
        candidate.picked &= candidate.in_use.is_none();
    }
    let mut findings = measure::all(&root, candidates, stop, &|progress: Measured| {
        let listed = progress.bytes >= SMALLEST;
        measured(Measured {
            bytes: if listed { progress.bytes } else { 0 },
            ..progress
        });
    });
    findings.retain(|finding| finding.bytes >= SMALLEST);
    findings.sort_by(|a, b| a.group.cmp(&b.group).then(b.bytes.cmp(&a.bytes)));
    Ok(findings)
}

/// Removes `findings`, which must come from a scan of `home`, one by one,
/// looking at each again right before: it must still be reached through
/// real folders inside `home`, still be the folder the scan measured, and
/// nothing may be using it by what `usage` reads then. Rebuildable and
/// discarded things are deleted, the Trash first so nothing moved into it
/// goes too; personal things go to the Trash through `trash`, which moves
/// the item at a path there and says where it went. `told` hears of each as
/// it is done. One failing stops nothing else.
pub fn remove(
    home: &Path,
    findings: &[Finding],
    usage: &dyn Fn() -> io::Result<Usage>,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
    stop: &AtomicBool,
    told: &mut dyn FnMut(&Removal),
) -> io::Result<Cleaned> {
    remove::all(home, findings, usage, trash, stop, told)
}

/// The large files in the home folder, by kind and largest first, never
/// picked: each is the user's own, and goes only to the Trash. The Library,
/// hidden folders, apps and other packages, and caches are never looked in.
/// `measured` hears after each folder of the home folder is looked through;
/// `stop` cuts the hunt short, keeping what was found.
pub fn large(
    hunt: &Hunt,
    stop: &AtomicBool,
    measured: &(dyn Fn(Measured) + Sync),
) -> io::Result<Vec<Finding>> {
    large::hunt(hunt, stop, measured)
}

/// `findings`, from a scan of `home`, as they stand now: what was removed or
/// made again since is left out, and who is using each is read again from
/// `usage`. Sizes stay as the scan measured them.
pub fn recheck(home: &Path, findings: &[Finding], usage: &Usage) -> io::Result<Vec<Finding>> {
    let root = walk::Dir::open(home)?;
    Ok(findings
        .iter()
        .filter(|finding| remove::still_there(&root, home, finding))
        .map(|finding| Finding {
            in_use: in_use(
                &finding.path,
                finding.project.as_deref(),
                &usage.busy,
                &usage.running,
            ),
            ..finding.clone()
        })
        .collect())
}

/// Who is using the folder at `path`, a build output of `project` if any: a
/// dev server running in the project, above it or below it, or else a
/// process holding a path inside the folder or the project.
pub(crate) fn in_use(
    path: &Path,
    project: Option<&Path>,
    busy: &[(PathBuf, String)],
    running: &[(PathBuf, String)],
) -> Option<String> {
    let server = running.iter().find(|(root, _)| {
        project.is_some_and(|project| root.starts_with(project) || project.starts_with(root))
    });
    let holder = || {
        busy.iter().find(|(held, _)| {
            held.starts_with(path) || project.is_some_and(|project| held.starts_with(project))
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

    use super::in_use;

    #[test]
    fn a_dev_server_above_or_below_a_project_is_using_it() {
        let web = Path::new("/Users/dev/code/mono/apps/web");
        let modules = web.join("node_modules");
        let server = |root: &str| vec![(PathBuf::from(root), "the server".to_string())];
        let using = |root: &str| in_use(&modules, Some(web), &[], &server(root)).is_some();

        assert!(using("/Users/dev/code/mono"));
        assert!(using("/Users/dev/code/mono/apps/web/src"));
        assert!(!using("/Users/dev/code/mono/apps/api"));
        assert!(!using("/Users/dev/code/mono/apps/webb"));
    }
}
