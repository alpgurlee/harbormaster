//! The engine wired to this machine, so the CLI and the app check the same
//! things before stopping anything.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use harbormaster_platform::{Error, Field, ProcessKey};

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
        std::env::current_exe().and_then(std::fs::canonicalize)?,
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
            let daemon = (process.name == "java")
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
    Ok(crate::apps::usage(&processes, &executable, skip))
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
/// `None` when the chain doesn't get to launchd: a gap could hide an ancestor.
pub fn lineage() -> Result<Option<Vec<u32>>, Error> {
    let parents: HashMap<u32, u32> = harbormaster_platform::processes()?
        .iter()
        .filter_map(|entry| entry.parent.map(|parent| (entry.key.pid.0, parent.0)))
        .collect();
    Ok(lineage_from(std::process::id(), &parents))
}

fn lineage_from(pid: u32, parents: &HashMap<u32, u32>) -> Option<Vec<u32>> {
    let mut lineage = vec![pid];
    let mut current = pid;
    while current > 1 {
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

    use super::{Daemon, daemon, lineage_from};

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
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
    fn traces_up_to_launchd_or_not_at_all() {
        let parents = HashMap::from([(77, 40), (40, 20), (20, 1)]);
        assert_eq!(lineage_from(77, &parents), Some(vec![77, 40, 20, 1]));

        let gap = HashMap::from([(77, 40), (40, 20)]);
        assert_eq!(lineage_from(77, &gap), None);

        let cycle = HashMap::from([(77, 40), (40, 77)]);
        assert_eq!(lineage_from(77, &cycle), None);
    }
}
