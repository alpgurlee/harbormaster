mod common;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use common::{DAY, Machine, age, link_folder, machine, node_project, reach, write};
#[cfg(target_os = "macos")]
use common::{MIB, set_mode};
use harbormaster_cleaner::{
    Cleaned, Finding, Group, Identity, Kind, Outcome, Survey, Usage, Why, remove, scan,
};

fn survey<'a>(home: &'a Path, roots: &'a [PathBuf], busy: &'a [(PathBuf, String)]) -> Survey<'a> {
    Survey {
        home,
        roots,
        busy,
        running: &[],
        idle_after: 30 * DAY,
        now: SystemTime::now(),
    }
}

/// The findings called `names`, as a scan of `survey` lists them.
fn chosen(survey: &Survey, names: &[&str]) -> Vec<Finding> {
    let findings = scan(survey, &AtomicBool::new(false)).expect("the scan should start");
    let chosen: Vec<Finding> = findings
        .iter()
        .filter(|finding| names.contains(&finding.name.as_str()))
        .cloned()
        .collect();
    assert_eq!(chosen.len(), names.len(), "{findings:?}");
    chosen
}

/// Scans `machine` and removes the findings called `names`, with `trash`
/// standing in for the system's Trash.
fn removed(
    machine: &Machine,
    names: &[&str],
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
) -> Cleaned {
    let roots = [machine.home.join("code")];
    let survey = survey(&machine.home, &roots, &[]);
    let chosen = chosen(&survey, names);
    remove(
        &reach(&machine.home),
        &chosen,
        &nothing_used,
        trash,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run")
}

fn nothing_used() -> io::Result<Usage> {
    Ok(Usage::default())
}

fn never(path: &Path) -> io::Result<PathBuf> {
    panic!("nothing should go to the Trash, yet {} did", path.display())
}

/// A stand-in for the system's Trash: moves what it is given into `bin`.
#[cfg(target_os = "macos")]
fn into(bin: &Path) -> impl Fn(&Path) -> io::Result<PathBuf> + '_ {
    move |path| {
        let to = bin.join(path.file_name().expect("a name"));
        fs::rename(path, &to)?;
        Ok(to)
    }
}

fn outcome<'a>(cleaned: &'a Cleaned, path: &Path) -> &'a Outcome {
    &cleaned
        .removals
        .iter()
        .find(|removal| removal.path == path)
        .expect("a removal for the path")
        .outcome
}

#[test]
fn deletes_a_build_output_folder_and_nothing_beside_it() {
    let machine = machine();
    let project = node_project(&machine, "old-shop");
    age(&project, 60 * DAY);

    let cleaned = removed(&machine, &["old-shop · node_modules"], &never);

    assert_eq!(
        outcome(&cleaned, &project.join("node_modules")),
        &Outcome::Deleted
    );
    assert!(!project.join("node_modules").exists());
    assert!(project.join("package.json").exists() && project.join(".next/cache/a").exists());
}

#[test]
#[cfg(target_os = "macos")]
fn empties_a_cache_but_keeps_its_folder() {
    let machine = machine();
    let cache = machine.home.join(".npm/_cacache");
    write(&cache.join("content-v2/a"), &vec![1; 2 * MIB]);
    write(&cache.join("index-v5/b"), b"x");

    let cleaned = removed(&machine, &["npm"], &never);

    assert_eq!(outcome(&cleaned, &cache), &Outcome::Deleted);
    assert!(
        fs::read_dir(&cache)
            .expect("the folder stays")
            .next()
            .is_none()
    );
}

#[test]
fn removes_a_link_inside_without_following_it() {
    let mine = machine();
    let elsewhere = machine();
    write(&elsewhere.home.join("kept/file"), b"keep me");
    let project = node_project(&mine, "linked");
    link_folder(
        &elsewhere.home.join("kept"),
        &project.join("node_modules/kept"),
    );
    age(&project, 60 * DAY);

    let cleaned = removed(&mine, &["linked · node_modules"], &never);

    assert_eq!(
        outcome(&cleaned, &project.join("node_modules")),
        &Outcome::Deleted
    );
    assert!(!project.join("node_modules").exists());
    assert!(elsewhere.home.join("kept/file").exists());
}

#[test]
fn deletes_read_only_files_along_with_the_rest() {
    let machine = machine();
    let project = node_project(&machine, "packed");
    let object = project.join("node_modules/left-pad/.git/objects/ab/cdef");
    write(&object, b"blob");
    let mut read_only = fs::metadata(&object).expect("the file").permissions();
    read_only.set_readonly(true);
    fs::set_permissions(&object, read_only).expect("read-only");
    age(&project, 60 * DAY);

    let cleaned = removed(&machine, &["packed · node_modules"], &never);

    assert_eq!(
        outcome(&cleaned, &project.join("node_modules")),
        &Outcome::Deleted
    );
    assert!(!project.join("node_modules").exists());
}

/// A program that opened a file without letting others delete it keeps it,
/// and with it its folder; the next finding goes all the same.
#[test]
#[cfg(windows)]
fn leaves_what_a_program_holds_shut_and_goes_on_with_the_rest() {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

    let machine = machine();
    let held = node_project(&machine, "held");
    let free = node_project(&machine, "free");
    age(&held, 60 * DAY);
    age(&free, 60 * DAY);
    let locked = held.join("node_modules/left-pad/index.js");
    let _open = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&locked)
        .expect("held open");

    let cleaned = removed(
        &machine,
        &["held · node_modules", "free · node_modules"],
        &never,
    );

    assert!(
        matches!(
            outcome(&cleaned, &held.join("node_modules")),
            Outcome::Failed { .. }
        ),
        "{cleaned:?}"
    );
    assert!(locked.exists());
    assert_eq!(
        outcome(&cleaned, &free.join("node_modules")),
        &Outcome::Deleted
    );
}

#[test]
fn refuses_what_a_process_is_using_by_then() {
    let machine = machine();
    let project = node_project(&machine, "busy");
    age(&project, 60 * DAY);
    let roots = [machine.home.join("code")];
    // Picked when nothing used it; looked at again right before it goes.
    let chosen = chosen(
        &survey(&machine.home, &roots, &[]),
        &["busy · node_modules"],
    );
    let busy = [(project.join("src/index.ts"), "code".to_string())];

    let cleaned = remove(
        &reach(&machine.home),
        &chosen,
        &|| {
            Ok(Usage {
                busy: busy.to_vec(),
                running: Vec::new(),
            })
        },
        &never,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");

    assert!(
        matches!(
            outcome(&cleaned, &project.join("node_modules")),
            Outcome::Refused { why: Why::InUse { by } } if by == "code"
        ),
        "{cleaned:?}"
    );
    assert!(project.join("node_modules/left-pad/index.js").exists());
}

#[test]
fn refuses_a_folder_swapped_for_a_link_since_the_scan() {
    let mine = machine();
    let elsewhere = machine();
    write(&elsewhere.home.join("kept/file"), b"keep me");
    let project = node_project(&mine, "swapped");
    age(&project, 60 * DAY);
    let roots = [mine.home.join("code")];
    let survey = survey(&mine.home, &roots, &[]);
    let chosen = chosen(&survey, &["swapped · node_modules"]);
    fs::remove_dir_all(project.join("node_modules")).expect("the folder");
    link_folder(&elsewhere.home.join("kept"), &project.join("node_modules"));

    let cleaned = remove(
        &reach(survey.home),
        &chosen,
        &nothing_used,
        &never,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");

    assert!(
        matches!(
            outcome(&cleaned, &project.join("node_modules")),
            Outcome::Refused {
                why: Why::NotReached { .. }
            }
        ),
        "{cleaned:?}"
    );
    assert!(elsewhere.home.join("kept/file").exists());
}

#[test]
fn refuses_a_folder_replaced_by_another_since_the_scan() {
    let machine = machine();
    let project = node_project(&machine, "replaced");
    age(&project, 60 * DAY);
    let roots = [machine.home.join("code")];
    let survey = survey(&machine.home, &roots, &[]);
    let chosen = chosen(&survey, &["replaced · node_modules"]);
    fs::rename(project.join("node_modules"), project.join("kept")).expect("moved aside");
    write(
        &project.join("node_modules/new/file"),
        b"not what was scanned",
    );

    let cleaned = remove(
        &reach(survey.home),
        &chosen,
        &nothing_used,
        &never,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");

    assert!(
        matches!(
            outcome(&cleaned, &project.join("node_modules")),
            Outcome::Refused { why: Why::Changed }
        ),
        "{cleaned:?}"
    );
    assert!(project.join("node_modules/new/file").exists());
    assert!(project.join("kept/left-pad/index.js").exists());
}

#[test]
fn refuses_anything_outside_the_home_folder() {
    let mine = machine();
    let elsewhere = machine();
    write(&elsewhere.home.join("code/far/node_modules/a"), b"x");
    let finding = Finding {
        group: Group::Projects,
        kind: Kind::Rebuildable,
        name: "far · node_modules".into(),
        path: elsewhere.home.join("code/far/node_modules"),
        bytes: 1,
        picked: true,
        in_use: None,
        touched: None,
        project: Some(elsewhere.home.join("code/far")),
        emptied: false,
        identity: Identity::default(),
    };

    let cleaned = remove(
        &reach(&mine.home),
        &[finding],
        &nothing_used,
        &never,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");

    assert!(
        matches!(
            &cleaned.removals[0].outcome,
            Outcome::Refused {
                why: Why::OutsideHome
            }
        ),
        "{cleaned:?}"
    );
    assert!(elsewhere.home.join("code/far/node_modules/a").exists());
}

#[test]
#[cfg(target_os = "macos")]
fn moves_a_personal_item_to_the_trash_whole() {
    let machine = machine();
    let archive = machine
        .home
        .join("Library/Developer/Xcode/Archives/2026-09-20");
    write(&archive.join("App.xcarchive/Info.plist"), &vec![1; 2 * MIB]);
    let bin = machine.home.join("bin");
    fs::create_dir(&bin).expect("a stand-in for the Trash");

    let cleaned = removed(&machine, &["2026-09-20"], &into(&bin));

    assert_eq!(outcome(&cleaned, &archive), &Outcome::Trashed);
    assert!(bin.join("2026-09-20/App.xcarchive/Info.plist").exists());
    assert!(!archive.exists());
}

#[test]
#[cfg(target_os = "macos")]
fn moves_a_personal_place_to_the_trash_whole() {
    let machine = machine();
    let caches = machine.home.join("Library/Caches/JetBrains");
    write(
        &caches.join("IntelliJIdea2025.1/LocalHistory/a"),
        &vec![1; 2 * MIB],
    );
    let bin = machine.home.join("bin");
    fs::create_dir(&bin).expect("a stand-in for the Trash");

    let cleaned = removed(&machine, &["JetBrains IDEs"], &into(&bin));

    assert_eq!(outcome(&cleaned, &caches), &Outcome::Trashed);
    assert!(
        bin.join("JetBrains/IntelliJIdea2025.1/LocalHistory/a")
            .exists()
    );
    assert!(!caches.exists());
}

#[test]
#[cfg(target_os = "macos")]
fn tells_when_the_trash_took_something_other_than_what_was_scanned() {
    let mine = machine();
    let elsewhere = machine();
    let archives = mine.home.join("Library/Developer/Xcode/Archives");
    write(&archives.join("2026-09-20/a"), &vec![1; 2 * MIB]);
    let decoys = elsewhere.home.join("decoys");
    write(&decoys.join("2026-09-20/keep"), b"not what was scanned");
    let bin = mine.home.join("bin");
    fs::create_dir(&bin).expect("a stand-in for the Trash");
    let aside = mine.home.join("Library/Developer/Xcode/Aside");
    // The folder holding the archive turns into a link elsewhere just
    // before the Trash looks the path up.
    let racing = |path: &Path| {
        fs::rename(&archives, &aside)?;
        std::os::unix::fs::symlink(&decoys, &archives)?;
        into(&bin)(path)
    };

    let cleaned = removed(&mine, &["2026-09-20"], &racing);

    assert!(
        matches!(
            outcome(&cleaned, &archives.join("2026-09-20")),
            Outcome::Failed { why } if why.contains("never saw")
        ),
        "{cleaned:?}"
    );
    assert!(aside.join("2026-09-20/a").exists());
    assert!(bin.join("2026-09-20/keep").exists());
}

#[test]
#[cfg(target_os = "macos")]
fn empties_the_trash_before_moving_anything_into_it() {
    let machine = machine();
    let trash = machine.home.join(".Trash");
    write(&trash.join("old/a"), &vec![1; 2 * MIB]);
    let archive = machine
        .home
        .join("Library/Developer/Xcode/Archives/2026-09-20");
    write(&archive.join("a"), &vec![1; 2 * MIB]);

    let cleaned = removed(&machine, &["2026-09-20", "Trash"], &into(&trash));

    assert_eq!(outcome(&cleaned, &trash), &Outcome::Deleted);
    assert_eq!(outcome(&cleaned, &archive), &Outcome::Trashed);
    assert!(trash.join("2026-09-20/a").exists());
    assert!(!trash.join("old").exists());
}

#[test]
#[cfg(target_os = "macos")]
fn goes_on_with_the_rest_when_one_fails_and_tells_of_each() {
    let machine = machine();
    let cache = machine.home.join(".npm/_cacache");
    write(&cache.join("open/a"), &vec![1; 2 * MIB]);
    let blocked = cache.join("blocked");
    write(&blocked.join("b"), b"x");
    let project = node_project(&machine, "fine");
    age(&project, 60 * DAY);
    let roots = [machine.home.join("code")];
    let survey = survey(&machine.home, &roots, &[]);
    let mut chosen = chosen(&survey, &["fine · node_modules", "npm"]);
    // The one that fails goes first, so going on after it shows.
    chosen.sort_by_key(|finding| finding.path != cache);
    let locked = set_mode(&blocked, 0o000);

    let mut told = Vec::new();
    let cleaned = remove(
        &reach(survey.home),
        &chosen,
        &nothing_used,
        &never,
        &AtomicBool::new(false),
        &mut |removal| told.push(removal.path.clone()),
    );
    drop(locked);
    let cleaned = cleaned.expect("removal should run");

    assert!(
        matches!(outcome(&cleaned, &cache), Outcome::Failed { .. }),
        "{cleaned:?}"
    );
    assert_eq!(
        outcome(&cleaned, &project.join("node_modules")),
        &Outcome::Deleted
    );
    assert_eq!(told, [cache, project.join("node_modules")]);
    assert!(blocked.join("b").exists());
}

#[test]
fn removes_nothing_once_told_to_stop() {
    let machine = machine();
    let project = node_project(&machine, "stopped");
    age(&project, 60 * DAY);
    let roots = [machine.home.join("code")];
    let survey = survey(&machine.home, &roots, &[]);
    let chosen = chosen(&survey, &["stopped · node_modules", "stopped · .next"]);

    let cleaned = remove(
        &reach(survey.home),
        &chosen,
        &nothing_used,
        &never,
        &AtomicBool::new(true),
        &mut |_| {},
    )
    .expect("removal should run");

    assert!(
        cleaned
            .removals
            .iter()
            .all(|removal| removal.outcome == Outcome::Stopped),
        "{cleaned:?}"
    );
    assert!(project.join("node_modules/left-pad/index.js").exists());
    assert!(project.join(".next/cache/a").exists());
}

#[test]
fn reads_what_is_in_use_again_before_each() {
    let machine = machine();
    let first = node_project(&machine, "first");
    let second = node_project(&machine, "second");
    age(&first, 60 * DAY);
    age(&second, 60 * DAY);
    let roots = [machine.home.join("code")];
    let survey = survey(&machine.home, &roots, &[]);
    let mut chosen = chosen(&survey, &["first · node_modules", "second · node_modules"]);
    chosen.sort_by(|a, b| a.path.cmp(&b.path));
    let reads = std::cell::Cell::new(0);
    // A dev server starts in the second project while the first one's output goes.
    let usage = || {
        reads.set(reads.get() + 1);
        let busy = if reads.get() == 1 {
            Vec::new()
        } else {
            vec![(second.join("server.js"), "node".to_string())]
        };
        Ok(Usage {
            busy,
            running: Vec::new(),
        })
    };

    let cleaned = remove(
        &reach(&machine.home),
        &chosen,
        &usage,
        &never,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");

    assert_eq!(
        outcome(&cleaned, &first.join("node_modules")),
        &Outcome::Deleted
    );
    assert!(
        matches!(
            outcome(&cleaned, &second.join("node_modules")),
            Outcome::Refused { why: Why::InUse { by } } if by == "node"
        ),
        "{cleaned:?}"
    );
    assert!(second.join("node_modules/left-pad/index.js").exists());
}

#[test]
fn leaves_alone_what_it_cant_tell_is_free() {
    let machine = machine();
    let project = node_project(&machine, "unknown");
    age(&project, 60 * DAY);
    let roots = [machine.home.join("code")];
    let survey = survey(&machine.home, &roots, &[]);
    let chosen = chosen(&survey, &["unknown · node_modules"]);

    let cleaned = remove(
        &reach(&machine.home),
        &chosen,
        &|| Err(io::Error::other("the process list couldn't be read")),
        &never,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");

    assert!(
        matches!(
            outcome(&cleaned, &project.join("node_modules")),
            Outcome::Refused {
                why: Why::Unchecked { .. }
            }
        ),
        "{cleaned:?}"
    );
    assert!(project.join("node_modules/left-pad/index.js").exists());
}
