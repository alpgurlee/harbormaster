use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use harbormaster_platform::BindProbe;

use crate::{Service, Snapshot};

/// Why a port is or isn't available.
#[derive(Debug, Clone, PartialEq)]
pub enum PortReport<'a> {
    InUse(Vec<&'a Service>),
    Free,
    /// Refused on loopback, free on every interface: macOS keeps ports below
    /// 1024 on 127.0.0.1 for root but lets anyone bind them on 0.0.0.0.
    FreeOnEveryInterface,
    /// Binding fails as if it were taken, yet no process we can see listens:
    /// another user's or the system's process, or a port the system reserves.
    InUseUnseen,
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

    let probes: Vec<(IpAddr, BindProbe)> = [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ]
    .into_iter()
    .map(|address| (address, probe(address, port)))
    .collect();
    let any = |wanted: BindProbe| probes.iter().any(|(_, outcome)| *outcome == wanted);
    if any(BindProbe::InUse) {
        return PortReport::InUseUnseen;
    }
    if any(BindProbe::Denied) {
        let everywhere = [
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        ]
        .map(|address| probe(address, port));
        return if everywhere.contains(&BindProbe::InUse) {
            PortReport::InUseUnseen
        } else if everywhere.contains(&BindProbe::Free) {
            PortReport::FreeOnEveryInterface
        } else {
            PortReport::Denied
        };
    }
    // A missing address, such as no IPv6 loopback, says nothing about the port.
    let failures: Vec<String> = probes
        .iter()
        .filter_map(|(address, outcome)| match outcome {
            BindProbe::Failed(reason) => Some(format!("{address}: {reason}")),
            _ => None,
        })
        .collect();
    if !failures.is_empty() {
        PortReport::Unknown(failures.join("; "))
    } else if any(BindProbe::Free) {
        PortReport::Free
    } else {
        PortReport::Unknown("this machine has no loopback address".into())
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
                protected: None,
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
                holders: Vec::new(),
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
            PortReport::InUseUnseen
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
    fn says_when_a_port_is_free_only_for_servers_on_every_interface() {
        let snapshot = Snapshot {
            services: Vec::new(),
        };
        let probe = |address: IpAddr, _| {
            if address.is_loopback() {
                BindProbe::Denied
            } else {
                BindProbe::Free
            }
        };

        assert_eq!(
            explain_port(80, &snapshot, &probe),
            PortReport::FreeOnEveryInterface
        );
    }

    #[test]
    fn treats_a_machine_without_ipv6_as_free_when_ipv4_binds() {
        let snapshot = Snapshot {
            services: Vec::new(),
        };
        let probe = |address: IpAddr, _| {
            if address.is_ipv6() {
                BindProbe::Unavailable
            } else {
                BindProbe::Free
            }
        };

        assert_eq!(explain_port(3000, &snapshot, &probe), PortReport::Free);
    }

    #[test]
    fn does_not_hide_an_unexpected_ipv6_failure_behind_a_free_ipv4() {
        let snapshot = Snapshot {
            services: Vec::new(),
        };
        let probe = |address: IpAddr, _| {
            if address.is_ipv6() {
                BindProbe::Failed("boom".into())
            } else {
                BindProbe::Free
            }
        };

        assert_eq!(
            explain_port(3000, &snapshot, &probe),
            PortReport::Unknown("::1: boom".into())
        );
    }
}
