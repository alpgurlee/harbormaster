use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use harbormaster_cleaner::{Finding, Group, Kind, Survey, scan, scan_with};

const MIB: usize = 1024 * 1024;

fn write(path: &Path, bytes: usize) {
    let folder = path.parent().expect("a file inside a folder");
    fs::create_dir_all(folder).expect("the folder should be created");
    fs::write(path, vec![7u8; bytes]).expect("the file should be written");
}

/// Temp folders sit under `/var`, a link to `/private/var`; the cleaner only
/// starts from a real path.
fn scanned(home: &Path) -> Vec<Finding> {
    let real = home.canonicalize().expect("the temp folder should exist");
    scan(&survey(&real), &AtomicBool::new(false)).expect("the scan should start")
}

/// A survey of `home` alone: no project roots, nothing in use.
fn survey(home: &Path) -> Survey<'_> {
    Survey {
        home,
        roots: &[],
        busy: &[],
        running: &[],
        idle_after: Duration::ZERO,
        now: SystemTime::now(),
    }
}

fn named<'a>(findings: &'a [Finding], name: &str) -> Option<&'a Finding> {
    findings.iter().find(|finding| finding.name == name)
}

#[test]
fn finds_and_measures_what_the_catalog_names() {
    let home = tempfile::tempdir().unwrap();
    write(&home.path().join(".npm/_cacache/content/a"), 2 * MIB);
    write(&home.path().join("Library/Caches/pip/http/b"), 3 * MIB / 2);

    let findings = scanned(home.path());

    let npm = named(&findings, "npm").unwrap();
    assert_eq!(
        (npm.group, npm.kind, npm.picked),
        (Group::Packages, Kind::Rebuildable, true)
    );
    assert_eq!(
        npm.path,
        home.path().canonicalize().unwrap().join(".npm/_cacache")
    );
    assert!(npm.bytes >= 2 * MIB as u64, "{npm:?}");
    assert!(named(&findings, "pip").unwrap().bytes >= 3 * MIB as u64 / 2);
    assert!(named(&findings, "Gradle").is_none(), "{findings:?}");
}

#[test]
fn never_follows_a_link() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("big"), 8 * MIB);
    write(&home.path().join(".npm/_cacache/small"), 3 * MIB / 2);
    symlink(
        outside.path().join("big"),
        home.path().join(".npm/_cacache/link"),
    )
    .unwrap();
    fs::create_dir_all(home.path().join("Library/Caches")).unwrap();
    symlink(outside.path(), home.path().join("Library/Caches/pip")).unwrap();

    let findings = scanned(home.path());

    assert!(named(&findings, "npm").unwrap().bytes < 8 * MIB as u64);
    assert!(named(&findings, "pip").is_none(), "{findings:?}");
}

#[test]
fn never_reaches_a_place_through_a_linked_folder() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("Caches/pip/data"), 2 * MIB);
    symlink(outside.path(), home.path().join("Library")).unwrap();

    assert!(named(&scanned(home.path()), "pip").is_none());
}

#[test]
fn counts_a_hard_linked_file_once() {
    let home = tempfile::tempdir().unwrap();
    let one = home.path().join(".npm/_cacache/one");
    write(&one, 2 * MIB);
    fs::hard_link(&one, home.path().join(".npm/_cacache/two")).unwrap();

    let bytes = named(&scanned(home.path()), "npm").unwrap().bytes;

    assert!((2 * MIB as u64..4 * MIB as u64).contains(&bytes), "{bytes}");
}

#[test]
fn names_more_tools_caches_and_leaves_their_owners_own_things_alone() {
    let home = tempfile::tempdir().unwrap();
    let listed = [
        ("Go build cache", "Library/Caches/go-build/aa/b"),
        ("Composer", "Library/Caches/composer/files/c"),
        ("pnpm metadata", "Library/Caches/pnpm/metadata/d"),
        ("Dart packages", ".pub-cache/hosted/pub.dev/e"),
        ("Dart git packages", ".pub-cache/git/f"),
        ("Deno modules", "Library/Caches/deno/remote/https/g"),
        (
            "Deno npm packages",
            "Library/Caches/deno/npm/registry.npmjs.org/h",
        ),
        ("Deno compiled code", "Library/Caches/deno/gen/file/i"),
        ("SwiftPM", "Library/Caches/org.swift.swiftpm/repositories/j"),
        (
            "Carthage",
            "Library/Caches/org.carthage.CarthageKit/dependencies/k",
        ),
    ];
    // Tools the user installed, and data an app keeps beside its cache.
    let kept = [
        ".pub-cache/bin/tool",
        ".pub-cache/global_packages/tool/l",
        "Library/Caches/deno/location_data/m/kv.sqlite3",
        "Library/Caches/deno/deno_history.txt",
    ];
    for (_, inside) in listed {
        write(&home.path().join(inside), 3 * MIB / 2);
    }
    for inside in kept {
        write(&home.path().join(inside), 3 * MIB / 2);
    }

    let findings = scanned(home.path());

    for (name, _) in listed {
        let finding = named(&findings, name).unwrap_or_else(|| panic!("{name}: {findings:?}"));
        assert_eq!(
            (finding.group, finding.kind),
            (Group::Packages, Kind::Rebuildable),
            "{name}"
        );
    }
    // Each is listed once, as what it is, and not again as an app's cache.
    assert!(named(&findings, "org.swift.swiftpm").is_none());
    assert!(named(&findings, "org.carthage.CarthageKit").is_none());
    let real = home.path().canonicalize().unwrap();
    for inside in kept {
        assert!(
            findings
                .iter()
                .all(|finding| !real.join(inside).starts_with(&finding.path)),
            "{inside}: {findings:?}"
        );
    }
}

#[test]
fn offers_the_caches_of_apps_but_never_apples() {
    let home = tempfile::tempdir().unwrap();
    for name in ["com.spotify.client", "com.apple.Safari", "CloudKit"] {
        write(
            &home.path().join("Library/Caches").join(name).join("data"),
            3 * MIB / 2,
        );
    }

    let findings = scanned(home.path());

    let spotify = named(&findings, "com.spotify.client").unwrap();
    assert_eq!((spotify.group, spotify.picked), (Group::Apps, true));
    assert!(named(&findings, "com.apple.Safari").is_none());
    assert!(named(&findings, "CloudKit").is_none());
}

#[test]
fn never_picks_the_users_own_work_or_what_cant_come_back() {
    let home = tempfile::tempdir().unwrap();
    write(
        &home
            .path()
            .join("Library/Developer/Xcode/Archives/2026-01-02/App.xcarchive/Info.plist"),
        3 * MIB / 2,
    );
    write(&home.path().join(".Trash/old.txt"), 3 * MIB / 2);

    let findings = scanned(home.path());

    let archive = named(&findings, "2026-01-02").unwrap();
    assert_eq!(
        (archive.group, archive.kind, archive.picked),
        (Group::Xcode, Kind::Personal, false)
    );
    let trash = named(&findings, "Trash").unwrap();
    assert_eq!(
        (trash.group, trash.kind, trash.picked),
        (Group::Trash, Kind::Discarded, false)
    );
}

#[test]
fn keeps_the_device_symbols_used_last() {
    let home = tempfile::tempdir().unwrap();
    let support = home
        .path()
        .join("Library/Developer/Xcode/iOS DeviceSupport");
    write(&support.join("16.4 (20E247)/Symbols/a"), 3 * MIB / 2);
    write(&support.join("17.5 (21F79)/Symbols/a"), 3 * MIB / 2);
    let older = SystemTime::now() - Duration::from_secs(86_400);
    File::open(support.join("16.4 (20E247)"))
        .unwrap()
        .set_modified(older)
        .unwrap();

    let findings = scanned(home.path());

    assert!(named(&findings, "16.4 (20E247)").unwrap().picked);
    assert!(!named(&findings, "17.5 (21F79)").unwrap().picked);
}

#[test]
fn lists_each_group_together_largest_first() {
    let home = tempfile::tempdir().unwrap();
    write(&home.path().join(".npm/_cacache/a"), 3 * MIB / 2);
    write(&home.path().join("Library/Caches/pip/a"), 3 * MIB);
    write(&home.path().join("Library/Logs/a.log"), 2 * MIB);

    let order: Vec<String> = scanned(home.path())
        .into_iter()
        .map(|finding| finding.name)
        .collect();

    assert_eq!(order, ["pip", "npm", "Logs"]);
}

#[test]
fn leaves_out_what_is_too_small_to_matter() {
    let home = tempfile::tempdir().unwrap();
    write(&home.path().join("Library/Caches/pip/a"), 10_000);

    assert!(named(&scanned(home.path()), "pip").is_none());
}

#[test]
fn starts_only_from_a_real_home_folder() {
    let place = tempfile::tempdir().unwrap();
    let real = place.path().canonicalize().unwrap();
    write(&real.join("home/.npm/_cacache/a"), 2 * MIB);
    symlink(real.join("home"), real.join("link")).unwrap();

    assert!(scan(&survey(&real.join("link")), &AtomicBool::new(false)).is_err());
    assert!(scan(&survey(Path::new("relative/home")), &AtomicBool::new(false)).is_err());
}

#[test]
fn takes_only_folders_for_apps_and_device_symbols() {
    let home = tempfile::tempdir().unwrap();
    write(
        &home.path().join("Library/Caches/com.example.data"),
        2 * MIB,
    );
    let support = home
        .path()
        .join("Library/Developer/Xcode/iOS DeviceSupport");
    write(&support.join("16.4 (20E247)/Symbols/a"), 3 * MIB / 2);
    write(&support.join("17.5 (21F79)/Symbols/a"), 3 * MIB / 2);
    let older = SystemTime::now() - Duration::from_secs(86_400);
    File::open(support.join("16.4 (20E247)"))
        .unwrap()
        .set_modified(older)
        .unwrap();
    write(&support.join(".DS_Store"), 2 * MIB);

    let findings = scanned(home.path());

    assert!(named(&findings, "com.example.data").is_none());
    assert!(named(&findings, ".DS_Store").is_none());
    assert!(!named(&findings, "17.5 (21F79)").unwrap().picked);
}

#[test]
fn keeps_jetbrains_local_history_as_the_users_own() {
    let home = tempfile::tempdir().unwrap();
    write(
        &home
            .path()
            .join("Library/Caches/JetBrains/IntelliJIdea2026.2/LocalHistory/changes.storageData"),
        2 * MIB,
    );

    let findings = scanned(home.path());

    let jetbrains = named(&findings, "JetBrains IDEs").unwrap();
    assert_eq!((jetbrains.kind, jetbrains.picked), (Kind::Personal, false));
}

#[test]
fn offers_maven_and_nuget_downloads_as_the_users_own_and_never_their_settings() {
    let home = tempfile::tempdir().unwrap();
    write(
        &home
            .path()
            .join(".m2/repository/org/acme/lib/1.0/lib-1.0.jar"),
        2 * MIB,
    );
    write(&home.path().join(".m2/settings.xml"), 2 * MIB);
    write(&home.path().join(".m2/settings-security.xml"), 2 * MIB);
    write(
        &home
            .path()
            .join(".nuget/packages/newtonsoft.json/13.0.3/lib.dll"),
        2 * MIB,
    );

    let findings = scanned(home.path());

    for (name, place) in [("Maven", ".m2/repository"), ("NuGet", ".nuget/packages")] {
        let found = named(&findings, name).unwrap_or_else(|| panic!("{name} in {findings:?}"));
        assert_eq!(
            (found.group, found.kind, found.picked, found.emptied),
            (Group::Packages, Kind::Personal, false, false)
        );
        assert!(found.path.ends_with(place), "{found:?}");
    }
    let real = home.path().canonicalize().unwrap();
    for name in [".m2/settings.xml", ".m2/settings-security.xml"] {
        let kept = real.join(name);
        assert!(
            findings
                .iter()
                .all(|finding| !kept.starts_with(&finding.path)),
            "{name}: {findings:?}"
        );
    }
}

#[test]
fn measures_nothing_once_told_to_stop() {
    let home = tempfile::tempdir().unwrap();
    write(&home.path().join(".npm/_cacache/a"), 2 * MIB);
    let real = home.path().canonicalize().unwrap();

    let findings = scan(&survey(&real), &AtomicBool::new(true)).unwrap();

    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn leaves_out_a_place_too_deep_to_measure_in_full() {
    let home = tempfile::tempdir().unwrap();
    let cache = home.path().join(".npm/_cacache");
    write(&cache.join("top"), 2 * MIB);
    let deep = (0..70).fold(cache.clone(), |path, _| path.join("d"));
    write(&deep.join("bottom"), 2 * MIB);

    assert!(named(&scanned(home.path()), "npm").is_none());
}

#[test]
fn leaves_out_a_place_with_an_unreadable_subfolder() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let cache = home.path().join(".npm/_cacache");
    let blocked = cache.join("blocked");
    write(&cache.join("top"), 2 * MIB);
    write(&blocked.join("bottom"), 2 * MIB);
    let original = fs::metadata(&blocked).unwrap().permissions();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();

    let findings = scanned(home.path());
    fs::set_permissions(&blocked, original).unwrap();

    assert!(named(&findings, "npm").is_none(), "{findings:?}");
}

#[test]
fn leaves_out_a_place_whose_entries_it_cannot_look_up() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let cache = home.path().join(".npm/_cacache");
    let listed_only = cache.join("listed-only");
    write(&cache.join("top"), 2 * MIB);
    write(&listed_only.join("inside"), 2 * MIB);
    let original = fs::metadata(&listed_only).unwrap().permissions();
    fs::set_permissions(&listed_only, fs::Permissions::from_mode(0o444)).unwrap();

    let findings = scanned(home.path());
    fs::set_permissions(&listed_only, original).unwrap();

    assert!(named(&findings, "npm").is_none(), "{findings:?}");
}

#[test]
fn says_whether_a_place_is_emptied_or_goes_whole() {
    let home = tempfile::tempdir().unwrap();
    write(&home.path().join(".npm/_cacache/a"), 2 * MIB);
    let support = home
        .path()
        .join("Library/Developer/Xcode/iOS DeviceSupport");
    write(&support.join("16.4 (20E247)/Symbols/a"), 2 * MIB);
    write(&support.join("17.5 (21F79)/Symbols/a"), 2 * MIB);
    write(
        &home
            .path()
            .join("Library/Caches/JetBrains/IntelliJIdea2025.1/a"),
        2 * MIB,
    );

    let findings = scanned(home.path());

    assert!(named(&findings, "npm").unwrap().emptied);
    assert!(!named(&findings, "16.4 (20E247)").unwrap().emptied);
    assert!(!named(&findings, "JetBrains IDEs").unwrap().emptied);
}

#[test]
fn tells_how_far_measuring_has_come() {
    let home = tempfile::tempdir().unwrap();
    write(&home.path().join(".npm/_cacache/a"), 2 * MIB);
    write(&home.path().join("Library/Caches/pip/b"), 3 * MIB);
    // Measured, but too small to be listed.
    write(&home.path().join("Library/Caches/Yarn/c"), 1024);
    let real = home.path().canonicalize().unwrap();
    let told = std::sync::Mutex::new(Vec::new());

    let findings = scan_with(&survey(&real), &AtomicBool::new(false), &|measured| {
        told.lock().unwrap().push(measured);
    })
    .unwrap();

    let told = told.into_inner().unwrap();
    let mut done: Vec<usize> = told.iter().map(|measured| measured.done).collect();
    done.sort_unstable();
    assert_eq!(done, [1, 2, 3]);
    assert!(told.iter().all(|measured| measured.total == 3), "{told:?}");
    let listed: u64 = findings.iter().map(|finding| finding.bytes).sum();
    let found: u64 = told.iter().map(|measured| measured.bytes).sum();
    assert_eq!(found, listed);
}

#[test]
fn tells_how_large_the_disk_is_and_how_much_is_free() {
    let home = tempfile::tempdir().unwrap();

    let disk = harbormaster_cleaner::disk(home.path()).unwrap();

    assert!(disk.total > 0 && disk.free <= disk.total, "{disk:?}");
}
