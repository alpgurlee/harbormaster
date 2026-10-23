#![cfg(windows)]

use std::net::TcpListener;
use std::process::{Command, Output};

fn harbormaster(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_harbormaster"))
        .args(args)
        .output()
        .expect("harbormaster should run")
}

#[test]
fn says_which_process_holds_a_port() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();

    let output = harbormaster(&["why", &port.to_string()]);

    let said = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        said.contains(&format!("(pid {})", std::process::id())),
        "{said}"
    );
}

#[test]
fn exits_with_3_for_a_port_nothing_holds() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    let output = harbormaster(&["why", &port.to_string()]);

    assert_eq!(output.status.code(), Some(3), "{output:?}");
}

#[test]
fn lists_what_listens_as_json() {
    let output = harbormaster(&["ls", "--all", "--json"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the listing should be JSON");
    assert_eq!(document["schema"], "harbormaster/v1");
    assert!(document["services"].is_array());
}

#[test]
fn has_no_way_yet_to_stop_from_the_command_line() {
    let output = harbormaster(&["stop", "--help"]);

    assert_ne!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn shows_what_could_go_without_removing_anything() {
    let output = harbormaster(&["clean", "--json"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the listing should be JSON");
    assert!(document["findings"].is_array(), "{document}");
    assert!(document.get("cleaned").is_none(), "{document}");
}
