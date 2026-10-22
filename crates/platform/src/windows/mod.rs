pub mod control;
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

/// Where Windows itself is installed, `C:\Windows` as a rule: the system's
/// own folder, not the one a Terminal Services session may hand a program.
pub(crate) fn windows_folder() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;

    let mut buffer = [0u16; 260];
    // SAFETY: the buffer holds as many units as the call is told.
    let length =
        unsafe { GetSystemWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    (length > 0 && length < buffer.len())
        .then(|| PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])))
}

/// The process behind the desktop's shell window: the Explorer the session
/// runs, which no other program can pose as.
pub(crate) fn shell_process() -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

    // SAFETY: GetShellWindow has no preconditions.
    let window = unsafe { GetShellWindow() };
    if window.is_null() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: the window handle came from the system; `pid` is ours to write.
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    (pid != 0).then_some(pid)
}
