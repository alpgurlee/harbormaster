use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

use harbormaster_platform::{
    Presence, ProcessKey, Signal, SignalOutcome, StartTime, presence, processes, signal,
};

fn sleeper() -> (Child, ProcessKey) {
    let child = Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .expect("sleep should start");
    let key = processes()
        .expect("reading the process table should succeed")
        .into_iter()
        .find(|entry| entry.key.pid.0 == child.id())
        .expect("child in the process table")
        .key;
    (child, key)
}

#[test]
fn terminates_the_process_a_key_names() {
    let (mut child, key) = sleeper();

    assert_eq!(signal(key, Signal::Terminate), SignalOutcome::Sent);
    assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGTERM));
}

#[test]
fn never_signals_a_process_that_only_shares_the_pid() {
    let (mut child, mut key) = sleeper();
    key.start = StartTime(key.start.0 + 1);

    let outcome = signal(key, Signal::Kill);
    let still_running = child.try_wait().unwrap().is_none();
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!(outcome, SignalOutcome::Replaced);
    assert!(still_running, "a reused pid must not receive the signal");
}

#[test]
fn reports_an_exited_process_as_gone() {
    let (mut child, key) = sleeper();
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!(presence(key), Presence::Gone);
    assert_eq!(signal(key, Signal::Terminate), SignalOutcome::Gone);
}

#[test]
fn does_not_count_a_zombie_as_running() {
    let (mut child, key) = sleeper();
    assert_eq!(presence(key), Presence::Running);
    child.kill().unwrap();

    // Not reaped yet, so the kernel keeps a zombie with the same identity.
    let deadline = Instant::now() + Duration::from_secs(2);
    while presence(key) == Presence::Running && Instant::now() < deadline {
        sleep(Duration::from_millis(10));
    }
    let after = presence(key);
    child.wait().unwrap();

    assert_eq!(after, Presence::Gone);
}

#[test]
fn says_when_a_pid_belongs_to_another_process_now() {
    let (mut child, mut key) = sleeper();
    key.start = StartTime(key.start.0 + 1);

    let found = presence(key);
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!(found, Presence::Replaced);
}
