use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

use harbormaster_platform::{ProcessKey, Signal, SignalOutcome};
use serde::Serialize;

use crate::{Protection, Service, Snapshot};

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
/// none of them now serves a protected service or one with processes nobody
/// approved. Call it before every round of signals. Processes that already
/// exited stay in the list; their signal reports them gone.
pub fn recheck<'a>(approved: &Service, now: &'a Snapshot) -> Result<Vec<ProcessKey>, Refusal<'a>> {
    let approved_keys: HashSet<ProcessKey> =
        approved.processes.iter().map(|member| member.key).collect();
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
    AccessDenied,
    StillRunning,
    Failed(String),
}

/// The OS calls a stop needs, so the sequence can be tested without one.
pub struct Signals<'a> {
    pub send: &'a dyn Fn(ProcessKey, Signal) -> SignalOutcome,
    pub is_running: &'a dyn Fn(ProcessKey) -> bool,
    pub sleep: &'a dyn Fn(Duration),
}

impl std::fmt::Debug for Signals<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Signals").finish_non_exhaustive()
    }
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
            SignalOutcome::Denied => outcomes.push((key, Outcome::AccessDenied)),
            SignalOutcome::Failed(reason) => outcomes.push((key, Outcome::Failed(reason))),
        }
    }
    let mut waited = Duration::ZERO;
    loop {
        waiting.retain(|&key| {
            let running = (signals.is_running)(key);
            if !running {
                outcomes.push((key, Outcome::Stopped));
            }
            running
        });
        if waiting.is_empty() || waited >= grace {
            break;
        }
        (signals.sleep)(POLL);
        waited += POLL;
    }
    outcomes.extend(waiting.into_iter().map(|key| (key, Outcome::StillRunning)));
    outcomes.sort_by_key(|(key, _)| targets.iter().position(|target| target == key));
    outcomes
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::time::Duration;

    use harbormaster_platform::{Pid, ProcessKey, Signal, SignalOutcome, StartTime};

    use super::{Outcome, Refusal, Signals, Target, choose, order, recheck, stop};
    use crate::{Classification, Member, Port, Protection, Service, Snapshot};

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

        assert!(matches!(recheck(&approved, &now), Err(Refusal::Changed)));
    }

    #[test]
    fn refuses_when_the_service_became_protected_since_it_was_approved() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        let mut shared = service(20, &[20, 30], 5173, LOOPBACK);
        shared.protected = Some(Protection::SharedSocket);
        let now = snapshot(vec![shared]);

        assert!(matches!(
            recheck(&approved, &now),
            Err(Refusal::Protected(_, Protection::SharedSocket))
        ));
    }

    #[test]
    fn still_targets_approved_processes_that_left_their_service() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        // npm exited on the request; the vite it started now stands alone.
        let orphaned = snapshot(vec![service(30, &[30], 5173, LOOPBACK)]);

        assert_eq!(recheck(&approved, &orphaned).unwrap(), [key(30), key(20)]);
        assert_eq!(
            recheck(&approved, &snapshot(Vec::new())).unwrap(),
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
            recheck(&approved, &now),
            Err(Refusal::Protected(_, Protection::Session))
        ));
    }

    #[test]
    fn refuses_when_another_service_took_in_an_approved_process() {
        let approved = service(20, &[20, 30], 5173, LOOPBACK);
        let now = snapshot(vec![service(40, &[40, 30], 8080, LOOPBACK)]);

        assert!(matches!(recheck(&approved, &now), Err(Refusal::Changed)));
    }

    #[test]
    fn reports_what_happened_to_each_process() {
        let replies = HashMap::from([
            (key(2), SignalOutcome::Gone),
            (key(3), SignalOutcome::Denied),
        ]);
        // key(1) exits on the second check; key(4) never does.
        let checks: RefCell<HashMap<ProcessKey, u32>> = RefCell::default();
        let sent: RefCell<Vec<ProcessKey>> = RefCell::default();
        let never_exits = key(4);
        let send = |target: ProcessKey, _: Signal| {
            sent.borrow_mut().push(target);
            replies.get(&target).cloned().unwrap_or(SignalOutcome::Sent)
        };
        let is_running = |target: ProcessKey| {
            let mut checks = checks.borrow_mut();
            let seen = checks.entry(target).or_default();
            *seen += 1;
            target == never_exits || *seen < 2
        };
        let signals = Signals {
            send: &send,
            is_running: &is_running,
            sleep: &|_| {},
        };

        let outcomes = stop(
            &[key(1), key(2), key(3), key(4)],
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
            ]
        );
        assert_eq!(*sent.borrow(), [key(1), key(2), key(3), key(4)]);
    }
}
