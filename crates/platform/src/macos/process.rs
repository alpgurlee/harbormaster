use std::ffi::{c_char, c_int};
use std::io;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use super::{all_pids, procargs};
use crate::{Field, Owner, Pid, ProcessDetails, ProcessEntry, ProcessKey, StartTime};

pub(crate) fn processes() -> io::Result<Vec<ProcessEntry>> {
    // SAFETY: getuid has no failure mode.
    let own_uid = unsafe { libc::getuid() };
    let entries = all_pids()?
        .into_iter()
        .filter_map(|pid| {
            let (info, key) = identify(pid).ok()?;
            // Drop the figures if the pid changed hands while we read them.
            let usage = rusage(pid).filter(|_| identify(pid).is_ok_and(|(_, again)| again == key));
            Some(ProcessEntry {
                key,
                parent: (info.pbi_ppid > 0).then_some(Pid(info.pbi_ppid)),
                name: name_of(&info),
                owner: match info.pbi_uid {
                    uid if uid == own_uid => Owner::CurrentUser,
                    0 => Owner::System,
                    _ => Owner::OtherUser,
                },
                memory_bytes: usage.map(|usage| usage.ri_phys_footprint),
                cpu_time: usage.map(|usage| {
                    Duration::from_nanos(mach_to_nanos(usage.ri_user_time + usage.ri_system_time))
                }),
            })
        })
        .collect();
    Ok(entries)
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
    let nanos = info.pbi_start_tvsec * 1_000_000_000 + info.pbi_start_tvusec * 1_000;
    let key = ProcessKey {
        pid: Pid(pid.unsigned_abs()),
        start: StartTime(nanos),
        exec: exec_version(pid),
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

/// xnu bumps `p_idversion` on exec. 0 if this macOS won't say, which leaves
/// the pid and start time to tell processes apart.
fn exec_version(pid: libc::pid_t) -> u32 {
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
        info.id_version as u32
    } else {
        0
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
    use super::c_string;

    #[test]
    fn c_string_stops_at_the_first_nul() {
        let raw = [
            b'n' as i8, b'o' as i8, b'd' as i8, b'e' as i8, 0, b'x' as i8,
        ];
        assert_eq!(c_string(&raw), "node");
    }
}
