use std::ffi::{c_char, c_int};
use std::io;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use super::{all_pids, kinfo, procargs};
use crate::{Field, Owner, Pid, ProcessDetails, ProcessEntry, ProcessKey, StartTime};

pub(crate) fn processes() -> io::Result<Vec<ProcessEntry>> {
    // SAFETY: getuid has no failure mode.
    let own_uid = unsafe { libc::getuid() };
    processes_from(kinfo::all(), own_uid)
}

fn processes_from(
    table: io::Result<Vec<kinfo::Entry>>,
    own_uid: libc::uid_t,
) -> io::Result<Vec<ProcessEntry>> {
    // A table that fails its checks is read the slow way instead.
    let table = match table {
        Ok(table) if matches_our_own_record(&table) => table,
        Ok(_) => return processes_one_by_one(own_uid),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            return processes_one_by_one(own_uid);
        }
        Err(error) => return Err(error),
    };
    let entries = table
        .into_iter()
        .filter(|entry| entry.pid > 0 && !entry.zombie)
        .filter_map(|entry| {
            let key = ProcessKey {
                pid: Pid(entry.pid.unsigned_abs()),
                start: entry.start,
                exec: exec_version(entry.pid).ok()?,
            };
            // Our own user's processes also give their full name and usage.
            let ours = identify(entry.pid).ok().filter(|(_, found)| *found == key);
            let usage = ours.as_ref().and_then(|_| usage_of(entry.pid, key));
            Some(ProcessEntry {
                key,
                parent: (entry.parent > 0).then_some(Pid(entry.parent.unsigned_abs())),
                name: ours.map_or(entry.command, |(info, _)| name_of(&info)),
                owner: owner(entry.user, own_uid),
                memory_bytes: usage.map(|usage| usage.ri_phys_footprint),
                cpu_time: usage.map(|usage| {
                    Duration::from_nanos(mach_to_nanos(usage.ri_user_time + usage.ri_system_time))
                }),
            })
        })
        .collect();
    Ok(entries)
}

/// The table's layout is checked against what proc_pidinfo says about us.
fn matches_our_own_record(table: &[kinfo::Entry]) -> bool {
    let own = std::process::id();
    let Ok(pid) = libc::pid_t::try_from(own) else {
        return false;
    };
    let Ok((info, key)) = identify(pid) else {
        return false;
    };
    table.iter().any(|entry| {
        entry.pid == pid
            && entry.start == key.start
            && entry.parent.unsigned_abs() == info.pbi_ppid
            && entry.user == info.pbi_uid
    })
}

/// The slower way, process by process, which only sees this user's processes.
fn processes_one_by_one(own_uid: libc::uid_t) -> io::Result<Vec<ProcessEntry>> {
    let entries = all_pids()?
        .into_iter()
        .filter_map(|pid| {
            let (info, key) = identify(pid).ok()?;
            let usage = usage_of(pid, key);
            Some(ProcessEntry {
                key,
                parent: (info.pbi_ppid > 0).then_some(Pid(info.pbi_ppid)),
                name: name_of(&info),
                owner: owner(info.pbi_uid, own_uid),
                memory_bytes: usage.map(|usage| usage.ri_phys_footprint),
                cpu_time: usage.map(|usage| {
                    Duration::from_nanos(mach_to_nanos(usage.ri_user_time + usage.ri_system_time))
                }),
            })
        })
        .collect();
    Ok(entries)
}

fn owner(uid: libc::uid_t, own_uid: libc::uid_t) -> Owner {
    match uid {
        uid if uid == own_uid => Owner::CurrentUser,
        0 => Owner::System,
        _ => Owner::OtherUser,
    }
}

/// Usage figures, dropped if the pid changed hands while we read them.
fn usage_of(pid: libc::pid_t, key: ProcessKey) -> Option<libc::rusage_info_v4> {
    rusage(pid).filter(|_| identify(pid).is_ok_and(|(_, again)| again == key))
}

pub(crate) fn details(key: ProcessKey) -> ProcessDetails {
    let Ok(pid) = libc::pid_t::try_from(key.pid.0) else {
        return Missing::Gone.details();
    };
    if let Some(missing) = identity_problem(pid, key) {
        return missing.details();
    }
    let details = ProcessDetails {
        exe: executable(pid),
        args: procargs::arguments(pid),
        cwd: working_directory(pid),
    };
    // The pid may have been reused while we were reading.
    match identity_problem(pid, key) {
        Some(missing) => missing.details(),
        None => details,
    }
}

/// Why nothing about a process can be read.
#[derive(Debug, Clone, Copy)]
enum Missing {
    Denied,
    Gone,
}

impl Missing {
    fn field<T>(self) -> Field<T> {
        match self {
            Missing::Denied => Field::Denied,
            Missing::Gone => Field::Gone,
        }
    }

    fn details(self) -> ProcessDetails {
        ProcessDetails {
            exe: self.field(),
            args: self.field(),
            cwd: self.field(),
        }
    }
}

fn identity_problem(pid: libc::pid_t, key: ProcessKey) -> Option<Missing> {
    match identify(pid) {
        Ok((_, found)) if found == key => None,
        Ok(_) => Some(Missing::Gone),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Some(Missing::Gone),
        Err(_) => Some(Missing::Denied),
    }
}

/// A process's details and the identity they belong to.
pub(super) fn identify(pid: libc::pid_t) -> io::Result<(libc::proc_bsdinfo, ProcessKey)> {
    let info = bsd_info(pid)?;
    let nanos = info
        .pbi_start_tvsec
        .saturating_mul(1_000_000_000)
        .saturating_add(info.pbi_start_tvusec.saturating_mul(1_000));
    let key = ProcessKey {
        pid: Pid(pid.unsigned_abs()),
        start: StartTime(nanos),
        exec: exec_version(pid)?,
    };
    Ok((info, key))
}

/// `struct proc_uniqidentifierinfo` from xnu's private proc_info.h.
#[repr(C)]
struct UniqueIdentifierInfo {
    uuid: [u8; 16],
    unique_id: u64,
    parent_unique_id: u64,
    id_version: i32,
    original_parent_id_version: i32,
    reserved: [u64; 2],
}

const PROC_PIDUNIQIDENTIFIERINFO: c_int = 17;

/// xnu bumps `p_idversion` on exec. Whether this macOS answers is judged once,
/// on our own process: if it does, a process it won't answer for can't be
/// identified this round; if it doesn't, every process gets 0 and the pid and
/// start time alone tell processes apart.
fn exec_version(pid: libc::pid_t) -> io::Result<u32> {
    static ANSWERS: OnceLock<bool> = OnceLock::new();
    let answers = match ANSWERS.get() {
        Some(&answers) => answers,
        None => {
            let own = libc::pid_t::try_from(std::process::id())
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
            let probe = id_version(own);
            let Some(answers) = verdict(&probe) else {
                // Not settled yet, so nothing can be identified this round.
                return probe;
            };
            *ANSWERS.get_or_init(|| answers)
        }
    };
    if answers { id_version(pid) } else { Ok(0) }
}

/// Only a success, or a flavor this kernel doesn't know, settles whether exec
/// versions are available; any other failure may pass.
fn verdict(probe: &io::Result<u32>) -> Option<bool> {
    match probe {
        Ok(_) => Some(true),
        Err(error) if matches!(error.raw_os_error(), Some(libc::EINVAL | libc::ENOTSUP)) => {
            Some(false)
        }
        Err(_) => None,
    }
}

fn id_version(pid: libc::pid_t) -> io::Result<u32> {
    // SAFETY: UniqueIdentifierInfo is plain old data; all-zero is a valid value.
    let mut info: UniqueIdentifierInfo = unsafe { zeroed() };
    let size = size_of::<UniqueIdentifierInfo>() as c_int;
    // SAFETY: `info` is a writable buffer of exactly `size` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDUNIQIDENTIFIERINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if written == size {
        Ok(info.id_version as u32)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn bsd_info(pid: libc::pid_t) -> io::Result<libc::proc_bsdinfo> {
    // SAFETY: proc_bsdinfo is plain old data; all-zero is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as c_int;
    // SAFETY: `info` is a writable buffer of exactly `size` bytes.
    let written =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    if written == size {
        Ok(info)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn name_of(info: &libc::proc_bsdinfo) -> String {
    let name = c_string(&info.pbi_name);
    if name.is_empty() {
        c_string(&info.pbi_comm)
    } else {
        name
    }
}

fn rusage(pid: libc::pid_t) -> Option<libc::rusage_info_v4> {
    // SAFETY: rusage_info_v4 is plain old data; all-zero is a valid value.
    let mut usage: libc::rusage_info_v4 = unsafe { zeroed() };
    // SAFETY: the kernel writes one rusage_info_v4 for the RUSAGE_INFO_V4 flavor.
    let result =
        unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, (&raw mut usage).cast()) };
    (result == 0).then_some(usage)
}

fn executable(pid: libc::pid_t) -> Field<PathBuf> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buffer` is writable for the length we pass.
    let length =
        unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return field_for(&io::Error::last_os_error());
    }
    buffer.truncate(length as usize);
    Field::Known(PathBuf::from(String::from_utf8_lossy(&buffer).into_owned()))
}

fn working_directory(pid: libc::pid_t) -> Field<PathBuf> {
    // SAFETY: proc_vnodepathinfo is plain old data; all-zero is a valid value.
    let mut info: libc::proc_vnodepathinfo = unsafe { zeroed() };
    let size = size_of::<libc::proc_vnodepathinfo>() as c_int;
    // SAFETY: `info` is a writable buffer of exactly `size` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if written != size {
        return field_for(&io::Error::last_os_error());
    }
    let path = c_string(info.pvi_cdir.vip_path.as_flattened());
    if path.is_empty() {
        Field::Unsupported
    } else {
        Field::Known(PathBuf::from(path))
    }
}

pub(super) fn field_for<T>(error: &io::Error) -> Field<T> {
    match error.raw_os_error() {
        Some(libc::ESRCH) => Field::Gone,
        _ => Field::Denied,
    }
}

fn c_string(raw: &[c_char]) -> String {
    let bytes: Vec<u8> = raw
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn mach_to_nanos(ticks: u64) -> u64 {
    static TIMEBASE: OnceLock<(u64, u64)> = OnceLock::new();
    let (numer, denom) = *TIMEBASE.get_or_init(|| {
        let mut info = mach2::mach_time::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: `info` is a valid out-pointer.
        unsafe { mach2::mach_time::mach_timebase_info(&raw mut info) };
        (u64::from(info.numer.max(1)), u64::from(info.denom.max(1)))
    });
    // Widen to avoid overflow on long-lived processes.
    (u128::from(ticks) * u128::from(numer) / u128::from(denom)) as u64
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{c_string, verdict};

    #[test]
    fn reads_the_slow_way_only_when_the_table_is_unsound() {
        // SAFETY: getuid has no failure mode.
        let uid = unsafe { libc::getuid() };

        let entries = super::processes_from(Err(io::ErrorKind::InvalidData.into()), uid).unwrap();
        assert!(
            entries
                .iter()
                .any(|entry| entry.key.pid.0 == std::process::id())
        );

        let error = super::processes_from(Err(io::ErrorKind::PermissionDenied.into()), uid)
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn decides_about_exec_versions_only_on_a_clear_answer() {
        assert_eq!(verdict(&Ok(7)), Some(true));
        assert_eq!(
            verdict(&Err(io::Error::from_raw_os_error(libc::EINVAL))),
            Some(false)
        );
        assert_eq!(
            verdict(&Err(io::Error::from_raw_os_error(libc::EAGAIN))),
            None
        );
    }

    #[test]
    fn c_string_stops_at_the_first_nul() {
        let raw = [
            b'n' as i8, b'o' as i8, b'd' as i8, b'e' as i8, 0, b'x' as i8,
        ];
        assert_eq!(c_string(&raw), "node");
    }
}
