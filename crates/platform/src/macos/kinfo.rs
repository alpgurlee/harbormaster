//! The whole process table in one call, other users' processes included,
//! which `proc_pidinfo` refuses to describe.

use std::collections::HashSet;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

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

// From xnu bsd/sys/proc.h: statuses run from SIDL (1) to SZOMB (5).
const SZOMB: u8 = 5;
/// Far more than any real process table, so a wild size can't exhaust memory.
const TABLE_LIMIT: usize = 64 << 20;

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
        let capacity = size
            .checked_add(size / 8)
            .filter(|&capacity| capacity <= TABLE_LIMIT)
            .ok_or_else(|| io::Error::from_raw_os_error(libc::ENOMEM))?;
        let mut buffer = vec![0u8; capacity];
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
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "the process table has an unexpected layout",
        )
    };
    if !buffer.len().is_multiple_of(RECORD) {
        return Err(invalid());
    }
    let entries: Vec<Entry> = buffer
        .as_chunks::<RECORD>()
        .0
        .iter()
        .map(entry)
        .collect::<Option<_>>()
        .ok_or_else(invalid)?;
    let mut pids = HashSet::with_capacity(entries.len());
    if !entries.iter().all(|entry| pids.insert(entry.pid)) {
        return Err(invalid());
    }
    Ok(entries)
}

/// A record, unless something in it makes no sense: a negative pid, an
/// unknown status, or a start time that is unset or in the future.
fn entry(record: &[u8; RECORD]) -> Option<Entry> {
    let status = record[STATUS];
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(u64::MAX, |elapsed| {
            u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX)
        });
    let seconds = i64::from_ne_bytes(bytes(record, START_SECONDS));
    let micros = i32::from_ne_bytes(bytes(record, START_MICROS));
    let command = &record[COMMAND..COMMAND + COMMAND_LENGTH];
    let end = command
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(command.len());
    let pid = libc::pid_t::from_ne_bytes(bytes(record, PID));
    let start = u64::try_from(seconds)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(micros).ok()?.checked_mul(1_000)?)?;
    let sound = pid >= 0
        && (1..=SZOMB).contains(&status)
        && (0..1_000_000).contains(&micros)
        && start > 0
        && start <= now;
    sound.then(|| Entry {
        pid,
        parent: libc::pid_t::from_ne_bytes(bytes(record, PARENT)),
        user: libc::uid_t::from_ne_bytes(bytes(record, USER)),
        start: StartTime(start),
        command: String::from_utf8_lossy(&command[..end]).into_owned(),
        zombie: status == SZOMB,
    })
}

fn bytes<const N: usize>(record: &[u8; RECORD], at: usize) -> [u8; N] {
    let mut value = [0; N];
    value.copy_from_slice(&record[at..at + N]);
    value
}

#[cfg(test)]
mod tests {
    use super::{COMMAND, PARENT, PID, RECORD, START_MICROS, START_SECONDS, STATUS, USER, parse};

    fn record(pid: i32, status: u8, start: i64) -> Vec<u8> {
        let mut record = vec![0u8; RECORD];
        record[START_SECONDS..START_SECONDS + 8].copy_from_slice(&start.to_ne_bytes());
        record[STATUS] = status;
        record[PID..PID + 4].copy_from_slice(&pid.to_ne_bytes());
        record[PARENT..PARENT + 4].copy_from_slice(&1i32.to_ne_bytes());
        record[USER..USER + 4].copy_from_slice(&501u32.to_ne_bytes());
        record[COMMAND..COMMAND + 4].copy_from_slice(b"node");
        record
    }

    #[test]
    fn reads_a_sound_record() {
        let table = parse(&record(42, 2, 1_700_000_000)).unwrap();

        assert_eq!(table[0].pid, 42);
        assert_eq!(table[0].command, "node");
    }

    #[test]
    fn rejects_a_table_with_a_record_that_makes_no_sense() {
        assert!(parse(&record(42, 9, 1_700_000_000)).is_err());
        assert!(parse(&record(-1, 2, 1_700_000_000)).is_err());
        assert!(parse(&record(42, 2, 0)).is_err());
        assert!(parse(&record(42, 2, i64::MAX)).is_err());
        assert!(
            parse(&[record(42, 2, 1_700_000_000), record(42, 2, 1_700_000_001)].concat()).is_err()
        );
    }

    #[test]
    fn accepts_microseconds_only_within_one_second() {
        for (micros, valid) in [
            (-1i32, false),
            (0, true),
            (999_999, true),
            (1_000_000, false),
            (i32::MAX, false),
        ] {
            let mut raw = record(42, 2, 1_700_000_000);
            raw[START_MICROS..START_MICROS + 4].copy_from_slice(&micros.to_ne_bytes());

            assert_eq!(parse(&raw).is_ok(), valid, "micros={micros}");
        }
    }
}
