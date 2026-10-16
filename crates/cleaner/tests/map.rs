mod common;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use common::{MIB, machine, write};
use harbormaster_cleaner::{Area, Chart, Short, map, still_there};

fn mapped(home: &Path, others: bool) -> Area {
    let chart = Chart { home, others };
    map(&chart, &AtomicBool::new(false), &mut |_| {}).expect("the map should start")
}

/// The area at `names` down from `area`.
fn at<'a>(area: &'a Area, names: &[&str]) -> &'a Area {
    names.iter().fold(area, |area, name| {
        area.inside
            .iter()
            .find(|each| each.name == *name)
            .unwrap_or_else(|| panic!("{name} in {:?}", area.inside))
    })
}

/// Every area holds at least what it shows and what it counts together.
fn adds_up(area: &Area) {
    let shown: u64 = area.inside.iter().map(|each| each.bytes).sum();
    assert!(area.bytes >= shown + area.rest, "{area:?}");
    assert!(
        area.inside
            .windows(2)
            .all(|pair| pair[0].bytes >= pair[1].bytes),
        "largest first: {:?}",
        area.inside
    );
    area.inside.iter().for_each(adds_up);
}

#[test]
fn maps_what_each_folder_holds_largest_first() {
    let machine = machine();
    let home = &machine.home;
    write(&home.join("Movies/trip.mov"), &vec![1; 3 * MIB]);
    write(&home.join("Movies/notes.txt"), b"small");
    write(&home.join("Library/Caches/app/blob"), &vec![1; 2 * MIB]);
    write(&home.join(".hidden/thing"), &vec![1; 2 * MIB]);
    write(
        &home.join("code/shop/node_modules/pkg/index.js"),
        &vec![1; 2 * MIB],
    );
    write(&home.join("Big.app/Contents/binary"), &vec![1; 2 * MIB]);

    let root = mapped(home, false);

    adds_up(&root);
    assert!(root.folder && root.bytes >= 11 * MIB as u64, "{root:?}");
    assert_eq!(root.short, None);
    assert_eq!(root.path, *home);
    let movies = at(&root, &["Movies"]);
    assert_eq!(movies.inside.len(), 1);
    assert_eq!(movies.inside[0].name, "trip.mov");
    assert!(!movies.inside[0].folder && movies.inside[0].bytes >= 3 * MIB as u64);
    // The small file is counted, just not shown on its own.
    assert!(movies.rest > 0);
    // Unlike the large files, the map counts everything: the Library,
    // hidden folders, packages and node_modules too.
    for names in [
        &["Library", "Caches", "app", "blob"][..],
        &[".hidden", "thing"],
        &["code", "shop", "node_modules", "pkg", "index.js"],
        &["Big.app", "Contents", "binary"],
    ] {
        assert!(at(&root, names).bytes >= 2 * MIB as u64, "{names:?}");
    }
    // Each area has its own number, the root first.
    assert_eq!(root.id, 0);
    assert_ne!(movies.id, movies.inside[0].id);
}

#[test]
fn never_follows_a_link_and_counts_a_file_with_more_names_once() {
    let mine = machine();
    let elsewhere = machine();
    write(&elsewhere.home.join("films/big.mov"), &vec![1; 4 * MIB]);
    fs::create_dir_all(mine.home.join("Movies")).expect("a folder");
    symlink(elsewhere.home.join("films"), mine.home.join("Movies/films")).expect("a link");
    write(&mine.home.join("Movies/a.mov"), &vec![1; 2 * MIB]);
    fs::hard_link(
        mine.home.join("Movies/a.mov"),
        mine.home.join("Movies/b.mov"),
    )
    .expect("a link");

    let root = mapped(&mine.home, false);

    assert!(root.bytes < 4 * MIB as u64, "{root:?}");
}

#[test]
fn leaves_other_apps_data_out_unless_asked() {
    let machine = machine();
    write(
        &machine
            .home
            .join("Library/Containers/com.example.app/Data/blob"),
        &vec![1; 2 * MIB],
    );
    write(
        &machine
            .home
            .join("Library/Group Containers/group.example/blob"),
        &vec![1; 2 * MIB],
    );

    let without = mapped(&machine.home, false);
    for name in ["Containers", "Group Containers"] {
        let skipped = at(&without, &["Library", name]);
        assert_eq!(skipped.short, Some(Short::Skipped));
        assert!(skipped.bytes < MIB as u64);
    }
    assert_eq!(without.short, Some(Short::Skipped));

    let with = mapped(&machine.home, true);
    assert!(at(&with, &["Library", "Containers"]).bytes >= 2 * MIB as u64);
    assert_eq!(with.short, None);
}

#[test]
fn says_where_it_could_not_look() {
    let machine = machine();
    let locked = machine.home.join("Documents/locked");
    write(&locked.join("secret"), &vec![1; 2 * MIB]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("locked");

    let root = mapped(&machine.home, false);

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("unlocked");
    assert_eq!(
        at(&root, &["Documents", "locked"]).short,
        Some(Short::Denied)
    );
    assert_eq!(at(&root, &["Documents"]).short, Some(Short::Denied));
    assert_eq!(root.short, Some(Short::Denied));
}

#[test]
fn keeps_what_it_measured_when_told_to_stop() {
    let machine = machine();
    write(&machine.home.join("Movies/trip.mov"), &vec![1; 2 * MIB]);
    let chart = Chart {
        home: &machine.home,
        others: false,
    };

    let root = map(&chart, &AtomicBool::new(true), &mut |_| {}).expect("the map should start");

    assert_eq!(root.short, Some(Short::Stopped));
}

#[test]
fn shows_only_the_largest_of_many_and_counts_the_rest_together() {
    let machine = machine();
    for index in 0..60 {
        write(
            &machine.home.join(format!("Downloads/file-{index:02}.bin")),
            &vec![1; MIB + index * 4096],
        );
    }

    let root = mapped(&machine.home, false);

    adds_up(&root);
    let downloads = at(&root, &["Downloads"]);
    assert_eq!(downloads.inside.len(), 50);
    assert_eq!(downloads.inside[0].name, "file-59.bin");
    assert!(downloads.rest >= 10 * MIB as u64);
}

#[test]
fn knows_a_place_is_still_there_only_as_the_map_found_it() {
    let machine = machine();
    let home = &machine.home;
    write(&home.join("Movies/trip.mov"), &vec![1; 2 * MIB]);
    write(&home.join("Movies/cut/a.mov"), &vec![1; 2 * MIB]);
    let root = mapped(home, false);
    let trip = at(&root, &["Movies", "trip.mov"]).clone();
    let cut = at(&root, &["Movies", "cut"]).clone();

    assert!(still_there(home, &trip.path, trip.identity));
    assert!(still_there(home, &cut.path, cut.identity));

    // Made again in its place, a file is another one.
    fs::remove_file(&trip.path).expect("removed");
    write(&trip.path, b"again");
    assert!(!still_there(home, &trip.path, trip.identity));
    // A link in a folder's place is never gone through, even to it.
    fs::rename(&cut.path, home.join("Movies/moved")).expect("moved");
    symlink(home.join("Movies/moved"), &cut.path).expect("a link");
    assert!(!still_there(home, &cut.path, cut.identity));
    // Nor is the home folder one of its own places.
    assert!(!still_there(home, &root.path, root.identity));
}
