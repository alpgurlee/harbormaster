use harbormaster_engine::{PortReport, Service};

use crate::format::{label, printable};

/// The sentences `harbormaster why` prints for a port.
pub(crate) fn lines(port: u16, report: &PortReport<'_>) -> Vec<String> {
    match report {
        PortReport::Free => vec![format!(":{port} is free.")],
        PortReport::FreeOnEveryInterface => vec![format!(
            ":{port} is free for a server that listens on every interface (0.0.0.0). On 127.0.0.1 only root may use ports below 1024."
        )],
        PortReport::InUseUnseen => vec![format!(
            ":{port} is in use or reserved by the system, but no process Harbormaster can see listens on it. It may belong to another user or to the system."
        )],
        PortReport::Denied => vec![format!(":{port} can't be used: the system refused it.")],
        PortReport::Unknown(reason) => vec![format!(
            ":{port} could not be checked: {}",
            printable(reason)
        )],
        PortReport::InUse(services) => services
            .iter()
            .flat_map(|service| holder(port, service))
            .collect(),
    }
}

/// 3 tells scripts the port is free; 1 that it could not be checked.
pub(crate) fn status(report: &PortReport<'_>) -> u8 {
    match report {
        PortReport::Free | PortReport::FreeOnEveryInterface => 3,
        PortReport::Unknown(_) => 1,
        PortReport::InUse(_) | PortReport::InUseUnseen | PortReport::Denied => 0,
    }
}

fn holder(port: u16, service: &Service) -> Vec<String> {
    let Some(root) = service.processes.first() else {
        return vec![format!(":{port} is in use.")];
    };
    let name = label(service);
    let root_holds_it = service.holders.iter().all(|holder| holder.pid == root.pid);
    let mut lines = Vec::new();
    if root_holds_it {
        lines.push(format!(
            ":{port} is used by {} (pid {}).",
            printable(name),
            root.pid
        ));
    } else {
        lines.push(format!(":{port} is used by {}.", printable(name)));
        let holders: Vec<String> = service
            .holders
            .iter()
            .map(|holder| format!("{} (pid {})", printable(&holder.name), holder.pid))
            .collect();
        lines.push(format!("  held by: {}", holders.join(", ")));
    }
    if let Some(command) = &service.command
        && command != name
    {
        let command = printable(command);
        lines.push(if root_holds_it {
            format!("  started with: {command}")
        } else {
            format!("  started with: {command} (pid {})", root.pid)
        });
    }
    let airplay =
        matches!(port, 5000 | 7000) && service.processes.iter().any(|p| p.name == "ControlCenter");
    if airplay {
        lines.push(
            "  This is the macOS AirPlay Receiver. Turn it off in System Settings → General → AirDrop & Handoff."
                .to_string(),
        );
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use harbormaster_engine::{Classification, Holder, Member, Port, PortReport, Project, Service};

    use harbormaster_platform::{Pid, ProcessKey, StartTime};

    use super::{lines, status};

    fn service(name: &str, command: &str, port: u16) -> Service {
        Service {
            id: "1-1".into(),
            classification: Classification::Other,
            protected: None,
            ports: vec![Port {
                port,
                address: IpAddr::from([0, 0, 0, 0]),
            }],
            project: None,
            command: Some(command.into()),
            started_at: 0,
            memory_bytes: None,
            cpu_percent: None,
            processes: vec![Member {
                key: ProcessKey {
                    pid: Pid(772),
                    start: StartTime(1),
                    exec: 0,
                },
                pid: 772,
                name: name.into(),
                command: Some(command.into()),
                memory_bytes: None,
                cpu_percent: None,
            }],
            holders: vec![Holder {
                pid: 772,
                name: name.into(),
            }],
            ancestors: Vec::new(),
        }
    }

    fn member(pid: u32, command: &str) -> Member {
        Member {
            key: ProcessKey {
                pid: Pid(pid),
                start: StartTime(1),
                exec: 0,
            },
            pid,
            name: "node".into(),
            command: Some(command.into()),
            memory_bytes: None,
            cpu_percent: None,
        }
    }

    #[test]
    fn does_not_claim_a_process_holds_a_port_the_system_may_reserve() {
        let lines = lines(5000, &PortReport::InUseUnseen);

        assert!(lines[0].contains("or reserved by the system"), "{lines:?}");
    }

    #[test]
    fn explains_a_privileged_port_that_only_binds_on_every_interface() {
        let lines = lines(80, &PortReport::FreeOnEveryInterface);

        assert!(lines[0].contains("0.0.0.0"), "{lines:?}");
    }

    #[test]
    fn names_the_process_holding_the_socket_and_the_one_that_started_it() {
        let mut server = service("node", "npm run dev", 5173);
        server.project = Some(Project {
            name: "inkwell-web".into(),
            root: "/dev/inkwell/web".into(),
            repo_root: None,
            framework: None,
            branch: None,
            worktree: false,
        });
        server.processes = vec![member(94786, "npm run dev"), member(94826, "vite --host")];
        server.holders = vec![Holder {
            pid: 94826,
            name: "node".into(),
        }];

        assert_eq!(
            lines(5173, &PortReport::InUse(vec![&server])),
            [
                ":5173 is used by inkwell-web.",
                "  held by: node (pid 94826)",
                "  started with: npm run dev (pid 94786)",
            ]
        );
    }

    #[test]
    fn exits_with_three_for_a_free_port() {
        let holder = service("node", "node server.js", 3000);

        assert_eq!(status(&PortReport::Free), 3);
        assert_eq!(status(&PortReport::FreeOnEveryInterface), 3);
        assert_eq!(status(&PortReport::InUse(vec![&holder])), 0);
        assert_eq!(status(&PortReport::InUseUnseen), 0);
        assert_eq!(status(&PortReport::Unknown("boom".into())), 1);
    }

    #[test]
    fn says_a_free_port_is_free() {
        assert_eq!(lines(3000, &PortReport::Free), [":3000 is free."]);
    }

    #[test]
    fn names_the_holder_and_explains_airplay_on_macos() {
        let holder = service("ControlCenter", "ControlCenter", 5000);

        let lines = lines(5000, &PortReport::InUse(vec![&holder]));

        assert_eq!(lines[0], ":5000 is used by ControlCenter (pid 772).");
        assert!(lines[1].contains("AirPlay Receiver"));
    }
}
