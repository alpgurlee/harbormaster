use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use harbormaster_engine::Signals;
use harbormaster_engine::{
    Holder, Outcome, Protection, Refusal, Scanner, Service, Snapshot, Standing, Target, live,
};
use harbormaster_platform::ProcessKey;
#[cfg(target_os = "macos")]
use harbormaster_platform::Signal;
use serde::Serialize;

use crate::scanning::home;

/// How long a stop waits for the server to exit before offering to force it.
const GRACE: Duration = Duration::from_secs(5);
/// How long a forced stop waits to see the processes go.
#[cfg(target_os = "macos")]
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
    /// The system wouldn't let Harbormaster make sure of one of its
    /// processes, or end it.
    #[cfg_attr(not(windows), allow(dead_code))]
    NotAllowed,
    /// It started programs outside it, which ending it could end too.
    #[cfg_attr(not(windows), allow(dead_code))]
    Strays,
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
    pub(crate) pid: u32,
    pub(crate) name: String,
    pub(crate) command: Option<String>,
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

impl Stopped {
    /// Each process's outcome, under the name it was shown with.
    pub(crate) fn new(
        outcomes: Vec<(ProcessKey, Outcome)>,
        name_of: impl Fn(ProcessKey) -> String,
    ) -> Self {
        Self {
            still_running: outcomes
                .iter()
                .any(|(_, outcome)| *outcome == Outcome::StillRunning),
            outcomes: outcomes
                .into_iter()
                .map(|(key, outcome)| Ended {
                    pid: key.pid.0,
                    name: name_of(key),
                    outcome,
                })
                .collect(),
        }
    }
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
        #[cfg(windows)]
        alone(service)?;
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
    #[cfg(target_os = "macos")]
    pub(crate) fn stop(&self, token: u64) -> Result<Stopped, Reason> {
        let approval = self.take(token, Instant::now(), Round::Terminate)?;
        let targets = recheck(&approval.service)?;
        let outcomes = harbormaster_engine::stop(&targets, Signal::Terminate, GRACE, &signals());
        Ok(self.settle(token, approval, outcomes, Round::Terminate))
    }

    /// Ends the approved service at once, after the checks the CLI runs, and
    /// waits for it to go. Windows has no request to quit that every server
    /// heeds, so this is the forced stop, and the only round an approval
    /// serves. Every process is held before any is ended, and the one that
    /// started the others goes first, so it isn't there to start them again.
    #[cfg(windows)]
    pub(crate) fn stop(&self, token: u64) -> Result<Stopped, Reason> {
        let approval = self.take(token, Instant::now(), Round::Terminate)?;
        recheck(&approval.service)?;
        alone(&approval.service)?;
        let targets: Vec<ProcessKey> = approval
            .service
            .processes
            .iter()
            .map(|member| member.key)
            .collect();
        let holding = harbormaster_engine::hold_all(&targets, &|key| {
            harbormaster_platform::hold(key).map(Grip)
        })
        .map_err(|(_, presence)| match presence {
            harbormaster_platform::Presence::Replaced => Reason::Changed,
            _ => Reason::NotAllowed,
        })?;
        let mut outcomes = holding.gone;
        outcomes.extend(harbormaster_engine::force(&holding.held, GRACE));
        // Settled as the last round, so the approval is spent whatever is left.
        Ok(self.settle(token, approval, outcomes, Round::Kill))
    }

    /// Ends what a stop left running, after the same checks again.
    #[cfg(target_os = "macos")]
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
        let stopped = Stopped::new(outcomes, |key| {
            approval
                .service
                .processes
                .iter()
                .find(|member| member.key == key)
                .map_or_else(String::new, |member| member.name.clone())
        });
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

/// Ending a process on Windows can end others too, through a job it holds,
/// so a service is left alone while ending it could reach anything outside
/// it: what it started, such as another server, or what its jobs hold.
/// Windows' console hosts, which end with their programs, don't count.
#[cfg(windows)]
fn alone(service: &Service) -> Result<(), Reason> {
    let keys: Vec<ProcessKey> = service.processes.iter().map(|member| member.key).collect();
    if live::strays(&keys).map_err(failed)?.is_empty() {
        Ok(())
    } else {
        Err(Reason::Strays)
    }
}

#[cfg(target_os = "macos")]
fn signals() -> Signals<'static> {
    Signals {
        send: &harbormaster_platform::signal,
        presence: &harbormaster_platform::presence,
        sleep: &std::thread::sleep,
    }
}

/// A Windows process held for the engine to end.
#[cfg(windows)]
struct Grip(harbormaster_platform::Held);

#[cfg(windows)]
impl harbormaster_engine::Held for Grip {
    fn terminate(&self) -> Result<(), String> {
        self.0.terminate().map_err(|error| error.to_string())
    }

    fn wait(&self, grace: Duration) -> harbormaster_engine::Waited {
        use harbormaster_engine::Waited;
        use harbormaster_platform::Ended;
        match self.0.wait(grace) {
            Ended::Exited => Waited::Exited,
            Ended::StillRunning => Waited::StillRunning,
            Ended::Unknown => Waited::Unknown,
        }
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

pub(crate) fn failed(error: harbormaster_platform::Error) -> Reason {
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
        // Both take at the instant it was approved: only the race decides.
        let now = Instant::now();
        let token = approvals.approve(service(), now);
        let barrier = Arc::new(Barrier::new(2));
        let racers: Vec<_> = (0..2)
            .map(|_| {
                let (approvals, barrier) = (Arc::clone(&approvals), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    approvals.take(token, now, Round::Terminate).is_ok()
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

    /// Set, to how it serves, for the copy of this test program that serves
    /// as the server.
    #[cfg(windows)]
    const SERVES: &str = "HARBORMASTER_TEST_SERVES";

    /// A server this test started, ended whatever happens to the test.
    #[cfg(windows)]
    struct Server {
        process: std::process::Child,
        port: u16,
    }

    #[cfg(windows)]
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.process.kill();
            let _ = self.process.wait();
        }
    }

    /// Starts a copy of this test program serving on loopback, as `how` says.
    #[cfg(windows)]
    fn serve(how: &str) -> Server {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let mut process = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "stopping::tests::serves_until_its_input_closes",
                "--ignored",
                "--nocapture",
            ])
            .env(SERVES, how)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let said = BufReader::new(process.stdout.take().unwrap());
        let mut server = Server { process, port: 0 };
        server.port = said
            .lines()
            .find_map(|line| line.ok()?.strip_prefix("listening on ")?.parse().ok())
            .expect("the server should say where it listens");
        server
    }

    /// What a scan sees now, and the id of the service on `port`.
    #[cfg(windows)]
    fn scanned(port: u16) -> (harbormaster_engine::Snapshot, String) {
        use harbormaster_engine::{Scanner, live};

        let snapshot = live::scan(&mut Scanner::default(), &crate::scanning::home()).unwrap();
        let id = snapshot
            .services
            .iter()
            .find(|service| service.ports.iter().any(|listening| listening.port == port))
            .map(|service| service.id.clone())
            .expect("the scan should find the server");
        (snapshot, id)
    }

    #[cfg(windows)]
    #[test]
    fn force_quits_a_server_on_windows_and_frees_its_port() {
        let mut server = serve("alone");
        let (snapshot, id) = scanned(server.port);
        let approvals = Approvals::default();

        let token = match approvals.preview(&snapshot, &id) {
            Preview::Ready { token, .. } => token,
            Preview::Refused { reason } => panic!("the stop was refused: {reason:?}"),
        };
        let stopped = approvals.stop(token).unwrap();

        assert!(!stopped.still_running);
        assert!(
            stopped
                .outcomes
                .iter()
                .all(|ended| ended.outcome == Outcome::Stopped),
            "{:?}",
            stopped.outcomes
        );
        assert_eq!(server.process.wait().unwrap().code(), Some(1));
        assert!(std::net::TcpListener::bind(("127.0.0.1", server.port)).is_ok());
        // One approval, one round.
        assert_eq!(approvals.stop(token).err(), Some(Reason::Expired));
    }

    #[cfg(windows)]
    #[test]
    fn wont_force_quit_a_server_on_windows_that_started_a_program_outside_it() {
        let mut server = serve("with a shell");
        let (snapshot, id) = scanned(server.port);

        match Approvals::default().preview(&snapshot, &id) {
            Preview::Refused { reason } => assert_eq!(reason, Reason::Strays),
            Preview::Ready { .. } => panic!("a stop was offered"),
        }
        assert!(server.process.try_wait().unwrap().is_none());
    }

    /// Not a test of its own: the server the tests above start. "with a
    /// shell" also starts a command prompt left open, a session of the
    /// user's that is no part of the server.
    #[cfg(windows)]
    #[test]
    #[ignore = "the Windows stop tests run it as their server"]
    fn serves_until_its_input_closes() {
        use std::process::{Command, Stdio};

        let Some(how) = std::env::var_os(SERVES) else {
            return;
        };
        // Its input closes when this exits, and it exits too.
        let _shell = (how == "with a shell").then(|| {
            let system = std::env::var_os("SystemRoot").expect("Windows says where it lives");
            Command::new(
                std::path::Path::new(&system)
                    .join("System32")
                    .join("cmd.exe"),
            )
            .arg("/k")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap()
        });
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        println!("listening on {}", socket.local_addr().unwrap().port());
        let _ = std::io::stdin().read_line(&mut String::new());
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
        assert_eq!(
            serde_json::to_value(Reason::NotAllowed).unwrap(),
            serde_json::json!({ "code": "notAllowed" })
        );
        assert_eq!(
            serde_json::to_value(Reason::Strays).unwrap(),
            serde_json::json!({ "code": "strays" })
        );
    }
}
