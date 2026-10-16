//! The cleaner window's work: scanning what could go, keeping the findings
//! it showed, and removing only those the window then picks from them.

use std::collections::HashSet;
use std::fmt::Display;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use harbormaster_cleaner::{Cleaned, Finding, Measured, Removal, Survey, Usage};
use harbormaster_engine::{Classification, Scanner, live};
use serde::Serialize;

use crate::scanning;

/// How long a project sits untouched before its build outputs are picked.
const IDLE_AFTER: Duration = Duration::from_secs(30 * 86_400);

/// What a scan tells the window as it goes.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ScanEvent {
    Measured(Measured),
    /// Everything found, under the token that removing some of it needs.
    Done {
        token: u64,
        home: PathBuf,
        findings: Vec<Finding>,
    },
}

/// What a removal tells the window as it goes.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum RunEvent {
    Removed(Removal),
    Done(Cleaned),
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
    /// The window that asked closed before the work could start.
    Closed,
    /// The scan the window names is gone: removed from already, or replaced.
    Expired,
    /// The window asked for something that scan didn't list.
    Unlisted,
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
}

#[derive(Debug, Default)]
struct Ledger {
    last: u64,
    shown: Option<Shown>,
    working: bool,
}

#[derive(Debug)]
struct Shown {
    token: u64,
    findings: Vec<Finding>,
}

/// The one scan or removal under way, taken before its work is queued so
/// that a stop from then on holds; another can start once it is dropped.
#[derive(Debug)]
pub(crate) struct Working(Arc<Cleaning>);

impl Drop for Working {
    fn drop(&mut self) {
        self.0.lock().working = false;
    }
}

impl Cleaning {
    /// Takes the slot for a scan or removal for the window `open` tells
    /// of. A stop from before is forgotten, so the window is asked only
    /// then: if it closed meanwhile, nothing starts. A stop after holds.
    pub(crate) fn begin(self: &Arc<Self>, open: impl FnOnce() -> bool) -> Result<Working, Refusal> {
        let working = {
            let mut ledger = self.lock();
            if ledger.working {
                return Err(Refusal::Busy);
            }
            ledger.working = true;
            self.stop.store(false, Ordering::Relaxed);
            Working(Arc::clone(self))
        };
        if !open() {
            return Err(Refusal::Closed);
        }
        Ok(working)
    }

    /// Cuts the scan or removal under way short.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Keeps `findings` as the ones shown, in place of any before, and
    /// hands back the token they go by.
    fn show(&self, findings: Vec<Finding>) -> u64 {
        let mut ledger = self.lock();
        ledger.last += 1;
        let token = ledger.last;
        ledger.shown = Some(Shown { token, findings });
        token
    }

    /// The findings at `paths` among those shown under `token`, each once,
    /// which can be taken from once. A path must be one listed, byte for byte.
    fn take(&self, token: u64, paths: &[PathBuf]) -> Result<Vec<Finding>, Refusal> {
        let mut ledger = self.lock();
        let shown = ledger
            .shown
            .as_ref()
            .filter(|shown| shown.token == token)
            .ok_or(Refusal::Expired)?;
        let mut seen = HashSet::new();
        let chosen = paths
            .iter()
            .filter(|path| seen.insert(path.as_os_str()))
            .map(|path| {
                shown
                    .findings
                    .iter()
                    .find(|finding| finding.path.as_os_str() == path.as_os_str())
                    .cloned()
                    .ok_or(Refusal::Unlisted)
            })
            .collect::<Result<Vec<_>, _>>()?;
        ledger.shown = None;
        Ok(chosen)
    }

    /// The findings shown under `token`, as they stand now: what was removed
    /// or made again since is left out, and who is using each is read again.
    /// What is shown stays as it was; a removal looks at each again anyway.
    pub(crate) fn recheck(&self, token: u64) -> Result<Vec<Finding>, Refusal> {
        let findings = self.shown(token)?;
        let home = harbormaster_cleaner::account_home().map_err(failed)?;
        let usage = usage_now().map_err(failed)?;
        harbormaster_cleaner::recheck(&home, &findings, &usage).map_err(failed)
    }

    fn shown(&self, token: u64) -> Result<Vec<Finding>, Refusal> {
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

    fn lock(&self) -> MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Working {
    /// Scans this user's home folder, telling `told` as it goes.
    pub(crate) fn scan(&self, told: &(dyn Fn(ScanEvent) + Sync)) -> Result<(), Refusal> {
        let cleaning = &self.0;
        let home = harbormaster_cleaner::account_home().map_err(failed)?;
        let roots = harbormaster_cleaner::usual_roots(&home);
        let usage = usage_now().map_err(failed)?;
        let survey = survey(&home, &roots, &usage);
        let mut findings = harbormaster_cleaner::scan_with(&survey, &cleaning.stop, &|measured| {
            told(ScanEvent::Measured(measured));
        })
        .map_err(failed)?;
        findings.retain(showable);
        let token = cleaning.show(findings.clone());
        told(ScanEvent::Done {
            token,
            home,
            findings,
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
        let cleaning = &self.0;
        let chosen = cleaning.take(token, paths)?;
        let home = harbormaster_cleaner::account_home().map_err(failed)?;
        let cleaned = harbormaster_cleaner::remove(
            &home,
            &chosen,
            &usage_now,
            &|path| harbormaster_platform::trash(path).map_err(io::Error::other),
            &cleaning.stop,
            &mut |removal| told(RunEvent::Removed(removal.clone())),
        )
        .map_err(failed)?;
        told(RunEvent::Done(cleaned));
        Ok(())
    }
}

/// Whether the window can be told of `finding`: its paths must be text.
/// APFS names always are; another disk mounted in the home folder may not be.
fn showable(finding: &Finding) -> bool {
    serde_json::to_value(finding).is_ok()
}

fn survey<'a>(home: &'a Path, roots: &'a [PathBuf], usage: &'a Usage) -> Survey<'a> {
    Survey {
        home,
        roots,
        busy: &usage.busy,
        running: &usage.running,
        idle_after: IDLE_AFTER,
        now: SystemTime::now(),
    }
}

/// The paths this user's other processes hold, and the projects with a dev
/// server running, each named the same in every language: `localhost:3000`.
fn usage_now() -> io::Result<Usage> {
    let busy = live::held().map_err(io::Error::other)?;
    let snapshot =
        live::scan(&mut Scanner::default(), &scanning::home()).map_err(io::Error::other)?;
    let running = snapshot
        .services
        .iter()
        .filter(|service| service.classification == Classification::Dev)
        .filter_map(|service| {
            let root = service.project.as_ref()?.root.clone();
            let port = service.ports.first()?.port;
            Some((root, format!("localhost:{port}")))
        })
        .collect();
    Ok(Usage { busy, running })
}

fn failed(error: impl Display) -> Refusal {
    Refusal::Failed {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use harbormaster_cleaner::{
        Cleaned, Finding, Group, Identity, Kind, Measured, Outcome, Removal, Why,
    };

    use super::{Cleaning, Refusal, RunEvent, ScanEvent};

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

    fn paths(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn hands_out_what_a_scan_showed_once() {
        let cleaning = Cleaning::default();
        let token = cleaning.show(vec![finding("/Users/dev/.npm/_cacache")]);

        let chosen = cleaning.take(token, &paths(&["/Users/dev/.npm/_cacache"]));

        assert_eq!(chosen.unwrap(), [finding("/Users/dev/.npm/_cacache")]);
        assert_eq!(
            cleaning.take(token, &paths(&["/Users/dev/.npm/_cacache"])),
            Err(Refusal::Expired)
        );
    }

    #[test]
    fn takes_nothing_the_scan_didnt_list() {
        let cleaning = Cleaning::default();
        let token = cleaning.show(vec![finding("/Users/dev/.npm/_cacache")]);

        let made_up = cleaning.take(token, &paths(&["/Users/dev/.npm/_cacache", "/Users/dev"]));

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
        let older = cleaning.show(vec![finding("/Users/dev/.npm/_cacache")]);
        let newer = cleaning.show(vec![finding("/Users/dev/.npm/_cacache")]);

        assert_eq!(cleaning.take(older, &[]), Err(Refusal::Expired));
        assert!(cleaning.take(newer, &[]).is_ok());
    }

    #[test]
    fn takes_each_listed_finding_once_and_only_by_its_exact_path() {
        let cleaning = Cleaning::default();
        let token = cleaning.show(vec![finding("/Users/dev/.npm/_cacache")]);

        let odd = cleaning.take(token, &paths(&["/Users/dev/.npm/./_cacache"]));
        let twice = cleaning.take(
            token,
            &paths(&["/Users/dev/.npm/_cacache", "/Users/dev/.npm/_cacache"]),
        );

        assert_eq!(odd, Err(Refusal::Unlisted));
        assert_eq!(twice.unwrap(), [finding("/Users/dev/.npm/_cacache")]);
    }

    #[test]
    fn looks_again_only_at_the_findings_shown_now() {
        let cleaning = Cleaning::default();
        let token = cleaning.show(vec![finding("/Users/dev/.npm/_cacache")]);

        assert_eq!(cleaning.shown(token + 1), Err(Refusal::Expired));
        assert_eq!(
            cleaning.shown(token),
            Ok(vec![finding("/Users/dev/.npm/_cacache")])
        );
        // Once a removal took them, there is nothing to look at.
        cleaning.take(token, &[]).unwrap();
        assert_eq!(cleaning.shown(token), Err(Refusal::Expired));
    }

    #[test]
    fn shows_in_finder_only_what_the_scan_listed() {
        let cleaning = Cleaning::default();
        assert!(!cleaning.lists(std::path::Path::new("/Users/dev/.npm/_cacache")));

        cleaning.show(vec![finding("/Users/dev/.npm/_cacache")]);

        assert!(cleaning.lists(std::path::Path::new("/Users/dev/.npm/_cacache")));
        assert!(!cleaning.lists(std::path::Path::new("/Users/dev/.npm")));
        assert!(!cleaning.lists(std::path::Path::new("/Users/dev/.npm/./_cacache")));
    }

    #[test]
    fn leaves_out_what_the_window_cant_be_told_of() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let mut odd = finding("/");
        odd.path = PathBuf::from(OsStr::from_bytes(b"/Users/dev/\xff"));

        assert!(super::showable(&finding("/Users/dev/.npm/_cacache")));
        assert!(!super::showable(&odd));
    }

    #[test]
    fn runs_one_scan_or_removal_at_a_time() {
        let cleaning = Arc::new(Cleaning::default());
        let working = cleaning.begin(|| true).unwrap();

        assert!(matches!(cleaning.begin(|| true), Err(Refusal::Busy)));
        drop(working);
        assert!(cleaning.begin(|| true).is_ok());
    }

    #[test]
    fn keeps_a_stop_that_comes_once_the_work_is_taken_on() {
        let cleaning = Arc::new(Cleaning::default());
        cleaning.stop();

        let _working = cleaning.begin(|| true).unwrap();
        assert!(!cleaning.stop.load(Ordering::Relaxed));

        cleaning.stop();
        assert!(cleaning.stop.load(Ordering::Relaxed));
    }

    #[test]
    fn starts_nothing_for_a_window_that_closed_before_the_work_was_taken_on() {
        let cleaning = Arc::new(Cleaning::default());
        let open = std::sync::atomic::AtomicBool::new(true);
        // The window closes, setting the stop, before the request is taken on.
        cleaning.stop();
        open.store(false, Ordering::Relaxed);

        // Taking it on forgets that stop, so it must see the window closed.
        let refused = cleaning.begin(|| open.load(Ordering::Relaxed));

        assert!(matches!(refused, Err(Refusal::Closed)));
        assert!(cleaning.begin(|| true).is_ok());
    }

    #[test]
    fn keeps_the_stop_of_a_window_closing_just_as_it_is_asked() {
        let cleaning = Arc::new(Cleaning::default());

        // Open when asked, and closed, with its stop, straight after.
        let _working = cleaning
            .begin(|| {
                cleaning.stop();
                true
            })
            .unwrap();

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
            token: 3,
            home: PathBuf::from("/Users/dev"),
            findings: Vec::new(),
        };
        let removed = RunEvent::Removed(Removal {
            path: PathBuf::from("/Users/dev/.npm/_cacache"),
            outcome: Outcome::Refused {
                why: Why::InUse { by: "node".into() },
            },
        });
        let done = RunEvent::Done(Cleaned {
            removals: Vec::new(),
            freed_bytes: 7,
        });
        let json = |value: serde_json::Result<serde_json::Value>| value.unwrap();

        assert_eq!(
            json(serde_json::to_value(measured)),
            serde_json::json!({"kind": "measured", "done": 2, "total": 9, "bytes": 5})
        );
        assert_eq!(
            json(serde_json::to_value(found)),
            serde_json::json!({"kind": "done", "token": 3, "home": "/Users/dev", "findings": []})
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
            serde_json::json!({"kind": "done", "removals": [], "freedBytes": 7})
        );
        assert_eq!(
            json(serde_json::to_value(Refusal::Unlisted)),
            serde_json::json!({"code": "unlisted"})
        );
    }
}
