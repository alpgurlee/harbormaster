mod command;
mod project;
mod role;
mod sampler;
mod service;
mod tree;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use harbormaster_platform::{Listener, Owner, ProcessDetails, ProcessEntry, ProcessKey};
use serde::Serialize;

pub use project::{Framework, Project};

use role::{Role, normalize};
use sampler::CpuSampler;
use tree::ProcessTree;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    /// Stable while the root process lives: `<pid>-<start>`.
    pub id: String,
    pub classification: Classification,
    pub protected: bool,
    pub ports: Vec<Port>,
    pub project: Option<Project>,
    pub command: Option<String>,
    /// Seconds since the Unix epoch.
    pub started_at: u64,
    pub memory_bytes: Option<u64>,
    pub cpu_percent: Option<f64>,
    pub processes: Vec<Member>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Classification {
    Dev,
    Other,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Port {
    pub port: u16,
    pub address: IpAddr,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub pid: u32,
    pub name: String,
    pub command: Option<String>,
    pub memory_bytes: Option<u64>,
    pub cpu_percent: Option<f64>,
}

/// Raw facts gathered from the OS for one scan.
#[derive(Clone, Copy)]
pub struct Input<'a> {
    pub listeners: &'a [Listener],
    pub processes: &'a [ProcessEntry],
    pub details: &'a dyn Fn(ProcessKey) -> ProcessDetails,
    pub home: &'a Path,
    pub now: Instant,
}

impl std::fmt::Debug for Input<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Input")
            .field("listeners", &self.listeners.len())
            .field("processes", &self.processes.len())
            .finish_non_exhaustive()
    }
}

/// Keeps what must survive between scans: CPU samples and cached lookups.
#[derive(Debug, Default)]
pub struct Scanner {
    cpu: CpuSampler,
    details: HashMap<ProcessKey, ProcessDetails>,
    projects: HashMap<PathBuf, (Option<Project>, Instant)>,
}

const DATABASES: &[&str] = &[
    "postgres",
    "mysqld",
    "mariadbd",
    "redis-server",
    "mongod",
    "memcached",
];
const PROJECT_CACHE_TTL: Duration = Duration::from_secs(15);

impl Scanner {
    pub fn scan(&mut self, input: Input<'_>) -> Snapshot {
        let alive: HashSet<ProcessKey> =
            input.processes.iter().map(|process| process.key).collect();
        self.details.retain(|key, _| alive.contains(key));
        self.cpu.retain(&alive);
        self.projects.retain(|_, (_, resolved_at)| {
            input.now.duration_since(*resolved_at) < PROJECT_CACHE_TTL
        });

        // Details are read once per process and reused while it lives.
        let cache = RefCell::new(std::mem::take(&mut self.details));
        let details_of = |key: ProcessKey| -> ProcessDetails {
            if let Some(known) = cache.borrow().get(&key) {
                return known.clone();
            }
            let fetched = (input.details)(key);
            cache.borrow_mut().insert(key, fetched.clone());
            fetched
        };
        let args_of = |entry: &ProcessEntry| details_of(entry.key).args.known().cloned();

        let tree = ProcessTree::new(input.processes);
        let mut services: Vec<Service> = service::group(input.listeners, &tree, &args_of)
            .into_iter()
            .filter_map(|group| self.describe(&group, &tree, &details_of, &args_of, input))
            .collect();
        services.sort_by_cached_key(|service| {
            let name = service
                .project
                .as_ref()
                .map(|project| project.name.to_lowercase());
            let port = service.ports.first().map(|port| port.port);
            (service.classification, name.is_none(), name, port)
        });

        self.details = cache.into_inner();
        Snapshot { services }
    }

    fn describe(
        &mut self,
        group: &service::Grouped,
        tree: &ProcessTree<'_>,
        details_of: &dyn Fn(ProcessKey) -> ProcessDetails,
        args_of: service::ArgsOf<'_>,
        input: Input<'_>,
    ) -> Option<Service> {
        let root = tree.get(group.root.pid)?;
        let owner = group.owners.first().and_then(|key| tree.get(key.pid))?;
        let members: Vec<&ProcessEntry> = group
            .members
            .iter()
            .filter_map(|key| tree.get(key.pid))
            .collect();
        let owner_details = details_of(owner.key);

        let folder = owner_details
            .cwd
            .known()
            .cloned()
            .or_else(|| details_of(root.key).cwd.known().cloned())
            .or_else(|| {
                owner_details
                    .exe
                    .known()
                    .and_then(|exe| exe.parent().map(Path::to_path_buf))
            });
        let project = folder.and_then(|folder| self.project_for(&folder, input));

        let members: Vec<Member> = members
            .iter()
            .map(|entry| Member {
                pid: entry.key.pid.0,
                name: entry.name.clone(),
                command: args_of(entry).map(|args| command::display(&args)),
                memory_bytes: entry.memory_bytes,
                cpu_percent: entry
                    .cpu_time
                    .and_then(|cpu| self.cpu.sample(entry.key, cpu, input.now)),
            })
            .collect();

        let mut ports: Vec<Port> = group
            .endpoints
            .iter()
            .map(|&(address, port)| Port { port, address })
            .collect();
        ports.sort_by_key(|port| (port.port, port.address));

        let names = [normalize(&owner.name), normalize(&root.name)];
        let protected = names.iter().any(|name| DATABASES.contains(&name.as_str()));
        let classification = classify(
            owner,
            &owner_details,
            &group.members,
            tree,
            args_of,
            project.is_some(),
            protected,
        );

        Some(Service {
            id: format!("{}-{}", root.key.pid.0, root.key.start.0),
            classification,
            protected,
            ports,
            project,
            command: args_of(root).map(|args| command::display(&args)),
            started_at: root.key.start.0 / 1_000_000_000,
            memory_bytes: sum(members.iter().map(|member| member.memory_bytes)),
            cpu_percent: sum(members.iter().map(|member| member.cpu_percent)),
            processes: members,
        })
    }

    fn project_for(&mut self, folder: &Path, input: Input<'_>) -> Option<Project> {
        if let Some((project, _)) = self.projects.get(folder) {
            return project.clone();
        }
        let project = project::resolve(folder, input.home);
        self.projects
            .insert(folder.to_path_buf(), (project.clone(), input.now));
        project
    }
}

fn classify(
    owner: &ProcessEntry,
    owner_details: &ProcessDetails,
    members: &[ProcessKey],
    tree: &ProcessTree<'_>,
    args_of: service::ArgsOf<'_>,
    in_a_project: bool,
    database: bool,
) -> Classification {
    if owner.owner != Owner::CurrentUser || service::is_system(owner) {
        return Classification::System;
    }
    if database {
        return Classification::Dev;
    }
    let exe = owner_details.exe.known();
    if exe.is_some_and(|exe| is_inside_app_bundle(exe)) {
        return Classification::Other;
    }
    let runs_code = members
        .iter()
        .filter_map(|key| tree.get(key.pid))
        .any(|entry| {
            matches!(
                role::role(&entry.name, args_of(entry).as_deref()),
                Role::Runtime | Role::Runner
            )
        });
    let built_by_go = exe.is_some_and(|exe| exe.to_string_lossy().contains("go-build"));
    if runs_code || built_by_go || in_a_project {
        Classification::Dev
    } else {
        Classification::Other
    }
}

/// Desktop apps on macOS, except the Python.app wrapper that python.org and
/// Homebrew framework builds run every script through.
fn is_inside_app_bundle(exe: &Path) -> bool {
    let path = exe.to_string_lossy();
    path.contains(".app/Contents/") && !path.contains("/Python.framework/Versions/")
}

fn sum<T: std::iter::Sum<T>>(values: impl Iterator<Item = Option<T>>) -> Option<T> {
    let known: Vec<T> = values.flatten().collect();
    (!known.is_empty()).then(|| known.into_iter().sum())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::IpAddr;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use harbormaster_platform::{
        Field, Listener, Owner, Pid, ProcessDetails, ProcessEntry, ProcessKey, StartTime,
    };

    use super::{Classification, Input, Scanner, Snapshot};

    struct Proc {
        entry: ProcessEntry,
        details: ProcessDetails,
    }

    fn proc(pid: u32, parent: u32, name: &str, exe: &str, args: &[&str], cwd: &Path) -> Proc {
        Proc {
            entry: ProcessEntry {
                key: ProcessKey {
                    pid: Pid(pid),
                    start: StartTime(u64::from(pid) * 1_000_000_000),
                },
                parent: Some(Pid(parent)),
                name: name.to_string(),
                owner: Owner::CurrentUser,
                memory_bytes: Some(100 * 1024 * 1024),
                cpu_time: Some(Duration::from_secs(1)),
            },
            details: ProcessDetails {
                exe: Field::Known(PathBuf::from(exe)),
                args: Field::Known(args.iter().map(|s| s.to_string()).collect()),
                cwd: Field::Known(cwd.to_path_buf()),
            },
        }
    }

    fn scan(
        scanner: &mut Scanner,
        procs: &[Proc],
        ports: &[(u32, u16)],
        home: &Path,
        now: Instant,
    ) -> Snapshot {
        let processes: Vec<ProcessEntry> = procs.iter().map(|p| p.entry.clone()).collect();
        let listeners: Vec<Listener> = ports
            .iter()
            .map(|(pid, port)| Listener {
                owner: procs
                    .iter()
                    .find(|p| p.entry.key.pid == Pid(*pid))
                    .unwrap()
                    .entry
                    .key,
                address: IpAddr::from([127, 0, 0, 1]),
                port: *port,
            })
            .collect();
        let details = |key: ProcessKey| {
            procs
                .iter()
                .find(|p| p.entry.key == key)
                .map(|p| p.details.clone())
                .unwrap_or(ProcessDetails {
                    exe: Field::Gone,
                    args: Field::Gone,
                    cwd: Field::Gone,
                })
        };
        scanner.scan(Input {
            listeners: &listeners,
            processes: &processes,
            details: &details,
            home,
            now,
        })
    }

    #[test]
    fn describes_a_node_server_by_its_project_and_command() {
        let home = tempfile::tempdir().unwrap();
        let app = home.path().join("dev/acme-store");
        fs::create_dir_all(&app).unwrap();
        fs::write(
            app.join("package.json"),
            r#"{"name":"acme-store","dependencies":{"next":"16.0.1"}}"#,
        )
        .unwrap();
        let procs = [
            proc(10, 1, "zsh", "/bin/zsh", &["-zsh"], &app),
            proc(
                20,
                10,
                "node",
                "/usr/local/bin/node",
                &[
                    "node",
                    "/usr/local/lib/node_modules/npm/bin/npm-cli.js",
                    "run",
                    "dev",
                ],
                &app,
            ),
            proc(
                30,
                20,
                "node",
                "/usr/local/bin/node",
                &[
                    "node",
                    "/x/acme-store/node_modules/next/dist/bin/next",
                    "dev",
                ],
                &app,
            ),
        ];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(30, 3000)],
            home.path(),
            Instant::now(),
        );

        let service = &snapshot.services[0];
        assert_eq!(service.classification, Classification::Dev);
        assert_eq!(service.project.as_ref().unwrap().name, "acme-store");
        assert_eq!(service.command.as_deref(), Some("npm run dev"));
        assert_eq!(service.ports[0].port, 3000);
        assert_eq!(service.memory_bytes, Some(200 * 1024 * 1024));
        assert!(!service.protected);
    }

    #[test]
    fn leaves_desktop_apps_out_of_the_dev_list() {
        let home = tempfile::tempdir().unwrap();
        let procs = [proc(
            700,
            1,
            "ControlCenter",
            "/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter",
            &["/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter"],
            Path::new("/"),
        )];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(700, 5000)],
            home.path(),
            Instant::now(),
        );

        assert_eq!(snapshot.services[0].classification, Classification::Other);
        assert!(snapshot.services[0].project.is_none());
    }

    #[test]
    fn puts_other_users_servers_in_the_system_group() {
        let home = tempfile::tempdir().unwrap();
        let mut daemon = proc(
            900,
            1,
            "httpd",
            "/usr/sbin/httpd",
            &["/usr/sbin/httpd"],
            Path::new("/"),
        );
        daemon.entry.owner = Owner::System;

        let snapshot = scan(
            &mut Scanner::default(),
            &[daemon],
            &[(900, 80)],
            home.path(),
            Instant::now(),
        );

        assert_eq!(snapshot.services[0].classification, Classification::System);
    }

    #[test]
    fn keeps_databases_visible_but_protected() {
        let home = tempfile::tempdir().unwrap();
        let procs = [proc(
            1167,
            1,
            "mongod",
            "/opt/homebrew/bin/mongod",
            &["mongod", "--config", "/opt/homebrew/etc/mongod.conf"],
            Path::new("/"),
        )];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(1167, 27017)],
            home.path(),
            Instant::now(),
        );

        assert_eq!(snapshot.services[0].classification, Classification::Dev);
        assert!(snapshot.services[0].protected);
    }

    #[test]
    fn lists_services_with_a_project_before_those_without() {
        let home = tempfile::tempdir().unwrap();
        let app = home.path().join("zeta");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("package.json"), r#"{"name":"zeta"}"#).unwrap();
        let procs = [
            proc(
                20,
                1,
                "node",
                "/usr/local/bin/node",
                &["node", "/opt/tool/gateway.js"],
                Path::new("/"),
            ),
            proc(
                30,
                1,
                "node",
                "/usr/local/bin/node",
                &["node", "/x/zeta/server.js"],
                &app,
            ),
        ];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(20, 4000), (30, 5000)],
            home.path(),
            Instant::now(),
        );

        let names: Vec<Option<&str>> = snapshot
            .services
            .iter()
            .map(|s| s.project.as_ref().map(|p| p.name.as_str()))
            .collect();
        assert_eq!(names, [Some("zeta"), None]);
    }

    #[test]
    fn reports_cpu_from_the_second_scan_on() {
        let home = tempfile::tempdir().unwrap();
        let mut procs = [proc(
            20,
            1,
            "node",
            "/usr/local/bin/node",
            &["node", "/srv/api.js"],
            home.path(),
        )];
        let mut scanner = Scanner::default();
        let start = Instant::now();

        let first = scan(&mut scanner, &procs, &[(20, 4000)], home.path(), start);
        procs[0].entry.cpu_time = Some(Duration::from_millis(1_500));
        let second = scan(
            &mut scanner,
            &procs,
            &[(20, 4000)],
            home.path(),
            start + Duration::from_secs(1),
        );

        assert_eq!(first.services[0].cpu_percent, None);
        assert_eq!(second.services[0].cpu_percent, Some(50.0));
    }
}
