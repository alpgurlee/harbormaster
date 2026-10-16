mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use common::{DAY, MIB, Machine, TAG, age, machine, node_project, write};
use harbormaster_cleaner::{Finding, Group, Survey, scan};

fn survey(
    machine: &Machine,
    busy: &[(PathBuf, String)],
    running: &[(PathBuf, String)],
) -> Vec<Finding> {
    let roots = [machine.home.join("code")];
    surveyed(&machine.home, &roots, busy, running)
}

fn surveyed(
    home: &Path,
    roots: &[PathBuf],
    busy: &[(PathBuf, String)],
    running: &[(PathBuf, String)],
) -> Vec<Finding> {
    let survey = Survey {
        home,
        roots,
        busy,
        running,
        idle_after: 30 * DAY,
        now: SystemTime::now(),
    };
    scan(&survey, &AtomicBool::new(false)).expect("the scan should start")
}

fn named<'a>(findings: &'a [Finding], name: &str) -> Option<&'a Finding> {
    findings.iter().find(|finding| finding.name == name)
}

#[test]
fn picks_the_build_outputs_of_a_project_left_alone_for_a_month() {
    let machine = machine();
    let project = node_project(&machine, "old-shop");
    age(&project, 60 * DAY);

    let findings = survey(&machine, &[], &[]);

    let modules = named(&findings, "old-shop · node_modules").unwrap();
    assert_eq!((modules.group, modules.picked), (Group::Projects, true));
    assert_eq!(modules.path, project.join("node_modules"));
    assert!(modules.touched.is_some());
    assert!(!modules.emptied);
    assert!(named(&findings, "old-shop · .next").unwrap().picked);
}

#[test]
fn lists_a_recent_project_without_picking_it() {
    let machine = machine();
    node_project(&machine, "fresh");

    let findings = survey(&machine, &[], &[]);

    assert!(!named(&findings, "fresh · node_modules").unwrap().picked);
}

#[test]
fn takes_a_rust_target_only_when_it_says_it_is_a_cache() {
    let machine = machine();
    let tagged = machine.home.join("code/tagged");
    write(&tagged.join("Cargo.toml"), b"[package]");
    write(&tagged.join("target/CACHEDIR.TAG"), TAG);
    write(&tagged.join("target/debug/app"), &vec![1; 2 * MIB]);
    let untagged = machine.home.join("code/untagged");
    write(&untagged.join("Cargo.toml"), b"[package]");
    write(&untagged.join("target/debug/app"), &vec![1; 2 * MIB]);

    let findings = survey(&machine, &[], &[]);

    assert!(named(&findings, "tagged · target").is_some());
    assert!(named(&findings, "untagged · target").is_none());
}

#[test]
fn never_picks_what_a_process_is_using() {
    let machine = machine();
    let project = node_project(&machine, "busy");
    age(&project, 60 * DAY);
    write(&machine.home.join(".npm/_cacache/a"), &vec![1; 2 * MIB]);
    let busy = [
        (project.join("src/index.ts"), "code".to_string()),
        (machine.home.join(".npm/_cacache/a"), "node".to_string()),
    ];

    let findings = survey(&machine, &busy, &[]);

    let modules = named(&findings, "busy · node_modules").unwrap();
    assert_eq!(
        (modules.picked, modules.in_use.as_deref()),
        (false, Some("code"))
    );
    let npm = named(&findings, "npm").unwrap();
    assert_eq!((npm.picked, npm.in_use.as_deref()), (false, Some("node")));
}

#[test]
fn never_picks_a_project_with_a_running_dev_server() {
    let machine = machine();
    let project = node_project(&machine, "serving");
    age(&project, 60 * DAY);

    let running = [(project, "the server on :3000".to_string())];

    let findings = survey(&machine, &[], &running);

    let modules = named(&findings, "serving · node_modules").unwrap();
    assert_eq!(
        (modules.picked, modules.in_use.as_deref()),
        (false, Some("the server on :3000"))
    );
}

#[test]
fn counts_git_activity_as_touching_the_project() {
    let machine = machine();
    let project = node_project(&machine, "committed");
    write(&project.join(".git/index"), b"DIRC");
    age(&project, 60 * DAY);

    let findings = survey(&machine, &[], &[]);

    assert!(!named(&findings, "committed · node_modules").unwrap().picked);
}

#[test]
fn takes_any_folder_that_says_it_is_a_cache() {
    let machine = machine();
    let api = machine.home.join("code/api");
    write(&api.join("pyproject.toml"), b"[project]");
    write(&api.join(".venv/CACHEDIR.TAG"), TAG);
    write(&api.join(".venv/lib/a"), &vec![1; 2 * MIB]);
    write(&api.join(".mypy_cache/a"), &vec![1; 2 * MIB]);

    let findings = survey(&machine, &[], &[]);

    assert!(named(&findings, "api · .venv").is_some(), "{findings:?}");
    assert!(
        named(&findings, "api · .mypy_cache").is_none(),
        "{findings:?}"
    );
}

#[test]
fn looks_only_inside_the_home_folder() {
    let mine = machine();
    let elsewhere = machine();
    let project = elsewhere.home.join("code/far");
    write(&project.join("package.json"), b"{}");
    write(&project.join("node_modules/a"), &vec![1; 2 * MIB]);
    age(&project, 60 * DAY);

    let findings = surveyed(&mine.home, &[elsewhere.home.join("code")], &[], &[]);

    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn never_looks_for_projects_inside_node_modules() {
    let machine = machine();
    let project = node_project(&machine, "app");
    write(&project.join("node_modules/left-pad/package.json"), b"{}");
    write(
        &project.join("node_modules/left-pad/node_modules/x/a"),
        &vec![1; 2 * MIB],
    );

    let findings = survey(&machine, &[], &[]);

    assert!(
        named(&findings, "left-pad · node_modules").is_none(),
        "{findings:?}"
    );
}

#[test]
fn lists_an_output_once_under_nested_roots() {
    let machine = machine();
    let project = node_project(&machine, "app");
    let roots = [machine.home.join("code"), project];

    let findings = surveyed(&machine.home, &roots, &[], &[]);

    let listed = findings
        .iter()
        .filter(|finding| finding.name == "app · node_modules")
        .count();
    assert_eq!(listed, 1, "{findings:?}");
}

#[test]
fn spells_a_project_as_the_disk_does_so_what_uses_it_is_found() {
    let machine = machine();
    let project = machine.home.join("Code/app");
    write(&project.join("package.json"), b"{}");
    write(&project.join("node_modules/a"), &vec![1; 2 * MIB]);
    age(&project, 60 * DAY);
    // Only a disk that ignores case can reach the project through a root
    // spelled differently; processes always report the disk's spelling.
    let roots = [machine.home.join("code")];
    if !roots[0].is_dir() {
        return;
    }
    let busy = [(project.join("src/index.ts"), "code".to_string())];

    let findings = surveyed(&machine.home, &roots, &busy, &[]);

    let modules = named(&findings, "app · node_modules").unwrap();
    assert_eq!(modules.path, project.join("node_modules"));
    assert_eq!(
        (modules.picked, modules.in_use.as_deref()),
        (false, Some("code"))
    );
}

#[test]
fn finds_a_deep_project_under_nested_roots_whichever_comes_first() {
    let machine = machine();
    let project = node_project(&machine, "a/b/c/d/e/app");
    let output = project.join("node_modules");
    let mut roots = [machine.home.join("code"), machine.home.join("code/a")];

    for _ in 0..2 {
        let findings = surveyed(&machine.home, &roots, &[], &[]);
        let listed = findings
            .iter()
            .filter(|finding| finding.path == output)
            .count();
        assert_eq!(listed, 1, "{roots:?} {findings:?}");
        roots.reverse();
    }
}
