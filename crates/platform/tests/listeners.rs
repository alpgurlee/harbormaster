use std::net::TcpListener;

use harbormaster_platform::{Listener, listeners, processes};

fn find_own(port: u16) -> Option<Listener> {
    let own = std::process::id();
    listeners()
        .expect("listing sockets should succeed")
        .into_iter()
        .find(|listener| listener.owner.pid.0 == own && listener.port == port)
}

#[test]
fn reports_an_ipv4_socket_this_process_listens_on() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();

    let found = find_own(port).expect("our own IPv4 listener should be reported");

    assert_eq!(
        found.address,
        "127.0.0.1".parse::<std::net::IpAddr>().unwrap()
    );
}

#[test]
fn reports_an_ipv6_socket_this_process_listens_on() {
    let socket = TcpListener::bind("[::1]:0").unwrap();
    let port = socket.local_addr().unwrap().port();

    let found = find_own(port).expect("our own IPv6 listener should be reported");

    assert_eq!(found.address, "::1".parse::<std::net::IpAddr>().unwrap());
}

#[test]
fn does_not_report_a_port_after_the_socket_closes() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);

    assert!(find_own(port).is_none());
}

#[test]
fn identifies_the_owner_by_the_same_key_as_the_process_table() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();

    let owner = find_own(port).expect("listener").owner;
    let entry = processes()
        .unwrap()
        .into_iter()
        .find(|entry| entry.key.pid == owner.pid)
        .expect("owner in process table");

    assert_eq!(owner, entry.key);
}

#[test]
fn tells_separate_sockets_apart() {
    let first = TcpListener::bind("127.0.0.1:0").unwrap();
    let second = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = find_own(first.local_addr().unwrap().port()).unwrap().socket;
    let b = find_own(second.local_addr().unwrap().port())
        .unwrap()
        .socket;

    assert!(a.is_some() && b.is_some());
    assert_ne!(a, b);
}
