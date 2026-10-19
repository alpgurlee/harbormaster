//! Scratch homes for the cleaner's tests.

#![allow(dead_code)]

use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const MIB: usize = 1024 * 1024;
pub const DAY: Duration = Duration::from_secs(86_400);
pub const TAG: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n# a cache\n";

/// A temp folder standing in for a home folder, under its real path: temp
/// folders sit under `/var`, a link to `/private/var`, and the cleaner only
/// starts from a real path.
pub struct Machine {
    _place: tempfile::TempDir,
    pub home: PathBuf,
}

pub fn machine() -> Machine {
    let place = tempfile::tempdir().expect("a temp folder");
    let home = place.path().canonicalize().expect("its real path");
    Machine {
        _place: place,
        home,
    }
}

/// What removing may reach from `home`, on a Mac with no shared app folder.
pub fn reach(home: &Path) -> harbormaster_cleaner::Reach<'_> {
    harbormaster_cleaner::Reach {
        home,
        shared: Path::new("/nonexistent"),
    }
}

pub fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("a file inside a folder")).expect("the folder");
    fs::write(path, bytes).expect("the file");
}

/// Sets everything directly in `project`, then the project itself, to `age` ago.
pub fn age(project: &Path, age: Duration) {
    let when = SystemTime::now() - age;
    for entry in fs::read_dir(project).expect("the project") {
        let path = entry.expect("an entry").path();
        File::open(&path)
            .expect("open")
            .set_modified(when)
            .expect("touch");
    }
    File::open(project)
        .expect("open")
        .set_modified(when)
        .expect("touch");
}

/// A Node project called `name` under `code`, with 2 MiB each in its
/// node_modules and .next.
pub fn node_project(machine: &Machine, name: &str) -> PathBuf {
    let project = machine.home.join("code").join(name);
    write(&project.join("package.json"), b"{}");
    write(
        &project.join("node_modules/left-pad/index.js"),
        &vec![1; 2 * MIB],
    );
    write(&project.join(".next/cache/a"), &vec![1; 2 * MIB]);
    project
}

/// A folder's mode, set until the guard drops.
pub struct Mode {
    path: PathBuf,
    original: fs::Permissions,
}

/// Sets `path` to `mode`. Root reads past a folder's mode, so a test that
/// relies on one needs an ordinary user.
pub fn set_mode(path: &Path, mode: u32) -> Mode {
    // SAFETY: geteuid has no preconditions.
    let user = unsafe { libc::geteuid() };
    assert_ne!(
        user, 0,
        "run the tests as an ordinary user: root reads past a folder's mode"
    );
    let original = fs::metadata(path).expect("the folder").permissions();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("the mode");
    Mode {
        path: path.to_path_buf(),
        original,
    }
}

impl Drop for Mode {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, self.original.clone());
    }
}
