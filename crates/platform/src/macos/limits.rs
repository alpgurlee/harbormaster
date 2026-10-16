//! How many files the process may hold open at once.

use std::io;
use std::mem::MaybeUninit;

/// The most a process may hold open, whatever its hard limit says.
const OPEN_MAX: libc::rlim_t = 10_240;

pub(crate) fn allow_more_open_files() -> io::Result<u64> {
    let mut limit = MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: `limit` has room for one rlimit, which getrlimit fills on success.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: getrlimit succeeded, so it filled `limit`.
    let mut limit = unsafe { limit.assume_init() };
    let wanted = limit.rlim_max.min(OPEN_MAX);
    if limit.rlim_cur < wanted {
        limit.rlim_cur = wanted;
        // SAFETY: `limit` is a valid rlimit whose soft limit is within its hard one.
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(limit.rlim_cur)
}
