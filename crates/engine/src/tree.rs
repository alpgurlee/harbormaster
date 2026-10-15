use std::collections::{HashMap, HashSet, VecDeque};

use harbormaster_platform::{Pid, ProcessEntry};

/// Parent and child links between processes, ignoring links the OS keeps
/// around after a parent exits and its pid is reused.
#[derive(Debug)]
pub(crate) struct ProcessTree<'a> {
    by_pid: HashMap<Pid, &'a ProcessEntry>,
    children: HashMap<Pid, Vec<Pid>>,
}

impl<'a> ProcessTree<'a> {
    pub(crate) fn new(processes: &'a [ProcessEntry]) -> Self {
        let by_pid: HashMap<Pid, &ProcessEntry> = processes
            .iter()
            .map(|process| (process.key.pid, process))
            .collect();
        let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for process in processes {
            if let Some(parent) = process.parent.and_then(|pid| by_pid.get(&pid))
                && is_parent_of(parent, process)
            {
                children
                    .entry(parent.key.pid)
                    .or_default()
                    .push(process.key.pid);
            }
        }
        for list in children.values_mut() {
            list.sort_unstable();
        }
        ProcessTree { by_pid, children }
    }

    pub(crate) fn get(&self, pid: Pid) -> Option<&'a ProcessEntry> {
        self.by_pid.get(&pid).copied()
    }

    pub(crate) fn parent(&self, pid: Pid) -> Option<&'a ProcessEntry> {
        let child = self.get(pid)?;
        let parent = self.get(child.parent?)?;
        is_parent_of(parent, child).then_some(parent)
    }

    /// `root` and everything below it, breadth first.
    pub(crate) fn subtree(&self, root: Pid) -> Vec<&'a ProcessEntry> {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([root]);
        let mut found = Vec::new();
        while let Some(pid) = queue.pop_front() {
            if !seen.insert(pid) {
                continue;
            }
            let Some(process) = self.get(pid) else {
                continue;
            };
            found.push(process);
            queue.extend(self.children.get(&pid).into_iter().flatten().copied());
        }
        found
    }

    pub(crate) fn children(&self, pid: Pid) -> impl Iterator<Item = &'a ProcessEntry> + '_ {
        self.children
            .get(&pid)
            .into_iter()
            .flatten()
            .filter_map(|child| self.get(*child))
    }
}

/// A recorded parent only counts if it already existed when the child started.
fn is_parent_of(parent: &ProcessEntry, child: &ProcessEntry) -> bool {
    parent.key.pid != child.key.pid && parent.key.start <= child.key.start
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Duration;

    use harbormaster_platform::{Owner, Pid, ProcessEntry, ProcessKey, StartTime};

    use super::ProcessTree;

    pub(crate) fn entry(pid: u32, parent: Option<u32>, start: u64, name: &str) -> ProcessEntry {
        ProcessEntry {
            key: ProcessKey {
                pid: Pid(pid),
                start: StartTime(start),
                exec: 0,
            },
            parent: parent.map(Pid),
            name: name.to_string(),
            owner: Owner::CurrentUser,
            memory_bytes: Some(1_000),
            cpu_time: Some(Duration::ZERO),
        }
    }

    #[test]
    fn follows_a_parent_that_started_before_the_child() {
        let processes = [
            entry(10, Some(1), 100, "zsh"),
            entry(20, Some(10), 200, "node"),
        ];
        let tree = ProcessTree::new(&processes);

        assert_eq!(tree.parent(Pid(20)).map(|p| p.key.pid), Some(Pid(10)));
    }

    #[test]
    fn ignores_a_parent_whose_pid_was_reused_after_the_child_started() {
        let processes = [
            entry(10, Some(1), 900, "Safari"),
            entry(20, Some(10), 200, "node"),
        ];
        let tree = ProcessTree::new(&processes);

        assert!(tree.parent(Pid(20)).is_none());
        assert_eq!(tree.children(Pid(10)).count(), 0);
    }

    #[test]
    fn subtree_lists_each_descendant_once_even_if_links_form_a_loop() {
        let processes = [
            entry(1, Some(3), 100, "a"),
            entry(2, Some(1), 100, "b"),
            entry(3, Some(2), 100, "c"),
        ];
        let tree = ProcessTree::new(&processes);

        let pids: Vec<u32> = tree.subtree(Pid(1)).iter().map(|p| p.key.pid.0).collect();

        assert_eq!(pids, [1, 2, 3]);
    }
}
