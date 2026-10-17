use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use harbormaster_engine::{Outcome, Port, Protection, Refusal, Service, Target};
use harbormaster_platform::{BindProbe, ProcessKey};

use crate::format::{label, ports, printable, table};

pub(crate) const FAILED: u8 = 1;
pub(crate) const NOT_FOUND: u8 = 3;
pub(crate) const REFUSED: u8 = 4;
pub(crate) const PARTIAL: u8 = 5;
pub(crate) const CANCELLED: u8 = 130;

/// How often a port still taken is looked at: a server on its way out may
/// hold it a moment longer.
pub(crate) const FREE_LOOKS: usize = 10;

/// Whether a stopped server's ports can be taken again.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Freed {
    Yes,
    /// One of them can't be bound yet: a process may still hold it, or
    /// connections it closed are still winding down.
    Taken,
    /// The system wouldn't say, as for a port only root may take.
    Unknown,
}

/// Whether `ports` can be taken again, as binding each says, which sees
/// what a list of listeners misses: a server on its way out, or a socket
/// bound without listening. `wait` comes between looks at a taken port.
pub(crate) fn freed(
    ports: &[Port],
    probe: &dyn Fn(IpAddr, u16) -> BindProbe,
    wait: &dyn Fn(),
) -> Freed {
    for look in 1..=FREE_LOOKS {
        let mut taken = false;
        for port in ports {
            // A listener on [::] may have taken the port for IPv4 too, which
            // the probe, kept to IPv6, wouldn't see: both have to be free.
            let ipv4 = (port.address == IpAddr::V6(Ipv6Addr::UNSPECIFIED))
                .then_some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
            for address in std::iter::once(port.address).chain(ipv4) {
                match probe(address, port.port) {
                    BindProbe::Free => {}
                    BindProbe::InUse => taken = true,
                    BindProbe::Denied | BindProbe::Unavailable | BindProbe::Failed(_) => {
                        return Freed::Unknown;
                    }
                }
            }
        }
        if !taken {
            return Freed::Yes;
        }
        if look < FREE_LOOKS {
            wait();
        }
    }
    Freed::Taken
}

/// What the user is asked to approve.
pub(crate) fn preview(service: &Service) -> Vec<String> {
    let members: Vec<Vec<String>> = service
        .processes
        .iter()
        .map(|member| {
            vec![
                member.pid.to_string(),
                member.name.clone(),
                member.command.clone().unwrap_or_default(),
            ]
        })
        .collect();
    let mut sections: Vec<(&str, String)> = table(&members)
        .lines()
        .enumerate()
        .map(|(index, line)| (if index == 0 { "stops" } else { "" }, line.to_string()))
        .collect();
    sections.push(("frees", ports(service)));
    if !service.ancestors.is_empty() {
        let ancestors: Vec<String> = service
            .ancestors
            .iter()
            .map(|ancestor| format!("{} ({})", printable(&ancestor.name), ancestor.pid))
            .collect();
        sections.push(("stays open", ancestors.join(", ")));
    }
    let mut lines = vec![format!(
        "Stop {} on {}?",
        printable(label(service)),
        ports(service)
    )];
    lines.extend(
        sections
            .into_iter()
            .map(|(heading, text)| format!("  {heading:<10}  {text}").trim_end().to_string()),
    );
    lines
}

/// Why nothing was stopped, and the exit status that says so.
pub(crate) fn refusal(refusal: &Refusal<'_>, target: &Target) -> (Vec<String>, u8) {
    match refusal {
        Refusal::NotFound => {
            let line = match target {
                Target::Port { port, address } => format!(
                    "{}:{port}: nothing Harbormaster can see listens there.",
                    address.map(bracketed).unwrap_or_default()
                ),
                Target::Pid(pid) => {
                    format!("pid {pid} is not part of a server Harbormaster can see.")
                }
                Target::Service(_) => "That server is no longer running.".to_string(),
            };
            (vec![line], NOT_FOUND)
        }
        Refusal::Ambiguous(services) => {
            let port = match target {
                Target::Port { port, .. } => *port,
                Target::Pid(_) | Target::Service(_) => 0,
            };
            let rows: Vec<Vec<String>> = services
                .iter()
                .map(|service| {
                    let address = service
                        .ports
                        .iter()
                        .find(|candidate| candidate.port == port)
                        .map(|candidate| candidate.address.to_string())
                        .unwrap_or_default();
                    vec![format!("  {address}"), named(service)]
                })
                .collect();
            let mut lines = vec![format!(":{port} is used by more than one server:")];
            lines.extend(table(&rows).lines().map(str::to_string));
            lines.push("Pick one with --address.".to_string());
            (lines, REFUSED)
        }
        Refusal::Protected(service, protection) => (
            vec![format!(
                "Harbormaster won't stop {}: {}.",
                printable(label(service)),
                reason(service, *protection)
            )],
            REFUSED,
        ),
        Refusal::RunsHarbormaster(service) => (
            vec![format!(
                "Harbormaster won't stop {}: Harbormaster itself runs inside it.",
                printable(label(service))
            )],
            REFUSED,
        ),
        Refusal::Changed => (
            vec![
                "The server changed since you approved it. Run the command again to see what is left."
                    .to_string(),
            ],
            REFUSED,
        ),
    }
}

/// The system can't say when a process execs another program, so no check
/// before a signal could prove it still runs what was approved.
pub(crate) fn untracked_exec() -> (Vec<String>, u8) {
    (
        vec![
            "Harbormaster won't stop anything on this system: it can't tell when a process starts running another program."
                .to_string(),
        ],
        REFUSED,
    )
}

fn reason(service: &Service, protection: Protection) -> String {
    match protection {
        Protection::System => "it is part of the operating system".into(),
        Protection::OtherUser => "it runs as another user".into(),
        Protection::Session => "the port belongs to your shell, terminal or coding agent".into(),
        Protection::Unverified => {
            "some of its processes couldn't be read, so it can't be checked".into()
        }
        Protection::Database => {
            "it runs a database; stop that with the database's own tools".into()
        }
        Protection::SharedSocket => {
            let outside: Vec<String> = service
                .holders
                .iter()
                .filter(|holder| {
                    !service
                        .processes
                        .iter()
                        .any(|member| member.pid == holder.pid)
                })
                .map(|holder| format!("{} (pid {})", printable(&holder.name), holder.pid))
                .collect();
            format!(
                "{} holds its port too, so stopping it would not free the port",
                outside.join(", ")
            )
        }
    }
}

/// What happened, and the exit status: 0 when everything stopped, 1 when a
/// check failed, 4 when a process turned out to be another one, 5 when some
/// of it still runs.
pub(crate) fn report(service: &Service, outcomes: &[(ProcessKey, Outcome)]) -> (Vec<String>, u8) {
    let problems: Vec<String> = outcomes
        .iter()
        .filter_map(|(key, outcome)| {
            let problem = match outcome {
                Outcome::Stopped | Outcome::AlreadyGone => return None,
                Outcome::Replaced => "is a different process now; left alone".to_string(),
                Outcome::StillRunning => "still running".to_string(),
                Outcome::AccessDenied => "not allowed to stop it".to_string(),
                Outcome::Failed(reason) => format!("failed: {}", printable(reason)),
            };
            Some(format!("  {}  {problem}", key.pid.0))
        })
        .collect();
    let any = |wanted: fn(&Outcome) -> bool| outcomes.iter().any(|(_, outcome)| wanted(outcome));
    let name = printable(label(service));
    if outcomes
        .iter()
        .all(|(_, outcome)| *outcome == Outcome::AlreadyGone)
    {
        return (vec![format!("{name} had already stopped.")], 0);
    }
    if problems.is_empty() {
        return (vec![format!("Stopped {name}.")], 0);
    }
    let mut lines = vec![format!("Could not stop all of {name}:")];
    lines.extend(problems);
    let status = if any(|outcome| matches!(outcome, Outcome::Failed(_))) {
        FAILED
    } else if any(|outcome| *outcome == Outcome::Replaced) {
        REFUSED
    } else {
        PARTIAL
    };
    (lines, status)
}

/// Only a plain yes goes ahead.
pub(crate) fn confirmed(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

fn named(service: &Service) -> String {
    match service.processes.first() {
        Some(root) => format!("{} (pid {})", printable(label(service)), root.pid),
        None => printable(label(service)),
    }
}

fn bracketed(address: IpAddr) -> String {
    match address {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use harbormaster_engine::{
        Classification, Holder, Member, Outcome, Port, Protection, Refusal, Service, Target,
    };
    use std::cell::Cell;

    use harbormaster_platform::{BindProbe, Pid, ProcessKey, StartTime};

    use super::{FREE_LOOKS, Freed, confirmed, freed, preview, refusal, report, untracked_exec};

    /// How `freed` answers when binding answers each look with `looks`, and
    /// how often it waited between them.
    fn freed_after(looks: &[BindProbe]) -> (Freed, usize) {
        let ports = [Port {
            port: 3000,
            address: "127.0.0.1".parse().unwrap(),
        }];
        let (look, waits) = (Cell::new(0), Cell::new(0));
        let probe = |_: IpAddr, _: u16| {
            let answer = looks[look.get().min(looks.len() - 1)].clone();
            look.set(look.get() + 1);
            answer
        };
        let answer = freed(&ports, &probe, &|| waits.set(waits.get() + 1));
        (answer, waits.get())
    }

    #[test]
    fn calls_a_port_free_once_binding_it_works() {
        assert_eq!(freed_after(&[BindProbe::Free]), (Freed::Yes, 0));
        // A server on its way out may hold the port a moment longer.
        assert_eq!(
            freed_after(&[BindProbe::InUse, BindProbe::Free]),
            (Freed::Yes, 1)
        );
    }

    #[test]
    fn calls_a_port_taken_while_something_still_holds_it() {
        assert_eq!(
            freed_after(&[BindProbe::InUse]),
            (Freed::Taken, FREE_LOOKS - 1)
        );
    }

    #[test]
    fn looks_at_both_families_a_dual_stack_port_takes() {
        let ports = [Port {
            port: 3000,
            address: "::".parse().unwrap(),
        }];
        // Something kept the port on IPv4, which an IPv6-only probe misses.
        let probe = |address: IpAddr, _: u16| {
            if address.is_ipv4() {
                BindProbe::InUse
            } else {
                BindProbe::Free
            }
        };

        assert_eq!(freed(&ports, &probe, &|| {}), Freed::Taken);
    }

    #[test]
    fn says_nothing_of_a_port_the_system_wont_tell_about() {
        assert_eq!(freed_after(&[BindProbe::Denied]), (Freed::Unknown, 0));
        assert_eq!(
            freed_after(&[BindProbe::InUse, BindProbe::Failed("odd".into())]),
            (Freed::Unknown, 1)
        );
    }

    fn member(pid: u32, command: &str) -> Member {
        Member {
            key: key(pid),
            pid,
            name: "node".into(),
            command: Some(command.into()),
            memory_bytes: None,
            cpu_percent: None,
        }
    }

    fn key(pid: u32) -> ProcessKey {
        ProcessKey {
            pid: Pid(pid),
            start: StartTime(1),
            exec: 0,
        }
    }

    fn server() -> Service {
        Service {
            id: "20-1".into(),
            classification: Classification::Dev,
            protected: None,
            ports: vec![Port {
                port: 5173,
                address: IpAddr::from([127, 0, 0, 1]),
            }],
            project: None,
            command: Some("npm run dev".into()),
            started_at: 0,
            memory_bytes: None,
            cpu_percent: None,
            processes: vec![member(20, "npm run dev"), member(30, "vite --host")],
            holders: vec![Holder {
                pid: 30,
                name: "node".into(),
            }],
            ancestors: vec![
                Holder {
                    pid: 10,
                    name: "zsh".into(),
                },
                Holder {
                    pid: 5,
                    name: "Terminal".into(),
                },
            ],
        }
    }

    const PORT: Target = Target::Port {
        port: 5173,
        address: None,
    };

    #[test]
    fn previews_what_stops_what_frees_up_and_what_stays_open() {
        assert_eq!(
            preview(&server()),
            [
                "Stop node on :5173?",
                "  stops       20  node  npm run dev",
                "              30  node  vite --host",
                "  frees       :5173",
                "  stays open  zsh (10), Terminal (5)",
            ]
        );
    }

    #[test]
    fn says_why_nothing_was_stopped_and_exits_accordingly() {
        let (lines, status) = refusal(&Refusal::NotFound, &PORT);
        assert_eq!(
            lines,
            [":5173: nothing Harbormaster can see listens there."]
        );
        assert_eq!(status, 3);

        let database = Service {
            protected: Some(Protection::Database),
            ..server()
        };
        let (lines, status) = refusal(&Refusal::Protected(&database, Protection::Database), &PORT);
        assert!(
            lines[0].starts_with("Harbormaster won't stop node:"),
            "{lines:?}"
        );
        assert_eq!(status, 4);
    }

    #[test]
    fn refuses_to_stop_where_a_key_cant_follow_an_exec() {
        let (lines, status) = untracked_exec();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("Harbormaster won't stop anything"),
            "{lines:?}"
        );
        assert_eq!(status, 4);
    }

    #[test]
    fn lists_the_choices_when_a_port_is_ambiguous() {
        let lan = Service {
            ports: vec![Port {
                port: 5173,
                address: IpAddr::from([192, 168, 1, 5]),
            }],
            ..server()
        };
        let local = server();

        let (lines, status) = refusal(&Refusal::Ambiguous(vec![&local, &lan]), &PORT);

        assert_eq!(
            lines,
            [
                ":5173 is used by more than one server:",
                "  127.0.0.1    node (pid 20)",
                "  192.168.1.5  node (pid 20)",
                "Pick one with --address.",
            ]
        );
        assert_eq!(status, 4);
    }

    #[test]
    fn reports_partial_stops_with_their_own_exit_code() {
        let (lines, status) = report(
            &server(),
            &[(key(30), Outcome::Stopped), (key(20), Outcome::Stopped)],
        );
        assert_eq!(lines, ["Stopped node."]);
        assert_eq!(status, 0);

        let (lines, status) = report(
            &server(),
            &[
                (key(30), Outcome::Stopped),
                (key(20), Outcome::StillRunning),
            ],
        );
        assert_eq!(
            lines,
            ["Could not stop all of node:", "  20  still running"]
        );
        assert_eq!(status, 5);
    }

    #[test]
    fn treats_a_changed_identity_as_refused_and_a_failed_check_as_an_error() {
        let (lines, status) = report(
            &server(),
            &[(key(30), Outcome::Stopped), (key(20), Outcome::Replaced)],
        );
        assert_eq!(
            lines,
            [
                "Could not stop all of node:",
                "  20  is a different process now; left alone",
            ]
        );
        assert_eq!(status, 4);

        let (_, status) = report(
            &server(),
            &[
                (key(30), Outcome::Replaced),
                (key(20), Outcome::Failed("boom".into())),
            ],
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn says_when_the_server_had_already_stopped() {
        let (lines, status) = report(
            &server(),
            &[
                (key(30), Outcome::AlreadyGone),
                (key(20), Outcome::AlreadyGone),
            ],
        );

        assert_eq!(lines, ["node had already stopped."]);
        assert_eq!(status, 0);
    }

    #[test]
    fn takes_only_a_clear_yes() {
        assert!(confirmed("y\n"));
        assert!(confirmed("Yes"));
        assert!(!confirmed(""));
        assert!(!confirmed("n"));
        assert!(!confirmed("yep"));
    }
}
