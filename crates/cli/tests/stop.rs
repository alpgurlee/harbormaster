use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Output, Stdio};
use std::thread::sleep;
use std::time::Duration;

use harbormaster_platform::{BindProbe, probe_bind};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// A test server, ended whatever happens to the test.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind((LOOPBACK, 0))
        .and_then(|socket| socket.local_addr())
        .expect("a free port")
        .port()
}

/// A plain listener that isn't part of any dev tooling.
fn listener() -> (Server, u16) {
    let port = free_port();
    let child = Command::new("/usr/bin/nc")
        .args(["-l", "127.0.0.1", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("nc should start");
    for _ in 0..100 {
        if probe_bind(LOOPBACK, port) == BindProbe::InUse {
            break;
        }
        sleep(Duration::from_millis(20));
    }
    (Server(child), port)
}

fn harbormaster(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_harbormaster"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("harbormaster should run")
}

#[test]
fn stops_the_server_on_a_port_when_told_yes() {
    let (mut server, port) = listener();

    let output = harbormaster(&["stop", &port.to_string(), "--yes"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(server.0.wait().unwrap().signal(), Some(libc::SIGTERM));
    assert!(String::from_utf8_lossy(&output.stdout).contains("is free now"));
}

#[test]
fn stops_nothing_without_a_yes() {
    let (mut server, port) = listener();

    let output = harbormaster(&["stop", &port.to_string()]);
    let still_running = server.0.try_wait().unwrap().is_none();

    assert_eq!(output.status.code(), Some(130), "{output:?}");
    assert!(still_running);
}

#[test]
fn stops_the_server_a_process_belongs_to() {
    let (mut server, _) = listener();

    let output = harbormaster(&["stop", "--pid", &server.0.id().to_string(), "--yes"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(server.0.wait().unwrap().signal(), Some(libc::SIGTERM));
}

#[test]
fn exits_with_3_when_nothing_listens() {
    let output = harbormaster(&["stop", &free_port().to_string(), "--yes"]);

    assert_eq!(output.status.code(), Some(3), "{output:?}");
}

/// A listener that ignores SIGTERM, as a hung server would.
fn stubborn_listener() -> (Server, u16) {
    let port = free_port();
    let child = Command::new("/bin/sh")
        .args([
            "-c",
            &format!("trap '' TERM; exec /usr/bin/nc -l 127.0.0.1 {port}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("sh should start");
    for _ in 0..100 {
        if probe_bind(LOOPBACK, port) == BindProbe::InUse {
            break;
        }
        sleep(Duration::from_millis(20));
    }
    (Server(child), port)
}

#[test]
fn reports_a_server_that_outlives_the_grace_period() {
    let (mut server, port) = stubborn_listener();

    let output = harbormaster(&["stop", &port.to_string(), "--yes", "--wait", "1"]);
    let still_running = server.0.try_wait().unwrap().is_none();

    assert_eq!(output.status.code(), Some(5), "{output:?}");
    assert!(still_running);
    assert!(String::from_utf8_lossy(&output.stdout).contains("--force"));
}

#[test]
fn ends_a_server_that_ignores_the_request_when_forced() {
    let (mut server, port) = stubborn_listener();

    let output = harbormaster(&["stop", &port.to_string(), "--yes", "--force", "--wait", "1"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(server.0.wait().unwrap().signal(), Some(libc::SIGKILL));
}
