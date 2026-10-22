//! What Windows ends along with a process: the processes of each job it
//! holds that ends them all once its last handle closes, as the job Node
//! starts programs in does. Read from the process's own list of handles.

use std::io;
use std::mem::offset_of;
use std::sync::OnceLock;

use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessHandleInformation};
use windows_sys::Win32::Foundation::{
    DuplicateHandle, ERROR_INVALID_HANDLE, ERROR_MORE_DATA, HANDLE, NTSTATUS,
    STATUS_INFO_LENGTH_MISMATCH, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject,
};
use windows_sys::Win32::System::SystemServices::JOB_OBJECT_QUERY;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, PROCESS_DUP_HANDLE, PROCESS_QUERY_INFORMATION, PROCESS_SYNCHRONIZE,
    WaitForSingleObject,
};

use super::process::{Handle, hold_with};
use crate::{Presence, ProcessKey};

/// The most a list of handles may take, some 400 000 of them.
const MOST_BYTES: usize = 16 * 1024 * 1024;
/// How often a growing answer is asked for again.
const TRIES: usize = 4;

/// One handle as `ProcessHandleInformation` lists it, after a head of two
/// words: how many there are, and one reserved.
#[repr(C)]
#[derive(Clone, Copy)]
struct Entry {
    value: HANDLE,
    _handles: usize,
    _pointers: usize,
    _access: u32,
    kind: u32,
    _attributes: u32,
    _reserved: u32,
}

/// The pids of the processes that the jobs held by the process `key` names
/// would end once it is gone, itself among them where it is in one. A
/// process that exited holds none.
pub(crate) fn bound_to(key: ProcessKey) -> io::Result<Vec<u32>> {
    let access = PROCESS_QUERY_INFORMATION | PROCESS_DUP_HANDLE | PROCESS_SYNCHRONIZE;
    let process = match hold_with(key, access) {
        Ok(process) => process,
        Err(Presence::Gone) => return Ok(Vec::new()),
        Err(_) => {
            return Err(io::Error::other(
                "couldn't make sure of a process to read its jobs",
            ));
        }
    };
    // SAFETY: the handle is open with the right to wait on it.
    match unsafe { WaitForSingleObject(process.0, 0) } {
        WAIT_OBJECT_0 => return Ok(Vec::new()),
        WAIT_TIMEOUT => {}
        _ => return Err(io::Error::last_os_error()),
    }
    let job = job_kind()?;
    let mut pids = Vec::new();
    for entry in handles(process.0)?.iter().filter(|entry| entry.kind == job) {
        // Closed since it was listed: no longer held.
        let Some(held) = duplicate(&process, entry.value)? else {
            continue;
        };
        if ends_with_last_handle(&held)? {
            pids.extend(members(&held)?);
        }
    }
    Ok(pids)
}

/// How this boot of Windows tells a job among the kinds of handles, found
/// from a job of our own.
fn job_kind() -> io::Result<u32> {
    static KIND: OnceLock<u32> = OnceLock::new();
    if let Some(kind) = KIND.get() {
        return Ok(*kind);
    }
    // SAFETY: no attributes and no name are asked for.
    let ours = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if ours.is_null() {
        return Err(io::Error::last_os_error());
    }
    let ours = Handle(ours);
    // SAFETY: the pseudo handle always names this process.
    let kind = handles(unsafe { GetCurrentProcess() })?
        .into_iter()
        .find(|entry| entry.value == ours.0)
        .map(|entry| entry.kind)
        .ok_or_else(|| io::Error::other("couldn't find a job of our own among our handles"))?;
    Ok(*KIND.get_or_init(|| kind))
}

/// Every handle `process` holds, as the system lists them.
fn handles(process: HANDLE) -> io::Result<Vec<Entry>> {
    const HEAD: usize = 2 * size_of::<usize>();
    let mut size: usize = 64 * 1024;
    for _ in 0..TRIES {
        // u64s keep the entries aligned.
        let mut buffer = vec![0u64; size.div_ceil(8)];
        let mut returned = 0u32;
        // SAFETY: the buffer holds at least `size` bytes, as many as may be written.
        let status: NTSTATUS = unsafe {
            NtQueryInformationProcess(
                process,
                ProcessHandleInformation,
                buffer.as_mut_ptr().cast(),
                size as u32,
                &mut returned,
            )
        };
        let returned = returned as usize;
        if status == STATUS_INFO_LENGTH_MISMATCH {
            // Room for the handles opened meanwhile too.
            size = returned.max(size) * 2;
            if size > MOST_BYTES {
                break;
            }
            continue;
        }
        if status != 0 {
            return Err(io::Error::other(format!(
                "Windows wouldn't list a process's handles ({status:#x})"
            )));
        }
        // SAFETY: the head is a word, and at least one was written.
        let count = unsafe { buffer.as_ptr().cast::<usize>().read() };
        let fits = count
            .checked_mul(size_of::<Entry>())
            .and_then(|bytes| bytes.checked_add(HEAD))
            .is_some_and(|bytes| bytes <= returned.min(size));
        if !fits {
            return Err(io::Error::other(
                "a process's handles were listed in a form not known here",
            ));
        }
        // SAFETY: `count` entries follow the head, all written, and u64s align them.
        let entries = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<u8>().add(HEAD).cast::<Entry>(),
                count,
            )
        };
        return Ok(entries.to_vec());
    }
    Err(io::Error::other("a process holds too many handles to read"))
}

/// A handle of ours to the job `process` holds as `value`, or `None` if it
/// closed it meanwhile.
fn duplicate(process: &Handle, value: HANDLE) -> io::Result<Option<Handle>> {
    let mut ours: HANDLE = std::ptr::null_mut();
    // SAFETY: the process handle has the right to duplicate; `ours` is ours to fill.
    let copied = unsafe {
        DuplicateHandle(
            process.0,
            value,
            GetCurrentProcess(),
            &mut ours,
            JOB_OBJECT_QUERY,
            0,
            0,
        )
    };
    if copied != 0 {
        return Ok(Some(Handle(ours)));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_INVALID_HANDLE as i32) {
        Ok(None)
    } else {
        Err(error)
    }
}

fn ends_with_last_handle(job: &Handle) -> io::Result<bool> {
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // SAFETY: the limits are as large as the call is told.
    let read = unsafe {
        QueryInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&raw mut limits).cast(),
            size_of_val(&limits) as u32,
            std::ptr::null_mut(),
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(limits.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0)
}

/// The pids in `job` and in the jobs nested in it.
fn members(job: &Handle) -> io::Result<Vec<u32>> {
    let first = offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
    let mut room = 64usize;
    for _ in 0..TRIES {
        // Words keep the list aligned; its head takes the first.
        let mut buffer = vec![0usize; first + room];
        // SAFETY: the buffer is as large as the call is told.
        let read = unsafe {
            QueryInformationJobObject(
                job.0,
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer[..]) as u32,
                std::ptr::null_mut(),
            )
        };
        if read == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
                return Err(error);
            }
        }
        // SAFETY: the buffer starts with the list's head, written or zeroed.
        let head = unsafe {
            buffer
                .as_ptr()
                .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>()
                .read()
        };
        let (assigned, listed) = (
            head.NumberOfAssignedProcesses as usize,
            head.NumberOfProcessIdsInList as usize,
        );
        if read != 0 && listed <= room && listed == assigned {
            return Ok(buffer[first..first + listed]
                .iter()
                .map(|&pid| pid as u32)
                .collect());
        }
        // Room for the processes added meanwhile too.
        room = assigned.max(room) * 2;
    }
    Err(io::Error::other("a job kept growing while it was read"))
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<Entry>() == 40);
