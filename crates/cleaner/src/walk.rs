//! Folders opened one step at a time from the root, never through a link.
//! Each step opens the next name relative to the folder already open, so a
//! folder swapped for a link in the meantime is refused, not followed.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::{Dir, Item, changed};
#[cfg(windows)]
pub(crate) use windows::{Dir, Item, changed, plain, same_name};

/// How a folder or file is stored, as far as the cleaner looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stat {
    pub(crate) kind: FileType,
    /// The disk it is on.
    pub(crate) device: u64,
    /// Which one it is on that disk.
    pub(crate) inode: u128,
    /// How many names it has, when the disk says.
    pub(crate) links: Option<u64>,
    /// The room it takes on disk.
    pub(crate) bytes: u64,
    pub(crate) modified: SystemTime,
    /// The user it belongs to.
    #[cfg(unix)]
    pub(crate) owner: u32,
    /// What it is locked against, as `chflags` sets it.
    #[cfg(target_os = "macos")]
    pub(crate) flags: u32,
    /// Whether File Explorer keeps it out of sight, as it does the system's
    /// own files in the home folder.
    #[cfg(windows)]
    pub(crate) hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileType {
    Folder,
    File,
    /// A link, which is never followed.
    Link,
    /// A pipe, a socket or a device.
    #[cfg(unix)]
    Other,
}

impl Stat {
    /// Whether this is one of several names of the same file, or may be.
    pub(crate) fn may_have_other_names(&self) -> bool {
        self.links.is_none_or(|links| links > 1)
    }
}

/// Whether `stat` describes a folder.
pub(crate) fn is_folder(stat: &Stat) -> bool {
    stat.kind == FileType::Folder
}

/// Whether `stat` describes a regular file.
pub(crate) fn is_file(stat: &Stat) -> bool {
    stat.kind == FileType::File
}

/// Whether `stat` describes a link.
pub(crate) fn is_link(stat: &Stat) -> bool {
    stat.kind == FileType::Link
}

/// `path` with every link in it resolved, spelled as the disk spells it.
pub(crate) fn canonical(path: &Path) -> io::Result<PathBuf> {
    let path = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    let path = plain(path);
    Ok(path)
}

fn plain_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "expected an absolute path of plain names",
    )
}
