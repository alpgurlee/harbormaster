use std::ffi::c_int;
use std::io;
use std::mem::size_of;

use super::process::field_for;
use crate::Field;

/// argv as the kernel recorded it at exec time (`KERN_PROCARGS2`).
pub(super) fn arguments(pid: libc::pid_t) -> Field<Vec<String>> {
    match raw(pid) {
        Ok(buffer) => parse(&buffer).map_or(Field::Unsupported, Field::Known),
        // Other users' processes fail with EINVAL rather than EPERM.
        Err(error) if error.raw_os_error() == Some(libc::EINVAL) => Field::Denied,
        Err(error) => field_for(&error),
    }
}

fn raw(pid: libc::pid_t) -> io::Result<Vec<u8>> {
    let mut arg_max: c_int = 0;
    let mut size = size_of::<c_int>();
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    // SAFETY: `arg_max` is a valid out-buffer of `size` bytes.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&raw mut arg_max).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || arg_max <= 0 {
        return Err(io::Error::last_os_error());
    }

    let mut buffer = vec![0u8; arg_max as usize];
    let mut size = buffer.len();
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // SAFETY: `buffer` is writable for `size` bytes; the kernel updates `size`.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(size);
    Ok(buffer)
}

/// Layout: argc (native int), the exec path, NUL padding, then argc strings.
/// The environment follows, which we deliberately never read here.
fn parse(buffer: &[u8]) -> Option<Vec<String>> {
    let argc_bytes = buffer.get(..size_of::<c_int>())?;
    let argc = c_int::from_ne_bytes(argc_bytes.try_into().ok()?);
    let argc = usize::try_from(argc).ok()?;

    let mut rest = &buffer[size_of::<c_int>()..];
    let exec_path_end = rest.iter().position(|&byte| byte == 0)?;
    rest = &rest[exec_path_end..];
    let first_arg = rest.iter().position(|&byte| byte != 0)?;
    rest = &rest[first_arg..];

    let mut arguments = Vec::with_capacity(argc);
    for _ in 0..argc {
        let end = rest.iter().position(|&byte| byte == 0)?;
        arguments.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        rest = &rest[end + 1..];
    }
    Some(arguments)
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn buffer(argc: i32, body: &[u8]) -> Vec<u8> {
        let mut bytes = argc.to_ne_bytes().to_vec();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn skips_the_exec_path_and_its_padding() {
        let raw = buffer(
            2,
            b"/usr/local/bin/node\0\0\0\0node\0server.js\0PATH=/usr/bin\0",
        );

        assert_eq!(parse(&raw), Some(vec!["node".into(), "server.js".into()]));
    }

    #[test]
    fn never_reads_into_the_environment() {
        let raw = buffer(1, b"/bin/sh\0sh\0SECRET=hunter2\0");

        assert_eq!(parse(&raw), Some(vec!["sh".into()]));
    }

    #[test]
    fn rejects_truncated_input() {
        assert_eq!(parse(&buffer(3, b"/bin/sh\0sh\0")), None);
        assert_eq!(parse(&[1, 0]), None);
    }
}
