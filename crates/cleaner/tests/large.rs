mod common;

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use common::{MIB, TAG, machine, reach, write};
use harbormaster_cleaner::{
    Finding, Group, Hunt, Kind, Outcome, Usage, Why, large, recheck, remove,
};

fn hunted(home: &Path, busy: &[(PathBuf, String)]) -> Vec<Finding> {
    let hunt = Hunt {
        home,
        smallest: MIB as u64,
        busy,
    };
    large(&hunt, &AtomicBool::new(false), &|_| {}).expect("the hunt should start")
}

fn names(found: &[Finding]) -> Vec<(&str, Group)> {
    found
        .iter()
        .map(|finding| (finding.name.as_str(), finding.group))
        .collect()
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

fn removed(home: &Path, finding: &Finding, bin: &Path) -> Outcome {
    let cleaned = remove(
        &reach(home),
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
fn finds_large_files_by_kind_largest_first_and_never_picks_them() {
    let machine = machine();
    let home = &machine.home;
    write(&home.join("Downloads/tool.dmg"), &vec![1; 3 * MIB]);
    write(&home.join("Downloads/old/setup.pkg"), &vec![1; 2 * MIB]);
    write(&home.join("Movies/trip.MOV"), &vec![1; 2 * MIB]);
    write(&home.join("Movies/clip.mp4"), &vec![1; 4 * MIB]);
    write(&home.join("Downloads/small.zip"), &vec![1; MIB / 2]);
    write(&home.join("report.pdf"), &vec![1; 2 * MIB]);
    write(&home.join("Desktop/data.bin"), &vec![1; 2 * MIB]);

    let found = hunted(home, &[]);

    assert_eq!(
        names(&found),
        [
            ("clip.mp4", Group::Videos),
            ("trip.MOV", Group::Videos),
            ("tool.dmg", Group::Archives),
            ("setup.pkg", Group::Installers),
            ("report.pdf", Group::Documents),
            ("data.bin", Group::OtherFiles),
        ]
    );
    let clip = &found[0];
    assert_eq!(clip.path, home.join("Movies/clip.mp4"));
    assert_eq!(clip.kind, Kind::Personal);
    assert!(!clip.picked && !clip.emptied && clip.in_use.is_none());
    assert!(clip.touched.is_some());
    assert!(clip.bytes >= 4 * MIB as u64);
}

#[test]
fn never_looks_in_the_library_hidden_folders_packages_or_caches() {
    let machine = machine();
    let home = &machine.home;
    for inside in [
        "Library/Containers/vm/disk.img",
        ".local/share/big.iso",
        "code/shop/node_modules/package/big.zip",
        "node_modules/package/big.zip",
        "Pictures/Photos Library.photoslibrary/originals/big.heic",
        "Photos.photoslibrary/originals/big.heic",
        "Applications/Big.app/Contents/Resources/big.mov",
        "Big.app/Contents/Resources/big.mov",
        "Virtual Machines/Linux.utm/Data/disk.img",
        "code/tool/target/debug/big.zip",
    ] {
        write(&home.join(inside), &vec![1; 2 * MIB]);
    }
    write(&home.join("code/tool/target/CACHEDIR.TAG"), TAG);

    assert_eq!(names(&hunted(home, &[])), []);
}

#[test]
fn never_enters_document_packages_but_lists_documents_saved_as_one_file() {
    let machine = machine();
    let home = &machine.home;
    for extension in ["key", "numbers", "pages"] {
        for above in ["", "Documents/"] {
            write(
                &home.join(format!("{above}Talk.{extension}/Data/movie.mov")),
                &vec![1; 2 * MIB],
            );
        }
    }
    assert_eq!(names(&hunted(home, &[])), []);

    write(&home.join("Documents/plain.pages"), &vec![1; 2 * MIB]);
    assert_eq!(
        names(&hunted(home, &[])),
        [("plain.pages", Group::Documents)]
    );
}

#[test]
fn leaves_out_a_file_with_other_links_since_trashing_one_frees_nothing() {
    let machine = machine();
    let film = machine.home.join("Movies/trip.mov");
    write(&film, &vec![1; 2 * MIB]);
    fs::create_dir_all(machine.home.join("Desktop")).expect("a folder");
    fs::hard_link(&film, machine.home.join("Desktop/trip.mov")).expect("a hard link");

    assert_eq!(names(&hunted(&machine.home, &[])), []);
}

#[test]
fn never_follows_a_link() {
    let mine = machine();
    let elsewhere = machine();
    write(&elsewhere.home.join("films/big.mov"), &vec![1; 2 * MIB]);
    fs::create_dir_all(mine.home.join("Movies")).expect("a folder");
    symlink(elsewhere.home.join("films"), mine.home.join("Movies/films")).expect("a link");
    symlink(
        elsewhere.home.join("films/big.mov"),
        mine.home.join("Movies/big.mov"),
    )
    .expect("a link");

    assert_eq!(names(&hunted(&mine.home, &[])), []);
}

#[test]
fn says_what_is_using_a_file() {
    let machine = machine();
    let film = machine.home.join("Movies/trip.mov");
    write(&film, &vec![1; 2 * MIB]);

    let found = hunted(&machine.home, &[(film, "QuickTime Player".into())]);

    assert_eq!(found[0].in_use.as_deref(), Some("QuickTime Player"));
}

#[test]
fn moves_a_large_file_to_the_trash_and_nothing_beside_it() {
    let machine = machine();
    let bin = machine.home.join(".Trash");
    fs::create_dir_all(&bin).expect("a Trash");
    let film = machine.home.join("Movies/trip.mov");
    write(&film, &vec![1; 2 * MIB]);
    write(&machine.home.join("Movies/notes.txt"), b"keep");
    let found = hunted(&machine.home, &[]);

    let outcome = removed(&machine.home, &found[0], &bin);

    assert_eq!(outcome, Outcome::Trashed);
    assert!(!film.exists());
    assert!(bin.join("trip.mov").exists());
    assert!(machine.home.join("Movies/notes.txt").exists());
}

#[test]
fn refuses_a_file_made_again_or_turned_into_a_folder_since() {
    let machine = machine();
    let bin = machine.home.join(".Trash");
    fs::create_dir_all(&bin).expect("a Trash");
    let again = machine.home.join("Movies/again.mov");
    let folder = machine.home.join("Movies/folder.mov");
    write(&again, &vec![1; 2 * MIB]);
    write(&folder, &vec![1; 2 * MIB]);
    let found = hunted(&machine.home, &[]);
    // Moved aside rather than removed, so the new file can't get its inode.
    fs::rename(&again, machine.home.join("Movies/again old.mov")).expect("moved aside by hand");
    write(&again, &vec![2; 2 * MIB]);
    fs::remove_file(&folder).expect("removed by hand");
    write(&folder.join("inside"), b"x");

    for finding in &found {
        let outcome = removed(&machine.home, finding, &bin);
        assert_eq!(
            outcome,
            Outcome::Refused { why: Why::Changed },
            "{}",
            finding.name
        );
    }
    assert!(again.exists() && folder.join("inside").exists());
}

#[test]
fn looks_again_at_a_file_as_at_a_folder() {
    let machine = machine();
    let kept = machine.home.join("Movies/kept.mov");
    let gone = machine.home.join("Movies/gone.mov");
    write(&kept, &vec![1; 2 * MIB]);
    write(&gone, &vec![1; 2 * MIB]);
    let found = hunted(&machine.home, &[]);
    fs::remove_file(&gone).expect("removed by hand");

    let now =
        recheck(&reach(&machine.home), &found, &Usage::default()).expect("the recheck should run");

    assert_eq!(names(&now), [("kept.mov", Group::Videos)]);
}
