//! What a developer's machine can do without: caches its tools rebuild, the
//! build outputs of projects left alone, logs, and the Trash. A scan only
//! looks; removal takes what the user confirmed, checking each item again.

mod account;
#[cfg(target_os = "macos")]
mod apps;
mod catalog;
mod large;
mod map;
mod measure;
mod projects;
mod remove;
mod space;
mod walk;

use std::fmt;
use std::io;
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
    /// The user's own apps.
    Applications,
}

impl Group {
    /// Whether the group holds the user's large files, which a hunt finds
    /// rather than a scan.
    pub fn of_large_files(self) -> bool {
        matches!(
            self,
            Self::Videos
                | Self::Archives
                | Self::Installers
                | Self::Audio
                | Self::Images
                | Self::Documents
                | Self::OtherFiles
        )
    }
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
    device: u64,
    inode: u128,
    pub(crate) folder: bool,
}

impl Identity {
    pub(crate) fn of(stat: &walk::Stat) -> Self {
        Self {
            device: stat.device,
            inode: stat.inode,
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

impl Outcome {
    /// Whether the finding was left just as the scan measured it: never
    /// reached, or turned down for what was using it. Anything else may be
    /// gone, partly gone, or something else by now.
    pub fn left_as_it_was(&self) -> bool {
        matches!(
            self,
            Self::Stopped
                | Self::Refused {
                    why: Why::InUse { .. } | Why::Unchecked { .. }
                }
        )
    }
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

/// The folder apps are shared from on a Mac.
pub const SHARED_APPS: &str = "/Applications";

/// Where removing may go: anywhere in the home folder, and in the shared
/// app folder for apps alone, which only ever go to the Trash.
#[derive(Debug, Clone, Copy)]
pub struct Reach<'a> {
    /// The account's home folder as the disk spells it, which `account_home` gives.
    pub home: &'a Path,
    /// The shared app folder: `SHARED_APPS` on a Mac.
    pub shared: &'a Path,
}

/// What a map of the home folder looks through.
#[derive(Debug, Clone, Copy)]
pub struct Chart<'a> {
    /// The account's home folder as the disk spells it, which `account_home` gives.
    pub home: &'a Path,
    /// Whether to look in other apps' data too, which macOS may ask about.
    pub others: bool,
}

/// A folder or file in a map of the home folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Area {
    /// Its place in the map, counted from the home folder's 0, which the
    /// window names it by.
    pub id: u32,
    pub name: String,
    /// The room it takes on disk, with everything it holds.
    pub bytes: u64,
    pub folder: bool,
    /// Why it may hold more than `bytes` says, when it may.
    pub short: Option<Short>,
    /// What it holds that is large enough to show, largest first.
    pub inside: Vec<Area>,
    /// What else it holds, each too small to show on its own.
    pub rest: u64,
    /// Whether all it holds is counted in `rest` to keep the map small,
    /// whatever the size of each.
    pub folded: bool,
    /// Where it is, and which folder or file it was, to show it in Finder.
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(skip)]
    pub identity: Identity,
}

/// Why an area may hold more than it says, the weightiest last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Short {
    /// It goes deeper than is looked into.
    Deep,
    /// Something in it changed while it was being looked at.
    Changed,
    /// Something in it couldn't be read, for its permissions or for macOS's.
    Denied,
    /// Other apps' data, left out unless asked for.
    Skipped,
    /// The map was stopped before it came to everything.
    Stopped,
}

/// How far a map has come, told now and then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Charting {
    /// Room counted so far.
    pub bytes: u64,
    /// Folders and files counted so far.
    pub items: u64,
    /// The folder being looked through, from the home folder, for showing.
    pub folder: String,
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

/// Removes `findings`, which must come from a scan, hunt or app listing of
/// `reach`, one by one, looking at each again right before: it must still be
/// reached through real folders inside the home folder, or for an app inside
/// the shared app folder, still be what was measured, and nothing may be
/// using it by what `usage` reads then. Rebuildable and
/// discarded things are deleted, the Trash first so nothing moved into it
/// goes too; personal things go to the Trash through `trash`, which moves
/// the item at a path there and says where it went. `told` hears of each as
/// it is done. One failing stops nothing else.
pub fn remove(
    reach: &Reach,
    findings: &[Finding],
    usage: &dyn Fn() -> io::Result<Usage>,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
    stop: &AtomicBool,
    told: &mut dyn FnMut(&Removal),
) -> io::Result<Cleaned> {
    let free = || Ok(space::disk(reach.home)?.free);
    remove::all(reach, findings, usage, trash, &free, stop, told)
}

/// A map of `chart.home`: what each folder holds, as the disk counts it,
/// largest first. Everything is counted, a file with more names once, but
/// never through a link or onto another disk, and other apps' data only
/// when asked. Small places are counted together, and the map keeps a
/// bounded number of places whatever the disk holds. `told` hears how far
/// it has come now and then; `stop` cuts it short, keeping what was counted.
pub fn map(chart: &Chart, stop: &AtomicBool, told: &mut dyn FnMut(Charting)) -> io::Result<Area> {
    map::chart(chart, stop, told)
}

/// The user's own apps in `reach`'s shared app folder and in their home
/// folder's, and one folder down in each, largest first and never picked:
/// each goes only to the Trash. An app belongs to someone else, as the App
/// Store's do, or is locked, is left out, as is anything reached through a
/// link or without its Info.plist. One a process of `busy` runs from is in
/// use. `measured` hears after each app is measured; `stop` cuts it short.
#[cfg(target_os = "macos")]
pub fn apps(
    reach: &Reach,
    busy: &[(PathBuf, String)],
    stop: &AtomicBool,
    measured: &(dyn Fn(Measured) + Sync),
) -> io::Result<Vec<Finding>> {
    apps::listed(reach, busy, stop, measured)
}

/// Whether the folder or file at `path` in `home` is still the one with
/// `identity`, as a map found it, reached through real folders.
pub fn still_there(home: &Path, path: &Path, identity: Identity) -> bool {
    walk::Dir::open(home).is_ok_and(|root| remove::still_there(&root, home, path, identity))
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

/// `findings`, from a scan, hunt or app listing of `reach`, as they stand
/// now: what was removed or made again since is left out, and who is using
/// each is read again from `usage`. Sizes stay as they were measured.
pub fn recheck(reach: &Reach, findings: &[Finding], usage: &Usage) -> io::Result<Vec<Finding>> {
    let roots = remove::Roots::open(reach)?;
    Ok(findings
        .iter()
        .filter(|finding| roots.still_there(finding))
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
pub fn in_use(
    path: &Path,
    project: Option<&Path>,
    busy: &[(PathBuf, String)],
    running: &[(PathBuf, String)],
) -> Option<String> {
    let server = running.iter().find(|(root, _)| {
        project.is_some_and(|project| within(root, project) || within(project, root))
    });
    let holder = || busy.iter().find(|(held, _)| holds(held, path, project));
    server.or_else(holder).map(|(_, who)| who.clone())
}

/// Whether a process holding `held` keeps the folder at `path`, a build
/// output of `project` if any, in use: it holds something inside the folder
/// or the project.
pub fn holds(held: &Path, path: &Path, project: Option<&Path>) -> bool {
    within(held, path) || project.is_some_and(|project| within(held, project))
}

/// Whether `path` is `folder` or inside it, a whole name at a time and with
/// names compared as the disk compares them: on Windows, whatever the case.
fn within(path: &Path, folder: &Path) -> bool {
    let mut names = path.components();
    folder.components().all(|own| {
        names.next().is_some_and(|name| match (own, name) {
            #[cfg(windows)]
            (std::path::Component::Normal(own), std::path::Component::Normal(name)) => {
                walk::same_name(own, name)
            }
            (own, name) => own == name,
        })
    })
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
/// whatever `$HOME` or `%USERPROFILE%` says, spelled as the disk spells it
/// and with any link resolved, so that it reads like the paths processes
/// report.
pub fn account_home() -> io::Result<PathBuf> {
    walk::canonical(&account::home()?)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Outcome, Why, holds, in_use};

    #[test]
    fn tells_what_was_left_just_as_the_scan_measured_it() {
        let refused = |why| Outcome::Refused { why };
        let error = || "x".to_string();

        assert!(Outcome::Stopped.left_as_it_was());
        assert!(refused(Why::InUse { by: "java".into() }).left_as_it_was());
        assert!(refused(Why::Unchecked { error: error() }).left_as_it_was());
        // Gone, partly gone, or no longer what was measured.
        assert!(!Outcome::Deleted.left_as_it_was());
        assert!(!Outcome::Trashed.left_as_it_was());
        assert!(!Outcome::Interrupted.left_as_it_was());
        assert!(!Outcome::Failed { why: error() }.left_as_it_was());
        assert!(!refused(Why::Changed).left_as_it_was());
        assert!(!refused(Why::NotReached { error: error() }).left_as_it_was());
        assert!(!refused(Why::OutsideHome).left_as_it_was());
    }

    #[test]
    fn a_process_holding_anything_in_the_folder_or_its_project_keeps_it_in_use() {
        let web = Path::new("/Users/dev/code/web");
        let modules = web.join("node_modules");

        assert!(holds(
            &modules.join("left-pad/index.js"),
            &modules,
            Some(web)
        ));
        assert!(holds(&web.join("src"), &modules, Some(web)));
        assert!(holds(&modules, &modules, None));
        assert!(!holds(
            Path::new("/Users/dev/code/webb"),
            &modules,
            Some(web)
        ));
        assert!(!holds(&web.join("src"), &modules, None));
    }

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
