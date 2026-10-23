use std::collections::HashSet;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use harbormaster_platform::{Presence, ProcessKey, Signal, SignalOutcome};
use serde::Serialize;

use crate::{Protection, Service, Snapshot, Standing};

/// How often to look whether stopped processes have exited.
const POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `address` picks one when several services use the port on different addresses.
    Port { port: u16, address: Option<IpAddr> },
    /// The service this process belongs to.
    Pid(u32),
    /// A service by its id, as the app refers to one.
    Service(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Refusal<'a> {
    /// Nothing listens there, or the process is in no service.
    NotFound,
    Ambiguous(Vec<&'a Service>),
    Protected(&'a Service, Protection),
    /// Stopping it would stop Harbormaster or what started it.
    RunsHarbormaster(&'a Service),
    /// It gained a process since it was approved. Ask again rather than stop
    /// something nobody saw.
    Changed,
}

/// The service to stop. `own` is Harbormaster's pid and its ancestors'.
pub fn choose<'a>(
    snapshot: &'a Snapshot,
    target: &Target,
    own: &[u32],
) -> Result<&'a Service, Refusal<'a>> {
    let candidates: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| match target {
            Target::Port { port, address } => service.ports.iter().any(|candidate| {
                candidate.port == *port
                    && address.is_none_or(|address| candidate.address == address)
            }),
            Target::Pid(pid) => service.processes.iter().any(|member| member.pid == *pid),
            Target::Service(id) => service.id == *id,
        })
        .collect();
    let service = match candidates.as_slice() {
        [] => return Err(Refusal::NotFound),
        [service] => *service,
        _ => return Err(Refusal::Ambiguous(candidates)),
    };
    allowed(service, own)
}

fn allowed<'a>(service: &'a Service, own: &[u32]) -> Result<&'a Service, Refusal<'a>> {
    if let Some(protection) = service.protected {
        return Err(Refusal::Protected(service, protection));
    }
    if service
        .processes
        .iter()
        .any(|member| own.contains(&member.pid))
    {
        return Err(Refusal::RunsHarbormaster(service));
    }
    Ok(service)
}

/// Deepest first, so a parent can't restart a child that was already told to go.
pub fn order(service: &Service) -> Vec<ProcessKey> {
    service
        .processes
        .iter()
        .rev()
        .map(|member| member.key)
        .collect()
}

/// The approved processes, deepest first, once a fresh look confirms that
/// none of them is protected now, nor serves a protected service or one with
/// processes nobody approved. Call it before every round of signals. Processes that already
/// exited stay in the list; their signal reports them gone.
pub fn recheck<'a>(
    approved: &Service,
    now: &'a Snapshot,
    guard: &dyn Fn(ProcessKey) -> Standing,
) -> Result<Vec<ProcessKey>, Refusal<'a>> {
    let approved_keys: HashSet<ProcessKey> =
        approved.processes.iter().map(|member| member.key).collect();
    // A process may also have left every service and changed on its way, or
    // be impossible to judge right now.
    if approved_keys
        .iter()
        .any(|&key| matches!(guard(key), Standing::Protected(_) | Standing::Unknown))
    {
        return Err(Refusal::Changed);
    }
    let touched = now.services.iter().filter(|service| {
        service
            .processes
            .iter()
            .any(|member| approved_keys.contains(&member.key))
    });
    for service in touched {
        if let Some(protection) = service.protected {
            return Err(Refusal::Protected(service, protection));
        }
        if service
            .processes
            .iter()
            .any(|member| !approved_keys.contains(&member.key))
        {
            return Err(Refusal::Changed);
        }
    }
    Ok(order(approved))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Stopped,
    AlreadyGone,
    /// Its pid now names another process, or it became another program, so
    /// it was left alone.
    Replaced,
    AccessDenied,
    StillRunning,
    Failed(String),
}

/// The OS calls a stop needs, so the sequence can be tested without one.
pub struct Signals<'a> {
    pub send: &'a dyn Fn(ProcessKey, Signal) -> SignalOutcome,
    pub presence: &'a dyn Fn(ProcessKey) -> Presence,
    pub sleep: &'a dyn Fn(Duration),
}

impl std::fmt::Debug for Signals<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Signals").finish_non_exhaustive()
    }
}

/// A process held to be ended, where the system ends processes through a
/// handle rather than a signal: what is ended is what was checked.
pub trait Held {
    /// Ends it without asking; a process gone meanwhile counts as ended.
    fn terminate(&self) -> Result<(), String>;
    /// Waits up to `grace` for it to be gone.
    fn wait(&self, grace: Duration) -> Waited;
}

/// How a held process stood when a wait was over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    Exited,
    StillRunning,
    /// The system wouldn't say.
    Unknown,
}

/// What holding a batch gave: the processes held, and those gone already.
#[derive(Debug)]
pub struct Holding<T> {
    pub held: Vec<(ProcessKey, T)>,
    pub gone: Vec<(ProcessKey, Outcome)>,
}

/// Holds every one of `targets` through `hold` before anything is ended:
/// one that is no longer what was approved, or can't be told, refuses the
/// whole batch; one already gone is just reported so.
pub fn hold_all<T>(
    targets: &[ProcessKey],
    hold: &dyn Fn(ProcessKey) -> Result<T, Presence>,
) -> Result<Holding<T>, (ProcessKey, Presence)> {
    let mut held = Vec::with_capacity(targets.len());
    let mut gone = Vec::new();
    for &key in targets {
        match hold(key) {
            Ok(target) => held.push((key, target)),
            Err(Presence::Gone) => gone.push((key, Outcome::AlreadyGone)),
            Err(presence) => return Err((key, presence)),
        }
    }
    Ok(Holding { held, gone })
}

/// Ends `held` in order, the one that started the others first, then waits
/// up to `grace` in all for them to be gone.
pub fn force<T: Held>(held: &[(ProcessKey, T)], grace: Duration) -> Vec<(ProcessKey, Outcome)> {
    end(held, grace, &T::terminate)
}

/// Does `how` to each of `held` in order, then waits up to `grace` in all
/// for them to be gone: what `how` refused is reported so, and not waited for.
pub fn end<T: Held>(
    held: &[(ProcessKey, T)],
    grace: Duration,
    how: &dyn Fn(&T) -> Result<(), String>,
) -> Vec<(ProcessKey, Outcome)> {
    let mut outcomes = Vec::with_capacity(held.len());
    let mut waiting = Vec::new();
    for (key, target) in held {
        match how(target) {
            Ok(()) => waiting.push((*key, target)),
            Err(reason) => outcomes.push((*key, Outcome::Failed(reason))),
        }
    }
    let deadline = Instant::now() + grace;
    for (key, target) in waiting {
        let left = deadline.saturating_duration_since(Instant::now());
        outcomes.push((
            key,
            match target.wait(left) {
                Waited::Exited => Outcome::Stopped,
                Waited::StillRunning => Outcome::StillRunning,
                Waited::Unknown => Outcome::Failed("could not tell whether it exited".into()),
            },
        ));
    }
    outcomes
}

/// Sends `signal` to `targets` in order, then waits up to `grace` for them to exit.
pub fn stop(
    targets: &[ProcessKey],
    signal: Signal,
    grace: Duration,
    signals: &Signals<'_>,
) -> Vec<(ProcessKey, Outcome)> {
    let mut outcomes = Vec::with_capacity(targets.len());
    let mut waiting = Vec::new();
    for &key in targets {
        match (signals.send)(key, signal) {
            SignalOutcome::Sent => waiting.push(key),
            SignalOutcome::Gone => outcomes.push((key, Outcome::AlreadyGone)),
            SignalOutcome::Replaced => outcomes.push((key, Outcome::Replaced)),
            SignalOutcome::Denied => outcomes.push((key, Outcome::AccessDenied)),
            SignalOutcome::Failed(reason) => outcomes.push((key, Outcome::Failed(reason))),
        }
    }
    let mut waited = Duration::ZERO;
    let mut unclear = Vec::new();
    loop {
        unclear.clear();
        waiting.retain(|&key| match (signals.presence)(key) {
            Presence::Running => true,
            // Not knowing is never taken for an exit.
            Presence::Unknown => {
                unclear.push(key);
                true
            }
            Presence::Gone => {
                outcomes.push((key, Outcome::Stopped));
                false
            }
            Presence::Replaced => {
                outcomes.push((key, Outcome::Replaced));
                false
            }
        });
        if waiting.is_empty() || waited >= grace {
            break;
        }
        (signals.sleep)(POLL);
        waited += POLL;
    }
    outcomes.extend(waiting.into_iter().map(|key| {
        let outcome = if unclear.contains(&key) {
            Outcome::Failed("could not tell whether it exited".into())
        } else {
            Outcome::StillRunning
        };
        (key, outcome)
    }));
    outcomes.sort_by_key(|(key, _)| targets.iter().position(|target| target == key));
    outcomes
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::time::Duration;

    use harbormaster_platform::{Pid, Presence, ProcessKey, Signal, SignalOutcome, StartTime};

    use super::{Outcome, Refusal, Signals, Target, choose, order, recheck, stop};
    use crate::{Classification, Member, Port, Protection, Service, Snapshot, Standing};

    fn key(pid: u32) -> ProcessKey {
        ProcessKey {
            pid: Pid(pid),
            start: StartTime(u64::from(pid)),
            exec: 0,
        }
    }

    fn service(root: u32, members: &[u32], port: u16, address: [u8; 4]) -> Service {
        Service {
            id: format!("{root}-{root}"),
            classification: Classification::Dev,
            protected: None,
            ports: vec![Port {
                port,
                address: IpAddr::from(address),
            }],
            project: None,
            command: Some("npm run dev".into()),
            started_at: 0,
            memory_bytes: None,
            cpu_percent: None,
            processes: members
                .iter()
                .map(|&pid| Member {
                    key: key(pid),
                    pid,
                    name: "node".into(),
                    command: None,
                    memory_bytes: None,
                    cpu_percent: None,
                })
                .collect(),
            holders: Vec::new(),
            ancestors: Vec::new(),
        }
    }

    fn snapshot(services: Vec<Service>) -> Snapshot {
        Snapshot { services }
    }

    const LOOPBACK: [u8; 4] = [127, 0, 0, 1];

    fn port(port: u16) -> Target {
        Target::Port {
            port,
            address: None,
        }
    }

    struct Fake<'a> {
        log: &'a RefCell<Vec<String>>,
        name: &'static str,
        ends: super::Waited,
        refuses: bool,
    }

    impl super::Held for Fake<'_> {
        fn terminate(&self) -> Result<(), String> {
            self.log.borrow_mut().push(format!("end {}", self.name));
            if self.refuses {
                Err("access denied".into())
            } else {
                Ok(())
            }
        }

        fn wait(&self, _grace: Duration) -> super::Waited {
            self.log.borrow_mut().push(format!("wait {}", self.name));
            self.ends
        }
    }

    #[test]
    fn ends_the_held_in_order_and_waits_for_each() {
        let log = RefCell::new(Vec::new());
        let fake = |name, ends, refuses| Fake {
            log: &log,
            name,
            ends,
            refuses,
        };
        // The one that refuses sits in the middle: the rest still go.
        let held = [
            (key(10), fake("npm", super::Waited::Exited, false)),
            (key(12), fake("worker", super::Waited::Exited, true)),
            (key(11), fake("node", super::Waited::StillRunning, false)),
            (key(13), fake("helper", super::Waited::Unknown, false)),
        ];

        let outcomes = super::force(&held, Duration::from_secs(5));

        assert_eq!(
            outcomes,
            [
                (key(12), Outcome::Failed("access denied".into())),
                (key(10), Outcome::Stopped),
                (key(11), Outcome::StillRunning),
                (
                    key(13),
                    Outcome::Failed("could not tell whether it exited".into())
                ),
            ]
        );
        assert_eq!(
            *log.borrow(),
            [
                "end npm",
                "end worker",
                "end node",
                "end helper",
                "wait npm",
                "wait node",
                "wait helper"
            ]
        );
    }

    #[test]
    fn asks_the_held_as_told_then_waits_for_all_of_them() {
        let log = RefCell::new(Vec::new());
        let fake = |name, ends| Fake {
            log: &log,
            name,
            ends,
            refuses: false,
        };
        let held = [
            (key(10), fake("app", super::Waited::Exited)),
            (key(11), fake("helper", super::Waited::StillRunning)),
            (key(12), fake("window", super::Waited::Exited)),
        ];
        // Only the app and its window are asked; the helper is only waited for.
        let ask = |target: &Fake<'_>| {
            log.borrow_mut().push(format!("ask {}", target.name));
            match target.name {
                "window" => Err("it shows no window to close".to_string()),
                _ => Ok(()),
            }
        };
        let only_asked = |target: &Fake<'_>| match target.name {
            "helper" => Ok(()),
            _ => ask(target),
        };

        let outcomes = super::end(&held, Duration::from_secs(5), &only_asked);

        assert_eq!(
            outcomes,
            [
                (
                    key(12),
                    Outcome::Failed("it shows no window to close".into())
                ),
                (key(10), Outcome::Stopped),
                (key(11), Outcome::StillRunning),
            ]
        );
        assert_eq!(
            *log.borrow(),
            ["ask app", "ask window", "wait app", "wait helper"]
        );
    }

    #[test]
    fn holds_every_target_first_or_none_at_all() {
        let hold = |key: ProcessKey| match key.pid.0 {
            10 => Ok(()),
            11 => Err(Presence::Gone),
            12 => Err(Presence::Replaced),
            _ => Err(Presence::Unknown),
        };

        let holding = super::hold_all(&[key(10), key(11)], &hold).unwrap();
        assert_eq!(holding.held, [(key(10), ())]);
        assert_eq!(holding.gone, [(key(11), Outcome::AlreadyGone)]);

        assert_eq!(
            super::hold_all(&[key(10), key(12)], &hold).unwrap_err(),
            (key(12), Presence::Replaced)
        );
        assert_eq!(
            super::hold_all(&[key(13), key(10)], &hold).unwrap_err(),
            (key(13), Presence::Unknown)
        );
    }

    #[test]
    fn finds_the_service_on_a_port() {
        let snapshot = snapshot(vec![service(20, &[20, 30], 5173, LOOPBACK)]);

        let chosen = choose(&snapshot, &port(5173), &[]).unwrap();

        assert_eq!(chosen.id, "20-20");
    }

    #[test]
    fn says_when_nothing_listens_on_the_port() {
        let snapshot = snapshot(vec![service(20, &[20], 5173, LOOPBACK)]);

        assert!(matches!(
            choose(&snapshot, &port(3000), &[]),
            Err(Refusal::NotFound)
        ));
    }

    #[test]
    fn asks_for_an_address_when_two_services_share_a_port_number() {
        let snapshot = snapshot(vec![
            service(20, &[20], 8080, LOOPBACK),
            service(40, &[40], 8080, [192, 168, 1, 5]),
        ]);

        assert!(matches!(
            choose(&snapshot, &port(8080), &[]),
            Err(Refusal::Ambiguous(services)) if services.len() == 2
        ));
        let lan = Target::Port {
            port: 8080,
            address: Some(IpAddr::from([192, 168, 1, 5])),
        };
        assert_eq!(choose(&snapshot, &lan, &[]).unwrap().id, "40-40");
    }

    #[test]
    fn finds_the_service_a_process_belongs_to() {
        let snapshot = snapshot(vec![service(20, &[20, 30], 5173, LOOPBACK)]);

        assert_eq!(
            choose(&snapshot, &Target::Pid(30), &[]).unwrap().id,
            "20-20"
        );
        assert!(matches!(
            choose(&snapshot, &Target::Pid(99), &[]),
            Err(Refusal::NotFound)
        ));
    }

    #[test]
    fn finds_a_service_by_its_id() {
        let snapshot = snapshot(vec![
            service(20, &[20, 30], 5173, LOOPBACK),
            service(40, &[40], 8080, LOOPBACK),
        ]);

        let chosen = choose(&snapshot, &Target::Service("40-40".into()), &[]).unwrap();

        assert_eq!(chosen.ports[0].port, 8080);
    }

    #[test]
    fn refuses_a_protected_service_and_says_why() {
        let mut database = service(20, &[20], 5432, LOOPBACK);
        database.protected = Some(Protection::Database);
        let snapshot = snapshot(vec![database]);

        assert!(matches!(
            choose(&snapshot, &port(5432), &[]),
            Err(Refusal::Protected(_, Protection::Database))
        ));
    }

    #[test]
    fn refuses_a_service_harbormaster_itself_runs_in() {
        let snapshot = snapshot(vec![service(20, &[20, 30], 5173, LOOPBACK)]);

        assert!(matches!(
            choose(&snapshot, &port(5173), &[77, 20]),
            Err(Refusal::RunsHarbormaster(_))
        ));
    }

    #[test]
    fn stops_the_deepest_processes_first() {
        let service = service(20, &[20, 30, 40], 5173, LOOPBACK);

        assert_eq!(order(&service), [key(40), key(30), key(20)]);
    }

    #[test]
    fn refuses_when_the_service_gained_a_process_since_it_was_approved() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        let now = snapshot(vec![service(20, &[20, 30, 31], 5173, LOOPBACK)]);

        assert!(matches!(
            recheck(&approved, &now, &|_| Standing::Clear),
            Err(Refusal::Changed)
        ));
    }

    #[test]
    fn refuses_when_the_service_became_protected_since_it_was_approved() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        let mut shared = service(20, &[20, 30], 5173, LOOPBACK);
        shared.protected = Some(Protection::SharedSocket);
        let now = snapshot(vec![shared]);

        assert!(matches!(
            recheck(&approved, &now, &|_| Standing::Clear),
            Err(Refusal::Protected(_, Protection::SharedSocket))
        ));
    }

    #[test]
    fn still_targets_approved_processes_that_left_their_service() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        // npm exited on the request; the vite it started now stands alone.
        let orphaned = snapshot(vec![service(30, &[30], 5173, LOOPBACK)]);

        assert_eq!(
            recheck(&approved, &orphaned, &|_| Standing::Clear).unwrap(),
            [key(30), key(20)]
        );
        assert_eq!(
            recheck(&approved, &snapshot(Vec::new()), &|_| Standing::Clear).unwrap(),
            [key(30), key(20)]
        );
    }

    #[test]
    fn refuses_when_an_approved_process_now_serves_a_protected_service() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        let mut session = service(30, &[30], 5173, LOOPBACK);
        session.protected = Some(Protection::Session);
        let now = snapshot(vec![session]);

        assert!(matches!(
            recheck(&approved, &now, &|_| Standing::Clear),
            Err(Refusal::Protected(_, Protection::Session))
        ));
    }

    #[test]
    fn refuses_when_an_approved_process_became_protected_on_its_own() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        // After the request it closed its port and calls itself an agent.
        let guard = |target: ProcessKey| {
            if target == key(30) {
                Standing::Protected(Protection::Session)
            } else {
                Standing::Clear
            }
        };

        assert!(matches!(
            recheck(&approved, &snapshot(Vec::new()), &guard),
            Err(Refusal::Changed)
        ));
    }

    #[test]
    fn refuses_when_an_approved_process_cannot_be_judged() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        let gone_or_unknown = |target: ProcessKey| {
            if target == key(30) {
                Standing::Unknown
            } else {
                Standing::Gone
            }
        };

        assert!(matches!(
            recheck(&approved, &snapshot(Vec::new()), &gone_or_unknown),
            Err(Refusal::Changed)
        ));
        assert!(recheck(&approved, &snapshot(Vec::new()), &|_| Standing::Gone).is_ok());
    }

    #[test]
    fn refuses_when_another_service_took_in_an_approved_process() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        let now = snapshot(vec![service(40, &[40, 30], 8080, LOOPBACK)]);

        assert!(matches!(
            recheck(&approved, &now, &|_| Standing::Clear),
            Err(Refusal::Changed)
        ));
    }

    #[test]
    fn reports_what_happened_to_each_process() {
        let replies = HashMap::from([
            (key(2), SignalOutcome::Gone),
            (key(3), SignalOutcome::Denied),
            (key(5), SignalOutcome::Replaced),
        ]);
        // key(1) exits on the second look; key(4) never does; key(6) can't
        // be looked at; key(7) turns into another process.
        let checks: RefCell<HashMap<ProcessKey, u32>> = RefCell::default();
        let sent: RefCell<Vec<ProcessKey>> = RefCell::default();
        let send = |target: ProcessKey, _: Signal| {
            sent.borrow_mut().push(target);
            replies.get(&target).cloned().unwrap_or(SignalOutcome::Sent)
        };
        let presence = |target: ProcessKey| {
            let mut checks = checks.borrow_mut();
            let seen = checks.entry(target).or_default();
            *seen += 1;
            match target.pid.0 {
                1 if *seen >= 2 => Presence::Gone,
                6 => Presence::Unknown,
                7 => Presence::Replaced,
                _ => Presence::Running,
            }
        };
        let signals = Signals {
            send: &send,
            presence: &presence,
            sleep: &|_| {},
        };
        let targets = [key(1), key(2), key(3), key(4), key(5), key(6), key(7)];

        let outcomes = stop(
            &targets,
            Signal::Terminate,
            Duration::from_secs(1),
            &signals,
        );

        assert_eq!(
            outcomes,
            [
                (key(1), Outcome::Stopped),
                (key(2), Outcome::AlreadyGone),
                (key(3), Outcome::AccessDenied),
                (key(4), Outcome::StillRunning),
                (key(5), Outcome::Replaced),
                (
                    key(6),
                    Outcome::Failed("could not tell whether it exited".into())
                ),
                (key(7), Outcome::Replaced),
            ]
        );
        assert_eq!(*sent.borrow(), targets);
    }
}
