use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use harbormaster_engine::{
    Holder, Outcome, Protection, Refusal, Scanner, Service, Signals, Snapshot, Standing, Target,
    live,
};
use harbormaster_platform::{ProcessKey, Signal};
use serde::Serialize;

use crate::scanning::home;

/// How long a stop waits for the server to exit before offering to force it.
const GRACE: Duration = Duration::from_secs(5);
/// How long a forced stop waits to see the processes go.
const KILL_GRACE: Duration = Duration::from_secs(2);
/// An approval left unused this long lapses, and the panel asks again.
const APPROVAL_LIFETIME: Duration = Duration::from_secs(120);

/// Why a stop was turned down.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Reason {
    /// The server is gone, or no scan has seen it.
    NotFound,
    Protected {
        protection: Protection,
    },
    RunsHarbormaster,
    /// It changed after it was approved.
    Changed,
    /// The approval lapsed or was already used.
    Expired,
    /// This system can't tell when a process execs another program.
    UntrackedExec,
    /// Harbormaster couldn't trace what started it, so it can't keep clear of it.
    UnknownLineage,
    Failed {
        message: String,
    },
}

/// What stopping a service would do, for the panel to ask about.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Preview {
    Ready {
        token: u64,
        stops: Vec<Stopping>,
        frees: Vec<u16>,
        stays_open: Vec<Holder>,
    },
    Refused {
        reason: Reason,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct Stopping {
    pid: u32,
    name: String,
    command: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Stopped {
    outcomes: Vec<Ended>,
    still_running: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct Ended {
    pid: u32,
    name: String,
    outcome: Outcome,
}

/// Services the user saw and agreed to stop, by the token the panel holds.
#[derive(Debug, Default)]
pub(crate) struct Approvals(Mutex<Ledger>);

#[derive(Debug, Default)]
struct Ledger {
    last: u64,
    open: HashMap<u64, Approval>,
}

/// TERM comes first; KILL only after a TERM left something running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Round {
    Terminate,
    Kill,
}

#[derive(Debug)]
struct Approval {
    service: Service,
    at: Instant,
    /// What the stop left running, for Force stop to end.
    left: Vec<ProcessKey>,
}

impl Approvals {
    pub(crate) fn preview(&self, snapshot: &Snapshot, id: &str) -> Preview {
        match self.prepare(snapshot, id) {
            Ok(preview) => preview,
            Err(reason) => Preview::Refused { reason },
        }
    }

    fn prepare(&self, snapshot: &Snapshot, id: &str) -> Result<Preview, Reason> {
        // Every check before a signal relies on keys that change on exec.
        if !harbormaster_platform::tracks_exec() {
            return Err(Reason::UntrackedExec);
        }
        let own = live::lineage()
            .map_err(failed)?
            .ok_or(Reason::UnknownLineage)?;
        let service = harbormaster_engine::choose(snapshot, &Target::Service(id.to_string()), &own)
            .map_err(|refusal| reason(&refusal))?;
        let stops = service
            .processes
            .iter()
            .map(|member| Stopping {
                pid: member.pid,
                name: member.name.clone(),
                command: member.command.clone(),
            })
            .collect();
        let mut frees: Vec<u16> = service.ports.iter().map(|port| port.port).collect();
        frees.sort_unstable();
        frees.dedup();
        let stays_open = service.ancestors.clone();
        let token = self.approve(service.clone(), Instant::now());
        Ok(Preview::Ready {
            token,
            stops,
            frees,
            stays_open,
        })
    }

    /// Asks the approved service to stop, after the checks the CLI runs, and
    /// waits for it to go.
    pub(crate) fn stop(&self, token: u64) -> Result<Stopped, Reason> {
        let approval = self.take(token, Instant::now(), Round::Terminate)?;
        let targets = recheck(&approval.service)?;
        let outcomes = harbormaster_engine::stop(&targets, Signal::Terminate, GRACE, &signals());
        Ok(self.settle(token, approval, outcomes, Round::Terminate))
    }

    /// Ends what a stop left running, after the same checks again.
    pub(crate) fn force(&self, token: u64) -> Result<Stopped, Reason> {
        let approval = self.take(token, Instant::now(), Round::Kill)?;
        recheck(&approval.service)?;
        let outcomes =
            harbormaster_engine::stop(&approval.left, Signal::Kill, KILL_GRACE, &signals());
        Ok(self.settle(token, approval, outcomes, Round::Kill))
    }

    /// Reports the round. A TERM that left something running keeps the
    /// approval for one KILL; nothing else keeps it.
    fn settle(
        &self,
        token: u64,
        mut approval: Approval,
        outcomes: Vec<(ProcessKey, Outcome)>,
        round: Round,
    ) -> Stopped {
        approval.left = outcomes
            .iter()
            .filter(|(_, outcome)| *outcome == Outcome::StillRunning)
            .map(|(key, _)| *key)
            .collect();
        let stopped = Stopped {
            still_running: !approval.left.is_empty(),
            outcomes: outcomes
                .into_iter()
                .map(|(key, outcome)| Ended {
                    pid: key.pid.0,
                    name: approval
                        .service
                        .processes
                        .iter()
                        .find(|member| member.key == key)
                        .map_or_else(String::new, |member| member.name.clone()),
                    outcome,
                })
                .collect(),
        };
        if round == Round::Terminate && stopped.still_running {
            self.lock().open.insert(token, approval);
        }
        stopped
    }

    fn approve(&self, service: Service, now: Instant) -> u64 {
        let mut ledger = self.lock();
        ledger
            .open
            .retain(|_, approval| now.duration_since(approval.at) < APPROVAL_LIFETIME);
        ledger.last += 1;
        let token = ledger.last;
        ledger.open.insert(
            token,
            Approval {
                service,
                at: now,
                left: Vec::new(),
            },
        );
        token
    }

    /// An approval serves one TERM round and, when that leaves something
    /// running, one KILL round; never a round twice.
    fn take(&self, token: u64, now: Instant, round: Round) -> Result<Approval, Reason> {
        let mut ledger = self.lock();
        let fits = ledger
            .open
            .get(&token)
            .is_some_and(|approval| approval.left.is_empty() == (round == Round::Terminate));
        if !fits {
            return Err(Reason::Expired);
        }
        let approval = ledger.open.remove(&token).ok_or(Reason::Expired)?;
        drop(ledger);
        if now.duration_since(approval.at) >= APPROVAL_LIFETIME {
            return Err(Reason::Expired);
        }
        Ok(approval)
    }

    fn lock(&self) -> MutexGuard<'_, Ledger> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The checks the CLI runs right before each round of signals.
fn recheck(service: &Service) -> Result<Vec<ProcessKey>, Reason> {
    if !harbormaster_platform::tracks_exec() {
        return Err(Reason::UntrackedExec);
    }
    let home = home();
    let current = live::scan(&mut Scanner::default(), &home).map_err(failed)?;
    let approved: Vec<ProcessKey> = service.processes.iter().map(|member| member.key).collect();
    let guard = live::guards(&home, &approved).map_err(failed)?;
    harbormaster_engine::recheck(service, &current, &|key| {
        guard.get(&key).copied().unwrap_or(Standing::Unknown)
    })
    .map_err(|refusal| reason(&refusal))
}

fn signals() -> Signals<'static> {
    Signals {
        send: &harbormaster_platform::signal,
        presence: &harbormaster_platform::presence,
        sleep: &std::thread::sleep,
    }
}

fn reason(refusal: &Refusal<'_>) -> Reason {
    match refusal {
        // Ids are unique, so a service picked by id is never ambiguous.
        Refusal::NotFound | Refusal::Ambiguous(_) => Reason::NotFound,
        Refusal::Protected(_, protection) => Reason::Protected {
            protection: *protection,
        },
        Refusal::RunsHarbormaster(_) => Reason::RunsHarbormaster,
        Refusal::Changed => Reason::Changed,
    }
}

fn failed(error: harbormaster_platform::Error) -> Reason {
    Reason::Failed {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    use harbormaster_engine::{Classification, Holder, Outcome, Port, Protection, Service};
    use harbormaster_platform::{Pid, ProcessKey, StartTime};

    use super::{APPROVAL_LIFETIME, Approvals, Ended, Preview, Reason, Round, Stopped, Stopping};

    fn key(pid: u32) -> ProcessKey {
        ProcessKey {
            pid: Pid(pid),
            start: StartTime(1),
            exec: 0,
        }
    }

    fn service() -> Service {
        Service {
            id: "20-1".into(),
            classification: Classification::Dev,
            protected: None,
            ports: vec![Port {
                port: 3000,
                address: IpAddr::from([127, 0, 0, 1]),
            }],
            project: None,
            command: Some("npm run dev".into()),
            started_at: 0,
            memory_bytes: None,
            cpu_percent: None,
            processes: Vec::new(),
            holders: Vec::new(),
            ancestors: Vec::new(),
        }
    }

    #[test]
    fn serves_one_round_of_each_signal_per_approval() {
        let approvals = Approvals::default();
        let now = Instant::now();
        let token = approvals.approve(service(), now);
        let running = || vec![(key(20), Outcome::StillRunning)];

        let approval = approvals.take(token, now, Round::Terminate).unwrap();
        assert_eq!(
            approvals.take(token, now, Round::Terminate).err(),
            Some(Reason::Expired)
        );
        approvals.settle(token, approval, running(), Round::Terminate);

        assert_eq!(
            approvals.take(token, now, Round::Terminate).err(),
            Some(Reason::Expired)
        );
        let approval = approvals.take(token, now, Round::Kill).unwrap();
        approvals.settle(token, approval, running(), Round::Kill);
        assert_eq!(
            approvals.take(token, now, Round::Kill).err(),
            Some(Reason::Expired)
        );
    }

    #[test]
    fn lets_one_of_two_racing_stops_through() {
        let approvals = Arc::new(Approvals::default());
        let token = approvals.approve(service(), Instant::now());
        let barrier = Arc::new(Barrier::new(2));
        let racers: Vec<_> = (0..2)
            .map(|_| {
                let (approvals, barrier) = (Arc::clone(&approvals), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    approvals
                        .take(token, Instant::now(), Round::Terminate)
                        .is_ok()
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
        let approvals = Approvals::default();
        let then = Instant::now();
        let token = approvals.approve(service(), then);

        let later = then + APPROVAL_LIFETIME + Duration::from_secs(1);
        assert_eq!(
            approvals.take(token, later, Round::Terminate).err(),
            Some(Reason::Expired)
        );
    }

    #[test]
    fn tells_the_panel_what_it_expects() {
        let ready = Preview::Ready {
            token: 7,
            stops: vec![Stopping {
                pid: 20,
                name: "node".into(),
                command: Some("npm run dev".into()),
            }],
            frees: vec![3000],
            stays_open: vec![Holder {
                pid: 10,
                name: "zsh".into(),
            }],
        };
        let refused = Preview::Refused {
            reason: Reason::Protected {
                protection: Protection::Database,
            },
        };

        assert_eq!(
            serde_json::to_value(ready).unwrap(),
            serde_json::json!({
                "kind": "ready",
                "token": 7,
                "stops": [{ "pid": 20, "name": "node", "command": "npm run dev" }],
                "frees": [3000],
                "staysOpen": [{ "pid": 10, "name": "zsh" }],
            })
        );
        let stopped = Stopped {
            outcomes: vec![Ended {
                pid: 20,
                name: "node".into(),
                outcome: Outcome::StillRunning,
            }],
            still_running: true,
        };
        assert_eq!(
            serde_json::to_value(stopped).unwrap(),
            serde_json::json!({
                "outcomes": [{ "pid": 20, "name": "node", "outcome": "stillRunning" }],
                "stillRunning": true,
            })
        );
        assert_eq!(
            serde_json::to_value(refused).unwrap(),
            serde_json::json!({
                "kind": "refused",
                "reason": { "code": "protected", "protection": "database" },
            })
        );
    }
}
