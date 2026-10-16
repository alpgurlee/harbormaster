//! Where the machine's memory goes, app by app.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use harbormaster_platform::{ProcessEntry, ProcessKey};
use serde::Serialize;

/// One app's share of memory: every process its bundle runs, or one plain program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    pub name: String,
    pub memory_bytes: u64,
    /// Part of the OS all through, which the panel never offers to quit.
    pub system: bool,
    /// What quitting it would reach.
    #[serde(skip)]
    pub members: Vec<ProcessKey>,
}

/// The memory of `processes` by app, largest first, leaving out `skip`: the
/// dev servers, which the panel counts on their own. A process whose memory
/// can't be read still belongs to its app, so quitting the app sees all it
/// would reach; an app with nothing measured is left out.
pub fn usage(
    processes: &[ProcessEntry],
    executable: &dyn Fn(ProcessKey) -> Option<PathBuf>,
    skip: &HashSet<ProcessKey>,
) -> Vec<AppUsage> {
    let mut by_app: HashMap<String, AppUsage> = HashMap::new();
    for entry in processes {
        if skip.contains(&entry.key) {
            continue;
        }
        let path = executable(entry.key);
        let name = path
            .as_deref()
            .and_then(app_name)
            .unwrap_or_else(|| entry.name.clone());
        let system =
            crate::service::is_system(entry) || path.as_deref().is_some_and(crate::is_os_component);
        let app = by_app.entry(name.clone()).or_insert(AppUsage {
            name,
            memory_bytes: 0,
            system: true,
            members: Vec::new(),
        });
        app.memory_bytes += entry.memory_bytes.unwrap_or(0);
        app.system &= system;
        app.members.push(entry.key);
    }
    let mut apps: Vec<AppUsage> = by_app
        .into_values()
        .filter(|app| app.memory_bytes > 0)
        .collect();
    apps.sort_by(|a, b| {
        b.memory_bytes
            .cmp(&a.memory_bytes)
            .then_with(|| a.name.cmp(&b.name))
    });
    apps
}

/// `Google Chrome` for anything inside `…/Google Chrome.app/…`: the outermost
/// bundle, so an app's helpers count as the app. A program whose file is named
/// by its version, as `…/claude/versions/2.1.300` is, goes by its folder.
/// `None` leaves the process's own name.
fn app_name(path: &Path) -> Option<String> {
    let bundle = path.components().find_map(|component| {
        component
            .as_os_str()
            .to_str()?
            .strip_suffix(".app")
            .map(str::to_string)
    });
    if bundle.is_some() {
        return bundle;
    }
    if !is_version(path.file_name()?.to_str()?) {
        return None;
    }
    path.ancestors()
        .skip(1)
        .filter_map(|folder| folder.file_name()?.to_str())
        .find(|name| *name != "versions" && !is_version(name))
        .map(str::to_string)
}

/// `2.1.300` or `v22.1.0`: numbers between dots and nothing else.
fn is_version(name: &str) -> bool {
    let numbers = name.strip_prefix('v').unwrap_or(name);
    numbers.contains('.')
        && numbers
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    use harbormaster_platform::{ProcessEntry, ProcessKey};

    use super::{AppUsage, usage};
    use crate::tree::tests::entry;

    const MIB: u64 = 1024 * 1024;

    fn using(pid: u32, name: &str, mib: Option<u64>) -> ProcessEntry {
        ProcessEntry {
            memory_bytes: mib.map(|mib| mib * MIB),
            ..entry(pid, Some(1), u64::from(pid), name)
        }
    }

    #[test]
    fn counts_every_helper_of_an_app_as_the_app() {
        let processes = [
            using(10, "Google Chrome", Some(300)),
            using(11, "Google Chrome Helper (Renderer)", Some(200)),
            using(20, "node", Some(50)),
        ];
        let paths = HashMap::from([
            (
                10,
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ),
            (
                11,
                "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)",
            ),
            (20, "/opt/homebrew/bin/node"),
        ]);
        let executable = |key: ProcessKey| paths.get(&key.pid.0).map(PathBuf::from);

        assert_eq!(
            usage(&processes, &executable, &HashSet::new()),
            [
                AppUsage {
                    name: "Google Chrome".into(),
                    memory_bytes: 500 * MIB,
                    system: false,
                    members: vec![processes[0].key, processes[1].key],
                },
                AppUsage {
                    name: "node".into(),
                    memory_bytes: 50 * MIB,
                    system: false,
                    members: vec![processes[2].key],
                },
            ]
        );
    }

    #[test]
    fn names_a_program_kept_by_version_after_its_folder() {
        let processes = [
            using(50, "2.1.300", Some(1000)),
            using(51, "2.1.301", Some(700)),
        ];
        let executable = |key: ProcessKey| {
            Some(PathBuf::from(format!(
                "/Users/dev/.local/share/claude/versions/2.1.{}",
                key.pid.0 + 250
            )))
        };

        assert_eq!(
            usage(&processes, &executable, &HashSet::new()),
            [AppUsage {
                name: "claude".into(),
                memory_bytes: 1700 * MIB,
                system: false,
                members: vec![processes[0].key, processes[1].key],
            }]
        );
    }

    #[test]
    fn marks_what_belongs_to_the_system() {
        let processes = [
            using(60, "replayd", Some(300)),
            using(61, "Google Chrome", Some(200)),
        ];
        let paths = HashMap::from([
            (60, "/usr/libexec/replayd"),
            (
                61,
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ),
        ]);
        let executable = |key: ProcessKey| paths.get(&key.pid.0).map(PathBuf::from);

        let apps = usage(&processes, &executable, &HashSet::new());

        let system: Vec<(&str, bool)> = apps
            .iter()
            .map(|app| (app.name.as_str(), app.system))
            .collect();
        assert_eq!(system, [("replayd", true), ("Google Chrome", false)]);
    }

    #[test]
    fn keeps_a_process_it_cannot_measure_among_its_apps() {
        let processes = [
            using(70, "Google Chrome", Some(300)),
            using(71, "Google Chrome Helper", None),
        ];
        let paths = HashMap::from([
            (
                70,
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ),
            (
                71,
                "/Applications/Google Chrome.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper",
            ),
        ]);
        let executable = |key: ProcessKey| paths.get(&key.pid.0).map(PathBuf::from);

        let apps = usage(&processes, &executable, &HashSet::new());

        assert_eq!(apps.len(), 1, "{apps:?}");
        assert_eq!(apps[0].memory_bytes, 300 * MIB);
        assert_eq!(apps[0].members, [processes[0].key, processes[1].key]);
    }

    #[test]
    fn leaves_out_the_dev_servers_and_what_it_cannot_measure() {
        let server = using(30, "node", Some(400));
        let processes = [server.clone(), using(40, "WindowServer", None)];

        let apps = usage(&processes, &|_| None, &HashSet::from([server.key]));

        assert!(apps.is_empty(), "{apps:?}");
    }
}
