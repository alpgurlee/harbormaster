use std::process::{Command, Stdio};

use harbormaster_platform::{Field, Owner, Pid, ProcessEntry, details, processes};

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
fn gives_a_process_that_execs_a_new_identity() {
    let mut child = Command::new("/bin/sh")
        .args(["-c", "/bin/sleep 0.3; exec /bin/sleep 30"])
        .spawn()
        .unwrap();
    let before = find(child.id()).key;
    std::thread::sleep(std::time::Duration::from_millis(800));
    let after = find(child.id()).key;
    let stale = details(before);
    child.kill().unwrap();
    child.wait().unwrap();

    assert_eq!((before.pid, before.start), (after.pid, after.start));
    assert_ne!(before, after, "exec must change the identity");
    assert_eq!(stale.args, Field::Gone);
}
