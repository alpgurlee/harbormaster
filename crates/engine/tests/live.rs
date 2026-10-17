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

#[test]
fn shows_processes_as_a_stop_would_and_leaves_out_the_exited() {
    let own = std::process::id();
    let mut exited = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = exited.id();
    let table = harbormaster_platform::processes().unwrap();
    let key_of = |pid: u32| {
        table
            .iter()
            .find(|entry| entry.key.pid.0 == pid)
            .unwrap()
            .key
    };
    let keys = [key_of(own), key_of(pid)];
    exited.kill().unwrap();
    exited.wait().unwrap();

    let members = live::members(&keys).unwrap();

    assert_eq!(members.len(), 1, "{members:?}");
    assert_eq!(members[0].pid, own);
    assert!(members[0].command.is_some());
}

#[test]
fn holds_this_programs_own_file_while_it_runs() {
    let own =
        std::fs::canonicalize(std::env::current_exe().expect("this program")).expect("its path");

    let held = harbormaster_engine::live::held().expect("what is held should be read");

    assert!(
        held.iter()
            .any(|(path, who)| *path == own && who == "Harbormaster"),
        "{own:?} in {held:?}"
    );
}

/// Run as a copy of this test program whose file is then taken away.
const MOVED: &str = "HARBORMASTER_TEST_MOVED_PROGRAM";

#[test]
fn says_nothing_can_be_told_free_once_it_cant_tell_where_it_runs_from() {
    if std::env::var_os(MOVED).is_some() {
        std::fs::remove_file(std::env::current_exe().expect("this program")).expect("taken away");
        assert!(harbormaster_engine::live::held().is_err());
        return;
    }
    let place = tempfile::tempdir().expect("a temp folder");
    let copy = place.path().join("moved");
    std::fs::copy(std::env::current_exe().expect("this program"), &copy).expect("a copy");

    let status = std::process::Command::new(&copy)
        .args([
            "--exact",
            "says_nothing_can_be_told_free_once_it_cant_tell_where_it_runs_from",
        ])
        .env(MOVED, "1")
        .status()
        .expect("the copy should run");

    assert!(status.success());
}

/// A program called `java` that only waits, started as a Gradle daemon's JVM
/// would be, in `folder`, with `JDK_JAVA_OPTIONS` set to `options` if any.
fn fake_gradle_daemon(folder: &std::path::Path, options: Option<&str>) -> std::process::Child {
    let source = folder.join("main.c");
    std::fs::write(
        &source,
        "#include <unistd.h>\nint main(void){char c;while(read(0,&c,1)==1)write(1,&c,1);return 0;}\n",
    )
    .expect("the program's source should be written");
    let java = folder.join("java");
    let built = std::process::Command::new("cc")
        .arg("-o")
        .arg(&java)
        .arg(&source)
        .status()
        .expect("cc should run");
    assert!(built.success());
    let mut command = std::process::Command::new(&java);
    match options {
        Some(options) => command.env("JDK_JAVA_OPTIONS", options),
        None => command.env_remove("JDK_JAVA_OPTIONS"),
    };
    command
        .args([
            "-Xmx64m",
            "-cp",
            "gradle-daemon-main-8.14.jar",
            "org.gradle.launcher.daemon.bootstrap.GradleDaemon",
            "8.14",
        ])
        .current_dir(folder)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("the program should start")
}

#[test]
fn knows_a_gradle_daemon_and_the_folder_it_works_in() {
    let folder = tempfile::tempdir().unwrap();
    let real = folder.path().canonicalize().unwrap();
    let mut daemon = fake_gradle_daemon(&real, None);
    let pid = daemon.id();

    let holdings = live::holdings();
    let named = live::held();
    daemon.kill().unwrap();
    daemon.wait().unwrap();

    let holding = holdings
        .unwrap()
        .into_iter()
        .find(|holding| holding.key.pid.0 == pid)
        .expect("the daemon among this user's processes");
    assert_eq!(holding.daemon, Some(live::Daemon::Gradle));
    assert_eq!(holding.name, "Gradle daemon");
    assert!(holding.paths.contains(&real), "{holding:?}");
    assert!(
        named
            .unwrap()
            .contains(&(real.clone(), "Gradle daemon".to_owned()))
    );
}

#[test]
fn knows_no_daemon_whose_launcher_could_have_taken_more_from_its_environment() {
    // JDK_JAVA_OPTIONS=-cp makes the daemon's class a class path, and what follows the main class.
    let folder = tempfile::tempdir().unwrap();
    let real = folder.path().canonicalize().unwrap();
    let mut daemon = fake_gradle_daemon(&real, Some("-cp"));
    let pid = daemon.id();

    let holdings = live::holdings();
    daemon.kill().unwrap();
    daemon.wait().unwrap();

    let holding = holdings
        .unwrap()
        .into_iter()
        .find(|holding| holding.key.pid.0 == pid)
        .expect("the process among this user's");
    assert_eq!(holding.daemon, None);
    assert_eq!(holding.name, "java");
}
