//! Quitting what uses memory, from the panel's breakdown, after the checks
//! a stop runs. A Mac asks politely and never forces. Windows asks an app
//! to close its windows; a program without any it ends at once, as it does
//! what outlived a close, each time only once the user agreed to that.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use harbormaster_engine::apps::AppUsage;
use harbormaster_engine::{Holder, Outcome, Protection, Standing, live};
use harbormaster_platform::ProcessKey;
use serde::Serialize;

use crate::scanning::home;
use crate::stopping::{Reason, Stopped, Stopping, failed};

/// How long to wait for what was asked to quit before saying it is still open.
const GRACE: Duration = Duration::from_secs(5);
/// An approval left unused this long lapses, and the panel asks again.
const APPROVAL_LIFETIME: Duration = Duration::from_secs(120);

/// What quitting an app would do, for the panel to ask about.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum QuitPreview {
    /// An app, asked to quit as ⌘Q asks it, or on Windows to close its
    /// windows; it may ask the user first.
    App {
        token: u64,
    },
    /// Programs with no app around them, each asked to exit as `kill` asks;
    /// on Windows, ended at once.
    Programs {
        token: u64,
        quits: Vec<Stopping>,
        stays_open: Vec<Holder>,
    },
    Refused {
        reason: Reason,
    },
}

/// One of the processes counted as the app, as the checks found it.
#[derive(Debug, Clone, Copy)]
struct Seen {
    key: ProcessKey,
    standing: Standing,
    /// Harbormaster itself, something above it, or something it started.
    own: bool,
    /// An app the system can ask to quit, rather than a bare program.
    app: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Plan {
    /// Ask these apps to quit; the rest of their processes go with them.
    Apps(Vec<ProcessKey>),
    /// Ask these programs to exit, and leave the others alone.
    Programs {
        quits: Vec<ProcessKey>,
        stays: Vec<ProcessKey>,
    },
}

impl Plan {
    fn targets(&self) -> &[ProcessKey] {
        match self {
            Self::Apps(apps) => apps,
            Self::Programs { quits, .. } => quits,
        }
    }
}

/// What the user saw and agreed to quit.
#[derive(Debug)]
struct Approval {
    plan: Plan,
    /// Everything the quit may end; none of it may run Harbormaster.
    reach: Vec<ProcessKey>,
    /// How each process it may end was shown, for the report.
    names: Vec<(ProcessKey, String)>,
    /// Whether an app among it may only be asked, as the system asks apps.
    /// Not so once the user agreed to end everything at once.
    polite: bool,
    at: Instant,
}

/// Quits the user agreed to, by the token the panel holds.
#[derive(Debug, Default)]
pub(crate) struct Quits(Mutex<Ledger>);

#[derive(Debug, Default)]
struct Ledger {
    last: u64,
    open: HashMap<u64, Approval>,
}

impl Quits {
    /// `skip` holds the dev servers' processes, which the breakdown leaves
    /// out; `id` names an app as the breakdown lists it.
    pub(crate) fn preview(&self, skip: &HashSet<ProcessKey>, id: &str) -> QuitPreview {
        self.prepare(skip, id, true)
            .unwrap_or_else(|reason| QuitPreview::Refused { reason })
    }

    /// What ending every process of the app at once would do, its windowed
    /// ones too: what Windows offers once asking it to close left some open.
    #[cfg(windows)]
    pub(crate) fn force_preview(&self, skip: &HashSet<ProcessKey>, id: &str) -> QuitPreview {
        self.prepare(skip, id, false)
            .unwrap_or_else(|reason| QuitPreview::Refused { reason })
    }

    /// `polite` false counts every process as a bare program.
    fn prepare(
        &self,
        skip: &HashSet<ProcessKey>,
        id: &str,
        polite: bool,
    ) -> Result<QuitPreview, Reason> {
        // Every check before the request relies on keys that change on exec.
        if !harbormaster_platform::tracks_exec() {
            return Err(Reason::UntrackedExec);
        }
        let own = live::ours()
            .map_err(failed)?
            .ok_or(Reason::UnknownLineage)?;
        let app = chosen(live::apps(skip).map_err(failed)?, id)?;
        let guard = live::guards(&home(), &app.members).map_err(failed)?;
        let seen: Vec<Seen> = app
            .members
            .iter()
            .map(|&key| Seen {
                key,
                standing: guard.get(&key).copied().unwrap_or(Standing::Unknown),
                own: own.contains(&key.pid.0),
                app: polite && appkit::is_app(key.pid.0),
            })
            .collect();
        let plan = plan(&seen)?;
        // Ending a program on Windows may end what it started outside the app.
        #[cfg(windows)]
        if let Plan::Programs { quits, .. } = &plan {
            crate::stopping::alone(quits)?;
        }
        let reach = match &plan {
            // Quitting an app ends everything it runs.
            Plan::Apps(_) => app.members.clone(),
            Plan::Programs { quits, .. } => quits.clone(),
        };
        let shown = live::members(&app.members).map_err(failed)?;
        let names = reach
            .iter()
            .map(|&key| {
                let name = shown
                    .iter()
                    .find(|member| member.key == key)
                    .map_or_else(|| app.name.clone(), |member| member.name.clone());
                (key, name)
            })
            .collect();
        let token = self.approve(Approval {
            plan: plan.clone(),
            reach,
            names,
            polite,
            at: Instant::now(),
        });
        Ok(match plan {
            Plan::Apps(_) => QuitPreview::App { token },
            Plan::Programs { quits, stays } => QuitPreview::Programs {
                token,
                quits: shown
                    .iter()
                    .filter(|member| quits.contains(&member.key))
                    .map(|member| Stopping {
                        pid: member.pid,
                        name: member.name.clone(),
                        command: member.command.clone(),
                    })
                    .collect(),
                stays_open: shown
                    .iter()
                    .filter(|member| stays.contains(&member.key))
                    .map(|member| Holder {
                        pid: member.pid,
                        name: member.name.clone(),
                    })
                    .collect(),
            },
        })
    }

    /// Does what was approved, after the checks a stop runs, and waits for
    /// it to go.
    pub(crate) fn quit(&self, token: u64) -> Result<Stopped, Reason> {
        let approval = self.take(token, Instant::now())?;
        recheck(&approval)?;
        let outcomes = request(&approval)?;
        Ok(Stopped::new(outcomes, |key| {
            approval
                .names
                .iter()
                .find(|(named, _)| *named == key)
                .map_or_else(String::new, |(_, name)| name.clone())
        }))
    }

    fn approve(&self, approval: Approval) -> u64 {
        let mut ledger = self.lock();
        let now = approval.at;
        ledger
            .open
            .retain(|_, open| now.duration_since(open.at) < APPROVAL_LIFETIME);
        ledger.last += 1;
        let token = ledger.last;
        ledger.open.insert(token, approval);
        token
    }

    /// An approval serves one quit.
    fn take(&self, token: u64, now: Instant) -> Result<Approval, Reason> {
        let approval = self.lock().open.remove(&token).ok_or(Reason::Expired)?;
        if now.duration_since(approval.at) >= APPROVAL_LIFETIME {
            return Err(Reason::Expired);
        }
        Ok(approval)
    }

    fn lock(&self) -> MutexGuard<'_, Ledger> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Asks the apps to quit as ⌘Q would, or the programs to exit as `kill`
/// would, and waits for them; nothing is forced.
#[cfg(target_os = "macos")]
fn request(approval: &Approval) -> Result<Vec<(ProcessKey, Outcome)>, Reason> {
    use harbormaster_engine::Signals;
    use harbormaster_platform::{Signal, SignalOutcome};

    let send: &dyn Fn(ProcessKey, Signal) -> SignalOutcome = match approval.plan {
        Plan::Apps(_) => &ask_app,
        Plan::Programs { .. } => &harbormaster_platform::signal,
    };
    Ok(harbormaster_engine::stop(
        approval.plan.targets(),
        Signal::Terminate,
        GRACE,
        &Signals {
            send,
            presence: &harbormaster_platform::presence,
            sleep: &std::thread::sleep,
        },
    ))
}

/// Asks the apps to close their windows, then waits for every process they
/// run, since asked is not closed. Programs are ended at once: Windows has
/// no request to quit that every program heeds. Each process is held from
/// before the first is asked to after the wait.
#[cfg(windows)]
fn request(approval: &Approval) -> Result<Vec<(ProcessKey, Outcome)>, Reason> {
    use crate::stopping::{Grip, alone, hold_all, hold_all_through};
    use harbormaster_platform::{Presence, watch};

    match &approval.plan {
        Plan::Apps(windowed) => {
            // What is asked must be made sure of; the rest is only waited for, as far as it can be.
            let mut holding = hold_all_through(windowed, &watch)?;
            for &key in approval.reach.iter().filter(|key| !windowed.contains(key)) {
                match watch(key) {
                    Ok(held) => holding.held.push((key, Grip(held))),
                    Err(Presence::Gone | Presence::Replaced) => {
                        holding.gone.push((key, Outcome::AlreadyGone));
                    }
                    Err(_) => holding.gone.push((
                        key,
                        Outcome::Failed("could not tell whether it exited".into()),
                    )),
                }
            }
            let close = |grip: &Grip| {
                if windowed.contains(&grip.0.key) {
                    grip.0.close().map_err(|error| error.to_string())
                } else {
                    Ok(())
                }
            };
            let mut outcomes = harbormaster_engine::end(&holding.held, GRACE, &close);
            outcomes.extend(holding.gone);
            Ok(outcomes)
        }
        Plan::Programs { quits, .. } => {
            alone(quits)?;
            let holding = hold_all(quits)?;
            let mut outcomes = holding.gone;
            outcomes.extend(harbormaster_engine::force(&holding.held, GRACE));
            Ok(outcomes)
        }
    }
}

/// The app `id` names, unless it is part of the OS.
fn chosen(apps: Vec<AppUsage>, id: &str) -> Result<AppUsage, Reason> {
    let app = apps
        .into_iter()
        .find(|app| app.id == id)
        .ok_or(Reason::NotFound)?;
    if app.system {
        return Err(Reason::Protected {
            protection: Protection::System,
        });
    }
    Ok(app)
}

/// What to ask to quit, or why nothing.
fn plan(seen: &[Seen]) -> Result<Plan, Reason> {
    let alive: Vec<&Seen> = seen
        .iter()
        .filter(|each| !matches!(each.standing, Standing::Gone | Standing::Replaced))
        .collect();
    if alive.iter().any(|each| each.app) {
        // Quitting the app ends all it runs, the terminal Harbormaster may run in included.
        if alive.iter().any(|each| each.own) {
            return Err(Reason::RunsHarbormaster);
        }
        if let Some(blocked) = alive
            .iter()
            .find(|each| each.app && !may_end(each.standing))
        {
            return Err(refusal(blocked.standing));
        }
        let apps = alive
            .iter()
            .filter(|each| each.app)
            .map(|each| each.key)
            .collect();
        return Ok(Plan::Apps(apps));
    }
    let (quits, stays): (Vec<&Seen>, Vec<&Seen>) = alive
        .into_iter()
        .partition(|each| !each.own && may_end(each.standing));
    if quits.is_empty() {
        return Err(if stays.iter().any(|each| each.own) {
            Reason::RunsHarbormaster
        } else {
            stays
                .first()
                .map_or(Reason::NotFound, |each| refusal(each.standing))
        });
    }
    Ok(Plan::Programs {
        quits: quits.iter().map(|each| each.key).collect(),
        stays: stays.iter().map(|each| each.key).collect(),
    })
}

/// A coding agent, shell or terminal may go too, unlike in a stop: the user
/// picked it by name and was warned.
fn may_end(standing: Standing) -> bool {
    matches!(
        standing,
        Standing::Clear | Standing::Protected(Protection::Session)
    )
}

fn refusal(standing: Standing) -> Reason {
    let protection = match standing {
        Standing::Protected(protection) => protection,
        // What couldn't be judged can't be checked either.
        Standing::Clear | Standing::Gone | Standing::Replaced | Standing::Unknown => {
            Protection::Unverified
        }
    };
    Reason::Protected { protection }
}

/// The checks a stop runs, again right before anything is asked to quit.
fn recheck(approval: &Approval) -> Result<(), Reason> {
    if !harbormaster_platform::tracks_exec() {
        return Err(Reason::UntrackedExec);
    }
    let own = live::ours()
        .map_err(failed)?
        .ok_or(Reason::UnknownLineage)?;
    let guard = live::guards(&home(), approval.plan.targets()).map_err(failed)?;
    still_fits(
        approval,
        &own,
        |key| guard.get(&key).copied().unwrap_or(Standing::Unknown),
        appkit::is_app,
    )
}

/// Whether what was approved can still go as it was shown: nothing it may end
/// runs Harbormaster, every target may still be ended, and no program has
/// become an app, which only a polite quit may reach unless the user agreed
/// to end it at once.
fn still_fits(
    approval: &Approval,
    own: &[u32],
    standing: impl Fn(ProcessKey) -> Standing,
    is_app: impl Fn(u32) -> bool,
) -> Result<(), Reason> {
    if approval.reach.iter().any(|key| own.contains(&key.pid.0)) {
        return Err(Reason::Changed);
    }
    let programs = approval.polite && matches!(approval.plan, Plan::Programs { .. });
    for &key in approval.plan.targets() {
        let now = standing(key);
        // What exited meanwhile is left for the request, which reports it gone.
        let gone = matches!(now, Standing::Gone | Standing::Replaced);
        if !gone && (!may_end(now) || (programs && is_app(key.pid.0))) {
            return Err(Reason::Changed);
        }
    }
    Ok(())
}

/// Asks the app to quit as ⌘Q would, if it is still the process approved.
#[cfg(target_os = "macos")]
fn ask_app(
    key: ProcessKey,
    _: harbormaster_platform::Signal,
) -> harbormaster_platform::SignalOutcome {
    use harbormaster_platform::{Presence, SignalOutcome};

    match harbormaster_platform::presence(key) {
        Presence::Running if appkit::ask_to_quit(key.pid.0) => SignalOutcome::Sent,
        Presence::Running => SignalOutcome::Failed("the app didn't take the request".into()),
        Presence::Gone => SignalOutcome::Gone,
        Presence::Replaced => SignalOutcome::Replaced,
        Presence::Unknown => SignalOutcome::Failed("could not tell whether it still runs".into()),
    }
}

#[cfg(target_os = "macos")]
mod appkit {
    //! These run on a blocking worker, which has no autorelease pool of its own.

    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication};

    /// An app with a Dock icon or a menu bar item, which the system can ask to quit.
    pub(super) fn is_app(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        autoreleasepool(|_| {
            NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|app| {
                app.activationPolicy() != NSApplicationActivationPolicy::Prohibited
            })
        })
    }

    /// Asks the app to quit as ⌘Q would; it may ask the user first, or say no.
    pub(super) fn ask_to_quit(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        autoreleasepool(|_| {
            NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
                .is_some_and(|app| app.terminate())
        })
    }
}

#[cfg(windows)]
mod appkit {
    /// A program with a window on the taskbar, which Windows can ask to close.
    pub(super) fn is_app(pid: u32) -> bool {
        harbormaster_platform::has_windows(pid)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    use harbormaster_engine::{Holder, Protection, Standing};
    use harbormaster_platform::{Pid, ProcessKey, StartTime};

    use harbormaster_engine::apps::AppUsage;

    use super::{
        APPROVAL_LIFETIME, Approval, Plan, QuitPreview, Quits, Seen, chosen, plan, still_fits,
    };
    use crate::stopping::{Reason, Stopping};

    fn key(pid: u32) -> ProcessKey {
        ProcessKey {
            pid: Pid(pid),
            start: StartTime(1),
            exec: 0,
        }
    }

    fn seen(pid: u32, standing: Standing) -> Seen {
        Seen {
            key: key(pid),
            standing,
            own: false,
            app: false,
        }
    }

    fn approval(at: Instant) -> Approval {
        Approval {
            plan: Plan::Apps(vec![key(10)]),
            reach: vec![key(10)],
            names: Vec::new(),
            polite: true,
            at,
        }
    }

    fn app(id: &str, system: bool) -> AppUsage {
        AppUsage {
            id: id.into(),
            name: "node.exe".into(),
            memory_bytes: 1,
            system,
            members: vec![key(10)],
        }
    }

    #[test]
    fn picks_the_app_by_id_and_never_a_part_of_the_system() {
        let apps = vec![
            app(r"user/path:c:\nvm\node.exe", false),
            app(r"user/path:c:\program files\nodejs\node.exe", false),
            app("system/name:svchost.exe", true),
        ];

        assert_eq!(
            chosen(apps.clone(), r"user/path:c:\program files\nodejs\node.exe").map(|app| app.id),
            Ok(r"user/path:c:\program files\nodejs\node.exe".into())
        );
        assert_eq!(
            chosen(apps.clone(), "system/name:svchost.exe").err(),
            Some(Reason::Protected {
                protection: Protection::System
            })
        );
        assert_eq!(chosen(apps, "node.exe").err(), Some(Reason::NotFound));
    }

    #[test]
    fn asks_only_the_app_itself_to_quit() {
        let app = Seen {
            app: true,
            ..seen(10, Standing::Clear)
        };

        assert_eq!(
            plan(&[app, seen(11, Standing::Clear)]),
            Ok(Plan::Apps(vec![key(10)]))
        );
    }

    #[test]
    fn quits_a_terminal_as_cmd_q_would() {
        let terminal = Seen {
            app: true,
            ..seen(10, Standing::Protected(Protection::Session))
        };

        assert_eq!(plan(&[terminal]), Ok(Plan::Apps(vec![key(10)])));
    }

    #[test]
    fn never_quits_an_app_harbormaster_runs_under() {
        let app = Seen {
            app: true,
            ..seen(10, Standing::Clear)
        };
        let terminal_host = Seen {
            own: true,
            ..seen(11, Standing::Clear)
        };

        assert_eq!(plan(&[app, terminal_host]), Err(Reason::RunsHarbormaster));
    }

    #[test]
    fn never_quits_a_part_of_the_system() {
        let finder = Seen {
            app: true,
            ..seen(10, Standing::Protected(Protection::System))
        };

        assert_eq!(
            plan(&[finder]),
            Err(Reason::Protected {
                protection: Protection::System
            })
        );
    }

    #[test]
    fn closes_programs_and_agents_but_leaves_what_it_must() {
        let members = [
            seen(20, Standing::Clear),
            seen(21, Standing::Protected(Protection::Session)),
            seen(22, Standing::Protected(Protection::Database)),
            Seen {
                own: true,
                ..seen(23, Standing::Protected(Protection::Session))
            },
            seen(24, Standing::Gone),
            seen(25, Standing::Unknown),
        ];

        assert_eq!(
            plan(&members),
            Ok(Plan::Programs {
                quits: vec![key(20), key(21)],
                stays: vec![key(22), key(23), key(25)],
            })
        );
    }

    #[test]
    fn says_why_when_nothing_can_be_closed() {
        assert_eq!(
            plan(&[seen(30, Standing::Protected(Protection::Database))]),
            Err(Reason::Protected {
                protection: Protection::Database
            })
        );
        let own = Seen {
            own: true,
            ..seen(31, Standing::Clear)
        };
        assert_eq!(plan(&[own]), Err(Reason::RunsHarbormaster));
        assert_eq!(plan(&[seen(32, Standing::Gone)]), Err(Reason::NotFound));
    }

    #[test]
    fn calls_off_closing_a_program_that_has_become_an_app() {
        let approval = Approval {
            plan: Plan::Programs {
                quits: vec![key(20)],
                stays: Vec::new(),
            },
            reach: vec![key(20)],
            names: Vec::new(),
            polite: true,
            at: Instant::now(),
        };

        assert_eq!(
            still_fits(&approval, &[], |_| Standing::Clear, |_| true),
            Err(Reason::Changed)
        );
        assert_eq!(
            still_fits(&approval, &[], |_| Standing::Clear, |_| false),
            Ok(())
        );
    }

    #[test]
    fn lets_a_force_the_user_agreed_to_reach_an_app_too() {
        let approval = Approval {
            plan: Plan::Programs {
                quits: vec![key(20)],
                stays: Vec::new(),
            },
            reach: vec![key(20)],
            names: Vec::new(),
            polite: false,
            at: Instant::now(),
        };

        assert_eq!(
            still_fits(&approval, &[], |_| Standing::Clear, |_| true),
            Ok(())
        );
    }

    #[test]
    fn calls_off_a_quit_that_would_reach_harbormaster_or_something_protected_now() {
        let approval = Approval {
            reach: vec![key(10), key(11)],
            ..approval(Instant::now())
        };

        assert_eq!(
            still_fits(&approval, &[11, 1], |_| Standing::Clear, |_| true),
            Err(Reason::Changed)
        );
        assert_eq!(
            still_fits(
                &approval,
                &[],
                |_| Standing::Protected(Protection::Unverified),
                |_| true
            ),
            Err(Reason::Changed)
        );
        assert_eq!(
            still_fits(&approval, &[], |_| Standing::Gone, |_| false),
            Ok(())
        );
    }

    #[test]
    fn serves_one_quit_per_approval() {
        let quits = Quits::default();
        let now = Instant::now();
        let token = quits.approve(approval(now));

        assert!(quits.take(token, now).is_ok());
        assert_eq!(quits.take(token, now).err(), Some(Reason::Expired));
    }

    #[test]
    fn lets_one_of_two_racing_quits_through() {
        let quits = Arc::new(Quits::default());
        // Both take at the instant it was approved: only the race decides.
        let now = Instant::now();
        let token = quits.approve(approval(now));
        let barrier = Arc::new(Barrier::new(2));
        let racers: Vec<_> = (0..2)
            .map(|_| {
                let (quits, barrier) = (Arc::clone(&quits), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    quits.take(token, now).is_ok()
                })
            })
            .collect();

        let through = racers
            .into_iter()
            .map(|racer| racer.join().unwrap())
            .filter(|&passed| passed)
            .count();
        assert_eq!(through, 1);
    }

    #[test]
    fn lets_an_approval_lapse() {
        let quits = Quits::default();
        let then = Instant::now();
        let token = quits.approve(approval(then));

        let later = then + APPROVAL_LIFETIME + Duration::from_secs(1);
        assert_eq!(quits.take(token, later).err(), Some(Reason::Expired));
    }

    #[test]
    fn tells_the_panel_what_it_expects() {
        let app = QuitPreview::App { token: 3 };
        let programs = QuitPreview::Programs {
            token: 4,
            quits: vec![Stopping {
                pid: 20,
                name: "node".into(),
                command: Some("node server.js".into()),
            }],
            stays_open: vec![Holder {
                pid: 21,
                name: "postgres".into(),
            }],
        };

        assert_eq!(
            serde_json::to_value(app).unwrap(),
            serde_json::json!({ "kind": "app", "token": 3 })
        );
        assert_eq!(
            serde_json::to_value(programs).unwrap(),
            serde_json::json!({
                "kind": "programs",
                "token": 4,
                "quits": [{ "pid": 20, "name": "node", "command": "node server.js" }],
                "staysOpen": [{ "pid": 21, "name": "postgres" }],
            })
        );
    }

    /// Set, for the copies of this test program the Windows tests start:
    /// where to write the pid once the window shows, how it answers a
    /// request to close (`closes` or `stays`), and, for the copy that only
    /// starts another and exits, which program that is.
    #[cfg(windows)]
    const SHOWN: &str = "HARBORMASTER_TEST_SHOWN";
    #[cfg(windows)]
    const ANSWERS: &str = "HARBORMASTER_TEST_ANSWERS";
    #[cfg(windows)]
    const LAUNCHES: &str = "HARBORMASTER_TEST_LAUNCHES";

    /// A program showing a window, which no process of this test started:
    /// anything Harbormaster starts is its own, which it never quits. Ended
    /// whatever happens to the test.
    #[cfg(windows)]
    struct Window {
        key: ProcessKey,
        _folder: tempfile::TempDir,
    }

    #[cfg(windows)]
    impl Drop for Window {
        fn drop(&mut self) {
            if let Ok(held) = harbormaster_platform::hold(self.key) {
                let _ = held.terminate();
                let _ = held.wait(Duration::from_secs(5));
            }
        }
    }

    /// Shows a window from a copy of this test program kept elsewhere, so it
    /// counts as an app of its own, started by a copy that exits at once.
    #[cfg(windows)]
    fn window(answers: &str) -> Window {
        use std::process::Command;

        let folder = tempfile::tempdir().unwrap();
        let program = folder.path().join("helper.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
        let shown = folder.path().join("shown");
        let launcher = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "quitting::tests::starts_a_window_and_exits",
                "--ignored",
            ])
            .env(LAUNCHES, &program)
            .env(SHOWN, &shown)
            .env(ANSWERS, answers)
            .status()
            .unwrap();
        assert!(launcher.success());
        let deadline = Instant::now() + Duration::from_secs(10);
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(&shown)
                .ok()
                .and_then(|pid| pid.parse::<u32>().ok())
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "the window should show");
            std::thread::sleep(Duration::from_millis(50));
        };
        let key = harbormaster_platform::processes()
            .unwrap()
            .into_iter()
            .find(|entry| entry.key.pid.0 == pid)
            .expect("the window's program should be listed")
            .key;
        Window {
            key,
            _folder: folder,
        }
    }

    /// The id the memory breakdown gives the app that `key` is part of.
    #[cfg(windows)]
    fn id_of(key: ProcessKey) -> String {
        harbormaster_engine::live::apps(&std::collections::HashSet::new())
            .unwrap()
            .into_iter()
            .find(|app| app.members.contains(&key))
            .expect("the breakdown should list the window's program")
            .id
    }

    #[cfg(windows)]
    #[test]
    fn closes_an_app_on_windows_by_asking_its_window_to_close() {
        let window = window("closes");
        let quits = Quits::default();
        let skip = std::collections::HashSet::new();

        let token = match quits.preview(&skip, &id_of(window.key)) {
            QuitPreview::App { token } => token,
            other => panic!("closing it as an app wasn't offered: {other:?}"),
        };
        let stopped = quits.quit(token).unwrap();

        assert!(!stopped.still_running, "{:?}", stopped.outcomes);
        assert_eq!(
            harbormaster_platform::presence(window.key),
            harbormaster_platform::Presence::Gone
        );
    }

    #[cfg(windows)]
    #[test]
    fn force_quits_an_app_on_windows_that_stayed_open_only_once_asked_again() {
        use harbormaster_engine::Outcome;

        let window = window("stays");
        let quits = Quits::default();
        let skip = std::collections::HashSet::new();
        let id = id_of(window.key);

        let token = match quits.preview(&skip, &id) {
            QuitPreview::App { token } => token,
            other => panic!("closing it as an app wasn't offered: {other:?}"),
        };
        let stopped = quits.quit(token).unwrap();
        assert!(stopped.still_running, "{:?}", stopped.outcomes);
        assert!(
            harbormaster_platform::has_windows(window.key.pid.0),
            "asked is not closed"
        );

        let (token, quits_shown) = match quits.force_preview(&skip, &id) {
            QuitPreview::Programs { token, quits, .. } => (token, quits),
            other => panic!("ending it wasn't offered: {other:?}"),
        };
        assert_eq!(
            quits_shown
                .iter()
                .map(|shown| shown.pid)
                .collect::<Vec<_>>(),
            [window.key.pid.0]
        );
        let stopped = quits.quit(token).unwrap();

        assert!(
            stopped
                .outcomes
                .iter()
                .all(|ended| ended.outcome == Outcome::Stopped),
            "{:?}",
            stopped.outcomes
        );
    }

    /// Not a test of its own: starts the window's program the tests above
    /// use, then exits, so that program has no parent left.
    #[cfg(windows)]
    #[test]
    #[ignore = "the Windows quit tests run it to start their window"]
    #[expect(
        clippy::zombie_processes,
        reason = "the program must outlive this copy, and Windows keeps no zombies"
    )]
    fn starts_a_window_and_exits() {
        let Some(program) = std::env::var_os(LAUNCHES) else {
            return;
        };
        std::process::Command::new(program)
            .args([
                "--exact",
                "quitting::tests::shows_a_window_until_it_closes",
                "--ignored",
            ])
            .env_remove(LAUNCHES)
            .spawn()
            .unwrap();
    }

    /// Not a test of its own: the window's program the tests above use.
    #[cfg(windows)]
    #[test]
    #[ignore = "the Windows quit tests run it as their app"]
    fn shows_a_window_until_it_closes() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DispatchMessageW, GetMessageW, IsWindow, MSG, WM_CLOSE,
            WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        };

        let (Some(shown), Ok(answers)) = (std::env::var_os(SHOWN), std::env::var(ANSWERS)) else {
            return;
        };
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        // SAFETY: the class is one Windows registers, named with a closing nul.
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                0,
                0,
                320,
                200,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!window.is_null(), "{}", std::io::Error::last_os_error());
        // Gone by itself in the end, should the test lose track of it.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(60));
            std::process::exit(2);
        });
        std::fs::write(shown, std::process::id().to_string()).unwrap();
        let mut message = MSG::default();
        // SAFETY: the window is this thread's, and `message` ours to fill.
        while unsafe { IsWindow(window) } != 0
            && unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0
        {
            // A window left to itself closes on WM_CLOSE.
            if answers == "stays" && message.message == WM_CLOSE {
                continue;
            }
            // SAFETY: the message came from this thread's queue.
            unsafe { DispatchMessageW(&message) };
        }
    }
}
