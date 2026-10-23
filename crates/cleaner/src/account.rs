//! Where the account this runs as keeps its home folder, as the account
//! itself says rather than as the environment does.

use std::io;
use std::path::PathBuf;

/// The home folder in this account's entry in the user database.
#[cfg(unix)]
pub(crate) fn home() -> io::Result<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

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

/// The profile folder Windows keeps for this process's account.
#[cfg(windows)]
pub(crate) fn home() -> io::Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Security::TOKEN_QUERY;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW;

    let mut token = std::ptr::null_mut();
    // SAFETY: the current process's pseudo-handle needs no closing; `token` receives a new handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken succeeded, so the handle is open and ours alone.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut length = 0u32;
    // SAFETY: with no buffer, only the length it needs is written.
    unsafe { GetUserProfileDirectoryW(token.as_raw_handle(), std::ptr::null_mut(), &mut length) };
    let mut folder = vec![0u16; length as usize];
    // SAFETY: the buffer holds `length` units, as many as asked for.
    if unsafe { GetUserProfileDirectoryW(token.as_raw_handle(), folder.as_mut_ptr(), &mut length) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    let end = folder
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(folder.len());
    Ok(PathBuf::from(OsString::from_wide(&folder[..end])))
}
