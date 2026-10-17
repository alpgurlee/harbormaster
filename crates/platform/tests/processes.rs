use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use harbormaster_platform::{
    Field, Owner, Pid, ProcessEntry, details, executable, processes, sets_variable, tracks_exec,
};

fn find(pid: u32) -> ProcessEntry {
    processes()
        .expect("reading the process table should succeed")
        .into_iter()
        .find(|entry| entry.key.pid == Pid(pid))
        .unwrap_or_else(|| panic!("pid {pid} missing from the process table"))
}

#[test]
fn lists_this_process_with_its_parent() {
    let own = find(std::process::id());

    assert_eq!(own.parent, Some(Pid(std::os::unix::process::parent_id())));
    assert_eq!(own.owner, Owner::CurrentUser);
}

#[test]
fn reports_a_stable_identity_for_the_same_process() {
    let first = find(std::process::id()).key;
    let second = find(std::process::id()).key;

    assert_eq!(first, second);
}

#[test]
fn measures_memory_and_cpu_time_of_own_processes() {
    let own = find(std::process::id());

    assert!(own.memory_bytes.is_some_and(|bytes| bytes > 0));
    assert!(own.cpu_time.is_some());
}

#[test]
fn reads_executable_arguments_and_working_directory_of_a_child() {
    let folder = tempfile::tempdir().unwrap();
    let mut child = Command::new("/bin/sleep")
        .arg("30")
        .current_dir(folder.path())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();

    let key = find(child.id()).key;
    let info = details(key);
    child.kill().unwrap();
    child.wait().unwrap();

    let expected_cwd = folder.path().canonicalize().unwrap();
    assert_eq!(info.cwd, Field::Known(expected_cwd));
    assert_eq!(info.exe, Field::Known("/bin/sleep".into()));
    assert_eq!(
        info.args,
        Field::Known(vec!["/bin/sleep".to_string(), "30".to_string()])
    );
}

#[test]
fn reports_gone_for_a_process_that_has_exited() {
    let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let key = find(child.id()).key;
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!(details(key).cwd, Field::Gone);
}

#[test]
fn reads_empty_arguments_without_reaching_the_environment() {
    use std::os::unix::process::CommandExt;

    let mut child = Command::new("/usr/bin/yes")
        .arg0("")
        .args(["", "x"])
        .env_clear()
        .env("SECRET", "value")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();

    let key = find(child.id()).key;
    let info = details(key);
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!(
        info.args,
        Field::Known(vec![String::new(), String::new(), "x".to_string()])
    );
}

#[test]
fn reads_the_executable_alone() {
    let own = find(std::process::id()).key;

    let Field::Known(path) = executable(own) else {
        panic!("our own executable should be readable");
    };
    let expected = std::env::current_exe().unwrap();
    assert_eq!(path.file_name(), expected.file_name());
}

#[test]
fn says_gone_for_the_executable_of_a_process_that_exited() {
    let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let key = find(child.id()).key;
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!(executable(key), Field::Gone);
}

#[test]
fn gives_a_process_that_execs_a_new_identity() {
    // The shell execs only once told to, after its first identity is read.
    let mut child = Command::new("/bin/sh")
        .args(["-c", "read go; exec /bin/sleep 30"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let before = find(child.id()).key;
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut after = find(child.id()).key;
    while after == before && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        after = find(child.id()).key;
    }
    let stale = details(before);
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!((before.pid, before.start), (after.pid, after.start));
    assert_ne!(before, after, "exec must change the identity");
    assert_eq!(stale.args, Field::Gone);
}

#[test]
fn keys_follow_an_exec_on_this_macos() {
    assert!(tracks_exec());
}

#[test]
fn lists_processes_of_other_users_too() {
    let launchd = find(1);

    assert_eq!(launchd.owner, Owner::System);
    assert_eq!(launchd.name, "launchd");
}

/// A program of our own that only waits: the system keeps its own
/// programs' environments from view, but not others'.
fn waiter(folder: &std::path::Path) -> std::path::PathBuf {
    let source = folder.join("main.c");
    std::fs::write(
        &source,
        "#include <unistd.h>\nint main(void){sleep(30);return 0;}\n",
    )
    .expect("the program's source should be written");
    let program = folder.join("waiter");
    let built = Command::new("cc")
        .arg("-o")
        .arg(&program)
        .arg(&source)
        .status()
        .expect("cc should run");
    assert!(built.success());
    program
}

#[test]
fn tells_whether_a_child_started_with_a_variable_set() {
    let folder = tempfile::tempdir().unwrap();
    let program = waiter(folder.path());
    let mut with = Command::new(&program)
        .env("JDK_JAVA_OPTIONS", "-cp")
        .spawn()
        .unwrap();
    let mut without = Command::new(&program)
        .env_remove("JDK_JAVA_OPTIONS")
        .spawn()
        .unwrap();
    let (set, unset) = (find(with.id()).key, find(without.id()).key);

    let answers = (
        sets_variable(set, "JDK_JAVA_OPTIONS"),
        sets_variable(unset, "JDK_JAVA_OPTIONS"),
    );
    for child in [&mut with, &mut without] {
        child.kill().unwrap();
        child.wait().unwrap();
    }

    assert_eq!(answers, (Field::Known(true), Field::Known(false)));
    assert_eq!(sets_variable(set, "JDK_JAVA_OPTIONS"), Field::Gone);
}
