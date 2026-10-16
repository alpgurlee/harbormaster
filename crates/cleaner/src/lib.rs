//! What a developer's machine can do without: caches its tools rebuild, logs,
//! and the Trash. Finding is read-only; nothing here removes anything.

mod catalog;
mod measure;
mod walk;

use std::ffi::{CStr, OsStr};
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::Serialize;

/// Less than this isn't worth the list's room.
const SMALLEST: u64 = 1024 * 1024;

/// How the cleaner window groups what it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    /// Package managers' downloads.
    Packages,
    /// Xcode's build data, device symbols and archives.
    Xcode,
    /// Apps' own caches.
    Apps,
    Logs,
    Trash,
}

/// How a finding would go, which decides whether it is picked unasked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Its tool rebuilds it: deleted, and picked.
    Rebuildable,
    /// The user's own work, such as an Xcode archive: moved to the Trash, never picked unasked.
    Personal,
    /// Already thrown away: emptying it is for good, so never picked unasked.
    Discarded,
}

/// Something that could go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub group: Group,
    pub kind: Kind,
    /// What the window calls it: `npm`, `17.5 (21F79)`, `com.spotify.client`.
    pub name: String,
    pub path: PathBuf,
    /// Space it takes on disk.
    pub bytes: u64,
    /// Chosen unless the user says otherwise.
    pub picked: bool,
}

/// What could go under `home`, measured, by group and largest first. `home`
/// must be an absolute path with a real folder at every step. `stop` cuts a
/// long scan short, keeping what was measured.
pub fn scan(home: &Path, stop: &AtomicBool) -> io::Result<Vec<Finding>> {
    let root = walk::Dir::open(home)?;
    let candidates = catalog::candidates(&root, home, stop);
    let mut findings = measure::all(&root, candidates, stop);
    findings.retain(|finding| finding.bytes >= SMALLEST);
    findings.sort_by(|a, b| a.group.cmp(&b.group).then(b.bytes.cmp(&a.bytes)));
    Ok(findings)
}

/// The home folder the user's account names, which the cleaner trusts over
/// whatever `$HOME` says.
pub fn account_home() -> io::Result<PathBuf> {
    let mut entry = MaybeUninit::<libc::passwd>::uninit();
    let mut text = vec![0; 16 * 1024];
    let mut found = std::ptr::null_mut();
    // SAFETY: `entry` and `text` are the sizes passed, and `found` is where
    // getpwuid_r reports the entry it filled.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            entry.as_mut_ptr(),
            text.as_mut_ptr(),
            text.len(),
            &raw mut found,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if found.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "this account has no entry in the user database",
        ));
    }
    // SAFETY: `found` points at `entry`, whose strings live in `text`.
    let folder = unsafe { CStr::from_ptr((*found).pw_dir) };
    Ok(PathBuf::from(OsStr::from_bytes(folder.to_bytes())))
}
