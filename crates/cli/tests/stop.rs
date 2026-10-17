use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Output, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Long enough for a slow machine, short enough that a hang fails the test.
const PATIENCE: Duration = Duration::from_secs(30);

/// One test at a time: macOS marks a new pipe close-on-exec only after
/// creating it, so a child another test starts in between could keep this
/// test's pipes open, and each test lists every listener on the machine.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn take_turn() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A server of our own: it takes a loopback port, a free one unless told
/// which, and only then says which, so it is ready and the port is its own.
/// Then it echoes each byte it reads, which shows it is still alive.
/// `stubborn` ignores SIGTERM first, as a hung server would; `bound` binds
/// the port without listening; `shared` sets SO_REUSEPORT, so another can
/// bind the port too.
const SERVER: &str = r#"
#include <arpa/inet.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "";
    int yes = 1;
    if (strstr(mode, "stubborn")) signal(SIGTERM, SIG_IGN);
    int s = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in at;
    memset(&at, 0, sizeof at);
    at.sin_family = AF_INET;
    at.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    at.sin_port = htons(argc > 2 ? atoi(argv[2]) : 0);
    socklen_t size = sizeof at;
    if (s < 0) return 1;
    if (strstr(mode, "shared") && setsockopt(s, SOL_SOCKET, SO_REUSEPORT, &yes, sizeof yes) != 0) return 1;
    if (bind(s, (struct sockaddr *)&at, size) != 0) return 1;
    if (!strstr(mode, "bound") && listen(s, 1) != 0) return 1;
    if (getsockname(s, (struct sockaddr *)&at, &size) != 0) return 1;
    printf("%d\n", ntohs(at.sin_port));
    fflush(stdout);
    char c;
    while (read(0, &c, 1) == 1) write(1, &c, 1);
    for (;;) pause();
}
"#;

fn program() -> &'static Path {
    static PROGRAM: OnceLock<PathBuf> = OnceLock::new();
    PROGRAM.get_or_init(|| {
        let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let source = folder.join("server.c");
        std::fs::write(&source, SERVER).expect("the server's source should be written");
        let program = folder.join("server");
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

/// A test server, ended whatever happens to the test.
struct Server {
    child: Child,
    output: BufReader<ChildStdout>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn start(args: &[&str]) -> (Self, u16) {
        let mut child = Command::new(program())
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the server should start");
        let output = BufReader::new(child.stdout.take().expect("the server's output"));
        let mut server = Self { child, output };
        let mut line = String::new();
        server
            .output
            .read_line(&mut line)
            .expect("the server should say its port");
        let port = line
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("the server said {line:?}"));
        (server, port)
    }

    /// Whether it still answers: no signal that ends it is waiting for it.
    fn answers(&mut self) -> bool {
        let stdin = self.child.stdin.as_mut().expect("the server's input");
        let mut echo = [0];
        stdin.write_all(b"?").is_ok() && self.output.read_exact(&mut echo).is_ok()
    }

    /// How it ended, once it has.
    fn ended(&mut self) -> ExitStatus {
        finished(&mut self.child).expect("the server should have ended")
    }
}

/// How `child` ended, if it did within the patience a test has.
fn finished(child: &mut Child) -> Option<ExitStatus> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(status) = child.try_wait().expect("the child should be waited for") {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        sleep(Duration::from_millis(20));
    }
}

fn harbormaster(args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_harbormaster"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("harbormaster should run");
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = pipe.read_to_end(&mut text);
            text
        })
    };
    let stdout = read(Box::new(child.stdout.take().expect("its output")));
    let stderr = read(Box::new(child.stderr.take().expect("its errors")));
    let Some(status) = finished(&mut child) else {
        let _ = child.kill();
        let _ = child.wait();
        panic!("harbormaster didn't finish within {PATIENCE:?}");
    };
    Output {
        status,
        stdout: stdout.join().expect("its output should be read"),
        stderr: stderr.join().expect("its errors should be read"),
    }
}

#[test]
fn stops_the_server_on_a_port_when_told_yes() {
    let _turn = take_turn();
    let (mut server, port) = Server::start(&[]);

    let output = harbormaster(&["stop", &port.to_string(), "--yes"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(server.ended().signal(), Some(libc::SIGTERM));
    assert!(String::from_utf8_lossy(&output.stdout).contains("is free now"));
}

#[test]
fn stops_nothing_without_a_yes() {
    let _turn = take_turn();
    let (mut server, port) = Server::start(&[]);

    let output = harbormaster(&["stop", &port.to_string()]);

    assert_eq!(output.status.code(), Some(130), "{output:?}");
    assert!(server.answers());
}

#[test]
fn stops_the_server_a_process_belongs_to() {
    let _turn = take_turn();
    let (mut server, _) = Server::start(&[]);

    let output = harbormaster(&["stop", "--pid", &server.child.id().to_string(), "--yes"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(server.ended().signal(), Some(libc::SIGTERM));
}

#[test]
fn exits_with_3_when_nothing_listens() {
    let _turn = take_turn();
    // Held, so nothing else takes it meanwhile, but not listened on.
    let (_held, port) = Server::start(&["bound"]);

    let output = harbormaster(&["stop", &port.to_string(), "--yes"]);

    assert_eq!(output.status.code(), Some(3), "{output:?}");
}

#[test]
fn says_a_port_isnt_free_yet_while_something_else_holds_it() {
    let _turn = take_turn();
    let (mut server, port) = Server::start(&["shared"]);
    let (_holder, _) = Server::start(&["shared bound", &port.to_string()]);

    let output = harbormaster(&["stop", &port.to_string(), "--yes"]);

    let said = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(server.ended().signal(), Some(libc::SIGTERM));
    assert!(said.contains("isn't free yet"), "{said}");
    assert!(!said.contains("is free now"), "{said}");
}

#[test]
fn reports_a_server_that_outlives_the_grace_period() {
    let _turn = take_turn();
    let (mut server, port) = Server::start(&["stubborn"]);

    let output = harbormaster(&["stop", &port.to_string(), "--yes", "--wait", "1"]);

    assert_eq!(output.status.code(), Some(5), "{output:?}");
    assert!(server.answers());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--force"));
}

#[test]
fn ends_a_server_that_ignores_the_request_when_forced() {
    let _turn = take_turn();
    let (mut server, port) = Server::start(&["stubborn"]);

    let output = harbormaster(&["stop", &port.to_string(), "--yes", "--force", "--wait", "1"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(server.ended().signal(), Some(libc::SIGKILL));
}
