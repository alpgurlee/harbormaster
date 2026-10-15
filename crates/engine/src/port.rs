use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use harbormaster_platform::BindProbe;

use crate::{Service, Snapshot};

/// Why a port is or isn't available.
#[derive(Debug, Clone, PartialEq)]
pub enum PortReport<'a> {
    InUse(Vec<&'a Service>),
    Free,
    /// Something holds it, but not a process this user can inspect.
    HiddenOwner,
    Denied,
    Unknown(String),
}

pub fn explain_port<'a>(
    port: u16,
    snapshot: &'a Snapshot,
    probe: &dyn Fn(IpAddr, u16) -> BindProbe,
) -> PortReport<'a> {
    let holders: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| service.ports.iter().any(|candidate| candidate.port == port))
        .collect();
    if !holders.is_empty() {
        return PortReport::InUse(holders);
    }

    let probes = [
        probe(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        probe(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
    ];
    if probes.contains(&BindProbe::InUse) {
        PortReport::HiddenOwner
    } else if probes.contains(&BindProbe::Denied) {
        PortReport::Denied
    } else if probes[0] == BindProbe::Free {
        // An IPv6 failure on its own usually means the machine has no IPv6.
        PortReport::Free
    } else {
        match &probes[0] {
            BindProbe::Failed(reason) => PortReport::Unknown(reason.clone()),
            _ => PortReport::Unknown(String::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use harbormaster_platform::BindProbe;

    use super::{PortReport, explain_port};
    use crate::{Classification, Port, Service, Snapshot};

    fn snapshot_with_port(port: u16) -> Snapshot {
        Snapshot {
            services: vec![Service {
                id: "1-1".into(),
                classification: Classification::Dev,
                protected: false,
                ports: vec![Port {
                    port,
                    address: IpAddr::from([127, 0, 0, 1]),
                }],
                project: None,
                command: Some("npm run dev".into()),
                started_at: 0,
                memory_bytes: None,
                cpu_percent: None,
                processes: Vec::new(),
            }],
        }
    }

    #[test]
    fn names_the_service_that_holds_the_port() {
        let snapshot = snapshot_with_port(3000);

        let report = explain_port(3000, &snapshot, &|_, _| BindProbe::InUse);

        assert_eq!(report, PortReport::InUse(vec![&snapshot.services[0]]));
    }

    #[test]
    fn says_free_when_nothing_listens_and_the_port_binds() {
        let snapshot = Snapshot {
            services: Vec::new(),
        };

        assert_eq!(
            explain_port(3000, &snapshot, &|_, _| BindProbe::Free),
            PortReport::Free
        );
    }

    #[test]
    fn spots_an_owner_we_cannot_see() {
        let snapshot = snapshot_with_port(8080);

        assert_eq!(
            explain_port(3000, &snapshot, &|_, _| BindProbe::InUse),
            PortReport::HiddenOwner
        );
    }

    #[test]
    fn reports_when_the_system_refuses_the_port() {
        let snapshot = Snapshot {
            services: Vec::new(),
        };

        assert_eq!(
            explain_port(80, &snapshot, &|_, _| BindProbe::Denied),
            PortReport::Denied
        );
    }

    #[test]
    fn treats_a_machine_without_ipv6_as_free_when_ipv4_binds() {
        let snapshot = Snapshot {
            services: Vec::new(),
        };
        let probe = |address: IpAddr, _| {
            if address.is_ipv6() {
                BindProbe::Failed("no IPv6".into())
            } else {
                BindProbe::Free
            }
        };

        assert_eq!(explain_port(3000, &snapshot, &probe), PortReport::Free);
    }
}
