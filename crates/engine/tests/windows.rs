#![cfg(windows)]

use std::net::TcpListener;

use harbormaster_engine::{Protection, Scanner, Standing, live};

#[test]
fn finds_the_server_this_process_runs() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let home = tempfile::tempdir().unwrap();

    let snapshot = live::scan(&mut Scanner::default(), home.path()).unwrap();

    let own = std::process::id();
    assert!(
        snapshot.services.iter().any(|service| {
            service.ports.iter().any(|candidate| candidate.port == port)
                && service.processes.iter().any(|member| member.pid == own)
        }),
        "no service holds :{port} with pid {own}"
    );
}

#[test]
fn refuses_to_clear_a_stop_it_cannot_check_without_a_command_line() {
    let own = std::process::id();
    let key = harbormaster_platform::processes()
        .unwrap()
        .into_iter()
        .find(|entry| entry.key.pid.0 == own)
        .unwrap()
        .key;
    let home = tempfile::tempdir().unwrap();

    let guards = live::guards(home.path(), &[key]).unwrap();

    assert_eq!(
        guards.get(&key),
        Some(&Standing::Protected(Protection::Unverified))
    );
}

#[test]
fn finds_no_chain_up_to_a_root_a_stop_could_trust() {
    // Windows doesn't hand an orphan to a root process as macOS does, so the
    // chain always has a gap and a stop is refused.
    assert_eq!(live::lineage().unwrap(), None);
}
