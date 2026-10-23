//! Where the machine's memory goes, app by app.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use harbormaster_platform::{Owner, ProcessEntry, ProcessKey};
use serde::Serialize;

use crate::role::normalize;
use crate::tree::ProcessTree;

/// One app's share of memory: every process its bundle runs, or one plain program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    /// Which app this is, whose processes it counts, for the panel to name
    /// it by when it asks to quit it.
    pub id: String,
    pub name: String,
    pub memory_bytes: u64,
    /// Part of the OS all through, which the panel never offers to quit.
    pub system: bool,
    /// What quitting it would reach.
    #[serde(skip)]
    pub members: Vec<ProcessKey>,
}

/// Windows' shared runtime for web pages inside apps: its processes serve
/// the app that started them, and count as that app's.
const WEB_VIEW: &str = "msedgewebview2";

/// The longest description that names a program in the panel's rows.
const LONGEST_NAME: usize = 64;

/// The memory of `processes` by app, largest first, leaving out `skip`: the
/// dev servers, which the panel counts on their own. A process whose memory
/// can't be read still belongs to its app, so quitting the app sees all it
/// would reach; an app with nothing measured is left out. A program outside
/// a bundle goes by what `described` says its file is, where that can stand
/// in for a name.
pub fn usage(
    processes: &[ProcessEntry],
    executable: &dyn Fn(ProcessKey) -> Option<PathBuf>,
    described: &dyn Fn(&Path) -> Option<String>,
    skip: &HashSet<ProcessKey>,
) -> Vec<AppUsage> {
    let tree = ProcessTree::new(processes);
    let mut by_app: HashMap<String, AppUsage> = HashMap::new();
    for entry in processes {
        if skip.contains(&entry.key) {
            continue;
        }
        let (id, name, system) = describe(served(entry, &tree, skip), executable, described);
        let app = by_app.entry(id.clone()).or_insert(AppUsage {
            id,
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
            .then_with(|| a.id.cmp(&b.id))
    });
    apps
}

/// The app `entry` counts as: the one a web view serves, or its own. A web
/// view whose app is gone, or is left out, counts as a program of its own.
fn served<'a>(
    entry: &'a ProcessEntry,
    tree: &ProcessTree<'a>,
    skip: &HashSet<ProcessKey>,
) -> &'a ProcessEntry {
    let mut current = entry;
    let mut seen = HashSet::new();
    while normalize(&current.name) == WEB_VIEW && seen.insert(current.key.pid) {
        match tree.parent(current.key.pid) {
            Some(parent) => current = parent,
            None => return entry,
        }
    }
    if skip.contains(&current.key) {
        entry
    } else {
        current
    }
}

/// The app's id, its name, and whether it is part of the OS. Programs of
/// one name kept in different places are different programs, and what
/// another user or the system runs is apart from the user's own.
fn describe(
    entry: &ProcessEntry,
    executable: &dyn Fn(ProcessKey) -> Option<PathBuf>,
    described: &dyn Fn(&Path) -> Option<String>,
) -> (String, String, bool) {
    let path = executable(entry.key);
    let (what, name) = match path.as_deref() {
        Some(path) => match app_name(path) {
            Some(name) => (format!("app:{name}"), name),
            None => (
                format!("path:{}", folded(path)),
                described(path)
                    .and_then(|description| presentable(&description))
                    .unwrap_or_else(|| entry.name.clone()),
            ),
        },
        None => (format!("name:{}", entry.name), entry.name.clone()),
    };
    let whose = match entry.owner {
        Owner::CurrentUser => "user",
        Owner::OtherSession => "session",
        Owner::OtherUser => "other",
        Owner::System => "system",
        Owner::Unknown => "unknown",
    };
    let system =
        crate::service::is_system(entry) || path.as_deref().is_some_and(crate::is_os_component);
    (format!("{whose}/{what}"), name, system)
}

/// Unicode's default-ignorable code points (DerivedCoreProperties.txt):
/// drawn as nothing, and some reorder or hide what comes after them.
const IGNORABLE: [(char, char); 17] = [
    ('\u{00AD}', '\u{00AD}'),
    ('\u{034F}', '\u{034F}'),
    ('\u{061C}', '\u{061C}'),
    ('\u{115F}', '\u{1160}'),
    ('\u{17B4}', '\u{17B5}'),
    ('\u{180B}', '\u{180F}'),
    ('\u{200B}', '\u{200F}'),
    ('\u{202A}', '\u{202E}'),
    ('\u{2060}', '\u{206F}'),
    ('\u{3164}', '\u{3164}'),
    ('\u{FE00}', '\u{FE0F}'),
    ('\u{FEFF}', '\u{FEFF}'),
    ('\u{FFA0}', '\u{FFA0}'),
    ('\u{FFF0}', '\u{FFF8}'),
    ('\u{1BCA0}', '\u{1BCA3}'),
    ('\u{1D173}', '\u{1D17A}'),
    ('\u{E0000}', '\u{E0FFF}'),
];

/// A program's description, where it can stand in for its name: some text
/// short enough for a row, with nothing in it that hides or reorders what
/// is shown. Any program can say anything of itself, so it only ever names.
fn presentable(description: &str) -> Option<String> {
    let description = description.trim();
    let hidden = |c: char| {
        c.is_control()
            || IGNORABLE
                .iter()
                .any(|&(first, last)| (first..=last).contains(&c))
    };
    (!description.is_empty()
        && description.chars().count() <= LONGEST_NAME
        && !description.chars().any(hidden))
    .then(|| description.to_string())
}

/// A Windows path, whose case doesn't tell files apart, in lower case;
/// any other as it is.
fn folded(path: &Path) -> String {
    let path = path.to_string_lossy();
    if path.as_bytes().get(1) == Some(&b':') || path.starts_with(r"\\") {
        path.to_lowercase()
    } else {
        path.into_owned()
    }
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
    use std::path::{Path, PathBuf};

    use harbormaster_platform::{Owner, Pid, ProcessEntry, ProcessKey};

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
            usage(&processes, &executable, &|_| None, &HashSet::new()),
            [
                AppUsage {
                    id: "user/app:Google Chrome".into(),
                    name: "Google Chrome".into(),
                    memory_bytes: 500 * MIB,
                    system: false,
                    members: vec![processes[0].key, processes[1].key],
                },
                AppUsage {
                    id: "user/path:/opt/homebrew/bin/node".into(),
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
            usage(&processes, &executable, &|_| None, &HashSet::new()),
            [AppUsage {
                id: "user/app:claude".into(),
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

        let apps = usage(&processes, &executable, &|_| None, &HashSet::new());

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

        let apps = usage(&processes, &executable, &|_| None, &HashSet::new());

        assert_eq!(apps.len(), 1, "{apps:?}");
        assert_eq!(apps[0].memory_bytes, 300 * MIB);
        assert_eq!(apps[0].members, [processes[0].key, processes[1].key]);
    }

    #[test]
    fn tells_apart_programs_of_one_name_kept_in_different_places() {
        let processes = [
            using(80, "node.exe", Some(100)),
            using(81, "node.exe", Some(200)),
            using(82, "node.exe", Some(300)),
        ];
        let paths = HashMap::from([
            (80, r"C:\Program Files\nodejs\node.exe"),
            (81, r"C:\Users\dev\AppData\Roaming\nvm\v22.1.0\node.exe"),
            (82, r"c:\program files\NODEJS\NODE.EXE"),
        ]);
        let executable = |key: ProcessKey| paths.get(&key.pid.0).map(PathBuf::from);

        let apps = usage(&processes, &executable, &|_| None, &HashSet::new());

        let groups: Vec<(&str, &str, Vec<ProcessKey>)> = apps
            .iter()
            .map(|app| (app.id.as_str(), app.name.as_str(), app.members.clone()))
            .collect();
        assert_eq!(
            groups,
            [
                (
                    r"user/path:c:\program files\nodejs\node.exe",
                    "node.exe",
                    vec![processes[0].key, processes[2].key]
                ),
                (
                    r"user/path:c:\users\dev\appdata\roaming\nvm\v22.1.0\node.exe",
                    "node.exe",
                    vec![processes[1].key]
                ),
            ]
        );
    }

    #[test]
    fn keeps_the_case_of_a_path_where_it_may_tell_files_apart() {
        let processes = [using(83, "tool", Some(10)), using(84, "tool", Some(20))];
        let paths = HashMap::from([(83, "/Users/dev/bin/tool"), (84, "/Users/dev/bin/TOOL")]);
        let executable = |key: ProcessKey| paths.get(&key.pid.0).map(PathBuf::from);

        assert_eq!(
            usage(&processes, &executable, &|_| None, &HashSet::new()).len(),
            2
        );
    }

    #[test]
    fn keeps_apart_what_others_run_from_the_same_program() {
        let processes = [
            using(85, "node.exe", Some(10)),
            ProcessEntry {
                owner: Owner::OtherUser,
                ..using(86, "node.exe", Some(20))
            },
            ProcessEntry {
                owner: Owner::OtherSession,
                ..using(87, "node.exe", Some(30))
            },
        ];
        let executable = |_| Some(PathBuf::from(r"C:\Program Files\nodejs\node.exe"));

        let apps = usage(&processes, &executable, &|_| None, &HashSet::new());

        let ids: Vec<&str> = apps.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                r"session/path:c:\program files\nodejs\node.exe",
                r"other/path:c:\program files\nodejs\node.exe",
                r"user/path:c:\program files\nodejs\node.exe"
            ]
        );
    }

    #[test]
    fn counts_a_web_view_as_the_app_it_serves() {
        let web_view = |pid, parent| ProcessEntry {
            parent: Some(Pid(parent)),
            ..using(pid, "msedgewebview2.exe", Some(100))
        };
        let processes = [
            using(100, "ms-teams.exe", Some(300)),
            web_view(101, 100),
            web_view(102, 101),
            using(200, "SearchHost.exe", Some(50)),
            web_view(201, 200),
            // Its app is gone.
            web_view(301, 300),
        ];
        let paths = HashMap::from([
            (100, r"C:\Program Files\WindowsApps\MSTeams\ms-teams.exe"),
            (
                200,
                r"C:\Windows\SystemApps\MicrosoftWindows.Client.CBS\SearchHost.exe",
            ),
        ]);
        let executable = |key: ProcessKey| {
            Some(PathBuf::from(paths.get(&key.pid.0).copied().unwrap_or(
                r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\msedgewebview2.exe",
            )))
        };

        let apps = usage(&processes, &executable, &|_| None, &HashSet::new());

        let groups: Vec<(&str, u64, bool, Vec<u32>)> = apps
            .iter()
            .map(|app| {
                let pids = app.members.iter().map(|key| key.pid.0).collect();
                (app.name.as_str(), app.memory_bytes / MIB, app.system, pids)
            })
            .collect();
        assert_eq!(
            groups,
            [
                ("ms-teams.exe", 500, false, vec![100, 101, 102]),
                ("SearchHost.exe", 150, true, vec![200, 201]),
                ("msedgewebview2.exe", 100, false, vec![301]),
            ]
        );
    }

    #[test]
    fn leaves_a_web_view_a_dev_server_started_on_its_own() {
        let server = using(400, "node.exe", Some(200));
        let processes = [
            server.clone(),
            ProcessEntry {
                parent: Some(Pid(400)),
                ..using(401, "msedgewebview2.exe", Some(100))
            },
        ];
        let executable = |key: ProcessKey| {
            Some(PathBuf::from(if key.pid.0 == 400 {
                r"C:\Program Files\nodejs\node.exe"
            } else {
                r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\msedgewebview2.exe"
            }))
        };

        let apps = usage(
            &processes,
            &executable,
            &|_| None,
            &HashSet::from([server.key]),
        );

        let groups: Vec<(&str, Vec<ProcessKey>)> = apps
            .iter()
            .map(|app| (app.name.as_str(), app.members.clone()))
            .collect();
        assert_eq!(groups, [("msedgewebview2.exe", vec![processes[1].key])]);
    }

    #[test]
    fn names_a_program_as_its_file_describes_it() {
        let processes = [
            using(500, "chrome.exe", Some(300)),
            using(501, "chrome.exe", Some(100)),
            using(510, "Google Chrome", Some(50)),
        ];
        let executable = |key: ProcessKey| {
            Some(PathBuf::from(match key.pid.0 {
                510 => "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                _ => r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            }))
        };
        // A bundle names its app, whatever its file says.
        let described = |_: &Path| Some("Chrome Browser".to_string());
        let windows = |path: &Path| (!path.starts_with("/")).then(|| "Google Chrome".to_string());

        let apps = usage(&processes, &executable, &windows, &HashSet::new());
        let named: Vec<(&str, &str)> = apps
            .iter()
            .map(|app| (app.id.as_str(), app.name.as_str()))
            .collect();
        assert_eq!(
            named,
            [
                (
                    r"user/path:c:\program files\google\chrome\application\chrome.exe",
                    "Google Chrome"
                ),
                ("user/app:Google Chrome", "Google Chrome"),
            ]
        );
        let apps = usage(&processes, &executable, &described, &HashSet::new());
        assert_eq!(apps[1].name, "Google Chrome");
    }

    #[test]
    fn keeps_a_programs_own_name_where_its_description_cant_stand_in() {
        for description in [
            "   ",
            "An app that describes itself at a length no row of the panel can hold",
            "Google\u{202E}emorhC",
            "Tab\tbed",
            "Zero\u{200B}width",
            "Arabic\u{061C}mark",
            "Word\u{2060}joiner",
            "Soft\u{00AD}hyphen",
            "Tagged\u{E0041}",
        ] {
            let processes = [using(520, "tool.exe", Some(10))];
            let executable = |_| Some(PathBuf::from(r"C:\Tools\tool.exe"));
            let described = |_: &Path| Some(description.to_string());

            let apps = usage(&processes, &executable, &described, &HashSet::new());

            assert_eq!(apps[0].name, "tool.exe", "{description:?}");
        }
        let processes = [using(521, "tool.exe", Some(10))];
        let executable = |_| Some(PathBuf::from(r"C:\Tools\tool.exe"));
        let described = |_: &Path| Some("  Build Tool  ".to_string());
        assert_eq!(
            usage(&processes, &executable, &described, &HashSet::new())[0].name,
            "Build Tool"
        );
    }

    #[test]
    fn groups_what_it_cannot_locate_by_its_name() {
        let processes = [
            using(90, "Discord.exe", Some(100)),
            using(91, "Discord.exe", Some(50)),
        ];

        let apps = usage(&processes, &|_| None, &|_| None, &HashSet::new());

        assert_eq!(
            apps,
            [AppUsage {
                id: "user/name:Discord.exe".into(),
                name: "Discord.exe".into(),
                memory_bytes: 150 * MIB,
                system: false,
                members: vec![processes[0].key, processes[1].key],
            }]
        );
    }

    #[test]
    fn leaves_out_the_dev_servers_and_what_it_cannot_measure() {
        let server = using(30, "node", Some(400));
        let processes = [server.clone(), using(40, "WindowServer", None)];

        let apps = usage(
            &processes,
            &|_| None,
            &|_| None,
            &HashSet::from([server.key]),
        );

        assert!(apps.is_empty(), "{apps:?}");
    }
}
