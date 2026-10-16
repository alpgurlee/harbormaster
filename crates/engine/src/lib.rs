mod command;
mod port;
mod project;
mod role;
mod sampler;
mod service;
mod stop;
mod tree;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use harbormaster_platform::{Listener, Owner, ProcessDetails, ProcessEntry, ProcessKey};
use serde::Serialize;

pub use port::{PortReport, explain_port};
pub use project::{Framework, Project};
pub use stop::{Outcome, Refusal, Signals, Target, choose, order, recheck, stop};

use role::{Role, normalize};
use sampler::CpuSampler;
use tree::ProcessTree;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    /// Stable while the root process lives: `<pid>-<start>`.
    pub id: String,
    pub classification: Classification,
    /// Why stopping it from here is refused, if it is.
    pub protected: Option<Protection>,
    pub ports: Vec<Port>,
    pub project: Option<Project>,
    pub command: Option<String>,
    /// Seconds since the Unix epoch.
    pub started_at: u64,
    pub memory_bytes: Option<u64>,
    pub cpu_percent: Option<f64>,
    pub processes: Vec<Member>,
    /// Every process holding one of its sockets, members or not.
    pub holders: Vec<Holder>,
    /// What it was started from and keeps running after a stop: the shell,
    /// terminal or agent above the root, nearest first.
    pub ancestors: Vec<Holder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Classification {
    Dev,
    Other,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Protection {
    System,
    /// Runs as another user, root included.
    OtherUser,
    /// The user's own shell, terminal or coding agent holds the socket.
    Session,
    /// Some of its processes couldn't be read, so none of the above can be
    /// ruled out.
    Unverified,
    Database,
    /// A process outside the service holds its socket too, so stopping the
    /// service would not free the port.
    SharedSocket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Port {
    pub port: u16,
    pub address: IpAddr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Holder {
    pub pid: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    /// What a stop checks right before each signal.
    #[serde(skip)]
    pub key: ProcessKey,
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
        let owner = tree.get(group.owner.pid)?;
        let members: Vec<&ProcessEntry> = group
            .members
            .iter()
            .filter_map(|key| tree.get(key.pid))
            .collect();
        let owner_details = details_of(owner.key);

        // The working folder usually names the project; a binary started from
        // elsewhere (`/tmp` running `repo/target/debug/api`) still lives in it.
        let candidates = [
            owner_details.cwd.known().cloned(),
            details_of(root.key).cwd.known().cloned(),
            owner_details
                .exe
                .known()
                .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        ];
        let project = candidates
            .into_iter()
            .flatten()
            .find_map(|folder| self.project_for(&folder, input));

        let is_database =
            |entry: &&ProcessEntry| DATABASES.contains(&normalize(&entry.name).as_str());
        let database = is_database(&owner) || members.iter().any(is_database);
        let someone_elses = members
            .iter()
            .any(|entry| entry.owner != Owner::CurrentUser);

        let unreadable = members.iter().any(|entry| {
            let details = details_of(entry.key);
            details.exe.known().is_none() || details.args.known().is_none()
        });
        // A part of the OS anywhere in the service keeps all of it out of reach.
        let os_component = std::iter::once(owner)
            .chain(members.iter().copied())
            .any(|entry| {
                details_of(entry.key)
                    .exe
                    .known()
                    .is_some_and(|exe| is_os_component(exe))
            });

        let members: Vec<Member> = members
            .iter()
            .map(|entry| Member {
                key: entry.key,
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

        let classification = classify(
            owner,
            &owner_details,
            &group.members,
            tree,
            args_of,
            project.is_some(),
            database,
        );
        // Stopping must never reach the system, another user or a database,
        // nor promise a free port while someone outside keeps the socket.
        let protected = if service::is_system(owner) {
            Some(Protection::System)
        } else if owner.owner != Owner::CurrentUser || someone_elses {
            Some(Protection::OtherUser)
        } else if os_component {
            Some(Protection::System)
        } else if unreadable {
            Some(Protection::Unverified)
        } else if service::is_session_boundary(owner, tree, args_of) {
            Some(Protection::Session)
        } else if database {
            Some(Protection::Database)
        } else if group
            .holders
            .iter()
            .any(|holder| !group.members.contains(holder))
        {
            Some(Protection::SharedSocket)
        } else {
            None
        };
        let holders = group
            .holders
            .iter()
            .filter_map(|key| tree.get(key.pid))
            .map(|entry| Holder {
                pid: entry.key.pid.0,
                name: entry.name.clone(),
            })
            .collect();

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
            holders,
            ancestors: ancestors(root, tree, args_of),
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

/// Why single processes may not be stopped now, whatever service they are
/// in. Processes no longer there are left out; their signal reports them.
pub fn protections(input: Input<'_>, keys: &[ProcessKey]) -> Vec<(ProcessKey, Protection)> {
    let tree = ProcessTree::new(input.processes);
    let args_of = |entry: &ProcessEntry| (input.details)(entry.key).args.known().cloned();
    keys.iter()
        .filter_map(|key| {
            let entry = tree.get(key.pid).filter(|entry| entry.key == *key)?;
            let details = (input.details)(entry.key);
            let protection = if service::is_system(entry) {
                Protection::System
            } else if entry.owner != Owner::CurrentUser {
                Protection::OtherUser
            } else if details.exe.known().is_some_and(|exe| is_os_component(exe)) {
                Protection::System
            } else if details.exe.known().is_none() || details.args.known().is_none() {
                Protection::Unverified
            } else if service::is_session_boundary(entry, &tree, &args_of) {
                Protection::Session
            } else if DATABASES.contains(&normalize(&entry.name).as_str()) {
                Protection::Database
            } else {
                return None;
            };
            Some((*key, protection))
        })
        .collect()
}

/// Up to the first system process, which is where a session ends.
fn ancestors(
    root: &ProcessEntry,
    tree: &ProcessTree<'_>,
    args_of: service::ArgsOf<'_>,
) -> Vec<Holder> {
    let mut found = Vec::new();
    let mut current = root;
    while let Some(parent) = tree.parent(current.key.pid) {
        if service::is_system(parent) {
            break;
        }
        let args = args_of(parent);
        // An agent installed as `versions/2.1.300` is known by how it was started.
        let name = match args.as_deref().and_then(<[String]>::first) {
            Some(first) if role::role(&parent.name, args.as_deref()) == Role::Agent => {
                normalize(first)
            }
            _ => parent.name.clone(),
        };
        found.push(Holder {
            pid: parent.key.pid.0,
            name,
        });
        current = parent;
    }
    found
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

/// Parts of the OS that run as the user, such as macOS's ControlCenter
/// (AirPlay on :5000). Runtimes the OS ships in /usr/bin stay stoppable.
fn is_os_component(exe: &Path) -> bool {
    let path = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    let windows = path
        .get(1..)
        .is_some_and(|rest| rest.starts_with(":/windows/"));
    windows
        || ["/system/", "/usr/libexec/", "/usr/sbin/", "/sbin/"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
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
        Field, Listener, Owner, Pid, ProcessDetails, ProcessEntry, ProcessKey, SocketId, StartTime,
    };

    use super::{Classification, Input, Protection, Scanner, Snapshot};

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
                    exec: 0,
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
        let sockets: Vec<(u32, u16, Option<u64>)> =
            ports.iter().map(|&(pid, port)| (pid, port, None)).collect();
        scan_sockets(scanner, procs, &sockets, home, now)
    }

    fn scan_sockets(
        scanner: &mut Scanner,
        procs: &[Proc],
        sockets: &[(u32, u16, Option<u64>)],
        home: &Path,
        now: Instant,
    ) -> Snapshot {
        let processes: Vec<ProcessEntry> = procs.iter().map(|p| p.entry.clone()).collect();
        let listeners: Vec<Listener> = sockets
            .iter()
            .map(|&(pid, port, socket)| Listener {
                owner: procs
                    .iter()
                    .find(|p| p.entry.key.pid == Pid(pid))
                    .unwrap()
                    .entry
                    .key,
                address: IpAddr::from([127, 0, 0, 1]),
                port,
                socket: socket.map(|kernel| SocketId {
                    kernel,
                    generation: 0,
                }),
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
        assert_eq!(service.protected, None);
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
        daemon.entry.owner = Owner::OtherUser;

        let snapshot = scan(
            &mut Scanner::default(),
            &[daemon],
            &[(900, 80)],
            home.path(),
            Instant::now(),
        );

        assert_eq!(snapshot.services[0].classification, Classification::System);
        assert_eq!(snapshot.services[0].protected, Some(Protection::OtherUser));
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
        assert_eq!(snapshot.services[0].protected, Some(Protection::Database));
    }

    #[test]
    fn protects_every_system_service() {
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

        assert_eq!(snapshot.services[0].protected, Some(Protection::System));
    }

    #[test]
    fn protects_a_service_with_a_database_among_its_processes() {
        let home = tempfile::tempdir().unwrap();
        let procs = [
            proc(10, 1, "zsh", "/bin/zsh", &["-zsh"], home.path()),
            proc(
                20,
                10,
                "node",
                "/usr/local/bin/node",
                &[
                    "node",
                    "/usr/local/lib/node_modules/npm/bin/npm-cli.js",
                    "run",
                    "db",
                ],
                home.path(),
            ),
            proc(
                30,
                20,
                "postgres",
                "/opt/pg/bin/postgres",
                &["postgres", "-D", "data"],
                home.path(),
            ),
            proc(
                40,
                20,
                "node",
                "/usr/local/bin/node",
                &["node", "/srv/api.js"],
                home.path(),
            ),
        ];

        // postgres only uses a unix socket here, so it is a member of the api's service.
        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(40, 4000)],
            home.path(),
            Instant::now(),
        );

        assert!(
            snapshot.services[0]
                .processes
                .iter()
                .any(|member| member.name == "postgres")
        );
        assert_eq!(snapshot.services[0].protected, Some(Protection::Database));
    }

    #[test]
    fn names_what_stays_open_the_way_it_was_started() {
        let home = tempfile::tempdir().unwrap();
        let claude = "/Users/a/.local/share/claude/versions/2.1.300";
        let procs = [
            proc(
                5,
                1,
                "2.1.300",
                claude,
                &["claude", "--resume"],
                home.path(),
            ),
            proc(
                10,
                5,
                "zsh",
                "/bin/zsh",
                &["/bin/zsh", "-c", "npm run dev"],
                home.path(),
            ),
            proc(
                20,
                10,
                "node",
                "/usr/local/bin/node",
                &["node", "/srv/server.js"],
                home.path(),
            ),
        ];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(20, 4000)],
            home.path(),
            Instant::now(),
        );

        let names: Vec<&str> = snapshot.services[0]
            .ancestors
            .iter()
            .map(|ancestor| ancestor.name.as_str())
            .collect();
        assert_eq!(names, ["zsh", "claude"]);
    }

    #[test]
    fn protects_parts_of_the_os_that_run_as_the_user() {
        let home = tempfile::tempdir().unwrap();
        let control_center =
            "/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter";
        let procs = [
            proc(
                700,
                1,
                "ControlCenter",
                control_center,
                &[control_center],
                home.path(),
            ),
            proc(
                701,
                1,
                "rapportd",
                "/usr/libexec/rapportd",
                &["/usr/libexec/rapportd"],
                home.path(),
            ),
            proc(
                702,
                1,
                "ruby",
                "/usr/bin/ruby",
                &["ruby", "-run", "-e", "httpd", ".", "-p", "8000"],
                home.path(),
            ),
        ];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(700, 5000), (701, 50594), (702, 8000)],
            home.path(),
            Instant::now(),
        );

        let protection = |port: u16| {
            snapshot
                .services
                .iter()
                .find(|service| service.ports[0].port == port)
                .unwrap()
                .protected
        };
        assert_eq!(protection(5000), Some(Protection::System));
        assert_eq!(protection(50594), Some(Protection::System));
        assert_eq!(protection(8000), None);
    }

    #[test]
    fn protects_a_service_with_a_part_of_the_os_among_its_processes() {
        let home = tempfile::tempdir().unwrap();
        let procs = [
            proc(10, 1, "zsh", "/bin/zsh", &["-zsh"], home.path()),
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
                home.path(),
            ),
            proc(
                30,
                20,
                "helper",
                "/usr/libexec/helper",
                &["/usr/libexec/helper"],
                home.path(),
            ),
            proc(
                40,
                20,
                "node",
                "/usr/local/bin/node",
                &["node", "/srv/api.js"],
                home.path(),
            ),
        ];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(40, 4000)],
            home.path(),
            Instant::now(),
        );

        assert_eq!(snapshot.services[0].protected, Some(Protection::System));
    }

    #[test]
    fn judges_single_processes_whatever_service_they_are_in() {
        let home = tempfile::tempdir().unwrap();
        let procs = [
            proc(10, 1, "zsh", "/bin/zsh", &["-zsh"], home.path()),
            proc(
                20,
                10,
                "node",
                "/usr/local/bin/node",
                &["codex"],
                home.path(),
            ),
            proc(
                30,
                10,
                "node",
                "/usr/local/bin/node",
                &["node", "/srv/api.js"],
                home.path(),
            ),
        ];
        let processes: Vec<ProcessEntry> = procs.iter().map(|p| p.entry.clone()).collect();
        let details = |key: ProcessKey| {
            procs
                .iter()
                .find(|p| p.entry.key == key)
                .map(|p| p.details.clone())
                .unwrap()
        };
        let input = Input {
            listeners: &[],
            processes: &processes,
            details: &details,
            home: home.path(),
            now: Instant::now(),
        };
        let keys: Vec<ProcessKey> = procs.iter().map(|p| p.entry.key).collect();

        let found = super::protections(input, &keys);

        assert_eq!(
            found,
            [
                (keys[0], Protection::Session),
                (keys[1], Protection::Session)
            ]
        );
    }

    #[test]
    fn protects_a_service_whose_processes_it_cannot_fully_read() {
        let home = tempfile::tempdir().unwrap();
        let mut hidden = proc(
            30,
            20,
            "node",
            "/usr/local/bin/node",
            &["node", "/srv/worker.js"],
            home.path(),
        );
        hidden.details.args = Field::Denied;
        let procs = [
            proc(10, 1, "zsh", "/bin/zsh", &["-zsh"], home.path()),
            proc(
                20,
                10,
                "node",
                "/usr/local/bin/node",
                &["node", "/srv/api.js"],
                home.path(),
            ),
            hidden,
        ];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(20, 4000)],
            home.path(),
            Instant::now(),
        );

        assert_eq!(snapshot.services[0].protected, Some(Protection::Unverified));
    }

    #[test]
    fn protects_a_socket_that_only_the_users_session_holds() {
        let home = tempfile::tempdir().unwrap();
        let helper = "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)";
        let procs = [
            proc(
                10,
                1,
                "Code Helper (Plugin)",
                helper,
                &[helper],
                home.path(),
            ),
            proc(20, 1, "zsh", "/bin/zsh", &["-zsh"], home.path()),
        ];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(10, 51234), (20, 7000)],
            home.path(),
            Instant::now(),
        );

        assert!(
            snapshot
                .services
                .iter()
                .all(|service| service.protected == Some(Protection::Session)),
            "{:?}",
            snapshot.services
        );
    }

    #[test]
    fn protects_a_service_whose_socket_is_held_outside_it_too() {
        let home = tempfile::tempdir().unwrap();
        let mut launchd = proc(
            1,
            0,
            "launchd",
            "/sbin/launchd",
            &["/sbin/launchd"],
            Path::new("/"),
        );
        launchd.entry.owner = Owner::System;
        let server = proc(
            20,
            1,
            "node",
            "/usr/local/bin/node",
            &["node", "/srv/a.js"],
            home.path(),
        );

        let snapshot = scan_sockets(
            &mut Scanner::default(),
            &[launchd, server],
            &[(1, 8000, Some(77)), (20, 8000, Some(77))],
            home.path(),
            Instant::now(),
        );

        let service = &snapshot.services[0];
        assert_eq!(service.classification, Classification::Dev);
        assert_eq!(service.protected, Some(Protection::SharedSocket));
        let holders: Vec<(u32, &str)> = service
            .holders
            .iter()
            .map(|holder| (holder.pid, holder.name.as_str()))
            .collect();
        assert_eq!(holders, [(1, "launchd"), (20, "node")]);
        let members: Vec<u32> = service.processes.iter().map(|member| member.pid).collect();
        assert_eq!(members, [20]);
    }

    #[test]
    fn finds_the_project_from_the_executable_when_the_working_folder_has_none() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("gateway");
        fs::create_dir_all(repo.join("target/debug")).unwrap();
        fs::write(repo.join("Cargo.toml"), "[package]\nname = \"gateway\"\n").unwrap();
        let elsewhere = home.path().join("scratch");
        fs::create_dir_all(&elsewhere).unwrap();
        let exe = repo.join("target/debug/gateway");
        let procs = [proc(
            20,
            1,
            "gateway",
            exe.to_str().unwrap(),
            &[exe.to_str().unwrap()],
            &elsewhere,
        )];

        let snapshot = scan(
            &mut Scanner::default(),
            &procs,
            &[(20, 8080)],
            home.path(),
            Instant::now(),
        );

        assert_eq!(
            snapshot.services[0]
                .project
                .as_ref()
                .map(|p| p.name.as_str()),
            Some("gateway")
        );
        assert_eq!(snapshot.services[0].classification, Classification::Dev);
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
