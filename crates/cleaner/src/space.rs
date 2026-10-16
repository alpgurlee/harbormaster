//! How large a disk is and how much of it is free, as the disk reports it.

use std::ffi::CString;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::Disk;

pub(crate) fn disk(path: &Path) -> io::Result<Disk> {
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
