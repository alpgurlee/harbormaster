use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, FILETIME,
    HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, IsWellKnownSid, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
    WELL_KNOWN_SID_TYPE, WinLocalServiceSid, WinLocalSystemSid, WinNetworkServiceSid,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_VM_READ,
    QueryFullProcessImageNameW, WaitForSingleObject,
};

use super::parameters;

use crate::{
    Field, Owner, Pid, Presence, ProcessDetails, ProcessEntry, ProcessKey, Signal, SignalOutcome,
    StartTime,
};

/// FILETIME counts 100 ns ticks from 1601; a StartTime counts nanoseconds
/// from 1970.
const TICKS_BEFORE_1970: u64 = 116_444_736_000_000_000;
/// The longest path Windows allows, in UTF-16 units.
const LONGEST_PATH: usize = 32_768;

/// A handle, closed when dropped.
pub(super) struct Handle(pub(super) HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is open and this value alone owns it.
        unsafe { CloseHandle(self.0) };
    }
}

/// While a handle to a process is open, Windows keeps its pid from going to
/// another process, so what is read through the handle is about the process
/// it was opened for.
fn open(pid: u32, access: u32) -> io::Result<Handle> {
    // SAFETY: OpenProcess has no memory-safety preconditions.
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Handle(handle))
    }
}

struct Times {
    start: StartTime,
    cpu: Duration,
}

fn times(process: &Handle) -> Option<Times> {
    let [mut created, mut exited, mut kernel, mut user] = [FILETIME::default(); 4];
    // SAFETY: the handle is open, and each pointer is to a FILETIME of ours.
    let read =
        unsafe { GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user) };
    if read == 0 {
        return None;
    }
    let cpu = ticks(kernel)
        .saturating_add(ticks(user))
        .saturating_mul(100);
    Some(Times {
        start: start_time(ticks(created))?,
        cpu: Duration::from_nanos(cpu),
    })
}

fn ticks(time: FILETIME) -> u64 {
    u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)
}

fn start_time(ticks: u64) -> Option<StartTime> {
    let nanos = ticks.checked_sub(TICKS_BEFORE_1970)?.checked_mul(100)?;
    Some(StartTime(nanos))
}

/// Who `pid` names now. Windows has no exec, so the pid and start time tell
/// a process apart for its whole life.
pub(super) fn identify(pid: u32) -> Option<ProcessKey> {
    let process = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
    Some(ProcessKey {
        pid: Pid(pid),
        start: times(&process)?.start,
        exec: 0,
    })
}

pub(crate) fn tracks_exec() -> bool {
    true
}

pub(crate) fn processes() -> io::Result<Vec<ProcessEntry>> {
    // SAFETY: the current process's pseudo handle needs no closing.
    let own = user(unsafe { GetCurrentProcess() })
        .ok_or_else(|| io::Error::other("couldn't read whom this process runs as"))?;
    // Each process the first snapshot lists, held open: while it is, its pid
    // names no other process, so the second snapshot's row for that pid is
    // about the process held.
    let held: HashMap<u32, Handle> = snapshot()?
        .into_iter()
        .filter_map(|row| {
            Some((
                row.pid,
                open(row.pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?,
            ))
        })
        .collect();
    let mut entries: Vec<ProcessEntry> = snapshot()?
        .into_iter()
        .filter_map(|row| {
            let process = held.get(&row.pid)?;
            entry(row, process, &own)
        })
        .collect();
    keep_true_parents(&mut entries);
    Ok(entries)
}

/// A process as the snapshot lists it.
struct Row {
    pid: u32,
    parent: u32,
    name: String,
}

fn snapshot() -> io::Result<Vec<Row>> {
    // SAFETY: CreateToolhelp32Snapshot has no memory-safety preconditions.
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = Handle(handle);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut rows = Vec::new();
    // SAFETY: the snapshot is open and `entry` gives its own size.
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while more {
        rows.push(Row {
            pid: entry.th32ProcessID,
            parent: entry.th32ParentProcessID,
            name: name(&entry.szExeFile),
        });
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(error);
    }
    Ok(rows)
}

/// The text before the first nul.
fn name(wide: &[u16]) -> String {
    let end = wide
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

/// One process from the snapshot, read through the handle that holds it. A
/// process whose start can't be read can't be told from a later one under
/// its pid, so it is left out.
fn entry(row: Row, process: &Handle, own: &Sid) -> Option<ProcessEntry> {
    let times = times(process)?;
    Some(ProcessEntry {
        key: ProcessKey {
            pid: Pid(row.pid),
            start: times.start,
            exec: 0,
        },
        parent: Some(Pid(row.parent)),
        name: row.name,
        owner: owner(process, own),
        memory_bytes: private_working_set(process),
        cpu_time: Some(times.cpu),
    })
}

/// Windows keeps the parent pid a child started with, after the parent exits
/// and once a later process has that pid: a parent must have started first.
fn keep_true_parents(entries: &mut [ProcessEntry]) {
    let starts: HashMap<Pid, StartTime> = entries
        .iter()
        .map(|entry| (entry.key.pid, entry.key.start))
        .collect();
    for entry in entries {
        let start = entry.key.start;
        entry.parent = entry
            .parent
            .filter(|parent| starts.get(parent).is_some_and(|&first| first <= start));
    }
}

/// What Task Manager counts as a process's memory: its private working set.
fn private_working_set(process: &Handle) -> Option<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        ..Default::default()
    };
    // SAFETY: the handle is open and `counters` is as large as `cb` says.
    let read = unsafe { GetProcessMemoryInfo(process.0, (&raw mut counters).cast(), counters.cb) };
    (read != 0).then_some(counters.PrivateWorkingSetSize as u64)
}

fn owner(process: &Handle, own: &Sid) -> Owner {
    match user(process.0) {
        Some(user) if user == *own => Owner::CurrentUser,
        Some(user) if user.is_service() => Owner::System,
        Some(_) => Owner::OtherUser,
        None => Owner::Unknown,
    }
}

/// A security identifier, copied out of the token that held it. Kept in u32s
/// for the alignment its fields need.
#[derive(Debug, PartialEq, Eq)]
struct Sid(Vec<u32>);

impl Sid {
    /// Copies the SID at `sid`.
    ///
    /// # Safety
    ///
    /// `sid` must point to a valid SID.
    unsafe fn copy(sid: PSID) -> Option<Sid> {
        // SAFETY: the caller vouches for `sid`.
        let length = unsafe { GetLengthSid(sid) } as usize;
        if length == 0 {
            return None;
        }
        let mut words = vec![0u32; length.div_ceil(4)];
        // SAFETY: `sid` holds `length` bytes, and `words` has room for them.
        unsafe {
            std::ptr::copy_nonoverlapping(sid.cast::<u8>(), words.as_mut_ptr().cast(), length);
        }
        Some(Sid(words))
    }

    fn is(&self, kind: WELL_KNOWN_SID_TYPE) -> bool {
        // SAFETY: the words hold a whole SID, which IsWellKnownSid only reads.
        unsafe { IsWellKnownSid(self.0.as_ptr().cast_mut().cast(), kind) != 0 }
    }

    /// Windows' own accounts, which run its services.
    fn is_service(&self) -> bool {
        [WinLocalSystemSid, WinLocalServiceSid, WinNetworkServiceSid]
            .into_iter()
            .any(|kind| self.is(kind))
    }
}

/// The user a process runs as, from its token.
fn user(process: HANDLE) -> Option<Sid> {
    let mut token = std::ptr::null_mut();
    // SAFETY: `process` is open, and `token` receives a new handle.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let token = Handle(token);
    let mut needed = 0;
    // SAFETY: an empty buffer only asks for the size needed.
    unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return None;
    }
    // u64s keep the pointer in TOKEN_USER aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer holds at least `needed` bytes.
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if read == 0 {
        return None;
    }
    // SAFETY: the buffer now starts with a TOKEN_USER whose SID it also holds.
    unsafe { Sid::copy((*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid) }
}

/// Whether this process runs as LocalSystem, Windows' own account.
pub(crate) fn runs_as_root() -> bool {
    // SAFETY: the current process's pseudo handle needs no closing.
    user(unsafe { GetCurrentProcess() }).is_some_and(|user| user.is(WinLocalSystemSid))
}

/// Why nothing about a process can be read.
#[derive(Debug, Clone, Copy)]
enum Missing {
    Denied,
    Gone,
}

impl Missing {
    fn of(error: &io::Error) -> Missing {
        // OpenProcess says a pid that names no process is an invalid parameter.
        if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Missing::Gone
        } else {
            Missing::Denied
        }
    }

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

/// The process `key` names, held open, if its pid still names it.
fn reopen(key: ProcessKey) -> Result<Handle, Missing> {
    reopen_with(key, PROCESS_QUERY_LIMITED_INFORMATION)
}

fn reopen_with(key: ProcessKey, access: u32) -> Result<Handle, Missing> {
    let process = open(key.pid.0, access).map_err(|error| Missing::of(&error))?;
    match times(&process) {
        Some(times) if times.start == key.start => Ok(process),
        Some(_) => Err(Missing::Gone),
        None => Err(Missing::Denied),
    }
}

pub(crate) fn details(key: ProcessKey) -> ProcessDetails {
    let process = match reopen(key) {
        Ok(process) => process,
        Err(missing) => return missing.details(),
    };
    // The working folder sits in the process's memory, which takes a right
    // the limited handle lacks; without it, the program and command line
    // are still told.
    let cwd = match reopen_with(key, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ) {
        Ok(readable) => parameters::working_folder(&readable),
        Err(missing) => missing.field(),
    };
    ProcessDetails {
        exe: image(&process),
        args: parameters::command_line(&process),
        cwd,
    }
}

pub(crate) fn executable_of(key: ProcessKey) -> Field<PathBuf> {
    match reopen(key) {
        Ok(process) => image(&process),
        Err(missing) => missing.field(),
    }
}

/// A process's environment isn't read on Windows yet.
pub(crate) fn sets(key: ProcessKey, _name: &str) -> Field<bool> {
    match reopen(key) {
        Ok(_) => Field::Unsupported,
        Err(missing) => missing.field(),
    }
}

/// The program file, asking again with more room while the path is longer.
fn image(process: &Handle) -> Field<PathBuf> {
    let mut room = 260;
    loop {
        let mut buffer = vec![0u16; room];
        let mut length = room as u32;
        // SAFETY: the handle is open and `length` is the buffer's size in UTF-16 units.
        let read = unsafe {
            QueryFullProcessImageNameW(
                process.0,
                PROCESS_NAME_WIN32,
                buffer.as_mut_ptr(),
                &mut length,
            )
        };
        if read != 0 {
            buffer.truncate(length as usize);
            return Field::Known(PathBuf::from(OsString::from_wide(&buffer)));
        }
        let short =
            io::Error::last_os_error().raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER as i32);
        if !short || room == LONGEST_PATH {
            return Field::Denied;
        }
        room = (room * 4).min(LONGEST_PATH);
    }
}

pub(crate) fn presence(key: ProcessKey) -> Presence {
    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
    let process = match open(key.pid.0, access) {
        Ok(process) => process,
        Err(error) => {
            return match Missing::of(&error) {
                Missing::Gone => Presence::Gone,
                Missing::Denied => Presence::Unknown,
            };
        }
    };
    match times(&process) {
        Some(times) if times.start != key.start => return Presence::Replaced,
        Some(_) => {}
        None => return Presence::Unknown,
    }
    // An exited process stays openable while anyone holds a handle to it.
    // SAFETY: the handle is open with SYNCHRONIZE access.
    match unsafe { WaitForSingleObject(process.0, 0) } {
        WAIT_TIMEOUT => Presence::Running,
        WAIT_OBJECT_0 => Presence::Gone,
        _ => Presence::Unknown,
    }
}

/// Stopping a process isn't done on Windows yet: nothing is sent.
pub(crate) fn signal(_key: ProcessKey, _signal: Signal) -> SignalOutcome {
    SignalOutcome::Failed("stopping a process isn't supported on Windows yet".into())
}

#[cfg(test)]
mod tests {
    use super::{TICKS_BEFORE_1970, keep_true_parents, name, start_time};
    use crate::{Owner, Pid, ProcessEntry, ProcessKey, StartTime};

    fn started(pid: u32, start: u64, parent: u32) -> ProcessEntry {
        ProcessEntry {
            key: ProcessKey {
                pid: Pid(pid),
                start: StartTime(start),
                exec: 0,
            },
            parent: Some(Pid(parent)),
            name: String::new(),
            owner: Owner::CurrentUser,
            memory_bytes: None,
            cpu_time: None,
        }
    }

    #[test]
    fn keeps_a_parent_only_if_it_started_before_its_child() {
        let mut entries = [
            started(10, 100, 1),
            started(20, 200, 10),
            started(30, 300, 40),
            started(40, 400, 10),
        ];

        keep_true_parents(&mut entries);

        let parents: Vec<_> = entries.iter().map(|entry| entry.parent).collect();
        // 1 isn't listed, and 40 took its pid after 30 started.
        assert_eq!(parents, [None, Some(Pid(10)), None, Some(Pid(10))]);
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16()
            .chain(std::iter::repeat_n(0, 4))
            .collect()
    }

    #[test]
    fn reads_a_name_up_to_its_nul() {
        assert_eq!(name(&wide("node.exe")), "node.exe");
        assert_eq!(name(&[0x6E, 0x6F]), "no");
        assert_eq!(name(&[0, 0x78]), "");
    }

    #[test]
    fn counts_a_start_from_1970_in_nanoseconds() {
        assert_eq!(start_time(TICKS_BEFORE_1970), Some(StartTime(0)));
        assert_eq!(start_time(TICKS_BEFORE_1970 + 3), Some(StartTime(300)));
        assert_eq!(start_time(TICKS_BEFORE_1970 - 1), None);
    }
}
