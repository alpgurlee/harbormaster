//! The engine wired to this machine, so the CLI and the app check the same
//! things before stopping anything.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use harbormaster_platform::{Error, ProcessKey};

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
pub fn held() -> Result<Vec<(PathBuf, String)>, Error> {
    let own = std::process::id();
    let program = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()
        .map(|path| (path, "Harbormaster".to_owned()));
    Ok(harbormaster_platform::whereabouts()?
        .into_iter()
        .filter(|process| process.key.pid.0 != own)
        .flat_map(|process| {
            // Only a JVM's arguments are read: `java` alone says little.
            let args = (process.name == "java")
                .then(|| harbormaster_platform::details(process.key).args)
                .and_then(|args| args.known().cloned());
            let name = holder(&process.name, args.as_deref());
            process
                .paths
                .into_iter()
                .map(move |path| (path, name.clone()))
        })
        .chain(program)
        .collect())
}

/// The daemons a JVM may run, by the class it starts from, and what they are called.
const DAEMONS: [(&str, &str); 2] = [
    (
        "org.gradle.launcher.daemon.bootstrap.GradleDaemon",
        "Gradle daemon",
    ),
    (
        "org.jetbrains.kotlin.daemon.KotlinCompileDaemon",
        "Kotlin daemon",
    ),
];

/// What a process holding a folder is called: a JVM by the daemon it runs.
fn holder(name: &str, args: Option<&[String]>) -> String {
    let daemon = || {
        args?.iter().find_map(|arg| {
            DAEMONS
                .iter()
                .find(|(class, _)| arg == class)
                .map(|(_, daemon)| *daemon)
        })
    };
    match name {
        "java" => daemon().unwrap_or(name).to_string(),
        _ => name.to_string(),
    }
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

    use super::{holder, lineage_from};

    #[test]
    fn names_a_java_process_by_the_daemon_it_runs() {
        let args = |args: &[&str]| args.iter().map(ToString::to_string).collect::<Vec<_>>();
        let gradle = args(&[
            "/usr/bin/java",
            "-Xmx512m",
            "org.gradle.launcher.daemon.bootstrap.GradleDaemon",
            "8.14",
        ]);
        let kotlin = args(&["java", "org.jetbrains.kotlin.daemon.KotlinCompileDaemon"]);

        assert_eq!(holder("java", Some(&gradle)), "Gradle daemon");
        assert_eq!(holder("java", Some(&kotlin)), "Kotlin daemon");
        assert_eq!(
            holder("java", Some(&args(&["java", "-jar", "app.jar"]))),
            "java"
        );
        assert_eq!(holder("java", None), "java");
        // Only a JVM is named by what it runs.
        assert_eq!(holder("node", Some(&gradle)), "node");
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
