//! The whole process table in one call, other users' processes included,
//! which `proc_pidinfo` refuses to describe.

use std::io;

use crate::StartTime;

// `struct kinfo_proc` from <sys/sysctl.h>: its size and the offsets of the
// fields we read, measured with offsetof against the macOS SDK. Reads are
// checked against our own process before the table is trusted.
const RECORD: usize = 648;
const START_SECONDS: usize = 0; // kp_proc.p_un.__p_starttime.tv_sec
const START_MICROS: usize = 8; // kp_proc.p_un.__p_starttime.tv_usec
const STATUS: usize = 36; // kp_proc.p_stat
const PID: usize = 40; // kp_proc.p_pid
const COMMAND: usize = 243; // kp_proc.p_comm
const COMMAND_LENGTH: usize = 17;
const USER: usize = 420; // kp_eproc.e_ucred.cr_uid
const PARENT: usize = 560; // kp_eproc.e_ppid
const _: () = assert!(PARENT + 4 <= RECORD && COMMAND + COMMAND_LENGTH <= RECORD);

// From xnu bsd/sys/proc.h.
const SZOMB: u8 = 5;

#[derive(Debug)]
pub(super) struct Entry {
    pub(super) pid: libc::pid_t,
    pub(super) parent: libc::pid_t,
    pub(super) user: libc::uid_t,
    pub(super) start: StartTime,
    /// The first 16 bytes of the executable's name.
    pub(super) command: String,
    pub(super) zombie: bool,
}

pub(super) fn all() -> io::Result<Vec<Entry>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_ALL];
    // Processes started between asking for the size and reading the table
    // make it too small; ask again.
    for _ in 0..4 {
        let mut size = 0;
        // SAFETY: a null buffer asks only for the size needed.
        let sized = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if sized != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0u8; size + size / 8];
        let mut length = buffer.len();
        // SAFETY: `buffer` is writable for `length` bytes; the kernel updates `length`.
        let read = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buffer.as_mut_ptr().cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        };
        if read == 0 {
            buffer.truncate(length);
            return parse(&buffer);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOMEM) {
            return Err(error);
        }
    }
    Err(io::Error::from_raw_os_error(libc::ENOMEM))
}

fn parse(buffer: &[u8]) -> io::Result<Vec<Entry>> {
    if !buffer.len().is_multiple_of(RECORD) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the process table has an unexpected layout",
        ));
    }
    Ok(buffer.as_chunks::<RECORD>().0.iter().map(entry).collect())
}

fn entry(record: &[u8; RECORD]) -> Entry {
    let seconds = i64::from_ne_bytes(bytes(record, START_SECONDS));
    let micros = i32::from_ne_bytes(bytes(record, START_MICROS));
    let command = &record[COMMAND..COMMAND + COMMAND_LENGTH];
    let end = command
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(command.len());
    Entry {
        pid: libc::pid_t::from_ne_bytes(bytes(record, PID)),
        parent: libc::pid_t::from_ne_bytes(bytes(record, PARENT)),
        user: libc::uid_t::from_ne_bytes(bytes(record, USER)),
        start: StartTime(
            seconds
                .unsigned_abs()
                .saturating_mul(1_000_000_000)
                .saturating_add(u64::from(micros.unsigned_abs()) * 1_000),
        ),
        command: String::from_utf8_lossy(&command[..end]).into_owned(),
        zombie: record[STATUS] == SZOMB,
    }
}

fn bytes<const N: usize>(record: &[u8; RECORD], at: usize) -> [u8; N] {
    let mut value = [0; N];
    value.copy_from_slice(&record[at..at + N]);
    value
}
