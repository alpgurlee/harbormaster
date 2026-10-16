use std::collections::HashSet;
use std::net::TcpListener;

use harbormaster_engine::{Scanner, Standing, live};

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
fn traces_this_process_up_to_launchd() {
    let lineage = live::lineage().unwrap().expect("an unbroken chain");

    assert_eq!(lineage.first(), Some(&std::process::id()));
    assert_eq!(lineage.last(), Some(&1));
}

#[test]
fn finds_nothing_against_stopping_a_plain_process_of_this_user() {
    let own = std::process::id();
    let key = harbormaster_platform::processes()
        .unwrap()
        .into_iter()
        .find(|entry| entry.key.pid.0 == own)
        .unwrap()
        .key;
    let home = tempfile::tempdir().unwrap();

    let guards = live::guards(home.path(), &[key]).unwrap();

    assert_eq!(guards.get(&key), Some(&Standing::Clear));
}

#[test]
fn tells_how_much_memory_each_app_uses() {
    let apps = live::apps(&HashSet::new()).unwrap();

    assert!(!apps.is_empty());
    assert!(
        apps.windows(2)
            .all(|pair| pair[0].memory_bytes >= pair[1].memory_bytes),
        "{apps:?}"
    );
}
