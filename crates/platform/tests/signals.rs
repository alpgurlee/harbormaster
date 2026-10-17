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

/// Waits until `child` has exited but isn't reaped yet, a zombie the kernel
/// keeps with the same identity.
fn zombie(child: &Child) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // SAFETY: an all-zero siginfo_t is valid, and waitid only fills it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `info` lives across the call; WNOWAIT leaves the child to `wait`.
        let status = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        assert_eq!(status, 0, "{}", std::io::Error::last_os_error());
        if u32::try_from(info.si_pid).is_ok_and(|pid| pid == child.id()) {
            return;
        }
        assert!(Instant::now() < deadline, "the child should have exited");
        sleep(Duration::from_millis(10));
    }
}

#[test]
fn does_not_count_a_zombie_as_running() {
    let (mut child, key) = sleeper();
    assert_eq!(presence(key), Presence::Running);
    child.kill().unwrap();
    zombie(&child);

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
