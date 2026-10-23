//! The engine wired to this machine, so the CLI and the app check the same
//! things before stopping anything.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use harbormaster_platform::{Error, Field, Owner, ProcessEntry, ProcessKey};

use crate::apps::AppUsage;
use crate::{Input, Member, Scanner, Snapshot, Standing, command, standings};

/// What listens on this machine right now.
pub fn scan(scanner: &mut Scanner, home: &Path) -> Result<Snapshot, Error> {
    let processes = harbormaster_platform::processes()?;
    let listeners = harbormaster_platform::listeners()?;
    let details = harbormaster_platform::details;
    Ok(scanner.scan(Input {
        listeners: &listeners,
        processes: &processes,
        details: &details,
        home,
        now: Instant::now(),
    }))
}

/// Every path this user's processes other than this one hold, such as
/// working folders, programs and open files, each with its process's name,
/// and this one's own program, so nothing it runs from is taken from it.
/// Not knowing where that is, it can't say what is free, so it fails.
pub fn held() -> Result<Vec<(PathBuf, String)>, Error> {
    let program = (
        std::env::current_exe().and_then(|program| harbormaster_platform::canonical(&program))?,
        "Harbormaster".to_owned(),
    );
    Ok(holdings()?
        .into_iter()
        .flat_map(|holding| {
            let name = holding.name;
            holding
                .paths
                .into_iter()
                .map(move |path| (path, name.clone()))
        })
        .chain([program])
        .collect())
}

/// One of this user's processes and the paths it holds.
#[derive(Debug, Clone)]
pub struct Holding {
    pub key: ProcessKey,
    /// What it is called: a JVM by the daemon it runs.
    pub name: String,
    pub daemon: Option<Daemon>,
    pub paths: Vec<PathBuf>,
}

/// This user's processes other than this one, each with the paths it holds.
pub fn holdings() -> Result<Vec<Holding>, Error> {
    let own = std::process::id();
    Ok(harbormaster_platform::whereabouts()?
        .into_iter()
        .filter(|process| process.key.pid.0 != own)
        .map(|process| {
            // Only a JVM's arguments are read: `java` alone says little.
            let daemon = process
                .name
                .eq_ignore_ascii_case("java")
                .then(|| jvm_daemon(process.key))
                .flatten();
            Holding {
                key: process.key,
                name: daemon.map_or(process.name, |daemon| daemon.name().to_owned()),
                daemon,
                paths: process.paths,
            }
        })
        .collect())
}

/// A dev daemon a JVM of this user's may run, which the cleaner may stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daemon {
    Gradle,
    Kotlin,
}

impl Daemon {
    pub fn name(self) -> &'static str {
        match self {
            Daemon::Gradle => "Gradle daemon",
            Daemon::Kotlin => "Kotlin daemon",
        }
    }
}

/// The daemons a JVM may run, by the class it starts from.
const DAEMONS: [(&str, Daemon); 2] = [
    (
        "org.gradle.launcher.daemon.bootstrap.GradleDaemon",
        Daemon::Gradle,
    ),
    (
        "org.jetbrains.kotlin.daemon.KotlinCompileDaemon",
        Daemon::Kotlin,
    ),
];

/// Options of the `java` launcher that take the next argument as their value.
const WITH_VALUE: [&str; 13] = [
    "-cp",
    "-classpath",
    "--class-path",
    "-p",
    "--module-path",
    "--upgrade-module-path",
    "--add-modules",
    "--enable-native-access",
    "--limit-modules",
    "--add-reads",
    "--add-exports",
    "--add-opens",
    "--patch-module",
];

/// Options of the `java` launcher that stand alone, by what they start with.
const ALONE: [&str; 20] = [
    "-D",
    "-X",
    "-javaagent:",
    "-agentlib:",
    "-agentpath:",
    "-splash:",
    "-verbose",
    "-ea",
    "-da",
    "-esa",
    "-dsa",
    "-enableassertions",
    "-disableassertions",
    "-enablesystemassertions",
    "-disablesystemassertions",
    "-server",
    "-client",
    "-showversion",
    "--show-version",
    "--enable-preview",
];

/// The daemon the JVM `key` names runs, if it is one the cleaner knows. Its
/// launcher puts what `JDK_JAVA_OPTIONS` holds before its arguments, so only
/// a JVM known to have started without it can be told by them.
fn jvm_daemon(key: ProcessKey) -> Option<Daemon> {
    if harbormaster_platform::sets_variable(key, "JDK_JAVA_OPTIONS") != Field::Known(false) {
        return None;
    }
    daemon(harbormaster_platform::details(key).args.known()?)
}

/// The daemon a JVM started with `args` runs, if it is one the cleaner knows.
fn daemon(args: &[String]) -> Option<Daemon> {
    let class = started_from(args)?;
    DAEMONS
        .iter()
        .find(|(known, _)| *known == class)
        .map(|(_, daemon)| *daemon)
}

/// What a JVM started from, read from its arguments as the `java` launcher
/// reads them: the first that is neither an option nor an option's value,
/// a class, or else a source or argument file, which is never a daemon's.
/// None after a jar, a module, an option it doesn't know, or an option whose
/// value is an argument file, whose contents the launcher puts in its place:
/// any of them could hide what really runs.
fn started_from(args: &[String]) -> Option<&str> {
    let mut rest = args.iter().skip(1).map(String::as_str);
    while let Some(arg) = rest.next() {
        let name = arg.split_once('=').map_or(arg, |(name, _)| name);
        if WITH_VALUE.contains(&arg) {
            rest.next().filter(|value| !value.starts_with('@'))?;
        } else if arg.starts_with("--") && WITH_VALUE.contains(&name) {
            // `--add-opens=java.base/java.util=ALL-UNNAMED`: the value comes with it.
        } else if arg.starts_with('-') {
            ALONE
                .iter()
                .any(|alone| arg.starts_with(alone))
                .then_some(())?;
        } else {
            return Some(arg);
        }
    }
    None
}

/// Where each of `keys` stands right now, read just before a round of signals.
pub fn guards(home: &Path, keys: &[ProcessKey]) -> Result<HashMap<ProcessKey, Standing>, Error> {
    let processes = harbormaster_platform::processes()?;
    let details = harbormaster_platform::details;
    let input = Input {
        listeners: &[],
        processes: &processes,
        details: &details,
        home,
        now: Instant::now(),
    };
    Ok(standings(input, keys, &harbormaster_platform::presence)
        .into_iter()
        .collect())
}

/// This machine's memory by app, largest first, leaving out `skip`.
pub fn apps(skip: &HashSet<ProcessKey>) -> Result<Vec<AppUsage>, Error> {
    let processes = harbormaster_platform::processes()?;
    let executable = |key| harbormaster_platform::executable(key).known().cloned();
    Ok(crate::apps::usage(
        &processes,
        &executable,
        &harbormaster_platform::description,
        skip,
    ))
}

/// `keys` as a stop shows them, leaving out what exited or changed since.
pub fn members(keys: &[ProcessKey]) -> Result<Vec<Member>, Error> {
    let processes = harbormaster_platform::processes()?;
    Ok(keys
        .iter()
        .filter_map(|key| processes.iter().find(|entry| entry.key == *key))
        .map(|entry| Member {
            key: entry.key,
            pid: entry.key.pid.0,
            name: entry.name.clone(),
            command: harbormaster_platform::details(entry.key)
                .args
                .known()
                .map(|args| command::display(args)),
            memory_bytes: entry.memory_bytes,
            cpu_percent: None,
        })
        .collect())
}

/// This process and everything above it, which a stop must never reach.
/// `None` when the chain doesn't get to a root the system started: a gap
/// could hide an ancestor. On a Mac the root is launchd; on Windows, the
/// user's own Explorer or the services that start sessions, each verified
/// to be Windows' own program.
pub fn lineage() -> Result<Option<Vec<u32>>, Error> {
    let processes = harbormaster_platform::processes()?;
    Ok(lineage_in(&processes))
}

/// This process, everything above it, and everything it started, such as
/// the web view that draws its windows: what quitting an app must never
/// reach. `None` as for [`lineage`].
pub fn ours() -> Result<Option<Vec<u32>>, Error> {
    let processes = harbormaster_platform::processes()?;
    Ok(lineage_in(&processes).map(|mut ours| {
        ours.extend(below(std::process::id(), &processes));
        ours
    }))
}

/// What `pid` started, and what those started, as `processes` show them.
fn below(pid: u32, processes: &[ProcessEntry]) -> Vec<u32> {
    crate::tree::ProcessTree::new(processes)
        .subtree(harbormaster_platform::Pid(pid))
        .into_iter()
        .map(|entry| entry.key.pid.0)
        .filter(|&found| found != pid)
        .collect()
}

fn lineage_in(processes: &[ProcessEntry]) -> Option<Vec<u32>> {
    let parents: HashMap<u32, u32> = processes
        .iter()
        .filter_map(|entry| entry.parent.map(|parent| (entry.key.pid.0, parent.0)))
        .collect();
    let windows = harbormaster_platform::windows_folder();
    let shell = harbormaster_platform::shell_process();
    let roots = roots(processes, windows.as_deref(), shell, &|key| {
        harbormaster_platform::executable(key).known().cloned()
    });
    lineage_from(std::process::id(), &parents, &roots)
}

/// The processes a chain may end at. Without a Windows folder it is
/// launchd alone; with one, the Explorer behind the desktop's shell window
/// and the session services, only where Windows' own program runs them.
/// Any other Explorer may be one a user started to put processes under.
fn roots(
    processes: &[ProcessEntry],
    windows: Option<&Path>,
    shell: Option<u32>,
    exe_of: &dyn Fn(ProcessKey) -> Option<PathBuf>,
) -> HashSet<u32> {
    processes
        .iter()
        .filter(|entry| match windows {
            None => entry.key.pid.0 == 1,
            Some(windows) => {
                let name = crate::role::normalize(&entry.name);
                // Each lives in one place, where only Windows writes.
                let (trusted, home) = match name.as_str() {
                    "explorer" => (
                        entry.owner == Owner::CurrentUser && shell == Some(entry.key.pid.0),
                        windows.to_path_buf(),
                    ),
                    "services" | "wininit" => {
                        (entry.owner == Owner::System, windows.join("System32"))
                    }
                    _ => (false, PathBuf::new()),
                };
                trusted
                    && exe_of(entry.key).is_some_and(|exe| {
                        inside(&exe, &home)
                            && crate::role::normalize(&exe.to_string_lossy()) == name
                    })
            }
        })
        .map(|entry| entry.key.pid.0)
        .collect()
}

/// The pids of what ending the processes `keys` name could end too, read
/// right before a stop on Windows: what they started outside themselves,
/// and what is in a job one of them holds that Windows ends once its last
/// handle closes, as Node's for the programs it starts.
pub fn strays(keys: &[ProcessKey]) -> Result<Vec<u32>, Error> {
    let processes = harbormaster_platform::processes()?;
    let mut bound = HashSet::new();
    for &key in keys {
        bound.extend(harbormaster_platform::bound_to(key)?);
    }
    let windows = harbormaster_platform::windows_folder();
    Ok(strays_among(
        &processes,
        keys,
        &bound,
        windows.as_deref(),
        &|key| harbormaster_platform::executable(key).known().cloned(),
    ))
}

/// Of `processes`, the children of the living `keys` and those whose pid
/// is `bound`, leaving out `keys` and the console hosts Windows starts for
/// a console program, which end once the programs using them have. A
/// bound pid the table doesn't list can't be told apart, so it counts.
fn strays_among(
    processes: &[ProcessEntry],
    keys: &[ProcessKey],
    bound: &HashSet<u32>,
    windows: Option<&Path>,
    exe_of: &dyn Fn(ProcessKey) -> Option<PathBuf>,
) -> Vec<u32> {
    let alive: HashSet<u32> = processes
        .iter()
        .filter(|entry| keys.contains(&entry.key))
        .map(|entry| entry.key.pid.0)
        .collect();
    let console_host = |entry: &ProcessEntry| {
        windows.is_some_and(|windows| {
            crate::role::normalize(&entry.name) == "conhost"
                && exe_of(entry.key).is_some_and(|exe| {
                    inside(&exe, &windows.join("System32"))
                        && crate::role::normalize(&exe.to_string_lossy()) == "conhost"
                })
        })
    };
    let listed: HashSet<u32> = processes.iter().map(|entry| entry.key.pid.0).collect();
    let mut unlisted: Vec<u32> = bound
        .iter()
        .copied()
        .filter(|pid| !listed.contains(pid))
        .collect();
    unlisted.sort_unstable();
    processes
        .iter()
        .filter(|entry| {
            bound.contains(&entry.key.pid.0)
                || entry.parent.is_some_and(|parent| alive.contains(&parent.0))
        })
        .filter(|entry| !keys.contains(&entry.key) && !console_host(entry))
        .map(|entry| entry.key.pid.0)
        .chain(unlisted)
        .collect()
}

/// Whether `path` is right under `folder`, however either is cased or
/// its separators are written.
fn inside(path: &Path, folder: &Path) -> bool {
    let spelled = |text: &str| {
        text.replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    let path = path.to_string_lossy();
    let parent = path
        .rsplit_once(['\\', '/'])
        .map_or("", |(parent, _)| parent);
    spelled(parent) == spelled(&folder.to_string_lossy())
}

fn lineage_from(pid: u32, parents: &HashMap<u32, u32>, roots: &HashSet<u32>) -> Option<Vec<u32>> {
    let mut lineage = vec![pid];
    let mut current = pid;
    while !roots.contains(&current) {
        let parent = *parents.get(&current)?;
        if lineage.contains(&parent) {
            return None;
        }
        lineage.push(parent);
        current = parent;
    }
    Some(lineage)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use harbormaster_platform::{Owner, Pid, ProcessEntry, ProcessKey, StartTime};

    use super::{Daemon, below, daemon, lineage_from, roots, strays_among};

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn finds_what_this_process_started_and_what_that_started() {
        let child = |pid, parent, start| ProcessEntry {
            key: ProcessKey {
                pid: Pid(pid),
                start: StartTime(start),
                exec: 0,
            },
            parent: Some(Pid(parent)),
            ..entry(pid, "msedgewebview2.exe", Owner::CurrentUser)
        };
        let processes = [
            child(10, 1, 10),
            child(11, 10, 11),
            child(12, 11, 12),
            // Started before us, under a pid ours took over later.
            child(13, 10, 5),
            child(14, 1, 14),
        ];

        assert_eq!(below(10, &processes), [11, 12]);
        assert_eq!(below(99, &processes), [0u32; 0]);
    }

    #[test]
    fn knows_a_gradle_or_kotlin_daemon_by_the_class_its_jvm_started_from() {
        let gradle = args(&[
            "/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home/bin/java",
            "--add-opens=java.base/java.util=ALL-UNNAMED",
            "-XX:MaxMetaspaceSize=384m",
            "-XX:+HeapDumpOnOutOfMemoryError",
            "-Xms256m",
            "-Xmx512m",
            "-Dfile.encoding=UTF-8",
            "-Duser.variant",
            "-cp",
            "/Users/dev/.gradle/wrapper/dists/gradle-8.14-bin/x/gradle-8.14/lib/gradle-daemon-main-8.14.jar",
            "-javaagent:/Users/dev/.gradle/wrapper/dists/gradle-8.14-bin/x/gradle-8.14/lib/agents/gradle-instrumentation-agent-8.14.jar",
            "org.gradle.launcher.daemon.bootstrap.GradleDaemon",
            "8.14",
        ]);
        let kotlin = args(&[
            "java",
            "-cp",
            "/Users/dev/.gradle/caches/kotlin-compiler-embeddable-2.1.0.jar",
            "-D$java.rmi.server.hostname=127.0.0.1",
            "-Xmx2048m",
            "-ea",
            "-XX:ReservedCodeCacheSize=320m",
            "org.jetbrains.kotlin.daemon.KotlinCompileDaemon",
            "--daemon-runFilesPath",
            "/Users/dev/Library/Application Support/kotlin/daemon",
        ]);

        assert_eq!(daemon(&gradle), Some(Daemon::Gradle));
        assert_eq!(daemon(&kotlin), Some(Daemon::Kotlin));
        assert_eq!(Daemon::Gradle.name(), "Gradle daemon");
    }

    #[test]
    fn knows_no_daemon_where_its_class_isnt_what_the_jvm_started_from() {
        let gradle = "org.gradle.launcher.daemon.bootstrap.GradleDaemon";
        for (why, line) in [
            (
                "an option's value",
                vec!["java", "-cp", gradle, "com.acme.Main"],
            ),
            (
                "a property",
                vec!["java", &format!("-Dmain={gradle}"), "com.acme.Main"],
            ),
            (
                "an argument of another class",
                vec!["java", "com.acme.Main", gradle],
            ),
            ("after a jar", vec!["java", "-jar", "app.jar", gradle]),
            ("a module", vec!["java", "-m", &format!("acme/{gradle}")]),
            ("an argument file", vec!["java", "@options", gradle]),
            (
                "an argument file as an option's value",
                vec!["java", "-cp", "@options", gradle],
            ),
            ("an unknown option", vec!["java", "--unknown", gradle]),
            ("a source file", vec!["java", "GradleDaemon.java", gradle]),
            ("no class at all", vec!["java", "-Xmx512m"]),
            ("nothing", vec![]),
        ] {
            assert_eq!(daemon(&args(&line)), None, "{why}");
        }
    }

    #[test]
    fn traces_up_to_a_root_or_not_at_all() {
        let roots = HashSet::from([1]);
        let parents = HashMap::from([(77, 40), (40, 20), (20, 1)]);
        assert_eq!(
            lineage_from(77, &parents, &roots),
            Some(vec![77, 40, 20, 1])
        );
        assert_eq!(
            lineage_from(77, &parents, &HashSet::from([20])),
            Some(vec![77, 40, 20])
        );

        let gap = HashMap::from([(77, 40), (40, 20)]);
        assert_eq!(lineage_from(77, &gap, &roots), None);

        let cycle = HashMap::from([(77, 40), (40, 77)]);
        assert_eq!(lineage_from(77, &cycle, &roots), None);
    }

    fn entry(pid: u32, name: &str, owner: Owner) -> ProcessEntry {
        ProcessEntry {
            key: ProcessKey {
                pid: Pid(pid),
                start: StartTime(u64::from(pid)),
                exec: 0,
            },
            parent: None,
            name: name.into(),
            owner,
            memory_bytes: None,
            cpu_time: None,
        }
    }

    #[test]
    fn finds_what_ending_approved_processes_could_end_too_but_windows_console_hosts() {
        let child = |pid, parent, name| ProcessEntry {
            parent: Some(Pid(parent)),
            ..entry(pid, name, Owner::CurrentUser)
        };
        let processes = [
            entry(20, "node.exe", Owner::CurrentUser),
            child(30, 20, "cmd.exe"),
            child(40, 30, "node.exe"),
            // Another server the approved one started.
            child(50, 40, "node.exe"),
            child(60, 30, "conhost.exe"),
            child(70, 30, "conhost.exe"),
            child(80, 99, "node.exe"),
            // In a job of the approved, its parent gone.
            child(90, 98, "node.exe"),
        ];
        let exe_of = |key: ProcessKey| {
            Some(PathBuf::from(match key.pid.0 {
                60 => r"C:\WINDOWS\system32\conhost.exe",
                70 => r"C:\Users\dev\conhost.exe",
                _ => r"C:\Program Files\nodejs\node.exe",
            }))
        };
        let key = |pid: u32| {
            processes
                .iter()
                .find(|entry| entry.key.pid.0 == pid)
                .unwrap()
                .key
        };
        let approved = [key(20), key(30), key(40)];
        let windows = Some(Path::new(r"C:\Windows"));
        // Whatever a job holds, the approved processes in it too, and one
        // the process table couldn't read.
        let bound = HashSet::from([20, 30, 60, 90, 77]);

        assert_eq!(
            strays_among(&processes, &approved, &bound, windows, &exe_of),
            [50, 70, 90, 77]
        );
        // Children of a pid that no longer names the approved process aren't its.
        let replaced = ProcessKey {
            start: StartTime(0),
            ..key(40)
        };
        assert_eq!(
            strays_among(&processes, &[replaced], &HashSet::new(), windows, &exe_of),
            [0u32; 0]
        );
    }

    #[test]
    fn takes_explorer_or_the_session_services_for_a_root_only_where_windows_runs_them() {
        let processes = [
            entry(1, "launchd", Owner::System),
            entry(10, "explorer.exe", Owner::CurrentUser),
            entry(11, "explorer.exe", Owner::OtherUser),
            entry(12, "explorer.exe", Owner::CurrentUser),
            entry(13, "explorer.exe", Owner::CurrentUser),
            entry(20, "services.exe", Owner::System),
            entry(21, "services.exe", Owner::CurrentUser),
            entry(22, "wininit.exe", Owner::System),
            entry(30, "svchost.exe", Owner::System),
        ];
        let exe_of = |key: ProcessKey| {
            Some(PathBuf::from(match key.pid.0 {
                10 | 11 | 13 => r"C:\WINDOWS\explorer.exe",
                12 => r"C:\Users\dev\explorer.exe",
                20 | 21 => r"C:\Windows\System32\services.exe",
                22 => r"C:\Windows\System32\wininit.exe",
                _ => r"C:\Windows\System32\svchost.exe",
            }))
        };
        let windows = Path::new(r"C:\Windows");

        // 13 is the real Explorer too, started by the user: only the one
        // behind the shell window counts.
        assert_eq!(
            roots(&processes, Some(windows), Some(10), &exe_of),
            HashSet::from([10, 20, 22])
        );
        assert_eq!(
            roots(&processes, Some(windows), None, &exe_of),
            HashSet::from([20, 22])
        );
        assert_eq!(
            roots(&processes, None, Some(10), &exe_of),
            HashSet::from([1])
        );
    }
}
