//! Quitting what uses memory, from the panel's breakdown: politely, after the
//! checks a stop runs, and never by force.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use harbormaster_engine::{Holder, Protection, Signals, Standing, live};
use harbormaster_platform::{Presence, ProcessKey, Signal, SignalOutcome};
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
    /// An app, asked to quit as ⌘Q asks it; it may ask the user first.
    App {
        token: u64,
    },
    /// Programs with no app around them, each asked to exit as `kill` asks.
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
    /// Harbormaster itself or something above it.
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
    /// How each target was shown, for the report.
    names: Vec<(ProcessKey, String)>,
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
    /// `skip` holds the dev servers' processes, which the breakdown leaves out.
    pub(crate) fn preview(&self, skip: &HashSet<ProcessKey>, name: &str) -> QuitPreview {
        self.prepare(skip, name)
            .unwrap_or_else(|reason| QuitPreview::Refused { reason })
    }

    fn prepare(&self, skip: &HashSet<ProcessKey>, name: &str) -> Result<QuitPreview, Reason> {
        // Every check before the request relies on keys that change on exec.
        if !harbormaster_platform::tracks_exec() {
            return Err(Reason::UntrackedExec);
        }
        let own = live::lineage()
            .map_err(failed)?
            .ok_or(Reason::UnknownLineage)?;
        let app = live::apps(skip)
            .map_err(failed)?
            .into_iter()
            .find(|app| app.name == name)
            .ok_or(Reason::NotFound)?;
        let guard = live::guards(&home(), &app.members).map_err(failed)?;
        let seen: Vec<Seen> = app
            .members
            .iter()
            .map(|&key| Seen {
                key,
                standing: guard.get(&key).copied().unwrap_or(Standing::Unknown),
                own: own.contains(&key.pid.0),
                app: appkit::is_app(key.pid.0),
            })
            .collect();
        let plan = plan(&seen)?;
        let shown = live::members(&app.members).map_err(failed)?;
        let names = plan
            .targets()
            .iter()
            .map(|&key| {
                let name = shown
                    .iter()
                    .find(|member| member.key == key)
                    .map_or_else(|| app.name.clone(), |member| member.name.clone());
                (key, name)
            })
            .collect();
        let reach = match &plan {
            // Quitting an app ends everything it runs.
            Plan::Apps(_) => app.members.clone(),
            Plan::Programs { quits, .. } => quits.clone(),
        };
        let token = self.approve(Approval {
            plan: plan.clone(),
            reach,
            names,
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

    /// Asks what was approved to quit, after the checks a stop runs, and waits
    /// for it to go. Nothing is ever forced.
    pub(crate) fn quit(&self, token: u64) -> Result<Stopped, Reason> {
        let approval = self.take(token, Instant::now())?;
        recheck(&approval)?;
        let send: &dyn Fn(ProcessKey, Signal) -> SignalOutcome = match approval.plan {
            Plan::Apps(_) => &ask_app,
            Plan::Programs { .. } => &harbormaster_platform::signal,
        };
        let outcomes = harbormaster_engine::stop(
            approval.plan.targets(),
            Signal::Terminate,
            GRACE,
            &Signals {
                send,
                presence: &harbormaster_platform::presence,
                sleep: &std::thread::sleep,
            },
        );
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
    let own = live::lineage()
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
/// become an app, which only a polite quit may reach.
fn still_fits(
    approval: &Approval,
    own: &[u32],
    standing: impl Fn(ProcessKey) -> Standing,
    is_app: impl Fn(u32) -> bool,
) -> Result<(), Reason> {
    if approval.reach.iter().any(|key| own.contains(&key.pid.0)) {
        return Err(Reason::Changed);
    }
    let programs = matches!(approval.plan, Plan::Programs { .. });
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
fn ask_app(key: ProcessKey, _: Signal) -> SignalOutcome {
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

/// Until there is a way to ask an app to close its windows here, nothing
/// counts as an app.
#[cfg(not(target_os = "macos"))]
mod appkit {
    pub(super) fn is_app(_: u32) -> bool {
        false
    }

    pub(super) fn ask_to_quit(_: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    use harbormaster_engine::{Holder, Protection, Standing};
    use harbormaster_platform::{Pid, ProcessKey, StartTime};

    use super::{APPROVAL_LIFETIME, Approval, Plan, QuitPreview, Quits, Seen, plan, still_fits};
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
            at,
        }
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
        let token = quits.approve(approval(Instant::now()));
        let barrier = Arc::new(Barrier::new(2));
        let racers: Vec<_> = (0..2)
            .map(|_| {
                let (quits, barrier) = (Arc::clone(&quits), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    quits.take(token, Instant::now()).is_ok()
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
}
