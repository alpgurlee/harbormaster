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

#[test]
fn keeps_two_sockets_one_process_holds_on_the_same_endpoint() {
    use socket2::{Domain, Socket, Type};

    let open = |port: u16| {
        let socket = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
        socket.set_reuse_port(true).unwrap();
        let address: std::net::SocketAddr = (std::net::Ipv4Addr::LOCALHOST, port).into();
        socket.bind(&address.into()).unwrap();
        socket.listen(8).unwrap();
        socket
    };
    let first = open(0);
    let port = first.local_addr().unwrap().as_socket().unwrap().port();
    let _second = open(port);

    let own = std::process::id();
    let sockets: Vec<_> = listeners()
        .unwrap()
        .into_iter()
        .filter(|listener| listener.owner.pid.0 == own && listener.port == port)
        .map(|listener| listener.socket)
        .collect();

    assert_eq!(
        sockets.len(),
        2,
        "both sockets should be reported: {sockets:?}"
    );
    assert_ne!(sockets[0], sockets[1]);
}

#[test]
fn tells_apart_sockets_two_processes_hold_on_one_port() {
    use std::process::{Command, Stdio};

    // macOS nc listens with SO_REUSEPORT, so two can share a port.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|socket| socket.local_addr())
        .expect("a free port")
        .port();
    let spawn = || {
        Command::new("/usr/bin/nc")
            .args(["-l", "127.0.0.1", &port.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("nc should start")
    };
    let mut first = spawn();
    let mut second = spawn();
    let pids = [first.id(), second.id()];
    let mut found = Vec::new();
    for _ in 0..100 {
        found = listeners()
            .expect("listing sockets should succeed")
            .into_iter()
            .filter(|listener| listener.port == port && pids.contains(&listener.owner.pid.0))
            .collect();
        if found.len() == 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = first.kill();
    let _ = second.kill();
    let _ = first.wait();
    let _ = second.wait();

    assert_eq!(found.len(), 2, "both nc processes should listen: {found:?}");
    assert_ne!(found[0].socket, found[1].socket, "{found:?}");
}
