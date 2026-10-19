#![cfg(target_os = "macos")]

use std::net::TcpListener;
use std::sync::{Mutex, MutexGuard, PoisonError};

use harbormaster_platform::{Listener, listeners, processes};

/// One test at a time: macOS marks a new socket close-on-exec only after
/// creating it, so a child another test starts in between would keep this
/// test's socket open, and its identity, after the test closes it.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn take_turn() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

fn find_own(port: u16) -> Option<Listener> {
    let own = std::process::id();
    listeners()
        .expect("listing sockets should succeed")
        .into_iter()
        .find(|listener| listener.owner.pid.0 == own && listener.port == port)
}

#[test]
fn reports_an_ipv4_socket_this_process_listens_on() {
    let _turn = take_turn();
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
    let _turn = take_turn();
    let socket = TcpListener::bind("[::1]:0").unwrap();
    let port = socket.local_addr().unwrap().port();

    let found = find_own(port).expect("our own IPv6 listener should be reported");

    assert_eq!(found.address, "::1".parse::<std::net::IpAddr>().unwrap());
}

#[test]
fn does_not_report_a_socket_after_it_closes() {
    let _turn = take_turn();
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let seen = find_own(port)
        .expect("the open socket should be reported")
        .socket;
    drop(socket);

    // Another test may take the port meanwhile, with a socket of its own.
    let still = listeners()
        .expect("listing sockets should succeed")
        .into_iter()
        .any(|listener| listener.port == port && listener.socket == seen);

    assert!(seen.is_some());
    assert!(!still);
}

#[test]
fn identifies_the_owner_by_the_same_key_as_the_process_table() {
    let _turn = take_turn();
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
    let _turn = take_turn();
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
    let _turn = take_turn();
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

/// A program that listens on a loopback port with SO_REUSEPORT, the one
/// given or a free one, and only then prints which.
const SHARER: &str = r#"
#include <arpa/inet.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(int argc, char **argv) {
    int s = socket(AF_INET, SOCK_STREAM, 0), yes = 1;
    struct sockaddr_in at;
    memset(&at, 0, sizeof at);
    at.sin_family = AF_INET;
    at.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    at.sin_port = htons(argc > 1 ? atoi(argv[1]) : 0);
    socklen_t size = sizeof at;
    if (s < 0 || setsockopt(s, SOL_SOCKET, SO_REUSEPORT, &yes, sizeof yes) != 0) return 1;
    if (bind(s, (struct sockaddr *)&at, size) != 0 || listen(s, 8) != 0) return 1;
    if (getsockname(s, (struct sockaddr *)&at, &size) != 0) return 1;
    printf("%d\n", ntohs(at.sin_port));
    fflush(stdout);
    for (;;) pause();
}
"#;

/// A child process, ended whatever happens to the test.
struct Sharing(std::process::Child);

impl Drop for Sharing {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn share(program: &std::path::Path, port: u16) -> (Sharing, u16) {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    let mut child = Command::new(program)
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the sharer should start");
    let output = child.stdout.take().expect("the sharer's output");
    let sharing = Sharing(child);
    let mut line = String::new();
    std::io::BufReader::new(output)
        .read_line(&mut line)
        .expect("the sharer should say its port");
    let port = line
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("the sharer said {line:?}"));
    (sharing, port)
}

#[test]
fn tells_apart_sockets_two_processes_hold_on_one_port() {
    let _turn = take_turn();
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("sharer.c");
    std::fs::write(&source, SHARER).unwrap();
    let program = folder.path().join("sharer");
    let built = std::process::Command::new("cc")
        .arg("-o")
        .arg(&program)
        .arg(&source)
        .status()
        .expect("cc should run");
    assert!(built.success());

    let (first, port) = share(&program, 0);
    let (second, _) = share(&program, port);
    let pids = [first.0.id(), second.0.id()];
    let found: Vec<Listener> = listeners()
        .expect("listing sockets should succeed")
        .into_iter()
        .filter(|listener| listener.port == port && pids.contains(&listener.owner.pid.0))
        .collect();

    assert_eq!(found.len(), 2, "both processes should listen: {found:?}");
    assert_ne!(found[0].socket, found[1].socket, "{found:?}");
}
