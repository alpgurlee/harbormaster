mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use common::{DAY, machine, node_project, reach, write};
use harbormaster_cleaner::{Finding, Survey, Usage, recheck, scan};

fn scanned(home: &Path, roots: &[PathBuf], busy: &[(PathBuf, String)]) -> Vec<Finding> {
    let survey = Survey {
        home,
        roots,
        busy,
        running: &[],
        idle_after: 30 * DAY,
        now: SystemTime::now(),
    };
    scan(&survey, &AtomicBool::new(false)).expect("the scan should start")
}

fn named<'a>(findings: &'a [Finding], name: &str) -> Option<&'a Finding> {
    findings.iter().find(|finding| finding.name == name)
}

#[test]
fn leaves_out_what_went_or_was_made_again_since_the_scan() {
    let machine = machine();
    node_project(&machine, "kept");
    let gone = node_project(&machine, "gone");
    let again = node_project(&machine, "again");
    let roots = [machine.home.join("code")];
    let findings = scanned(&machine.home, &roots, &[]);
    fs::remove_dir_all(gone.join("node_modules")).expect("removed by hand");
    // Moved aside rather than removed, so the new folder can't get its inode.
    fs::rename(again.join("node_modules"), again.join("node_modules.old"))
        .expect("moved aside by hand");
    write(&again.join("node_modules/left-pad/index.js"), b"x");

    let now = recheck(&reach(&machine.home), &findings, &Usage::default())
        .expect("the recheck should run");

    assert!(named(&now, "gone · node_modules").is_none());
    assert!(named(&now, "again · node_modules").is_none());
    let kept = named(&now, "kept · node_modules").expect("still there");
    assert_eq!(Some(kept), named(&findings, "kept · node_modules"));
    assert_eq!(now.len(), findings.len() - 2);
}

#[test]
fn reads_again_what_is_using_each() {
    let machine = machine();
    let freed = node_project(&machine, "freed");
    let held = node_project(&machine, "held");
    let roots = [machine.home.join("code")];
    let findings = scanned(
        &machine.home,
        &roots,
        &[(freed.join("src/index.ts"), "code".into())],
    );
    assert!(
        named(&findings, "freed · node_modules").is_some_and(|finding| finding.in_use.is_some())
    );
    let usage = Usage {
        busy: vec![(held.join("node_modules/left-pad/index.js"), "java".into())],
        running: Vec::new(),
    };

    let now = recheck(&reach(&machine.home), &findings, &usage).expect("the recheck should run");

    let in_use = |name| named(&now, name).expect(name).in_use.as_deref();
    assert_eq!(in_use("freed · node_modules"), None);
    assert_eq!(in_use("held · node_modules"), Some("java"));
}
