use std::ffi::c_int;
use std::io;
use std::mem::size_of;

use super::process::field_for;
use crate::Field;

/// argv as the kernel recorded it at exec time (`KERN_PROCARGS2`).
pub(super) fn arguments(pid: libc::pid_t) -> Field<Vec<String>> {
    read(pid, parse)
}

/// Whether the process had `name` set to something in its environment when
/// it started. Only that one variable is looked for, and only whether it is
/// set is told: nothing else in the environment ever leaves here.
pub(super) fn sets(pid: libc::pid_t, name: &str) -> Field<bool> {
    read(pid, |buffer| sets_in(buffer, name))
}

fn read<T>(pid: libc::pid_t, parse: impl FnOnce(&[u8]) -> Option<T>) -> Field<T> {
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

/// Whether what follows argv, the environment and what the system adds
/// after it, sets `name` to something anywhere. Empty entries are passed
/// over, since an environment may hold one, and every other entry is only
/// compared with `name=`. None when nothing follows argv at all, as the
/// system keeps its own programs' environments from view, or when the last
/// entry is cut short: unset can't be told then.
fn sets_in(buffer: &[u8], name: &str) -> Option<bool> {
    let (_, rest) = split(buffer)?;
    let mut entries = rest.split(|&byte| byte == 0);
    // What follows the last NUL must be nothing, or the entry was cut short.
    if entries.next_back().is_some_and(|last| !last.is_empty()) {
        return None;
    }
    let mut seen = false;
    let mut set = false;
    for entry in entries.filter(|entry| !entry.is_empty()) {
        seen = true;
        set |= entry
            .strip_prefix(name.as_bytes())
            .and_then(|after| after.strip_prefix(b"="))
            .is_some_and(|value| !value.is_empty());
    }
    seen.then_some(set)
}

fn parse(buffer: &[u8]) -> Option<Vec<String>> {
    split(buffer).map(|(arguments, _)| arguments)
}

/// Layout: argc (native int), the exec path, NULs that pad it to a pointer
/// boundary, then argc strings. The environment follows, handed back unread
/// with what comes after it.
fn split(buffer: &[u8]) -> Option<(Vec<String>, &[u8])> {
    let argc_bytes = buffer.get(..size_of::<c_int>())?;
    let argc = c_int::from_ne_bytes(argc_bytes.try_into().ok()?);
    let argc = usize::try_from(argc).ok()?;

    let strings = &buffer[size_of::<c_int>()..];
    let path_end = strings.iter().position(|&byte| byte == 0)? + 1;
    // Knowing exactly where argv starts keeps empty arguments from shifting
    // the environment into view. Anything but NULs there means we misread.
    let argv_start = path_end.next_multiple_of(size_of::<usize>());
    if strings
        .get(path_end..argv_start)?
        .iter()
        .any(|&byte| byte != 0)
    {
        return None;
    }

    let mut rest = &strings[argv_start..];
    let mut arguments = Vec::with_capacity(argc.min(rest.len()));
    for _ in 0..argc {
        let end = rest.iter().position(|&byte| byte == 0)?;
        arguments.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        rest = &rest[end + 1..];
    }
    Some((arguments, rest))
}

#[cfg(test)]
mod tests {
    use super::{parse, sets_in};

    fn buffer(argc: i32, body: &[u8]) -> Vec<u8> {
        let mut bytes = argc.to_ne_bytes().to_vec();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn skips_the_exec_path_and_its_padding() {
        let raw = buffer(
            2,
            b"/usr/local/bin/node\0\0\0\0\0node\0server.js\0PATH=/usr/bin\0",
        );

        assert_eq!(parse(&raw), Some(vec!["node".into(), "server.js".into()]));
    }

    #[test]
    fn never_reads_into_the_environment() {
        let raw = buffer(1, b"/bin/sh\0sh\0SECRET=hunter2\0");

        assert_eq!(parse(&raw), Some(vec!["sh".into()]));
    }

    #[test]
    fn keeps_empty_arguments_where_the_process_had_them() {
        // Captured from `yes` started with argv ["", "", "x"].
        let raw = buffer(3, b"/usr/bin/yes\0\0\0\0\0\0x\0SECRET=value\0\0");

        assert_eq!(parse(&raw), Some(vec!["".into(), "".into(), "x".into()]));
    }

    #[test]
    fn never_mistakes_the_environment_for_an_empty_argv0() {
        // The path ends on byte 7, one NUL pads it to 8, then argv[0] is empty.
        let raw = buffer(1, b"/bin/x\0\0\0SECRET=value\0");

        assert_eq!(parse(&raw), Some(vec!["".into()]));
    }

    #[test]
    fn refuses_padding_that_is_not_blank() {
        let raw = buffer(1, b"/bin/x\0Zsh\0");

        assert_eq!(parse(&raw), None);
    }

    #[test]
    fn tells_only_whether_one_variable_is_set_to_something() {
        let raw = buffer(
            2,
            b"/usr/bin/java\0\0\0java\0Main\0PATH=/usr/bin\0JDK_JAVA_OPTIONS=-cp\0\0executable_path=/usr/bin/java\0",
        );
        let blank = buffer(1, b"/usr/bin/java\0\0\0java\0JDK_JAVA_OPTIONS=\0");
        let other = buffer(1, b"/usr/bin/java\0\0\0java\0XJDK_JAVA_OPTIONS=-cp\0");
        // An argument that looks like it is not the environment.
        let argument = buffer(
            2,
            b"/usr/bin/java\0\0\0java\0JDK_JAVA_OPTIONS=-cp\0PATH=/usr/bin\0",
        );

        assert_eq!(sets_in(&raw, "JDK_JAVA_OPTIONS"), Some(true));
        assert_eq!(sets_in(&blank, "JDK_JAVA_OPTIONS"), Some(false));
        assert_eq!(sets_in(&other, "JDK_JAVA_OPTIONS"), Some(false));
        assert_eq!(sets_in(&argument, "JDK_JAVA_OPTIONS"), Some(false));
        assert_eq!(
            sets_in(&buffer(3, b"/bin/sh\0sh\0"), "JDK_JAVA_OPTIONS"),
            None
        );
    }

    #[test]
    fn looks_past_empty_entries_and_into_what_follows_the_environment() {
        // An environment may hold an empty entry; it ends with a null pointer, not one.
        let after_empty = buffer(
            1,
            b"/usr/bin/java\0\0\0java\0PATH=/usr/bin\0\0JDK_JAVA_OPTIONS=-cp\0",
        );
        // With nothing to pad, what the system adds follows the environment at once.
        let unpadded = buffer(
            1,
            b"/usr/bin/java\0\0\0java\0PATH=/usr/bin\0executable_path=/usr/bin/java\0",
        );
        // Set twice, once to nothing: set all the same.
        let twice = buffer(
            1,
            b"/usr/bin/java\0\0\0java\0JDK_JAVA_OPTIONS=\0JDK_JAVA_OPTIONS=-cp\0",
        );

        assert_eq!(sets_in(&after_empty, "JDK_JAVA_OPTIONS"), Some(true));
        assert_eq!(sets_in(&unpadded, "JDK_JAVA_OPTIONS"), Some(false));
        assert_eq!(sets_in(&twice, "JDK_JAVA_OPTIONS"), Some(true));
    }

    #[test]
    fn cant_tell_from_an_entry_cut_short() {
        let cut = buffer(1, b"/usr/bin/java\0\0\0java\0PATH=/usr/bin\0JDK_JAVA_OPT");

        assert_eq!(sets_in(&cut, "JDK_JAVA_OPTIONS"), None);
    }

    #[test]
    fn cant_tell_when_the_environment_is_kept_from_view() {
        // Captured from /bin/sleep on macOS 27, started with JDK_JAVA_OPTIONS set:
        // the system shows nothing after argv for its own programs.
        let hidden = buffer(2, b"/bin/sleep\0\0\0\0\0\0/bin/sleep\x0030\0\0");

        assert_eq!(sets_in(&hidden, "JDK_JAVA_OPTIONS"), None);
    }

    #[test]
    fn rejects_truncated_input() {
        assert_eq!(parse(&buffer(3, b"/bin/sh\0sh\0")), None);
        assert_eq!(parse(&[1, 0]), None);
    }
}
