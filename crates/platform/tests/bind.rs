use std::net::{IpAddr, Ipv4Addr, TcpListener};

use harbormaster_platform::{BindProbe, probe_bind};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

#[test]
fn reports_a_port_another_socket_listens_on_as_in_use() {
    let socket = TcpListener::bind((LOOPBACK, 0)).unwrap();
    let port = socket.local_addr().unwrap().port();

    assert_eq!(probe_bind(LOOPBACK, port), BindProbe::InUse);
}

#[test]
fn reports_an_unused_port_as_free_and_releases_it() {
    let port = TcpListener::bind((LOOPBACK, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    assert_eq!(probe_bind(LOOPBACK, port), BindProbe::Free);
    assert!(
        TcpListener::bind((LOOPBACK, port)).is_ok(),
        "the probe must not keep the port"
    );
}

#[test]
fn reports_an_address_this_machine_does_not_have_as_unavailable() {
    // 192.0.2.0/24 is reserved for documentation and never assigned locally.
    let address = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));

    assert_eq!(probe_bind(address, 3000), BindProbe::Unavailable);
}
