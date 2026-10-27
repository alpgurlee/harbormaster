//! The cleaner window's work: scanning what could go, keeping the findings
//! it showed, and removing only those the window then picks from them.

use std::collections::HashSet;
use std::fmt::Display;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

#[cfg(target_os = "macos")]
use harbormaster_cleaner::App;
use harbormaster_cleaner::{
    Apps, Cleaned, Finding, Group, Hunt, Measured, Outcome, Reach, Removal, Survey, Usage, Why,
};
use harbormaster_cleaner::{Area, Chart, Charting};
use harbormaster_engine::{Classification, Scanner, live};
use serde::Serialize;

use crate::activity::{Activity, Doing};
use crate::scanning;

/// The least room a file takes to be listed as large: 50 MB, as the window says.
const LARGE_FROM: u64 = 50 * 1024 * 1024;

/// What a scan tells the window as it goes.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ScanEvent {
    Measured(Measured),
    /// Everything found, by this scan or hunt and the last of the other.
    Done {
        home: PathBuf,
        #[serde(flatten)]
        listed: Listed,
        /// How long a project had to sit for this scan to pick its build
        /// outputs; none for a hunt or a listing of apps.
        idle_days: Option<u32>,
    },
}

/// What the window shows from then on, under the token that removing some
/// of it needs.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Listed {
    token: u64,
    findings: Vec<Finding>,
    /// Whether what tools and apps leave was scanned for, whether the large
    /// files were hunted for, and whether the user's apps were listed, so
    /// that none listed means none found.
    scanned: bool,
    hunted: bool,
    apps_listed: bool,
}

impl From<Shown> for Listed {
    fn from(shown: Shown) -> Self {
        Self {
            token: shown.token,
            findings: shown.findings,
            scanned: shown.scanned,
            hunted: shown.hunted,
            apps_listed: shown.apps_listed,
        }
    }
}

/// What a removal tells the window as it goes.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum RunEvent {
    Removed(Removal),
    /// What the removal did, and what is shown from then on: what wasn't
    /// chosen and what was left as it was.
    Done {
        #[serde(flatten)]
        cleaned: Cleaned,
        #[serde(flatten)]
        listed: Listed,
    },
    /// The removal began but couldn't finish: why, and what is shown from then on.
    Failed {
        refusal: Refusal,
        #[serde(flatten)]
        listed: Listed,
    },
}

/// What a map tells the window as it goes.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum MapEvent {
    Charting(Charting),
    /// The map, under the token showing its places in Finder or File Explorer takes.
    Done {
        token: u64,
        area: Area,
    },
}

/// Why the cleaner turned the window down.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Refusal {
    /// A scan or a removal is under way already.
    Busy,
    /// The scan the window names is gone: removed from already, or replaced.
    Expired,
    /// The window asked for something that scan didn't list.
    Unlisted,
    /// What the window names isn't what was found there any more.
    Changed,
    Failed {
        message: String,
    },
}

/// The findings last shown to the window, under the token it holds, and
/// the scan or removal under way.
#[derive(Debug, Default)]
pub(crate) struct Cleaning {
    ledger: Mutex<Ledger>,
    stop: AtomicBool,
    /// What a restart must not cut short, shared with the stops and quits.
    activity: Arc<Activity>,
}

#[derive(Debug, Default)]
struct Ledger {
    last: u64,
    shown: Option<Shown>,
    /// The last map, kept whole so its places can be shown in Finder or File Explorer; none
    /// of them is ever taken to be removed.
    map: Option<Mapped>,
    /// The apps moved to the Trash from here while the app runs, for what
    /// they left behind.
    #[cfg(target_os = "macos")]
    trashed: Vec<App>,
    working: bool,
}

/// A map shown to the window, under the token it holds.
#[derive(Debug)]
struct Mapped {
    token: u64,
    root: Area,
}

#[derive(Debug, Clone)]
struct Shown {
    token: u64,
    findings: Vec<Finding>,
    /// What a look again found gone since: no longer shown, and left out of
    /// the next search's list, yet still taken if a window not told yet asks
    /// for it, so the removal says it isn't there rather than nothing going.
    gone: Vec<Finding>,
    scanned: bool,
    hunted: bool,
    apps_listed: bool,
}

/// Findings taken to be removed, those of them that were shown, and the
/// rest of what was shown, which is shown again once the removal is over.
#[derive(Debug)]
struct Taken {
    chosen: Vec<Finding>,
    /// The chosen that were shown, rather than gone already: only they may
    /// be shown again.
    shown: Vec<Finding>,
    rest: Shown,
}

/// What looked for the findings: a scan for what tools and apps leave, a
/// hunt for the user's large files, or a listing of the user's apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Search {
    Scan,
    Hunt,
    Apps,
}

impl Search {
    /// The search that finds what is in `group`.
    fn of(group: Group) -> Self {
        if group == Group::Applications {
            Self::Apps
        } else if group.of_large_files() {
            Self::Hunt
        } else {
            Self::Scan
        }
    }
}

/// The one scan or removal under way, taken before its work is queued so
/// that a stop from then on holds; another can start once it is dropped.
/// It outlives the window, which only hides when closed.
#[derive(Debug)]
pub(crate) struct Working {
    cleaning: Arc<Cleaning>,
    /// Counted as under way until the work ends, so an update waits for it.
    _doing: Doing,
}

impl Drop for Working {
    fn drop(&mut self) {
        self.cleaning.lock().working = false;
    }
}

impl Cleaning {
    /// Takes the slot for a scan or removal. A stop from before is
    /// forgotten; one from then on holds.
    pub(crate) fn begin(self: &Arc<Self>) -> Result<Working, Refusal> {
        let doing = self.activity.begin().map_err(|_| Refusal::Failed {
            message: crate::commands::UPDATING.into(),
        })?;
        let mut ledger = self.lock();
        if ledger.working {
            return Err(Refusal::Busy);
        }
        ledger.working = true;
        self.stop.store(false, Ordering::Relaxed);
        Ok(Working {
            cleaning: Arc::clone(self),
            _doing: doing,
        })
    }

    /// One that counts its work with `activity`, as the stops and quits do.
    pub(crate) fn sharing(activity: Arc<Activity>) -> Self {
        Self {
            activity,
            ..Self::default()
        }
    }

    /// Cuts the scan or removal under way short.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Shows what `search` found in place of what the last of its kind
    /// found, beside what the other kind found, all under a new token, and
    /// hands all of it back.
    fn show(&self, search: Search, findings: Vec<Finding>) -> Shown {
        let mut ledger = self.lock();
        ledger.last += 1;
        let mut shown = ledger.shown.take().unwrap_or(Shown {
            token: 0,
            findings: Vec::new(),
            gone: Vec::new(),
            scanned: false,
            hunted: false,
            apps_listed: false,
        });
        shown.token = ledger.last;
        shown.gone.clear();
        shown
            .findings
            .retain(|finding| Search::of(finding.group) != search);
        shown.findings.extend(findings);
        match search {
            Search::Scan => shown.scanned = true,
            Search::Hunt => shown.hunted = true,
            Search::Apps => shown.apps_listed = true,
        }
        ledger.shown = Some(shown.clone());
        shown
    }

    /// The findings at `paths` among those shown under `token`, each once,
    /// with the rest of what was shown. A path must be one listed, byte for
    /// byte. Once taken, nothing is shown, so the token is spent and no look
    /// again can change the list while the removal goes on; turned down,
    /// what is shown stays as it was.
    fn take(&self, token: u64, paths: &[PathBuf]) -> Result<Taken, Refusal> {
        let mut ledger = self.lock();
        let Some(mut rest) = ledger.shown.take_if(|shown| shown.token == token) else {
            return Err(Refusal::Expired);
        };
        let mut seen = HashSet::new();
        let chosen = paths
            .iter()
            .filter(|path| seen.insert(path.as_os_str()))
            .map(|path| {
                rest.findings
                    .iter()
                    .chain(&rest.gone)
                    .find(|finding| finding.path.as_os_str() == path.as_os_str())
                    .cloned()
                    .ok_or(Refusal::Unlisted)
            })
            .collect::<Result<Vec<_>, _>>();
        let chosen = match chosen {
            Ok(chosen) => chosen,
            Err(refusal) => {
                ledger.shown = Some(rest);
                return Err(refusal);
            }
        };
        let (shown, kept) = std::mem::take(&mut rest.findings)
            .into_iter()
            .partition(|finding| chosen.iter().any(|taken| taken.path == finding.path));
        rest.findings = kept;
        rest.gone.clear();
        Ok(Taken {
            chosen,
            shown,
            rest,
        })
    }

    /// Shows `rest`, what a removal didn't take, with `stayed`, what it
    /// left as it was, under a new token, and hands it all back.
    fn put_back(&self, mut rest: Shown, stayed: Vec<Finding>) -> Shown {
        let mut ledger = self.lock();
        ledger.last += 1;
        rest.token = ledger.last;
        rest.findings.extend(stayed);
        ledger.shown = Some(rest.clone());
        rest
    }

    /// The findings shown under `token`, as they stand now: what was removed
    /// or made again since is left out, and who is using each is read again.
    /// They are shown from then on, so the next search keeps them so.
    pub(crate) fn recheck(&self, token: u64) -> Result<Vec<Finding>, Refusal> {
        let findings = self.shown(token)?;
        let home = harbormaster_cleaner::account_home().map_err(failed)?;
        let usage = usage_now().map_err(failed)?;
        let now =
            harbormaster_cleaner::recheck(&reach(&home), &findings, &usage).map_err(failed)?;
        self.looked_again(token, now.clone())?;
        Ok(now)
    }

    /// Shows `now`, what a look again found of the findings shown under
    /// `token`, in their place; what it left out is set aside as gone.
    fn looked_again(&self, token: u64, now: Vec<Finding>) -> Result<(), Refusal> {
        let mut ledger = self.lock();
        let shown = ledger
            .shown
            .as_mut()
            .filter(|shown| shown.token == token)
            .ok_or(Refusal::Expired)?;
        let went = shown
            .findings
            .drain(..)
            .filter(|finding| !now.iter().any(|kept| kept.path == finding.path));
        shown.gone.extend(went);
        shown.findings = now;
        Ok(())
    }

    pub(crate) fn shown(&self, token: u64) -> Result<Vec<Finding>, Refusal> {
        self.lock()
            .shown
            .as_ref()
            .filter(|shown| shown.token == token)
            .map(|shown| shown.findings.clone())
            .ok_or(Refusal::Expired)
    }

    /// Whether the findings shown now list `path`, byte for byte.
    pub(crate) fn lists(&self, path: &Path) -> bool {
        self.lock().shown.as_ref().is_some_and(|shown| {
            shown
                .findings
                .iter()
                .any(|finding| finding.path.as_os_str() == path.as_os_str())
        })
    }

    /// Keeps `root` as the map shown from now on, in place of the last,
    /// and gives the token that showing its places in Finder or File Explorer takes.
    fn keep_map(&self, root: Area) -> u64 {
        let mut ledger = self.lock();
        ledger.last += 1;
        let token = ledger.last;
        ledger.map = Some(Mapped { token, root });
        token
    }

    /// Where the place `id` of the map under `token` is, if it is still
    /// what the map found there. The home folder itself isn't one of them.
    pub(crate) fn mapped(&self, token: u64, id: u32) -> Result<PathBuf, Refusal> {
        let (home, path, identity) = {
            let ledger = self.lock();
            let map = ledger
                .map
                .as_ref()
                .filter(|map| map.token == token)
                .ok_or(Refusal::Expired)?;
            let place = map
                .root
                .inside
                .iter()
                .find_map(|each| place(each, id))
                .ok_or(Refusal::Unlisted)?;
            (map.root.path.clone(), place.path.clone(), place.identity)
        };
        // Finder is told a path as text, so one that isn't would be told as another.
        #[cfg(target_os = "macos")]
        if path.to_str().is_none() {
            return Err(failed("this path can't be shown in Finder"));
        }
        // Looked at once the lock is let go: it reads the disk.
        if !harbormaster_cleaner::still_there(&home, &path, identity) {
            return Err(Refusal::Changed);
        }
        Ok(path)
    }

    /// Keeps `apps`, moved to the Trash from here, for what they left behind.
    #[cfg(target_os = "macos")]
    fn remember_trashed(&self, apps: impl IntoIterator<Item = App>) {
        let mut ledger = self.lock();
        for app in apps {
            if !ledger.trashed.iter().any(|known| known.ids == app.ids) {
                ledger.trashed.push(app);
            }
        }
    }

    /// The apps moved to the Trash from here while the app runs.
    #[cfg(target_os = "macos")]
    fn trashed(&self) -> Vec<App> {
        self.lock().trashed.clone()
    }

    fn lock(&self) -> MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Working {
    /// Scans this user's home folder, telling `told` as it goes: projects
    /// in `roots`, or the usual folders when there are none, their build
    /// outputs picked once left alone for `idle_days`.
    pub(crate) fn scan(
        &self,
        roots: &[PathBuf],
        idle_days: u32,
        apps: Option<&Apps>,
        told: &(dyn Fn(ScanEvent) + Sync),
    ) -> Result<(), Refusal> {
        let cleaning = &self.cleaning;
        let home = harbormaster_cleaner::account_home().map_err(failed)?;
        let roots = roots_or_usual(&home, roots);
        let usage = usage_now().map_err(failed)?;
        let temp = harbormaster_cleaner::temp_folder(&home);
        // A Mac's scan is told what was moved to the Trash from here.
        #[cfg(target_os = "macos")]
        let trashed = Apps {
            removed: cleaning.trashed(),
            ..Apps::default()
        };
        #[cfg(target_os = "macos")]
        let apps = apps.or(Some(&trashed));
        let survey = Survey {
            apps,
            ..survey(
                &home,
                &roots,
                idle_after(idle_days),
                &usage,
                temp.as_deref(),
            )
        };
        let mut findings = harbormaster_cleaner::scan_with(&survey, &cleaning.stop, &|measured| {
            told(ScanEvent::Measured(measured));
        })
        .map_err(failed)?;
        findings.retain(showable);
        let listed = cleaning.show(Search::Scan, findings).into();
        told(ScanEvent::Done {
            home,
            listed,
            idle_days: Some(idle_days),
        });
        Ok(())
    }

    /// Hunts for this user's large files, telling `told` as it goes.
    pub(crate) fn hunt(&self, told: &(dyn Fn(ScanEvent) + Sync)) -> Result<(), Refusal> {
        let cleaning = &self.cleaning;
        let home = harbormaster_cleaner::account_home().map_err(failed)?;
        let usage = usage_now().map_err(failed)?;
        let hunt = Hunt {
            home: &home,
            smallest: LARGE_FROM,
            busy: &usage.busy,
        };
        let mut findings = harbormaster_cleaner::large(&hunt, &cleaning.stop, &|measured| {
            told(ScanEvent::Measured(measured));
        })
        .map_err(failed)?;
        findings.retain(showable);
        let listed = cleaning.show(Search::Hunt, findings).into();
        told(ScanEvent::Done {
            home,
            listed,
            idle_days: None,
        });
        Ok(())
    }

    /// Maps this user's home folder, and other apps' data in it when
    /// `others` asks, telling `told` as it goes.
    pub(crate) fn map(&self, others: bool, told: &mut dyn FnMut(MapEvent)) -> Result<(), Refusal> {
        let cleaning = &self.cleaning;
        let home = harbormaster_cleaner::account_home().map_err(failed)?;
        let chart = Chart {
            home: &home,
            others,
        };
        let root = harbormaster_cleaner::map(&chart, &cleaning.stop, &mut |charting| {
            told(MapEvent::Charting(charting));
        })
        .map_err(failed)?;
        let token = cleaning.keep_map(root.clone());
        told(MapEvent::Done { token, area: root });
        Ok(())
    }

    /// Lists this user's own apps, telling `told` as it goes.
    #[cfg(target_os = "macos")]
    pub(crate) fn apps(&self, told: &(dyn Fn(ScanEvent) + Sync)) -> Result<(), Refusal> {
        let cleaning = &self.cleaning;
        let home = harbormaster_cleaner::account_home().map_err(failed)?;
        let usage = usage_now().map_err(failed)?;
        let mut findings =
            harbormaster_cleaner::apps(&reach(&home), &usage.busy, &cleaning.stop, &|measured| {
                told(ScanEvent::Measured(measured));
            })
            .map_err(failed)?;
        findings.retain(showable);
        let listed = cleaning.show(Search::Apps, findings).into();
        told(ScanEvent::Done {
            home,
            listed,
            idle_days: None,
        });
        Ok(())
    }

    /// Removes the findings at `paths` out of those the scan under `token`
    /// showed, telling `told` of each as it goes.
    pub(crate) fn run(
        &self,
        token: u64,
        paths: &[PathBuf],
        told: &mut dyn FnMut(RunEvent),
    ) -> Result<(), Refusal> {
        let cleaning = &self.cleaning;
        let Taken {
            chosen,
            shown,
            rest,
        } = cleaning.take(token, paths)?;
        let mut removals = Vec::new();
        let home = harbormaster_cleaner::account_home();
        // Who each app is must be read before it goes, from its own bundle.
        #[cfg(target_os = "macos")]
        let identities: Vec<(PathBuf, App)> = home
            .as_ref()
            .map(|home| {
                chosen
                    .iter()
                    .filter(|finding| finding.group == Group::Applications)
                    .filter_map(|finding| {
                        harbormaster_cleaner::app_identity(&reach(home), finding)
                            .map(|app| (finding.path.clone(), app))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let cleaned = home.and_then(|home| {
            harbormaster_cleaner::remove(
                &reach(&home),
                &chosen,
                &usage_now,
                &|path| harbormaster_platform::trash(path).map_err(io::Error::other),
                &cleaning.stop,
                &mut |removal| {
                    removals.push(removal.clone());
                    told(RunEvent::Removed(removal.clone()));
                },
            )
        });
        // Only an app that did go to the Trash is one to look for the data of.
        #[cfg(target_os = "macos")]
        cleaning.remember_trashed(identities.into_iter().filter_map(|(path, app)| {
            removals
                .iter()
                .any(|removal| removal.path == path && removal.outcome == Outcome::Trashed)
                .then_some(app)
        }));
        // However it ended, the list is shown again before the window hears.
        let listed = cleaning.put_back(rest, stayed(shown, &removals)).into();
        told(match cleaned {
            Ok(cleaned) => RunEvent::Done { cleaned, listed },
            Err(error) => RunEvent::Failed {
                refusal: failed(error),
                listed,
            },
        });
        Ok(())
    }
}

/// Of the findings chosen, those a removal left just as they were, or
/// never came to, each in use by what it found using it; the rest may be
/// gone or partly gone.
fn stayed(chosen: Vec<Finding>, removals: &[Removal]) -> Vec<Finding> {
    chosen
        .into_iter()
        .filter_map(|mut finding| {
            let outcome = removals
                .iter()
                .find(|removal| removal.path == finding.path)
                .map(|removal| &removal.outcome);
            match outcome {
                Some(Outcome::Refused {
                    why: Why::InUse { by },
                }) => finding.in_use = Some(by.clone()),
                Some(outcome) if !outcome.left_as_it_was() => return None,
                _ => {}
            }
            Some(finding)
        })
        .collect()
}

/// The place numbered `id`, `area` or one below it.
fn place(area: &Area, id: u32) -> Option<&Area> {
    if area.id == id {
        return Some(area);
    }
    area.inside.iter().find_map(|each| place(each, id))
}

/// Where removing may go for this user: their home folder, and the Mac's
/// shared app folder for their own apps.
fn reach(home: &Path) -> Reach<'_> {
    Reach {
        home,
        shared: harbormaster_cleaner::SHARED_APPS.map(Path::new),
    }
}

/// Whether the window can be told of `finding`: its paths must be text.
/// APFS names always are; another disk mounted in the home folder may not be.
fn showable(finding: &Finding) -> bool {
    serde_json::to_value(finding).is_ok()
}

/// Where the user said their projects are, or the usual folders when they
/// said nowhere.
fn roots_or_usual(home: &Path, chosen: &[PathBuf]) -> Vec<PathBuf> {
    if chosen.is_empty() {
        harbormaster_cleaner::usual_roots(home)
    } else {
        chosen.to_vec()
    }
}

fn idle_after(days: u32) -> Duration {
    Duration::from_secs(u64::from(days) * 86_400)
}

fn survey<'a>(
    home: &'a Path,
    roots: &'a [PathBuf],
    idle_after: Duration,
    usage: &'a Usage,
    temp: Option<&'a Path>,
) -> Survey<'a> {
    Survey {
        home,
        roots,
        busy: &usage.busy,
        running: &usage.running,
        idle_after,
        now: SystemTime::now(),
        temp,
        apps: None,
    }
}

/// The paths this user's other processes hold, and the projects with a dev
/// server running.
fn usage_now() -> io::Result<Usage> {
    let busy = live::held().map_err(io::Error::other)?;
    let running = servers_running().map_err(io::Error::other)?;
    Ok(Usage { busy, running })
}

/// The projects with a dev server running, each with what to call the
/// server, named the same in every language: `localhost:3000`.
pub(crate) fn servers_running() -> Result<Vec<(PathBuf, String)>, harbormaster_platform::Error> {
    let snapshot = live::scan(&mut Scanner::default(), &scanning::home())?;
    Ok(snapshot
        .services
        .iter()
        .filter(|service| service.classification == Classification::Dev)
        .filter_map(|service| {
            let root = service.project.as_ref()?.root.clone();
            let port = service.ports.first()?.port;
            Some((root, format!("localhost:{port}")))
        })
        .collect())
}

fn failed(error: impl Display) -> Refusal {
    Refusal::Failed {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    #[cfg(target_os = "macos")]
    use harbormaster_cleaner::{Area, Chart, Charting};
    use harbormaster_cleaner::{
        Cleaned, Finding, Group, Identity, Kind, Measured, Outcome, Removal, Why,
    };

    use std::time::Duration;

    #[cfg(target_os = "macos")]
    use super::MapEvent;
    use super::{
        Cleaning, Listed, Refusal, RunEvent, ScanEvent, Search, idle_after, roots_or_usual, stayed,
    };

    fn finding(path: &str) -> Finding {
        Finding {
            group: Group::Packages,
            kind: Kind::Rebuildable,
            name: "npm".into(),
            path: PathBuf::from(path),
            bytes: 1,
            picked: true,
            in_use: None,
            touched: None,
            project: None,
            emptied: true,
            identity: Identity::default(),
        }
    }

    /// `path` and then a unit that isn't text, as a disk may hold in a name.
    fn untextual(path: &str) -> PathBuf {
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::OsStr::from_bytes(&[path.as_bytes(), b"\xff"].concat()).to_owned()
        };
        #[cfg(windows)]
        let path = {
            use std::os::windows::ffi::OsStringExt;
            let lone_surrogate = 0xD800;
            let units: Vec<u16> = path.encode_utf16().chain([lone_surrogate]).collect();
            std::ffi::OsString::from_wide(&units)
        };
        PathBuf::from(path)
    }

    fn paths(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    /// Shows `findings` as a scan found them, handing back their token.
    fn scanned(cleaning: &Cleaning, findings: Vec<Finding>) -> u64 {
        cleaning.show(Search::Scan, findings).token
    }

    /// What a removal under `token` would take of `paths`.
    fn chosen(cleaning: &Cleaning, token: u64, paths: &[PathBuf]) -> Result<Vec<Finding>, Refusal> {
        cleaning.take(token, paths).map(|taken| taken.chosen)
    }

    /// A home folder holding one large film, under its real path.
    #[cfg(target_os = "macos")]
    fn home_with_a_film() -> (tempfile::TempDir, PathBuf) {
        let place = tempfile::tempdir().unwrap();
        let home = place.path().canonicalize().unwrap();
        fs::create_dir_all(home.join("Movies")).unwrap();
        fs::write(home.join("Movies/trip.mov"), vec![1; 2 << 20]).unwrap();
        (place, home)
    }

    #[cfg(target_os = "macos")]
    fn mapped(home: &std::path::Path) -> Area {
        let chart = Chart {
            home,
            others: false,
        };
        let stop = std::sync::atomic::AtomicBool::new(false);
        harbormaster_cleaner::map(&chart, &stop, &mut |_| {}).unwrap()
    }

    /// The number of the place at `names` down from `area`.
    #[cfg(target_os = "macos")]
    fn id(area: &Area, names: &[&str]) -> u32 {
        names
            .iter()
            .fold(area, |area, name| {
                area.inside.iter().find(|each| each.name == *name).unwrap()
            })
            .id
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn shows_in_finder_only_a_place_of_the_last_map_still_as_it_was() {
        let (_place, home) = home_with_a_film();
        let cleaning = Cleaning::default();
        let root = mapped(&home);
        let trip = id(&root, &["Movies", "trip.mov"]);
        let token = cleaning.keep_map(root);

        assert_eq!(
            cleaning.mapped(token, trip),
            Ok(home.join("Movies/trip.mov"))
        );
        assert_eq!(cleaning.mapped(token + 1, trip), Err(Refusal::Expired));
        assert_eq!(cleaning.mapped(token, 999), Err(Refusal::Unlisted));
        // The home folder itself isn't one of its places.
        assert_eq!(cleaning.mapped(token, 0), Err(Refusal::Unlisted));

        fs::remove_file(home.join("Movies/trip.mov")).unwrap();
        assert_eq!(cleaning.mapped(token, trip), Err(Refusal::Changed));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn lets_a_new_map_replace_the_last() {
        let (_place, home) = home_with_a_film();
        let cleaning = Cleaning::default();
        let root = mapped(&home);
        let trip = id(&root, &["Movies", "trip.mov"]);

        let first = cleaning.keep_map(root.clone());
        let second = cleaning.keep_map(root);

        assert_eq!(cleaning.mapped(first, trip), Err(Refusal::Expired));
        assert!(cleaning.mapped(second, trip).is_ok());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn never_takes_a_mapped_place_for_removal() {
        let (_place, home) = home_with_a_film();
        let cleaning = Cleaning::default();
        let film = home.join("Movies/trip.mov");
        let cache = finding("/Users/dev/.npm/_cacache");
        let scan = scanned(&cleaning, vec![cache.clone()]);

        let token = cleaning.keep_map(mapped(&home));

        assert_ne!(scan, token);
        assert!(cleaning.lists(&cache.path));
        assert!(!cleaning.lists(&film));
        assert_eq!(chosen(&cleaning, token, &[film]), Err(Refusal::Expired));
        assert_eq!(
            chosen(&cleaning, scan, std::slice::from_ref(&cache.path)),
            Ok(vec![cache])
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn never_shows_in_finder_a_path_finder_would_be_told_another_way() {
        let cleaning = Cleaning::default();
        let place = |id, path: PathBuf, inside| Area {
            id,
            name: String::new(),
            bytes: 0,
            folder: true,
            short: None,
            inside,
            rest: 0,
            folded: false,
            path,
            identity: Identity::default(),
        };
        let odd = untextual("/Users/dev/caf");
        let root = place(
            0,
            PathBuf::from("/Users/dev"),
            vec![place(1, odd, Vec::new())],
        );

        let token = cleaning.keep_map(root);

        assert!(
            matches!(cleaning.mapped(token, 1), Err(Refusal::Failed { .. })),
            "{:?}",
            cleaning.mapped(token, 1)
        );
    }

    #[test]
    fn takes_the_chosen_out_at_once_and_shows_nothing_while_they_go() {
        let cleaning = Cleaning::default();
        let npm = finding("/Users/dev/.npm/_cacache");
        let pip = finding("/Users/dev/Library/Caches/pip");
        let token = scanned(&cleaning, vec![npm.clone(), pip.clone()]);

        let taken = cleaning
            .take(token, &paths(&["/Users/dev/.npm/_cacache"]))
            .unwrap();

        assert_eq!(
            (taken.chosen, taken.rest.findings),
            (vec![npm], vec![pip.clone()])
        );
        // Nothing is shown meanwhile, so no look again can write it back.
        assert!(!cleaning.lists(&pip.path));
        assert_eq!(
            cleaning.looked_again(token, Vec::new()),
            Err(Refusal::Expired)
        );
    }

    #[test]
    fn turns_a_request_down_without_changing_what_is_shown() {
        let cleaning = Cleaning::default();
        let npm = finding("/Users/dev/.npm/_cacache");
        let token = scanned(&cleaning, vec![npm.clone()]);

        assert_eq!(
            chosen(&cleaning, token, &paths(&["/Users/dev"])),
            Err(Refusal::Unlisted)
        );
        assert!(cleaning.lists(&npm.path));
        assert!(chosen(&cleaning, token, &paths(&["/Users/dev/.npm/_cacache"])).is_ok());
    }

    #[test]
    fn shows_what_stayed_beside_the_rest_under_a_new_token() {
        let cleaning = Cleaning::default();
        let npm = finding("/Users/dev/.npm/_cacache");
        let pip = finding("/Users/dev/Library/Caches/pip");
        let token = scanned(&cleaning, vec![npm.clone(), pip.clone()]);
        let taken = cleaning
            .take(token, &paths(&["/Users/dev/.npm/_cacache"]))
            .unwrap();

        let shown = cleaning.put_back(taken.rest, vec![npm.clone()]);

        assert_ne!(shown.token, token);
        assert_eq!(shown.findings, [pip, npm]);
        assert!(
            chosen(
                &cleaning,
                shown.token,
                &paths(&["/Users/dev/.npm/_cacache"])
            )
            .is_ok()
        );
    }

    #[test]
    fn never_shows_again_what_went_before_the_removal_began() {
        let cleaning = Cleaning::default();
        let npm = finding("/Users/dev/.npm/_cacache");
        let pip = finding("/Users/dev/Library/Caches/pip");
        let token = scanned(&cleaning, vec![npm, pip.clone()]);
        cleaning.looked_again(token, vec![pip.clone()]).unwrap();

        let taken = cleaning
            .take(
                token,
                &paths(&["/Users/dev/.npm/_cacache", "/Users/dev/Library/Caches/pip"]),
            )
            .unwrap();
        assert_eq!(taken.chosen.len(), 2);
        // Whatever becomes of it, say stopped before its turn, npm isn't shown again.
        let shown = cleaning.put_back(taken.rest, stayed(taken.shown, &[]));

        assert_eq!(shown.findings, [pip]);
    }

    #[test]
    fn puts_back_only_what_was_left_just_as_it_was() {
        let at = |path: &str, outcome| Removal {
            path: PathBuf::from(path),
            outcome,
        };
        let removals = [
            at("/a", Outcome::Deleted),
            at(
                "/b",
                Outcome::Refused {
                    why: Why::InUse { by: "java".into() },
                },
            ),
            at("/c", Outcome::Interrupted),
        ];

        // `/d` was never come to before something went wrong.
        let chosen = ["/a", "/b", "/c", "/d"].map(finding).to_vec();

        // What was found in use is shown as such.
        let held = Finding {
            in_use: Some("java".into()),
            ..finding("/b")
        };
        assert_eq!(stayed(chosen, &removals), [held, finding("/d")]);
    }

    #[test]
    fn keeps_what_a_look_again_saw_through_the_next_search() {
        let cleaning = Cleaning::default();
        let npm = finding("/Users/dev/.npm/_cacache");
        let pip = finding("/Users/dev/Library/Caches/pip");
        let token = scanned(&cleaning, vec![npm.clone(), pip.clone()]);
        let held = Finding {
            in_use: Some("java".into()),
            ..pip
        };

        // npm was removed by hand, and pip is in use now.
        cleaning.looked_again(token, vec![held.clone()]).unwrap();

        assert_eq!(cleaning.show(Search::Hunt, Vec::new()).findings, [held]);
    }

    #[test]
    fn still_takes_what_went_for_a_window_not_yet_told() {
        let cleaning = Cleaning::default();
        let npm = finding("/Users/dev/.npm/_cacache");
        let pip = finding("/Users/dev/Library/Caches/pip");
        let token = scanned(&cleaning, vec![npm.clone(), pip.clone()]);
        cleaning.looked_again(token, vec![pip.clone()]).unwrap();
        // What went is no longer shown in Finder, only what is still there.
        assert!(!cleaning.lists(&npm.path));
        assert!(cleaning.lists(&pip.path));

        // The removal then finds it gone and says so, rather than nothing going at all.
        let chosen = chosen(
            &cleaning,
            token,
            &paths(&["/Users/dev/.npm/_cacache", "/Users/dev/Library/Caches/pip"]),
        );

        assert_eq!(chosen, Ok(vec![npm, pip]));
    }

    #[test]
    fn looks_again_only_at_the_scan_still_shown() {
        let cleaning = Cleaning::default();
        let older = scanned(&cleaning, vec![finding("/Users/dev/.npm/_cacache")]);
        let newer = scanned(&cleaning, vec![finding("/Users/dev/.npm/_cacache")]);

        assert_eq!(
            cleaning.looked_again(older, Vec::new()),
            Err(Refusal::Expired)
        );
        assert_eq!(cleaning.shown(newer).map(|shown| shown.len()), Ok(1));
    }

    #[test]
    fn keeps_a_hunt_beside_the_scan_each_in_place_of_its_own_last() {
        let cleaning = Cleaning::default();
        let npm = finding("/Users/dev/.npm/_cacache");
        let pip = finding("/Users/dev/Library/Caches/pip");
        let film = Finding {
            group: Group::Videos,
            kind: Kind::Personal,
            ..finding("/Users/dev/Movies/trip.mov")
        };

        let hunt = cleaning.show(Search::Hunt, vec![film.clone()]);
        assert_eq!((hunt.scanned, hunt.hunted), (false, true));
        let scan = cleaning.show(Search::Scan, vec![npm.clone()]);
        assert_eq!(scan.findings, [film.clone(), npm.clone()]);
        assert_eq!((scan.scanned, scan.hunted), (true, true));
        assert_eq!(chosen(&cleaning, hunt.token, &[]), Err(Refusal::Expired));
        let again = cleaning.show(Search::Scan, vec![pip.clone()]);
        assert_eq!(again.findings, [film, pip.clone()]);
        let none = cleaning.show(Search::Hunt, Vec::new());
        assert_eq!(none.findings, [pip]);
        assert_eq!(chosen(&cleaning, scan.token, &[]), Err(Refusal::Expired));
    }

    #[test]
    fn keeps_the_apps_beside_a_scan_and_a_hunt_in_place_of_their_own_last() {
        let cleaning = Cleaning::default();
        let npm = finding("/Users/dev/.npm/_cacache");
        let film = Finding {
            group: Group::Videos,
            kind: Kind::Personal,
            ..finding("/Users/dev/Movies/trip.mov")
        };
        let app = |path: &str| Finding {
            group: Group::Applications,
            kind: Kind::Personal,
            ..finding(path)
        };
        cleaning.show(Search::Scan, vec![npm.clone()]);
        cleaning.show(Search::Hunt, vec![film.clone()]);

        let apps = cleaning.show(Search::Apps, vec![app("/Applications/Old.app")]);
        assert_eq!(
            (apps.scanned, apps.hunted, apps.apps_listed),
            (true, true, true)
        );
        let again = cleaning.show(Search::Apps, vec![app("/Applications/New.app")]);

        assert_eq!(
            again.findings,
            [npm.clone(), film.clone(), app("/Applications/New.app")]
        );
        let scan = cleaning.show(Search::Scan, Vec::new());
        assert_eq!(scan.findings, [film, app("/Applications/New.app")]);
    }

    #[test]
    fn hands_out_what_a_scan_showed_once() {
        let cleaning = Cleaning::default();
        let token = scanned(&cleaning, vec![finding("/Users/dev/.npm/_cacache")]);

        let taken = chosen(&cleaning, token, &paths(&["/Users/dev/.npm/_cacache"]));

        assert_eq!(taken.unwrap(), [finding("/Users/dev/.npm/_cacache")]);
        assert_eq!(
            chosen(&cleaning, token, &paths(&["/Users/dev/.npm/_cacache"])),
            Err(Refusal::Expired)
        );
    }

    #[test]
    fn takes_nothing_the_scan_didnt_list() {
        let cleaning = Cleaning::default();
        let token = scanned(&cleaning, vec![finding("/Users/dev/.npm/_cacache")]);

        let made_up = chosen(
            &cleaning,
            token,
            &paths(&["/Users/dev/.npm/_cacache", "/Users/dev"]),
        );

        assert_eq!(made_up, Err(Refusal::Unlisted));
        assert!(
            cleaning
                .take(token, &paths(&["/Users/dev/.npm/_cacache"]))
                .is_ok()
        );
    }

    #[test]
    fn lets_a_newer_scan_replace_the_one_before() {
        let cleaning = Cleaning::default();
        let older = scanned(&cleaning, vec![finding("/Users/dev/.npm/_cacache")]);
        let newer = scanned(&cleaning, vec![finding("/Users/dev/.npm/_cacache")]);

        assert_eq!(chosen(&cleaning, older, &[]), Err(Refusal::Expired));
        assert!(chosen(&cleaning, newer, &[]).is_ok());
    }

    #[test]
    fn takes_each_listed_finding_once_and_only_by_its_exact_path() {
        let cleaning = Cleaning::default();
        let token = scanned(&cleaning, vec![finding("/Users/dev/.npm/_cacache")]);

        let odd = chosen(&cleaning, token, &paths(&["/Users/dev/.npm/./_cacache"]));
        let twice = chosen(
            &cleaning,
            token,
            &paths(&["/Users/dev/.npm/_cacache", "/Users/dev/.npm/_cacache"]),
        );

        assert_eq!(odd, Err(Refusal::Unlisted));
        assert_eq!(twice.unwrap(), [finding("/Users/dev/.npm/_cacache")]);
    }

    #[test]
    fn looks_again_only_at_the_findings_shown_now() {
        let cleaning = Cleaning::default();
        let token = scanned(&cleaning, vec![finding("/Users/dev/.npm/_cacache")]);

        assert_eq!(cleaning.shown(token + 1), Err(Refusal::Expired));
        assert_eq!(
            cleaning.shown(token),
            Ok(vec![finding("/Users/dev/.npm/_cacache")])
        );
        // Once a removal took them, there is nothing to look at.
        chosen(&cleaning, token, &[]).unwrap();
        assert_eq!(cleaning.shown(token), Err(Refusal::Expired));
    }

    #[test]
    fn shows_in_finder_only_what_the_scan_listed() {
        let cleaning = Cleaning::default();
        assert!(!cleaning.lists(std::path::Path::new("/Users/dev/.npm/_cacache")));

        scanned(&cleaning, vec![finding("/Users/dev/.npm/_cacache")]);

        assert!(cleaning.lists(std::path::Path::new("/Users/dev/.npm/_cacache")));
        assert!(!cleaning.lists(std::path::Path::new("/Users/dev/.npm")));
        assert!(!cleaning.lists(std::path::Path::new("/Users/dev/.npm/./_cacache")));
    }

    #[test]
    fn leaves_out_what_the_window_cant_be_told_of() {
        let mut odd = finding("/");
        odd.path = untextual("/Users/dev/");

        assert!(super::showable(&finding("/Users/dev/.npm/_cacache")));
        assert!(!super::showable(&odd));
    }

    #[test]
    fn runs_one_scan_or_removal_at_a_time() {
        let cleaning = Arc::new(Cleaning::default());
        let working = cleaning.begin().unwrap();

        assert!(matches!(cleaning.begin(), Err(Refusal::Busy)));
        drop(working);
        assert!(cleaning.begin().is_ok());
    }

    #[test]
    fn keeps_a_stop_that_comes_once_the_work_is_taken_on() {
        let cleaning = Arc::new(Cleaning::default());
        cleaning.stop();

        let _working = cleaning.begin().unwrap();
        assert!(!cleaning.stop.load(Ordering::Relaxed));

        cleaning.stop();
        assert!(cleaning.stop.load(Ordering::Relaxed));
    }

    #[test]
    fn tells_the_window_what_it_expects() {
        let measured = ScanEvent::Measured(Measured {
            done: 2,
            total: 9,
            bytes: 5,
        });
        let found = ScanEvent::Done {
            home: PathBuf::from("/Users/dev"),
            listed: Listed {
                token: 3,
                findings: Vec::new(),
                scanned: true,
                hunted: false,
                apps_listed: false,
            },
            idle_days: Some(14),
        };
        let removed = RunEvent::Removed(Removal {
            path: PathBuf::from("/Users/dev/.npm/_cacache"),
            outcome: Outcome::Refused {
                why: Why::InUse { by: "node".into() },
            },
        });
        let done = RunEvent::Done {
            cleaned: Cleaned {
                removals: Vec::new(),
                freed_bytes: 7,
            },
            listed: Listed {
                token: 4,
                findings: Vec::new(),
                scanned: true,
                hunted: false,
                apps_listed: false,
            },
        };
        let failed = RunEvent::Failed {
            refusal: Refusal::Failed {
                message: "no disk".into(),
            },
            listed: Listed {
                token: 5,
                findings: Vec::new(),
                scanned: true,
                hunted: true,
                apps_listed: false,
            },
        };
        let json = |value: serde_json::Result<serde_json::Value>| value.unwrap();
        assert_eq!(
            json(serde_json::to_value(failed)),
            serde_json::json!({"kind": "failed", "refusal": {"code": "failed", "message": "no disk"}, "token": 5, "findings": [], "scanned": true, "hunted": true, "appsListed": false})
        );

        assert_eq!(
            json(serde_json::to_value(measured)),
            serde_json::json!({"kind": "measured", "done": 2, "total": 9, "bytes": 5})
        );
        assert_eq!(
            json(serde_json::to_value(found)),
            serde_json::json!({"kind": "done", "token": 3, "home": "/Users/dev", "findings": [], "scanned": true, "hunted": false, "appsListed": false, "idleDays": 14})
        );
        assert_eq!(
            json(serde_json::to_value(removed)),
            serde_json::json!({
                "kind": "removed",
                "path": "/Users/dev/.npm/_cacache",
                "outcome": "refused",
                "why": {"code": "inUse", "by": "node"},
            })
        );
        assert_eq!(
            json(serde_json::to_value(done)),
            serde_json::json!({"kind": "done", "removals": [], "freedBytes": 7, "token": 4, "findings": [], "scanned": true, "hunted": false, "appsListed": false})
        );
        assert_eq!(
            json(serde_json::to_value(Refusal::Unlisted)),
            serde_json::json!({"code": "unlisted"})
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn tells_the_window_of_its_map_as_it_expects() {
        let charting = MapEvent::Charting(Charting {
            bytes: 8,
            items: 2,
            folder: "Library/Caches".into(),
        });
        let (_place, home) = home_with_a_film();
        let mut area = mapped(&home);
        area.inside.clear();
        area.bytes = 6;
        area.rest = 6;
        area.short = None;
        let charted = MapEvent::Done { token: 6, area };
        let json = |value: serde_json::Result<serde_json::Value>| value.unwrap();
        assert_eq!(
            json(serde_json::to_value(charting)),
            serde_json::json!({"kind": "charting", "bytes": 8, "items": 2, "folder": "Library/Caches"})
        );
        let name = home.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            json(serde_json::to_value(charted)),
            serde_json::json!({"kind": "done", "token": 6, "area": {"id": 0, "name": name, "bytes": 6, "folder": true, "short": null, "inside": [], "rest": 6, "folded": false}})
        );
        assert_eq!(
            json(serde_json::to_value(Refusal::Changed)),
            serde_json::json!({"code": "changed"})
        );
    }

    #[test]
    fn looks_for_projects_where_the_user_said_or_else_in_the_usual_folders() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join("code")).unwrap();
        let chosen = vec![home.path().join("work")];

        assert_eq!(roots_or_usual(home.path(), &[]), [home.path().join("code")]);
        assert_eq!(roots_or_usual(home.path(), &chosen), chosen);
    }

    #[test]
    fn chooses_build_outputs_after_the_days_the_user_set() {
        assert_eq!(idle_after(14), Duration::from_secs(14 * 86_400));
    }
}
