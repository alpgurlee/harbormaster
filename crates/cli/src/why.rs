use harbormaster_engine::{PortReport, Service};

use crate::format::printable;

/// The sentences `harbormaster why` prints for a port.
pub(crate) fn lines(port: u16, report: &PortReport<'_>) -> Vec<String> {
    match report {
        PortReport::Free => vec![format!(":{port} is free.")],
        PortReport::HiddenOwner => vec![format!(
            ":{port} is taken by a process Harbormaster can't see, usually one run by another user or the system."
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

fn holder(port: u16, service: &Service) -> Vec<String> {
    let Some(process) = service.processes.first() else {
        return vec![format!(":{port} is in use.")];
    };
    let name = service
        .project
        .as_ref()
        .map_or(&process.name, |project| &project.name);
    let mut lines = vec![format!(
        ":{port} is used by {} (pid {}).",
        printable(name),
        process.pid
    )];
    if let Some(command) = &service.command
        && command != name
    {
        lines.push(format!("  started with: {}", printable(command)));
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

    use harbormaster_engine::{Classification, Member, Port, PortReport, Service};

    use super::lines;

    fn service(name: &str, command: &str, port: u16) -> Service {
        Service {
            id: "1-1".into(),
            classification: Classification::Other,
            protected: false,
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
                pid: 772,
                name: name.into(),
                command: Some(command.into()),
                memory_bytes: None,
                cpu_percent: None,
            }],
        }
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
