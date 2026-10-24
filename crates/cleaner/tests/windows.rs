#![cfg(windows)]

mod common;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use common::{DAY, MIB, NPM, age, link_folder, machine, node_project, reach, write};
use harbormaster_cleaner::{
    Cleaned, Finding, Group, Hunt, Kind, Outcome, Survey, Usage, Why, large, remove, scan,
    usual_roots,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, SetFileAttributesW,
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

/// Removes `finding` from `home` with nothing in use, `trash` standing in
/// for the Recycle Bin.
fn removed(
    home: &Path,
    finding: &Finding,
    trash: &dyn Fn(&Path) -> io::Result<PathBuf>,
) -> Cleaned {
    remove(
        &reach(home),
        std::slice::from_ref(finding),
        &|| Ok(Usage::default()),
        trash,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("removal should run")
}

#[test]
fn moves_what_maven_installed_to_the_recycle_bin_and_leaves_its_settings() {
    let machine = machine();
    let repository = machine.home.join(r".m2\repository");
    write(
        &repository.join(r"com\shop\core\1.0\core-1.0.jar"),
        &vec![1; 2 * MIB],
    );
    write(&machine.home.join(r".m2\settings.xml"), b"<settings/>");

    let findings = scanned(&machine.home, &[]);
    let maven = named(&findings, "Maven").expect("Maven's repository");
    assert_eq!(
        (maven.kind, maven.picked, maven.emptied),
        (Kind::Personal, false, false)
    );
    let cleaned = removed(&machine.home, maven, &|path| {
        harbormaster_platform::trash(path).map_err(io::Error::other)
    });

    assert_eq!(cleaned.removals[0].outcome, Outcome::Trashed, "{cleaned:?}");
    assert!(!repository.exists());
    assert!(machine.home.join(r".m2\settings.xml").exists());
}

#[test]
fn moves_a_personal_place_the_disk_spells_in_other_case_than_the_catalog() {
    let machine = machine();
    let packages = machine.home.join(r".NuGet\Packages");
    write(
        &packages.join(r"newtonsoft.json\13.0.3\a.nupkg"),
        &vec![1; 2 * MIB],
    );
    let bin = machine.home.join("bin");
    std::fs::create_dir(&bin).expect("a stand-in for the Recycle Bin");

    let findings = scanned(&machine.home, &[]);
    let nuget = named(&findings, "NuGet").expect("NuGet's packages");
    let cleaned = removed(&machine.home, nuget, &|path| {
        let to = bin.join("$R0001");
        std::fs::rename(path, &to)?;
        Ok(to)
    });

    assert_eq!(cleaned.removals[0].outcome, Outcome::Trashed, "{cleaned:?}");
    assert!(bin.join(r"$R0001\newtonsoft.json\13.0.3\a.nupkg").exists());
}

fn hunted(home: &Path) -> Vec<Finding> {
    let hunt = Hunt {
        home,
        smallest: MIB as u64,
        busy: &[],
    };
    large(&hunt, &AtomicBool::new(false), &|_| {}).expect("the hunt should start")
}

/// Gives what is at `path` the `attributes` that keep it out of File
/// Explorer's sight, as Windows does its own files in the home folder.
fn hide(path: &Path, attributes: u32) {
    use std::os::windows::ffi::OsStrExt;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the path ends in a nul.
    let set = unsafe { SetFileAttributesW(wide.as_ptr(), attributes) };
    assert_ne!(set, 0, "{}", io::Error::last_os_error());
}

#[test]
fn hunts_large_files_but_never_in_appdata_or_among_what_windows_hides() {
    let machine = machine();
    let home = &machine.home;
    write(&home.join(r"Downloads\setup.exe"), &vec![1; 3 * MIB]);
    write(&home.join(r"Videos\trip.mp4"), &vec![1; 2 * MIB]);
    write(&home.join(r"AppData\Local\Temp\big.iso"), &vec![1; 2 * MIB]);
    write(
        &home.join(r"source\repos\app\node_modules\pkg\big.zip"),
        &vec![1; 2 * MIB],
    );
    write(&home.join("NTUSER.DAT"), &vec![1; 2 * MIB]);
    hide(
        &home.join("NTUSER.DAT"),
        FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM,
    );
    write(&home.join(r"Documents\Private\film.mkv"), &vec![1; 2 * MIB]);
    hide(&home.join(r"Documents\Private"), FILE_ATTRIBUTE_HIDDEN);
    write(&home.join(r"Documents\swap.img"), &vec![1; 2 * MIB]);
    hide(&home.join(r"Documents\swap.img"), FILE_ATTRIBUTE_SYSTEM);

    let found = hunted(home);

    let names: Vec<_> = found
        .iter()
        .map(|finding| (finding.name.as_str(), finding.group))
        .collect();
    assert_eq!(
        names,
        [
            ("trip.mp4", Group::Videos),
            ("setup.exe", Group::Installers)
        ]
    );
    assert_eq!(found[0].path, home.join(r"Videos\trip.mp4"));
    assert_eq!((found[0].kind, found[0].picked), (Kind::Personal, false));
}

#[test]
fn never_hunts_inside_a_virtual_machine_or_a_device_backup() {
    let machine = machine();
    let home = &machine.home;
    write(
        &home.join(r"VirtualBox VMs\Ubuntu\Ubuntu.vbox"),
        b"<VirtualBox/>",
    );
    write(
        &home.join(r"VirtualBox VMs\Ubuntu\Ubuntu.vdi"),
        &vec![1; 2 * MIB],
    );
    write(
        &home.join(r"Documents\Virtual Machines\Win\Win.vmx"),
        b"config",
    );
    write(
        &home.join(r"Documents\Virtual Machines\Win\Win-disk1.vmdk"),
        &vec![1; 2 * MIB],
    );
    write(
        &home.join(r"Apple\mobilesync\Backup\00008030\ab\ab12cd"),
        &vec![1; 2 * MIB],
    );

    assert_eq!(hunted(home), []);
}

#[test]
fn never_lists_the_disk_of_a_machine_kept_right_in_the_home_folder() {
    let machine = machine();
    let home = &machine.home;
    write(&home.join("Ubuntu.vbox"), b"<VirtualBox/>");
    write(&home.join("Ubuntu.vdi"), &vec![1; 2 * MIB]);
    write(&home.join(r"Videos\trip.mp4"), &vec![1; 2 * MIB]);

    let found = hunted(home);

    let names: Vec<_> = found.iter().map(|finding| finding.name.as_str()).collect();
    assert_eq!(names, ["trip.mp4"]);
}

#[test]
fn leaves_out_a_large_file_with_another_name_since_recycling_one_frees_nothing() {
    let machine = machine();
    let film = machine.home.join(r"Videos\trip.mp4");
    write(&film, &vec![1; 2 * MIB]);
    std::fs::create_dir_all(machine.home.join("Desktop")).expect("a folder");
    std::fs::hard_link(&film, machine.home.join(r"Desktop\trip.mp4")).expect("a hard link");

    assert_eq!(hunted(&machine.home), []);
}

#[test]
fn never_hunts_through_a_junction() {
    let mine = machine();
    let elsewhere = machine();
    write(&elsewhere.home.join(r"films\big.mp4"), &vec![1; 2 * MIB]);
    std::fs::create_dir_all(mine.home.join("Videos")).expect("a folder");
    link_folder(
        &elsewhere.home.join("films"),
        &mine.home.join(r"Videos\films"),
    );

    assert_eq!(hunted(&mine.home), []);
}

#[test]
fn moves_a_large_file_to_the_recycle_bin_and_nothing_beside_it() {
    let machine = machine();
    let film = machine.home.join(r"Videos\trip.mp4");
    write(&film, &vec![1; 2 * MIB]);
    write(&machine.home.join(r"Videos\notes.txt"), b"keep");
    let found = hunted(&machine.home);

    let cleaned = removed(&machine.home, &found[0], &|path| {
        harbormaster_platform::trash(path).map_err(io::Error::other)
    });

    assert_eq!(cleaned.removals[0].outcome, Outcome::Trashed, "{cleaned:?}");
    assert!(!film.exists());
    assert!(machine.home.join(r"Videos\notes.txt").exists());
}

#[test]
fn finds_each_browser_profiles_cache_and_nothing_else_of_the_profile() {
    let machine = machine();
    let home = &machine.home;
    let chrome = home.join(r"AppData\Local\Google\Chrome\User Data");
    write(
        &chrome.join(r"Default\Cache\Cache_Data\data_1"),
        &vec![1; 2 * MIB],
    );
    write(
        &chrome.join(r"Profile 1\Cache\Cache_Data\data_1"),
        &vec![1; 2 * MIB],
    );
    write(&chrome.join(r"Default\History"), &vec![1; 2 * MIB]);
    write(&chrome.join(r"Default\Login Data"), &vec![1; 2 * MIB]);
    let firefox = home.join(r"AppData\Local\Mozilla\Firefox\Profiles");
    write(
        &firefox.join(r"k3x9abcd.default-release\cache2\entries\A1"),
        &vec![1; 2 * MIB],
    );

    let findings = scanned(home, &[]);

    let default = named(&findings, "Google Chrome").expect("Chrome's cache");
    assert_eq!(default.path, chrome.join(r"Default\Cache"));
    assert_eq!(
        (default.group, default.picked, default.emptied),
        (Group::Apps, true, true)
    );
    let other = named(&findings, "Google Chrome · Profile 1").expect("the other profile's");
    assert_eq!(other.path, chrome.join(r"Profile 1\Cache"));
    let firefox_cache = named(&findings, "Firefox · default-release").expect("Firefox's cache");
    assert_eq!(
        firefox_cache.path,
        firefox.join(r"k3x9abcd.default-release\cache2")
    );
    assert_eq!(findings.len(), 3, "{findings:?}");
}

#[test]
fn takes_an_apps_cache_to_be_in_use_while_the_app_runs() {
    let machine = machine();
    let home = &machine.home;
    write(
        &home.join(r"AppData\Local\Google\Chrome\User Data\Default\Cache\a"),
        &vec![1; 2 * MIB],
    );
    write(
        &home.join(r"AppData\Roaming\Code\Cache\Cache_Data\a"),
        &vec![1; 2 * MIB],
    );
    let chrome = [(
        PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
        "chrome".to_string(),
    )];

    let findings = scanned_while(home, &[], &chrome);

    let browser = named(&findings, "Google Chrome").expect("Chrome's cache");
    assert_eq!(
        (browser.picked, browser.in_use.as_deref()),
        (false, Some("chrome"))
    );
    let editor = named(&findings, "Visual Studio Code").expect("VS Code's cache");
    assert_eq!((editor.picked, editor.in_use.as_deref()), (true, None));
}

#[test]
fn lists_crash_dumps_as_the_users_own() {
    let machine = machine();
    write(
        &machine
            .home
            .join(r"AppData\Local\CrashDumps\app.exe.1234.dmp"),
        &vec![1; 2 * MIB],
    );

    let findings = scanned(&machine.home, &[]);

    let dumps = named(&findings, "Crash dumps").expect("the crash dumps");
    assert_eq!(
        (dumps.group, dumps.kind, dumps.picked),
        (Group::Logs, Kind::Personal, false)
    );
}
