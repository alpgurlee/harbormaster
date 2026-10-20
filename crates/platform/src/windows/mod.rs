mod language;
mod memory;
mod parameters;
mod process;
mod socket;
mod tcp;
mod theme;

use std::io;
use std::path::{Path, PathBuf};

use crate::{Thermal, Whereabouts};

pub(crate) use language::preferred_language;
pub(crate) use memory::memory;
pub(crate) use process::{
    details, executable_of, presence, processes, runs_as_root, sets, signal, tracks_exec,
};
pub(crate) use socket::{alone, every};
pub(crate) use tcp::listeners;
pub(crate) use theme::light_taskbar;

/// Not read on Windows yet. An empty list would say that nothing uses a
/// folder, so this fails instead.
pub(crate) fn whereabouts() -> io::Result<Vec<Whereabouts>> {
    Err(io::ErrorKind::Unsupported.into())
}

pub(crate) fn trash(_path: &Path) -> io::Result<PathBuf> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Windows has no limit like macOS's on open files to raise.
pub(crate) fn allow_more_open_files() -> io::Result<u64> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Windows says how hot the machine runs only through an API still in preview.
pub(crate) fn thermal() -> Thermal {
    Thermal::Unknown
}
