//! The engine wired to this machine, so the CLI and the app check the same
//! things before stopping anything.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use harbormaster_platform::{Error, ProcessKey};

use crate::{Input, Scanner, Snapshot, Standing, standings};

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

    use super::lineage_from;

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
