#![cfg(windows)]

use std::net::{IpAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use harbormaster_platform::{
    Field, Owner, Pid, Presence, ProcessEntry, ProcessKey, Signal, SignalOutcome, StartTime,
    details, executable, listeners, presence, processes, runs_as_root, signal, trash, whereabouts,
};

/// Long enough for a slow machine, short enough that a hang fails the test.
const PATIENCE: Duration = Duration::from_secs(10);

/// One test at a time where each lists every listener on the machine, so
/// one test's sockets never stand in for another's.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn take_turn() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

fn find(pid: u32) -> Option<ProcessEntry> {
    processes()
        .expect("reading the process table should succeed")
        .into_iter()
        .find(|entry| entry.key.pid == Pid(pid))
}

fn own() -> ProcessEntry {
    find(std::process::id()).expect("this process should be listed")
}

/// Windows' own `sort`, which waits until its input closes.
fn waiter() -> PathBuf {
    let system = std::env::var_os("SystemRoot").expect("Windows says where it lives");
    Path::new(&system).join("System32").join("sort.exe")
}

/// A waiting child, ended whatever happens to the test.
struct Waiting(Child);

impl Drop for Waiting {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait() -> (Waiting, ProcessKey) {
    let child = Command::new(waiter())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("sort should start");
    let waiting = Waiting(child);
    let key = find(waiting.0.id())
        .expect("the child should be listed")
        .key;
    (waiting, key)
}

#[test]
fn lists_this_process_by_name_with_a_parent_that_started_first() {
    let own = own();

    let parent = own
        .parent
        .expect("the test runner should be listed as our parent");
    let parent = find(parent.0).expect("our parent should be listed");
    assert!(parent.key.start <= own.key.start);
    assert_eq!(own.owner, Owner::CurrentUser);
    let program = std::env::current_exe().unwrap();
    assert_eq!(
        Some(own.name.as_str()),
        program.file_name().and_then(|name| name.to_str())
    );
}

#[test]
fn reports_a_stable_identity_for_the_same_process() {
    assert_eq!(own().key, own().key);
}

#[test]
fn measures_memory_and_cpu_time_of_own_processes() {
    let own = own();

    assert!(own.memory_bytes.is_some_and(|bytes| bytes > 0));
    assert!(own.cpu_time.is_some());
}

#[test]
fn lists_processes_windows_runs_for_itself() {
    let table = processes().unwrap();

    assert!(
        table.iter().any(|entry| entry.owner == Owner::System),
        "no process of Windows' own accounts in {} listed",
        table.len()
    );
}

/// Whether `a` and `b` name the same folder, however each is spelled.
fn same_folder(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// A child of `program` started in `folder` with `/r`, and its key.
fn started_in(program: &Path, folder: &Path) -> (Waiting, ProcessKey) {
    let child = Command::new(program)
        .arg("/r")
        .current_dir(folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("the child should start");
    let waiting = Waiting(child);
    let key = find(waiting.0.id())
        .expect("the child should be listed")
        .key;
    (waiting, key)
}

#[test]
fn reads_a_childs_program_arguments_and_working_folder() {
    let folder = tempfile::tempdir().unwrap();
    let (_waiting, key) = started_in(&waiter(), folder.path());

    let info = details(key);

    let Field::Known(exe) = &info.exe else {
        panic!("the child's program should be readable: {:?}", info.exe);
    };
    assert!(
        exe.to_string_lossy()
            .to_ascii_lowercase()
            .ends_with("sort.exe"),
        "{exe:?}"
    );
    let Field::Known(args) = &info.args else {
        panic!(
            "the child's command line should be readable: {:?}",
            info.args
        );
    };
    assert!(
        args[0].to_ascii_lowercase().ends_with("sort.exe"),
        "{args:?}"
    );
    assert_eq!(args[1..], ["/r"]);
    let Field::Known(cwd) = &info.cwd else {
        panic!(
            "the child's working folder should be readable: {:?}",
            info.cwd
        );
    };
    assert!(
        same_folder(cwd, folder.path()),
        "{cwd:?} is not {:?}",
        folder.path()
    );
}

#[test]
fn reads_a_32_bit_childs_arguments_and_working_folder_too() {
    let system = std::env::var_os("SystemRoot").expect("Windows says where it lives");
    let sort32 = Path::new(&system).join("SysWOW64").join("sort.exe");
    let folder = tempfile::tempdir().unwrap();
    let (_waiting, key) = started_in(&sort32, folder.path());

    let info = details(key);

    assert_eq!(
        info.args.known().map(|args| &args[1..]),
        Some(&["/r".to_string()][..]),
        "{info:?}"
    );
    assert!(
        info.cwd
            .known()
            .is_some_and(|cwd| same_folder(cwd, folder.path())),
        "{info:?}"
    );
}

#[test]
fn reads_the_working_folder_a_process_moved_to() {
    use std::io::{BufRead, BufReader};

    let folder = tempfile::tempdir().unwrap();
    let moved = folder.path().join("moved");
    std::fs::create_dir(&moved).unwrap();
    let script = format!(
        "[Environment]::CurrentDirectory = '{}'; [Console]::Out.WriteLine('moved'); [Console]::Out.Flush(); [void][Console]::In.ReadToEnd()",
        moved.display()
    );
    let child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .current_dir(folder.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("PowerShell should start");
    let mut waiting = Waiting(child);
    let mut line = String::new();
    BufReader::new(waiting.0.stdout.take().unwrap())
        .read_line(&mut line)
        .expect("PowerShell should say it moved");
    assert_eq!(line.trim(), "moved");
    let key = find(waiting.0.id())
        .expect("PowerShell should be listed")
        .key;

    let info = details(key);

    assert!(
        info.cwd.known().is_some_and(|cwd| same_folder(cwd, &moved)),
        "{info:?}"
    );
    assert!(
        info.args
            .known()
            .is_some_and(|args| args.iter().any(|arg| arg == "-NoProfile")),
        "{info:?}"
    );
}

#[test]
fn reads_the_executable_alone() {
    let Field::Known(path) = executable(own().key) else {
        panic!("our own executable should be readable");
    };
    let expected = std::env::current_exe().unwrap();
    assert_eq!(path.file_name(), expected.file_name());
}

#[test]
fn tells_a_running_process_from_one_that_exited_while_still_held() {
    let (mut waiting, key) = wait();
    assert_eq!(presence(key), Presence::Running);

    drop(waiting.0.stdin.take());
    waiting.0.wait().unwrap();

    // Our handle keeps the exited process openable, under the same start.
    assert_eq!(presence(key), Presence::Gone);
}

#[test]
fn says_gone_for_a_process_once_nothing_holds_it() {
    let (waiting, key) = wait();
    drop(waiting);

    let deadline = Instant::now() + PATIENCE;
    while executable(key) != Field::Gone {
        assert!(Instant::now() < deadline, "still {:?}", executable(key));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(details(key).exe, Field::Gone);
}

#[test]
fn reads_nothing_through_a_key_whose_start_isnt_its_pids_now() {
    let mut stale = own().key;
    stale.start = StartTime(stale.start.0 - 1);

    assert_eq!(details(stale).exe, Field::Gone);
    assert_eq!(executable(stale), Field::Gone);
    assert_eq!(presence(stale), Presence::Replaced);
}

/// A job this test holds open.
struct Job(windows_sys::Win32::Foundation::HANDLE);

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is open and this value alone owns it.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// A job holding `waiting`; if `ending`, it ends its processes once its
/// last handle closes, as the job Node starts programs in does.
fn job_for(waiting: &Waiting, ending: bool) -> Job {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    // SAFETY: no attributes and no name are asked for.
    let job = Job(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
    assert!(!job.0.is_null());
    if ending {
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the limits are as large as the call is told.
        let set = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of_val(&limits) as u32,
            )
        };
        assert_ne!(set, 0);
    }
    // SAFETY: both handles are open; the child's has every right.
    let assigned = unsafe { AssignProcessToJobObject(job.0, waiting.0.as_raw_handle()) };
    assert_ne!(assigned, 0);
    job
}

#[test]
fn finds_what_a_job_this_process_holds_would_end_along_with_it() {
    use harbormaster_platform::bound_to;

    let (bound, bound_key) = wait();
    let (kept, kept_key) = wait();
    let _ending = job_for(&bound, true);
    let _keeping = job_for(&kept, false);

    let found = bound_to(own().key).expect("our own jobs should be readable");

    assert!(found.contains(&bound_key.pid.0), "{found:?}");
    assert!(!found.contains(&kept_key.pid.0), "{found:?}");
}

#[test]
fn finds_no_job_held_by_a_process_that_exited() {
    use harbormaster_platform::bound_to;

    let (mut waiting, key) = wait();
    drop(waiting.0.stdin.take());
    waiting.0.wait().unwrap();

    // Still held by us, so its pid names it, but it holds nothing now.
    assert_eq!(bound_to(key).unwrap(), Vec::<u32>::new());
}

#[test]
fn ends_a_held_child_and_sees_it_gone() {
    use harbormaster_platform::{Ended, hold};

    let (_waiting, key) = wait();
    let held = hold(key).expect("our child should be held");

    held.terminate().expect("ending our own child should work");

    assert_eq!(held.wait(PATIENCE), Ended::Exited);
    assert_eq!(presence(key), Presence::Gone);
    // Ending it again finds it ended, not refused.
    held.terminate().expect("an exited process counts as ended");
}

#[test]
fn holds_no_process_that_isnt_the_one_named() {
    use harbormaster_platform::hold;

    let (mut waiting, key) = wait();
    let mut stale = key;
    stale.start = StartTime(stale.start.0 - 1);
    assert!(matches!(hold(stale), Err(Presence::Replaced)));

    drop(waiting.0.stdin.take());
    waiting.0.wait().unwrap();

    // Exited but still held by us: nothing left to end.
    assert!(matches!(hold(key), Err(Presence::Gone)));
}

#[test]
fn sends_nothing_to_a_process_when_asked_to_stop_it() {
    let (_waiting, key) = wait();

    assert!(matches!(
        signal(key, Signal::Terminate),
        SignalOutcome::Failed(_)
    ));
    assert_eq!(presence(key), Presence::Running);
}

#[test]
fn refuses_what_it_cannot_do_yet_rather_than_answer_nothing() {
    assert!(
        whereabouts().is_err(),
        "an empty answer would say nothing uses a folder"
    );
    let folder = tempfile::tempdir().unwrap();
    assert!(trash(folder.path()).is_err());
    assert!(folder.path().exists());
}

#[test]
fn runs_as_an_ordinary_account() {
    assert!(!runs_as_root());
}

fn find_own(port: u16) -> Option<harbormaster_platform::Listener> {
    let own = std::process::id();
    listeners()
        .expect("listing sockets should succeed")
        .into_iter()
        .find(|listener| listener.owner.pid.0 == own && listener.port == port)
}

#[test]
fn reports_sockets_this_process_listens_on_with_their_owner() {
    let _turn = take_turn();
    let v4 = TcpListener::bind("127.0.0.1:0").unwrap();
    let v6 = TcpListener::bind("[::1]:0").unwrap();

    let found = find_own(v4.local_addr().unwrap().port()).expect("our IPv4 listener");
    assert_eq!(found.address, "127.0.0.1".parse::<IpAddr>().unwrap());
    assert_eq!(found.owner, own().key);
    assert_eq!(found.socket, None, "Windows reports no socket identity");
    let found = find_own(v6.local_addr().unwrap().port()).expect("our IPv6 listener");
    assert_eq!(found.address, "::1".parse::<IpAddr>().unwrap());
}

#[test]
fn does_not_report_a_socket_after_it_closes() {
    let _turn = take_turn();
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    assert!(find_own(port).is_some());

    drop(socket);

    assert!(find_own(port).is_none());
}

/// A PowerShell of our own that listens on a free loopback port, says
/// which, and waits until its input closes.
const LISTEN: &str = "$l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0); \
$l.Start(); [Console]::Out.WriteLine($l.LocalEndpoint.Port); [Console]::Out.Flush(); \
[void][Console]::In.ReadToEnd()";

#[test]
fn tells_whose_socket_a_listener_is() {
    use std::io::{BufRead, BufReader};

    let _turn = take_turn();
    let child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", LISTEN])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("PowerShell should start");
    let mut waiting = Waiting(child);
    let mut line = String::new();
    BufReader::new(waiting.0.stdout.take().unwrap())
        .read_line(&mut line)
        .expect("PowerShell should say its port");
    let port: u16 = line
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("it said {line:?}"));
    let key = find(waiting.0.id())
        .expect("PowerShell should be listed")
        .key;

    let found: Vec<_> = listeners()
        .unwrap()
        .into_iter()
        .filter(|listener| listener.port == port)
        .collect();

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].owner, key);
    assert_ne!(found[0].owner, own().key);
}

/// Set, to how the window answers, for the copy of this test program that
/// shows one: `closes` when asked to, `stays` open, or is a `tool` palette.
const SHOWS: &str = "HARBORMASTER_TEST_WINDOW";

/// A copy of this test program showing a window, ended whatever happens to the test.
struct Showing {
    _process: Waiting,
    key: ProcessKey,
}

fn shows(how: &str) -> Showing {
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc;

    let program = std::env::current_exe().expect("this test program should know its file");
    let mut child = Command::new(program)
        .args([
            "--exact",
            "shows_a_window_until_it_closes",
            "--ignored",
            "--nocapture",
        ])
        .env(SHOWS, how)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .expect("this test program should start again");
    let output = child.stdout.take().expect("its output is piped");
    let process = Waiting(child);
    let (shown, seen) = mpsc::channel();
    // Read to the end, so the copy never writes into a closed pipe.
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if line == "shown" {
                let _ = shown.send(());
            }
        }
    });
    seen.recv_timeout(PATIENCE)
        .expect("the copy should show its window");
    let key = find(process.0.id()).expect("the copy should be listed").key;
    Showing {
        _process: process,
        key,
    }
}

#[test]
#[ignore = "run as a program of its own by the tests that need a window"]
fn shows_a_window_until_it_closes() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, IsWindow, MSG, WM_CLOSE, WS_EX_TOOLWINDOW,
        WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    let Ok(how) = std::env::var(SHOWS) else {
        return;
    };
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let extended = if how == "tool" { WS_EX_TOOLWINDOW } else { 0 };
    // SAFETY: the class is one Windows registers, named with a closing nul.
    let window = unsafe {
        CreateWindowExW(
            extended,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            0,
            0,
            320,
            200,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!window.is_null(), "{}", std::io::Error::last_os_error());
    println!("shown");
    let mut message = MSG::default();
    // SAFETY: the window is this thread's, and `message` ours to fill.
    while unsafe { IsWindow(window) } != 0
        && unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0
    {
        // A window left to itself closes on WM_CLOSE.
        if how == "stays" && message.message == WM_CLOSE {
            continue;
        }
        // SAFETY: the message came from this thread's queue.
        unsafe { DispatchMessageW(&message) };
    }
}

#[test]
fn tells_a_program_showing_a_window_from_one_that_shows_none() {
    use harbormaster_platform::has_windows;

    let showing = shows("closes");
    let palette = shows("tool");
    let (_waiting, quiet) = wait();

    assert!(has_windows(showing.key.pid.0));
    assert!(
        !has_windows(palette.key.pid.0),
        "a tool palette has no taskbar button"
    );
    assert!(!has_windows(quiet.pid.0));
}

#[test]
fn asks_a_held_program_to_close_its_window_and_sees_it_go() {
    use harbormaster_platform::{Ended, hold};

    let showing = shows("closes");
    let held = hold(showing.key).expect("our copy should be held");

    held.close()
        .expect("asking our own copy to close should work");

    assert_eq!(held.wait(PATIENCE), Ended::Exited);
}

#[test]
fn leaves_a_program_that_wont_close_running_until_it_is_ended() {
    use harbormaster_platform::{Ended, has_windows, hold};

    let showing = shows("stays");
    let held = hold(showing.key).expect("our copy should be held");

    held.close().expect("the request should go out");

    // Asked is not closed.
    assert_eq!(held.wait(Duration::from_secs(1)), Ended::StillRunning);
    assert!(has_windows(showing.key.pid.0));
    held.terminate().expect("ending our own copy should work");
    assert_eq!(held.wait(PATIENCE), Ended::Exited);
}

#[test]
fn has_no_window_to_close_for_a_program_that_shows_none() {
    use harbormaster_platform::{Ended, hold};

    let (_waiting, key) = wait();
    let held = hold(key).expect("our child should be held");

    assert!(held.close().is_err());
    assert_eq!(held.wait(Duration::ZERO), Ended::StillRunning);
}
