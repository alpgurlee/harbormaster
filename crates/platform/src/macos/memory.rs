use std::mem::{size_of, zeroed};
use std::sync::OnceLock;

use crate::Memory;

/// Activity Monitor's "Memory Used": app memory, wired and compressed.
pub(crate) fn memory() -> Option<Memory> {
    let total = total()?;
    // SAFETY: vm_statistics64 is plain old data; all-zero is a valid value.
    let mut stats: libc::vm_statistics64 = unsafe { zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: `stats` is writable for the `count` integers the kernel may fill.
    let result = unsafe {
        libc::host_statistics64(
            host(),
            libc::HOST_VM_INFO64,
            (&raw mut stats).cast(),
            &mut count,
        )
    };
    if result != libc::KERN_SUCCESS {
        return None;
    }
    // SAFETY: sysconf has no preconditions.
    let page_size = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
    let app = u64::from(stats.internal_page_count).saturating_sub(u64::from(stats.purgeable_count));
    let pages = app + u64::from(stats.wire_count) + u64::from(stats.compressor_page_count);
    Some(Memory {
        used: (pages * page_size).min(total),
        total,
    })
}

/// Each mach_host_self() call adds a port reference, so take one for good.
fn host() -> libc::mach_port_t {
    static HOST: OnceLock<libc::mach_port_t> = OnceLock::new();
    // SAFETY: mach_host_self has no preconditions.
    *HOST.get_or_init(|| unsafe { mach2::mach_init::mach_host_self() })
}

fn total() -> Option<u64> {
    let mut total: u64 = 0;
    let mut size = size_of::<u64>();
    // SAFETY: the name is NUL-terminated and `total` is writable for `size` bytes.
    let result = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut total).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (result == 0 && total > 0).then_some(total)
}
