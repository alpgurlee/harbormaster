use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::net::IpAddr;

use harbormaster_platform::{Listener, Owner, Pid, ProcessEntry, ProcessKey};

use crate::role::{Role, is_dev_supervisor, is_runner_by_title, normalize, role, runs_a_script};
use crate::tree::ProcessTree;

const SYSTEM_PROCESSES: &[&str] = &[
    "launchd",
    "init",
    "systemd",
    "kernel_task",
    "explorer",
    "services",
    "svchost",
    "wininit",
    "winlogon",
    "csrss",
    "smss",
    "system",
    "sihost",
    "runtimebroker",
];

/// Processes that serve the same sockets, and the command that started them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Grouped {
    pub(crate) root: ProcessKey,
    /// The root first, then everything it started, breadth first.
    pub(crate) members: Vec<ProcessKey>,
    /// The socket holder that does the serving.
    pub(crate) owner: ProcessKey,
    /// Every process holding one of the sockets. The system or the user's
    /// shell may be among them without being a member.
    pub(crate) holders: Vec<ProcessKey>,
    pub(crate) endpoints: Vec<(IpAddr, u16)>,
}

pub(crate) type ArgsOf<'a> = &'a dyn Fn(&ProcessEntry) -> Option<Vec<String>>;

pub(crate) fn group(
    listeners: &[Listener],
    tree: &ProcessTree<'_>,
    args_of: ArgsOf<'_>,
) -> Vec<Grouped> {
    let listeners: Vec<&Listener> = listeners
        .iter()
        .filter(|listener| {
            tree.get(listener.owner.pid)
                .is_some_and(|entry| entry.key == listener.owner)
        })
        .collect();

    let mut sets = DisjointSets::default();
    // Only processes holding the very same socket serve together, e.g. a
    // reloader and its worker. Unrelated processes that bound one port with
    // SO_REUSEPORT stay apart.
    let mut holder_of_socket: HashMap<u64, Pid> = HashMap::new();
    for listener in &listeners {
        sets.add(listener.owner.pid);
        if let Some(socket) = listener.socket
            && let Some(holder) = holder_of_socket.insert(socket, listener.owner.pid)
        {
            sets.union(holder, listener.owner.pid);
        }
    }

    let mut sockets: BTreeMap<Pid, SharedSockets> = BTreeMap::new();
    for listener in &listeners {
        let shared = sockets.entry(sets.find(listener.owner.pid)).or_default();
        shared.owners.insert(listener.owner);
        if !shared
            .endpoints
            .contains(&(listener.address, listener.port))
        {
            shared.endpoints.push((listener.address, listener.port));
        }
    }
    let group_of: HashMap<Pid, Pid> = listeners
        .iter()
        .map(|listener| (listener.owner.pid, sets.find(listener.owner.pid)))
        .collect();

    let mut by_root: BTreeMap<Pid, Grouped> = BTreeMap::new();
    for (group, SharedSockets { owners, endpoints }) in sockets {
        let is_foreign = |pid: Pid| group_of.get(&pid).is_some_and(|other| *other != group);
        // launchd activating a socket or a shell that inherited one holds it
        // too, but never leads the service.
        let servers: BTreeSet<ProcessKey> = owners
            .iter()
            .copied()
            .filter(|owner| {
                tree.get(owner.pid)
                    .is_some_and(|entry| !is_session_boundary(entry, tree, args_of))
            })
            .collect();
        let (owner, root, members) = match topmost(&servers, tree) {
            Some(top) => {
                let root = climb(top, tree, args_of, &is_foreign);
                (top.key, root.key, members(root, tree, args_of, &is_foreign))
            }
            None => {
                let Some(top) = topmost(&owners, tree) else {
                    continue;
                };
                (top.key, top.key, vec![top.key])
            }
        };
        let service = by_root.entry(root.pid).or_insert_with(|| Grouped {
            root,
            members,
            owner,
            holders: Vec::new(),
            endpoints: Vec::new(),
        });
        service.holders.extend(owners);
        service.endpoints.extend(endpoints);
    }
    by_root.into_values().collect()
}

/// Processes that hold the same listening sockets, and those sockets.
#[derive(Debug, Default)]
struct SharedSockets {
    owners: BTreeSet<ProcessKey>,
    endpoints: Vec<(IpAddr, u16)>,
}

/// The socket holder that started the others, or the oldest one.
fn topmost<'a>(owners: &BTreeSet<ProcessKey>, tree: &ProcessTree<'a>) -> Option<&'a ProcessEntry> {
    let covers_all = |candidate: &ProcessKey| {
        let below: HashSet<ProcessKey> = tree
            .subtree(candidate.pid)
            .iter()
            .map(|entry| entry.key)
            .collect();
        owners.iter().all(|owner| below.contains(owner))
    };
    let top = owners
        .iter()
        .find(|candidate| covers_all(candidate))
        .or_else(|| owners.iter().min_by_key(|owner| owner.start))?;
    tree.get(top.pid)
}

/// Walks up to the command the user ran, stopping at shells, terminals,
/// coding agents and any process that also supervises another server.
fn climb<'a>(
    from: &'a ProcessEntry,
    tree: &ProcessTree<'a>,
    args_of: ArgsOf<'_>,
    is_foreign: &dyn Fn(Pid) -> bool,
) -> &'a ProcessEntry {
    let mut current = from;
    while let Some(parent) = tree.parent(current.key.pid) {
        if !is_climbable(parent, tree, args_of)
            || tree
                .subtree(parent.key.pid)
                .iter()
                .any(|entry| is_foreign(entry.key.pid))
        {
            break;
        }
        current = parent;
        if is_runner_by_title(&parent.name, args_of(parent).as_deref()) {
            break;
        }
    }
    current
}

fn is_climbable(entry: &ProcessEntry, tree: &ProcessTree<'_>, args_of: ArgsOf<'_>) -> bool {
    if is_system(entry) {
        return false;
    }
    let args = args_of(entry);
    match role(&entry.name, args.as_deref()) {
        Role::Runner => true,
        Role::Runtime => is_dev_supervisor(&entry.name, args.as_deref()),
        Role::Shell => is_runner_shell(entry, tree, args_of),
        Role::Terminal | Role::Agent | Role::Other => false,
    }
}

/// Never part of a service: the user's shells, terminals, coding agents and the system.
pub(crate) fn is_session_boundary(
    entry: &ProcessEntry,
    tree: &ProcessTree<'_>,
    args_of: ArgsOf<'_>,
) -> bool {
    if is_system(entry) {
        return true;
    }
    match role(&entry.name, args_of(entry).as_deref()) {
        Role::Terminal | Role::Agent => true,
        Role::Shell => !is_runner_shell(entry, tree, args_of),
        Role::Runner | Role::Runtime | Role::Other => false,
    }
}

/// A shell that only exists to run a script for a runner, such as the
/// `sh -c "next dev"` that npm spawns. Any other shell is the user's session.
pub(crate) fn is_runner_shell(
    entry: &ProcessEntry,
    tree: &ProcessTree<'_>,
    args_of: ArgsOf<'_>,
) -> bool {
    if !runs_a_script(args_of(entry).as_deref()) {
        return false;
    }
    tree.parent(entry.key.pid)
        .is_some_and(|parent| role(&parent.name, args_of(parent).as_deref()) == Role::Runner)
}

pub(crate) fn is_system(entry: &ProcessEntry) -> bool {
    entry.key.pid.0 <= 1
        || entry.owner == Owner::System
        || SYSTEM_PROCESSES.contains(&normalize(&entry.name).as_str())
}

/// The root and everything it started, except branches that lead to another server.
fn members(
    root: &ProcessEntry,
    tree: &ProcessTree<'_>,
    args_of: ArgsOf<'_>,
    is_foreign: &dyn Fn(Pid) -> bool,
) -> Vec<ProcessKey> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([root]);
    while let Some(entry) = queue.pop_front() {
        if !seen.insert(entry.key.pid) {
            continue;
        }
        found.push(entry.key);
        for child in tree.children(entry.key.pid) {
            if is_session_boundary(child, tree, args_of) {
                continue;
            }
            if !tree
                .subtree(child.key.pid)
                .iter()
                .any(|below| is_foreign(below.key.pid))
            {
                queue.push_back(child);
            }
        }
    }
    found
}

#[derive(Debug, Default)]
struct DisjointSets {
    parent: HashMap<Pid, Pid>,
}

impl DisjointSets {
    fn add(&mut self, pid: Pid) {
        self.parent.entry(pid).or_insert(pid);
    }

    fn find(&mut self, pid: Pid) -> Pid {
        let parent = *self.parent.entry(pid).or_insert(pid);
        if parent == pid {
            return pid;
        }
        let root = self.find(parent);
        self.parent.insert(pid, root);
        root
    }

    fn union(&mut self, a: Pid, b: Pid) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent.insert(a.max(b), a.min(b));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::IpAddr;

    use harbormaster_platform::{Listener, Pid, ProcessEntry};

    use super::{Grouped, group};
    use crate::tree::ProcessTree;
    use crate::tree::tests::entry;

    struct Machine {
        processes: Vec<ProcessEntry>,
        args: HashMap<u32, Vec<String>>,
        listeners: Vec<Listener>,
    }

    impl Machine {
        fn new() -> Self {
            Machine {
                processes: Vec::new(),
                args: HashMap::new(),
                listeners: Vec::new(),
            }
        }

        fn process(mut self, pid: u32, parent: u32, name: &str, args: &[&str]) -> Self {
            self.processes
                .push(entry(pid, Some(parent), u64::from(pid), name));
            if !args.is_empty() {
                self.args
                    .insert(pid, args.iter().map(|s| s.to_string()).collect());
            }
            self
        }

        fn listening(self, pid: u32, port: u16) -> Self {
            let socket = u64::from(pid) * 100_000 + u64::from(port);
            self.sharing(pid, port, socket)
        }

        /// A socket that several processes hold, e.g. inherited across fork.
        fn sharing(mut self, pid: u32, port: u16, socket: u64) -> Self {
            let owner = self
                .processes
                .iter()
                .find(|p| p.key.pid == Pid(pid))
                .unwrap()
                .key;
            self.listeners.push(Listener {
                owner,
                address: IpAddr::from([127, 0, 0, 1]),
                port,
                socket: Some(socket),
            });
            self
        }

        fn services(&self) -> Vec<Grouped> {
            let tree = ProcessTree::new(&self.processes);
            let args = |entry: &ProcessEntry| self.args.get(&entry.key.pid.0).cloned();
            let mut services = group(&self.listeners, &tree, &args);
            services.sort_by_key(|service| service.root.pid);
            services
        }
    }

    fn pids(keys: &[harbormaster_platform::ProcessKey]) -> Vec<u32> {
        keys.iter().map(|key| key.pid.0).collect()
    }

    const NPM: &str = "/usr/local/lib/node_modules/npm/bin/npm-cli.js";

    #[test]
    fn climbs_from_the_server_to_the_package_manager_but_not_the_shell() {
        let machine = Machine::new()
            .process(10, 1, "zsh", &["-zsh"])
            .process(20, 10, "node", &["node", NPM, "run", "dev"])
            .process(30, 20, "sh", &["sh", "-c", "next dev"])
            .process(
                40,
                30,
                "node",
                &["node", "/app/node_modules/.bin/next", "dev"],
            )
            .listening(40, 3000);

        let services = machine.services();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].root.pid, Pid(20));
        assert_eq!(pids(&services[0].members), [20, 30, 40]);
    }

    #[test]
    fn stops_below_a_coding_agent() {
        let machine = Machine::new()
            .process(5, 1, "claude", &["claude"])
            .process(6, 5, "zsh", &["/bin/zsh", "-c", "npm run dev"])
            .process(7, 6, "node", &["node", NPM, "run", "dev"])
            .process(8, 7, "node", &["node", "/app/node_modules/.bin/vite"])
            .listening(8, 5173);

        let services = machine.services();

        assert_eq!(services[0].root.pid, Pid(7));
        assert_eq!(pids(&services[0].members), [7, 8]);
    }

    #[test]
    fn keeps_two_servers_under_one_runner_apart() {
        let machine = Machine::new()
            .process(1, 0, "zsh", &["-zsh"])
            .process(2, 1, "node", &["node", NPM, "run", "dev"])
            .process(
                3,
                2,
                "node",
                &[
                    "node",
                    "/app/node_modules/.bin/concurrently",
                    "vite",
                    "node api.js",
                ],
            )
            .process(4, 3, "node", &["node", "/app/node_modules/.bin/vite"])
            .process(5, 3, "node", &["node", "/app/api.js"])
            .listening(4, 5173)
            .listening(5, 3001);

        let services = machine.services();

        assert_eq!(services.len(), 2);
        assert_eq!(pids(&services[0].members), [4]);
        assert_eq!(pids(&services[1].members), [5]);
    }

    #[test]
    fn treats_a_reloader_and_its_worker_on_one_socket_as_one_service() {
        let machine = Machine::new()
            .process(1, 0, "zsh", &["-zsh"])
            .process(
                2,
                1,
                "Python",
                &["python3", "-m", "uvicorn", "app:app", "--reload"],
            )
            .process(
                3,
                2,
                "Python",
                &[
                    "python3",
                    "-c",
                    "from multiprocessing.spawn import spawn_main",
                ],
            )
            .sharing(2, 8000, 77)
            .sharing(3, 8000, 77);

        let services = machine.services();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].root.pid, Pid(2));
        assert_eq!(pids(&services[0].members), [2, 3]);
        assert_eq!(services[0].endpoints.len(), 1);
    }

    #[test]
    fn keeps_independent_sockets_on_one_port_apart() {
        let machine = Machine::new()
            .process(1, 0, "zsh", &["-zsh"])
            .process(20, 1, "node", &["node", "/srv/a.js"])
            .process(30, 1, "node", &["node", "/srv/b.js"])
            .listening(20, 8000)
            .listening(30, 8000);

        assert_eq!(machine.services().len(), 2);
    }

    #[test]
    fn gives_one_service_for_one_process_on_two_ports() {
        let machine = Machine::new()
            .process(1, 0, "zsh", &["-zsh"])
            .process(2, 1, "node", &["node", "server.js"])
            .listening(2, 3000)
            .listening(2, 3001);

        let services = machine.services();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].endpoints.len(), 2);
    }

    #[test]
    fn never_roots_a_service_at_a_system_process_that_shares_its_socket() {
        let machine = Machine::new()
            .process(1, 0, "launchd", &[])
            .process(10, 1, "zsh", &["-zsh"])
            .process(20, 1, "node", &["node", "/srv/a.js"])
            .process(30, 1, "node", &["node", "/srv/b.js"])
            .sharing(1, 8000, 77)
            .sharing(20, 8000, 77);

        let services = machine.services();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].root.pid, Pid(20));
        assert_eq!(pids(&services[0].members), [20]);
        assert_eq!(pids(&services[0].holders), [1, 20]);
    }

    #[test]
    fn a_socket_only_the_system_holds_brings_in_nothing_else() {
        let machine = Machine::new()
            .process(1, 0, "launchd", &[])
            .process(20, 1, "node", &["node", "/srv/a.js"])
            .listening(1, 22);

        let services = machine.services();

        assert_eq!(services[0].root.pid, Pid(1));
        assert_eq!(pids(&services[0].members), [1]);
    }

    #[test]
    fn never_roots_a_service_at_the_users_shell_that_shares_its_socket() {
        let machine = Machine::new()
            .process(5, 1, "Terminal", &[])
            .process(10, 5, "zsh", &["-zsh"])
            .process(20, 10, "node", &["node", "/srv/a.js"])
            .sharing(10, 3000, 77)
            .sharing(20, 3000, 77);

        let services = machine.services();

        assert_eq!(services[0].root.pid, Pid(20));
        assert_eq!(pids(&services[0].members), [20]);
        assert_eq!(pids(&services[0].holders), [10, 20]);
    }

    #[test]
    fn climbs_into_a_titled_package_manager_through_its_script_shell() {
        let machine = Machine::new()
            .process(10, 1, "zsh", &["-zsh"])
            .process(20, 10, "node", &["npm run dev", "", ""])
            .process(30, 20, "sh", &["sh", "-c", "vite"])
            .process(40, 30, "node", &["node", "/app/node_modules/.bin/vite"])
            .listening(40, 5173);

        let services = machine.services();

        assert_eq!(services[0].root.pid, Pid(20));
        assert_eq!(pids(&services[0].members), [20, 30, 40]);
    }

    #[test]
    fn climbs_into_a_titled_package_manager_whose_shell_became_the_server() {
        // `sh -c "vite --host"` execs vite in place, so npm is its parent.
        let machine = Machine::new()
            .process(10, 1, "zsh", &["-zsh"])
            .process(20, 10, "node", &["npm run dev   ", "", ""])
            .process(
                30,
                20,
                "node",
                &["node", "/app/node_modules/.bin/vite", "--host"],
            )
            .listening(30, 5173);

        let services = machine.services();

        assert_eq!(services[0].root.pid, Pid(20));
        assert_eq!(pids(&services[0].members), [20, 30]);
    }

    #[test]
    fn goes_no_higher_than_a_runner_known_only_by_its_title() {
        let machine = Machine::new()
            .process(10, 1, "zsh", &["-zsh"])
            .process(20, 10, "node", &["node", NPM, "run", "dev"])
            .process(30, 20, "node", &["npm run dev"])
            .process(31, 20, "node", &["node", "/srv/worker.js"])
            .process(40, 30, "node", &["node", "/srv/server.js"])
            .listening(40, 4000);

        let services = machine.services();

        assert_eq!(services[0].root.pid, Pid(30));
        assert_eq!(pids(&services[0].members), [30, 40]);
    }

    #[test]
    fn never_climbs_into_a_plain_runtime_parent() {
        let machine = Machine::new()
            .process(10, 1, "zsh", &["-zsh"])
            .process(20, 10, "node", &["node", "/srv/supervisor.js"])
            .process(30, 20, "node", &["node", "/srv/server.js"])
            .listening(30, 4000);

        let services = machine.services();

        assert_eq!(services[0].root.pid, Pid(30));
        assert_eq!(pids(&services[0].members), [30]);
    }

    #[test]
    fn climbs_through_a_dev_server_that_supervises_its_worker() {
        let machine = Machine::new()
            .process(10, 1, "zsh", &["-zsh"])
            .process(20, 10, "node", &["node", NPM, "run", "dev"])
            .process(30, 20, "sh", &["sh", "-c", "next dev"])
            .process(
                40,
                30,
                "node",
                &["node", "/app/node_modules/next/dist/bin/next", "dev"],
            )
            .process(50, 40, "next-server", &["next-server (v16.0.1)"])
            .listening(50, 3000);

        let services = machine.services();

        assert_eq!(services[0].root.pid, Pid(20));
        assert_eq!(pids(&services[0].members), [20, 30, 40, 50]);
    }

    #[test]
    fn stops_at_a_script_shell_that_a_shell_started() {
        let machine = Machine::new()
            .process(10, 1, "zsh", &["-zsh"])
            .process(20, 10, "node", &["node", NPM, "run", "dev"])
            .process(30, 20, "sh", &["sh", "-c", "bash ./dev.sh"])
            .process(40, 30, "bash", &["bash", "./dev.sh"])
            .process(50, 40, "node", &["node", "/srv/server.js"])
            .listening(50, 4000);

        assert_eq!(machine.services()[0].root.pid, Pid(50));
    }

    #[test]
    fn leaves_interactive_shells_and_agents_below_a_server_out_of_it() {
        let machine = Machine::new()
            .process(10, 1, "zsh", &["-zsh"])
            .process(20, 10, "node", &["node", "/srv/server.js"])
            .process(30, 20, "zsh", &["-zsh"])
            .process(40, 20, "claude", &["claude"])
            .process(50, 20, "node", &["node", "/srv/worker.js"])
            .listening(20, 4000);

        assert_eq!(pids(&machine.services()[0].members), [20, 50]);
    }

    #[test]
    fn includes_the_go_toolchain_that_built_and_started_the_binary() {
        let machine = Machine::new()
            .process(1, 0, "zsh", &["-zsh"])
            .process(2, 1, "go", &["go", "run", "./cmd/gateway"])
            .process(
                3,
                2,
                "gateway",
                &["/var/folders/x/go-build123/b001/exe/gateway"],
            )
            .listening(3, 8080);

        let services = machine.services();

        assert_eq!(services[0].root.pid, Pid(2));
        assert_eq!(pids(&services[0].members), [2, 3]);
    }
}
