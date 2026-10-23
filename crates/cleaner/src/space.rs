//! How large a disk is and how much of it is free, as the disk reports it.

use std::io;
use std::path::Path;

use crate::Disk;

#[cfg(unix)]
pub(crate) fn disk(path: &Path) -> io::Result<Disk> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes())?;
    let mut stats = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `path` is a valid C string and `stats` has room for one statfs.
    if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statfs succeeded, so it filled `stats`.
    let stats = unsafe { stats.assume_init() };
    let block = u64::from(stats.f_bsize);
    Ok(Disk {
        total: stats.f_blocks * block,
        free: stats.f_bavail * block,
    })
}

/// Free counts only what this user may still take, quotas included.
#[cfg(windows)]
pub(crate) fn disk(path: &Path) -> io::Result<Disk> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: the path ends in a nul; the total free on the disk isn't asked for.
    if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(Disk { total, free })
}
