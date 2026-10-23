#![cfg(windows)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use common::{DAY, MIB, NPM, age, link_folder, machine, node_project, reach, write};
use harbormaster_cleaner::{
    Finding, Group, Outcome, Survey, Usage, Why, remove, scan, usual_roots,
};

fn scanned(home: &Path, roots: &[PathBuf]) -> Vec<Finding> {
    scanned_while(home, roots, &[])
}

fn scanned_while(home: &Path, roots: &[PathBuf], busy: &[(PathBuf, String)]) -> Vec<Finding> {
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
fn finds_the_caches_windows_tools_keep_in_the_profile() {
    let machine = machine();
    write(
        &machine.home.join(NPM).join("content-v2/a"),
        &vec![1; 2 * MIB],
    );
    let pnpm = machine.home.join(r"AppData\Local\pnpm\store");
    write(&pnpm.join("v10/files/00/a"), &vec![1; 2 * MIB]);
    // pnpm's own commands sit beside its store and stay.
    write(
        &machine.home.join(r"AppData\Local\pnpm\pnpm.exe"),
        &vec![1; 2 * MIB],
    );
    write(
        &machine.home.join(r".cargo\registry\cache\index\a.crate"),
        &vec![1; 2 * MIB],
    );

    let findings = scanned(&machine.home, &[]);

    let npm = named(&findings, "npm").expect("npm's cache");
    assert_eq!(npm.path, machine.home.join(NPM));
    assert_eq!(
        (npm.group, npm.picked, npm.emptied),
        (Group::Packages, true, true)
    );
    assert_eq!(named(&findings, "pnpm").expect("pnpm's store").path, pnpm);
    assert!(named(&findings, "Cargo").is_some(), "{findings:?}");
    assert_eq!(findings.len(), 3, "{findings:?}");
}

#[test]
fn never_lists_a_cache_reached_through_a_junction() {
    let mine = machine();
    let elsewhere = machine();
    write(
        &elsewhere.home.join(r"npm-cache\_cacache\a"),
        &vec![1; 2 * MIB],
    );
    std::fs::create_dir_all(mine.home.join(r"AppData\Local")).expect("the folder");
    link_folder(
        &elsewhere.home.join("npm-cache"),
        &mine.home.join(r"AppData\Local\npm-cache"),
    );

    let findings = scanned(&mine.home, &[]);

    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn looks_for_projects_where_visual_studio_and_github_desktop_put_them() {
    let machine = machine();
    for folder in [r"source\repos", r"Documents\GitHub"] {
        let project = node_project(&machine, "app");
        let moved = machine.home.join(folder).join("app");
        std::fs::create_dir_all(moved.parent().expect("a parent")).expect("the folder");
        std::fs::rename(&project, &moved).expect("moved");
        age(&moved, 60 * DAY);
        let roots = usual_roots(&machine.home);

        let findings = scanned(&machine.home, &roots);

        let output = named(&findings, "app · node_modules").expect("the project's output");
        assert_eq!(output.path, moved.join("node_modules"));
        std::fs::remove_dir_all(machine.home.join(folder)).expect("cleared");
    }
}

/// What a tool has open can't be read on Windows, so its running is taken
/// as its using its own cache.
#[test]
fn takes_a_cache_to_be_in_use_while_its_tool_runs() {
    let machine = machine();
    let registry = machine.home.join(r".cargo\registry\cache");
    write(&registry.join(r"index\a.crate"), &vec![1; 2 * MIB]);
    write(&machine.home.join(NPM).join("a"), &vec![1; 2 * MIB]);
    let cargo = [(PathBuf::from(r"C:\Rust\bin\cargo.exe"), "Cargo".to_string())];

    let findings = scanned_while(&machine.home, &[], &cargo);

    let registry = named(&findings, "Cargo").expect("Cargo's cache");
    assert_eq!(
        (registry.picked, registry.in_use.as_deref()),
        (false, Some("Cargo"))
    );
    let npm = named(&findings, "npm").expect("npm's cache");
    assert_eq!((npm.picked, npm.in_use.as_deref()), (true, None));

    let cleaned = remove(
        &reach(&machine.home),
        std::slice::from_ref(registry),
        &|| {
            Ok(Usage {
                busy: cargo.to_vec(),
                running: Vec::new(),
            })
        },
        &|_| panic!("nothing goes to the Recycle Bin"),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");

    assert!(
        matches!(
            &cleaned.removals[0].outcome,
            Outcome::Refused { why: Why::InUse { by } } if by == "Cargo"
        ),
        "{cleaned:?}"
    );
    assert!(
        machine
            .home
            .join(r".cargo\registry\cache\index\a.crate")
            .exists()
    );
}
