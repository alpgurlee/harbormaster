#![cfg(target_os = "macos")]

use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::{Duration, Instant};

use harbormaster_platform::{
    Presence, ProcessKey, Signal, SignalOutcome, StartTime, presence, processes, signal,
};

/// A program of our own that echoes its input until it closes, so a test
/// can see it still answers.
fn echo() -> &'static Path {
    static PROGRAM: OnceLock<PathBuf> = OnceLock::new();
    PROGRAM.get_or_init(|| {
        let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let source = folder.join("echo.c");
        std::fs::write(
            &source,
            "#include <unistd.h>\nint main(void){char c;while(read(0,&c,1)==1)write(1,&c,1);return 0;}\n",
        )
        .expect("the program's source should be written");
        let program = folder.join("echo");
        let built = Command::new("cc")
            .arg("-o")
            .arg(&program)
            .arg(&source)
            .status()
            .expect("cc should run");
        assert!(built.success());
        program
    })
}

/// Whether `child` still answers: no signal that ends it is waiting for it.
fn answers(child: &mut Child) -> bool {
    let mut echo = [0];
    child
        .stdin
        .as_mut()
        .is_some_and(|stdin| stdin.write_all(b"?").is_ok())
        && child
            .stdout
            .as_mut()
            .is_some_and(|stdout| stdout.read_exact(&mut echo).is_ok())
}

fn waiter() -> (Child, ProcessKey) {
    let child = Command::new(echo())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the program should start");
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
    let (mut child, key) = waiter();

    assert_eq!(signal(key, Signal::Terminate), SignalOutcome::Sent);
    assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGTERM));
}

#[test]
fn never_signals_a_process_that_only_shares_the_pid() {
    let (mut child, mut key) = waiter();
    key.start = StartTime(key.start.0 + 1);

    let outcome = signal(key, Signal::Kill);
    let still_running = answers(&mut child);
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!(outcome, SignalOutcome::Replaced);
    assert!(still_running, "a reused pid must not receive the signal");
}

#[test]
fn reports_an_exited_process_as_gone() {
    let (mut child, key) = waiter();
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
    let (mut child, key) = waiter();
    assert_eq!(presence(key), Presence::Running);
    child.kill().unwrap();
    zombie(&child);

    let after = presence(key);
    child.wait().unwrap();

    assert_eq!(after, Presence::Gone);
}

#[test]
fn says_when_a_pid_belongs_to_another_process_now() {
    let (mut child, mut key) = waiter();
    key.start = StartTime(key.start.0 + 1);

    let found = presence(key);
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!(found, Presence::Replaced);
}
