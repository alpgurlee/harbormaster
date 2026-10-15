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
