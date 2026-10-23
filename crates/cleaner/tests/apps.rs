#![cfg(target_os = "macos")]

mod common;

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use common::{MIB, machine, write};
use harbormaster_cleaner::{
    Finding, Group, Kind, Outcome, Reach, Usage, Why, apps, recheck, remove,
};

/// An app bundle called `name` in `folder`, its program taking `bytes`.
fn bundle(folder: &Path, name: &str, bytes: usize) {
    write(&folder.join(name).join("Contents/Info.plist"), b"<plist/>");
    write(
        &folder.join(name).join("Contents/MacOS/app"),
        &vec![1; bytes],
    );
}

fn listed(reach: &Reach, busy: &[(PathBuf, String)]) -> Vec<Finding> {
    apps(reach, busy, &AtomicBool::new(false), &|_| {}).expect("the apps should be listed")
}

fn names(found: &[Finding]) -> Vec<&str> {
    found.iter().map(|app| app.name.as_str()).collect()
}

#[test]
fn lists_the_users_own_apps_largest_first_and_picks_none() {
    let mine = machine();
    let shared = machine();
    bundle(&shared.home, "Big.app", 3 * MIB);
    bundle(&shared.home, "Utilities/Tool.app", MIB);
    bundle(&mine.home.join("Applications"), "Mine.app", 2 * MIB);
    let reach = Reach {
        home: &mine.home,
        shared: &shared.home,
    };

    let found = listed(&reach, &[]);

    assert_eq!(names(&found), ["Big", "Mine", "Tool"]);
    for app in &found {
        assert_eq!(app.group, Group::Applications);
        assert_eq!(app.kind, Kind::Personal);
        assert!(!app.picked && app.in_use.is_none() && !app.emptied);
    }
    assert!(found[0].bytes >= 3 * MIB as u64);
    assert_eq!(found[2].path, shared.home.join("Utilities/Tool.app"));
}

#[test]
fn leaves_out_links_deeper_apps_and_bundles_without_their_info() {
    let mine = machine();
    let shared = machine();
    let elsewhere = machine();
    bundle(&elsewhere.home, "Linked.app", MIB);
    symlink(
        elsewhere.home.join("Linked.app"),
        shared.home.join("Linked.app"),
    )
    .expect("a link");
    bundle(&shared.home, "a/b/Deep.app", MIB);
    write(
        &shared.home.join("Bare.app/Contents/MacOS/app"),
        &vec![1; MIB],
    );
    bundle(&elsewhere.home, "Real.app", MIB);
    fs::create_dir_all(shared.home.join("Faked.app/Contents")).expect("a folder");
    symlink(
        elsewhere.home.join("Real.app/Contents/Info.plist"),
        shared.home.join("Faked.app/Contents/Info.plist"),
    )
    .expect("a link");
    fs::create_dir_all(shared.home.join("Hollow.app/Contents/Info.plist")).expect("a folder");
    write(
        &shared.home.join("Hollow.app/Contents/MacOS/app"),
        &vec![1; MIB],
    );
    bundle(&shared.home, "Kept.app", MIB);
    let reach = Reach {
        home: &mine.home,
        shared: &shared.home,
    };

    assert_eq!(names(&listed(&reach, &[])), ["Kept"]);
}

#[test]
fn locks_an_app_something_runs_from() {
    let mine = machine();
    let shared = machine();
    bundle(&shared.home, "Busy.app", MIB);
    let program = shared.home.join("Busy.app/Contents/MacOS/app");
    let reach = Reach {
        home: &mine.home,
        shared: &shared.home,
    };

    let found = listed(&reach, &[(program, "Busy".into())]);

    assert_eq!(found[0].in_use.as_deref(), Some("Busy"));
}

#[test]
fn lists_the_home_folders_apps_when_there_is_no_shared_folder() {
    let mine = machine();
    bundle(&mine.home.join("Applications"), "Mine.app", MIB);
    let reach = Reach {
        home: &mine.home,
        shared: &mine.home.join("nowhere"),
    };

    assert_eq!(names(&listed(&reach, &[])), ["Mine"]);
}

fn nothing_used() -> io::Result<Usage> {
    Ok(Usage::default())
}

/// A stand-in for the system's Trash: moves what it is given into `bin`.
fn into(bin: &Path) -> impl Fn(&Path) -> io::Result<PathBuf> + '_ {
    move |path| {
        let to = bin.join(path.file_name().expect("a name"));
        fs::rename(path, &to)?;
        Ok(to)
    }
}

fn removed(reach: &Reach, finding: &Finding, bin: &Path) -> Outcome {
    let cleaned = remove(
        reach,
        std::slice::from_ref(finding),
        &nothing_used,
        &into(bin),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run");
    cleaned.removals[0].outcome.clone()
}

#[test]
fn moves_an_app_from_the_shared_folder_to_the_trash() {
    let (mine, shared, bin) = (machine(), machine(), machine());
    bundle(&shared.home, "Big.app", MIB);
    let reach = Reach {
        home: &mine.home,
        shared: &shared.home,
    };
    let app = listed(&reach, &[]).remove(0);

    assert_eq!(removed(&reach, &app, &bin.home), Outcome::Trashed);
    assert!(bin.home.join("Big.app/Contents/Info.plist").exists());
    assert!(!app.path.exists());
}

#[test]
fn never_deletes_from_the_shared_folder_nor_takes_anything_but_an_app_from_it() {
    let (mine, shared, bin) = (machine(), machine(), machine());
    bundle(&shared.home, "Big.app", MIB);
    let reach = Reach {
        home: &mine.home,
        shared: &shared.home,
    };
    let app = listed(&reach, &[]).remove(0);
    let deleted = Finding {
        kind: Kind::Rebuildable,
        ..app.clone()
    };
    let other = Finding {
        group: Group::Packages,
        ..app.clone()
    };

    for finding in [deleted, other] {
        assert_eq!(
            removed(&reach, &finding, &bin.home),
            Outcome::Refused {
                why: Why::OutsideHome
            }
        );
    }
    assert!(app.path.join("Contents/Info.plist").exists());
}

#[test]
fn looks_again_at_an_app_in_the_shared_folder() {
    let (mine, shared) = (machine(), machine());
    bundle(&shared.home, "Big.app", MIB);
    let reach = Reach {
        home: &mine.home,
        shared: &shared.home,
    };
    let app = listed(&reach, &[]).remove(0);
    let again = || recheck(&reach, std::slice::from_ref(&app), &Usage::default()).expect("a look");

    assert_eq!(again().len(), 1);

    fs::rename(&app.path, shared.home.join("Old.app")).expect("moved aside");
    bundle(&shared.home, "Big.app", MIB);
    assert!(again().is_empty());
}
