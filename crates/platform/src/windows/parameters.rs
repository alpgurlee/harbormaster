//! A process's command line and working folder, which Windows keeps in the
//! process's own memory: the command line is asked for, the working folder
//! read out of the process's parameters.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::OnceLock;

use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, PROCESSINFOCLASS, ProcessBasicInformation, ProcessWow64Information,
};
use windows_sys::Win32::Foundation::NTSTATUS;
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::SystemInformation::{IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, IsWow64Process2, PROCESS_BASIC_INFORMATION,
};

use super::process::Handle;
use crate::Field;
use crate::command_line::arguments;
use crate::peb::{Layout, Text, descriptor_in, pointer_in, text_in};

/// `ProcessCommandLineInformation`: the command line, answered into the
/// caller's buffer as a text descriptor followed by the text. Windows
/// doesn't document it, but has answered it since 8.1 for a handle with
/// limited query rights.
const COMMAND_LINE_INFORMATION: PROCESSINFOCLASS = 60;
const STATUS_SUCCESS: NTSTATUS = 0;
const STATUS_INFO_LENGTH_MISMATCH: NTSTATUS = 0xC000_0004_u32 as NTSTATUS;
/// The most a command line or a path may take: Windows allows 32 767
/// UTF-16 units, and the answer carries a descriptor too.
const MOST_BYTES: usize = 128 * 1024;
/// How often a growing answer is asked for again.
const TRIES: usize = 3;

/// Whether this is a 64-bit build on x64 Windows, where the layouts below
/// hold. Elsewhere, under emulation on ARM64 too, nothing is read.
fn on_x64() -> bool {
    static ON_X64: OnceLock<bool> = OnceLock::new();
    *ON_X64.get_or_init(|| {
        if !cfg!(target_arch = "x86_64") {
            return false;
        }
        let (mut process, mut native): (IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE) = (0, 0);
        // SAFETY: the pseudo handle is always valid; both out-params are ours.
        let asked = unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut native) };
        asked != 0 && native == IMAGE_FILE_MACHINE_AMD64
    })
}

/// The command line the process was started with, split as it sees it.
pub(super) fn command_line(process: &Handle) -> Field<Vec<String>> {
    if !on_x64() {
        return Field::Unsupported;
    }
    let mut size = 1024usize;
    for _ in 0..TRIES {
        // u64s keep the descriptor's pointer aligned.
        let mut buffer = vec![0u64; size.div_ceil(8)];
        let mut returned = 0u32;
        // SAFETY: the buffer holds at least `size` bytes, as many as may be written.
        let status = unsafe {
            NtQueryInformationProcess(
                process.0,
                COMMAND_LINE_INFORMATION,
                buffer.as_mut_ptr().cast(),
                size as u32,
                &mut returned,
            )
        };
        let returned = returned as usize;
        if status == STATUS_INFO_LENGTH_MISMATCH {
            size = if returned > size { returned } else { size * 4 };
            if size > MOST_BYTES {
                return Field::Denied;
            }
            continue;
        }
        if status != STATUS_SUCCESS || returned > size {
            return Field::Denied;
        }
        // SAFETY: the u64s are one run of `size_of_val` bytes, `returned` of them written.
        let image = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<u8>(),
                std::mem::size_of_val(&buffer[..]),
            )
        };
        let base = buffer.as_ptr() as u64;
        let line = descriptor_in(&image[..returned], 0, Layout::Bits64).and_then(|text| {
            let at = usize::try_from(text.at.checked_sub(base)?).ok()?;
            text_in(&image[..returned], at, text.bytes)
        });
        return match line {
            Some(line) => Field::Known(arguments(&line)),
            None => Field::Denied,
        };
    }
    Field::Denied
}

/// The process's working folder, read out of its parameters. The handle
/// needs the right to read its memory.
pub(super) fn working_folder(process: &Handle) -> Field<PathBuf> {
    if !on_x64() {
        return Field::Unsupported;
    }
    let Some((peb, layout)) = peb_of(process) else {
        return Field::Denied;
    };
    // Every address below comes out of the process's own memory, which it
    // may have put anything in.
    let folder = layout
        .parameters_at(peb)
        .and_then(|at| read(process, at, layout.descriptor_size() / 2))
        .and_then(|raw| pointer_in(&raw, 0, layout))
        .filter(|&parameters| parameters != 0)
        .and_then(|parameters| layout.working_folder_at(parameters))
        .and_then(|at| read(process, at, layout.descriptor_size()))
        .and_then(|raw| descriptor_in(&raw, 0, layout))
        .filter(|text| text.bytes > 0 && text.bytes <= MOST_BYTES)
        .and_then(|Text { bytes, at }| {
            let raw = read(process, at, bytes)?;
            text_in(&raw, 0, bytes)
        });
    match folder {
        Some(folder) => Field::Known(path(&folder)),
        None => Field::Denied,
    }
}

/// A path as Windows spells it, without the separator a working folder
/// ends in; a root keeps its own.
fn path(text: &str) -> PathBuf {
    let units: Vec<u16> = text.encode_utf16().collect();
    PathBuf::from(OsString::from_wide(&units))
        .components()
        .collect()
}

/// Where the process's PEB is, and how it is laid out: a 32-bit process on
/// 64-bit Windows has a 32-bit PEB of its own.
fn peb_of(process: &Handle) -> Option<(u64, Layout)> {
    let mut wow64 = 0usize;
    let mut returned = 0u32;
    // SAFETY: `wow64` is as large as the class answers with, a pointer.
    let status = unsafe {
        NtQueryInformationProcess(
            process.0,
            ProcessWow64Information,
            (&raw mut wow64).cast(),
            std::mem::size_of::<usize>() as u32,
            &mut returned,
        )
    };
    if status == STATUS_SUCCESS && wow64 != 0 {
        return Some((wow64 as u64, Layout::Bits32));
    }
    // SAFETY: zeroed is a valid PROCESS_BASIC_INFORMATION: plain integers and pointers.
    let mut basic: PROCESS_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `basic` is as large as the class answers with.
    let status = unsafe {
        NtQueryInformationProcess(
            process.0,
            ProcessBasicInformation,
            (&raw mut basic).cast(),
            std::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
            &mut returned,
        )
    };
    (status == STATUS_SUCCESS && !basic.PebBaseAddress.is_null())
        .then_some((basic.PebBaseAddress as u64, Layout::Bits64))
}

/// `bytes` bytes of the process's memory at `at`, all of them or none.
fn read(process: &Handle, at: u64, bytes: usize) -> Option<Vec<u8>> {
    let mut buffer = vec![0u8; bytes];
    let mut got = 0usize;
    // SAFETY: the buffer holds `bytes` bytes, as many as are asked for.
    let read = unsafe {
        ReadProcessMemory(
            process.0,
            at as usize as *const std::ffi::c_void,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut got,
        )
    };
    (read != 0 && got == bytes).then_some(buffer)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::path;

    #[test]
    fn keeps_a_root_whole_and_drops_the_separator_a_folder_ends_in() {
        assert_eq!(path(r"C:\"), Path::new(r"C:\"));
        assert_eq!(path(r"\\server\share\"), Path::new(r"\\server\share\"));
        assert_eq!(path(r"C:\Users\dev\"), Path::new(r"C:\Users\dev"));
        assert_eq!(path(r"C:\Users\dev"), Path::new(r"C:\Users\dev"));
    }
}
