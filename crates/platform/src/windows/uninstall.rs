//! Starting a program's own uninstaller, as its uninstall entry names it.
//! Nothing runs that the entry doesn't name, and an administrator is asked
//! for only for an uninstaller of the whole PC's kept where only
//! administrators can change it.

use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, ERROR_ELEVATION_REQUIRED};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Threading::{CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW};
use windows_sys::Win32::UI::Shell::{
    FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Windows, SEE_MASK_NOASYNC,
    SHELLEXECUTEINFOW, SHEvaluateSystemCommandTemplate, SHGetKnownFolderPath, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_sys::core::{GUID, PWSTR};

use super::installed::{Installed, Scope, entry};

/// Starts the uninstaller of `listed`, if its entry is still as listed.
/// Whether the program then goes is the uninstaller's to say.
pub fn start_uninstaller(listed: &Installed) -> io::Result<()> {
    let now = entry(listed.registry, &listed.key)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "it is no longer installed"))?;
    if now.uninstall != listed.uninstall {
        return Err(io::Error::other(
            "its uninstall entry changed since it was listed",
        ));
    }
    let (program, command, parameters) = evaluated(&listed.uninstall)?;
    // Some uninstallers count on starting in their own folder.
    let folder = listed
        .location
        .clone()
        .filter(|location| location.is_dir())
        .or_else(|| program.parent().map(Path::to_path_buf))
        .ok_or_else(|| io::Error::other("its uninstaller has no folder to start in"))?;
    match created(&program, &command, &folder) {
        Err(error) if error.raw_os_error() == Some(ERROR_ELEVATION_REQUIRED as i32) => {
            if listed.registry.scope == Scope::User {
                return Err(io::Error::other(
                    "it asks for an administrator, which an app of yours shouldn't; start it from Windows Settings if you trust it",
                ));
            }
            if !protected(&program) {
                return Err(io::Error::other(
                    "it asks for an administrator, yet isn't kept where only administrators can change it; start it from Windows Settings if you trust it",
                ));
            }
            elevated(&program, &parameters, &folder)
        }
        started => started,
    }
}

/// The program `template` names, the command line to start it with, and
/// the parameters alone, as Windows reads the command, refusing one whose
/// program it can't be sure of, such as an unquoted path with spaces.
fn evaluated(template: &str) -> io::Result<(PathBuf, Vec<u16>, Vec<u16>)> {
    let template: Vec<u16> = template.encode_utf16().chain([0]).collect();
    let mut program: PWSTR = std::ptr::null_mut();
    let mut command: PWSTR = std::ptr::null_mut();
    let mut parameters: PWSTR = std::ptr::null_mut();
    // SAFETY: the template ends in a nul; each out pointer is ours to fill.
    let done = unsafe {
        SHEvaluateSystemCommandTemplate(
            template.as_ptr(),
            &mut program,
            &mut command,
            &mut parameters,
        )
    };
    // SAFETY: each is null or a string the call allocated, taken once.
    let (program, command, parameters) =
        unsafe { (taken(program), taken(command), taken(parameters)) };
    if done < 0 || program.is_empty() {
        return Err(io::Error::other(
            "Windows can't tell for sure which program its uninstaller is",
        ));
    }
    let program = PathBuf::from(std::ffi::OsString::from_wide(&program));
    Ok((
        program,
        command.into_iter().chain([0]).collect(),
        parameters.into_iter().chain([0]).collect(),
    ))
}

/// The units of the string at `text`, freed after, or none for null.
///
/// # Safety
/// `text` is null or a nul-ended string from the COM allocator, not used after.
unsafe fn taken(text: PWSTR) -> Vec<u16> {
    if text.is_null() {
        return Vec::new();
    }
    let mut length = 0;
    // SAFETY: the string ends in a nul, as the caller promises.
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` units were read above.
    let units = unsafe { std::slice::from_raw_parts(text, length) }.to_vec();
    // SAFETY: the COM allocator gave it, and nothing uses it after.
    unsafe { CoTaskMemFree(text.cast()) };
    units
}

/// Starts `program` with `command` in `folder`, as this user.
fn created(program: &Path, command: &[u16], folder: &Path) -> io::Result<()> {
    let program = wide(program);
    let folder = wide(folder);
    let mut command = command.to_vec();
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut started = PROCESS_INFORMATION::default();
    // SAFETY: every string ends in a nul; the command line is ours to change.
    let done = unsafe {
        CreateProcessW(
            program.as_ptr(),
            command.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            folder.as_ptr(),
            &startup,
            &mut started,
        )
    };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both handles came from CreateProcessW and are closed once.
    unsafe {
        CloseHandle(started.hProcess);
        CloseHandle(started.hThread);
    }
    Ok(())
}

/// Starts `program` with `parameters` in `folder` as an administrator, once
/// Windows has asked the user.
fn elevated(program: &Path, parameters: &[u16], folder: &Path) -> io::Result<()> {
    let verb: Vec<u16> = "runas".encode_utf16().chain([0]).collect();
    let program = wide(program);
    let folder = wide(folder);
    let mut asked = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: verb.as_ptr(),
        lpFile: program.as_ptr(),
        lpParameters: parameters.as_ptr(),
        lpDirectory: folder.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // SAFETY: every string ends in a nul and outlives the call.
    if unsafe { ShellExecuteExW(&mut asked) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
            return Err(io::Error::other("the administrator was declined"));
        }
        return Err(error);
    }
    Ok(())
}

/// Whether `program` is kept where only administrators can change it: in
/// Program Files or in the Windows folder.
fn protected(program: &Path) -> bool {
    let Ok(program) = super::canonical(program) else {
        return false;
    };
    [
        FOLDERID_ProgramFiles,
        FOLDERID_ProgramFilesX86,
        FOLDERID_Windows,
    ]
    .iter()
    .filter_map(known_folder)
    .any(|folder| within(&program, &folder))
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut path: PWSTR = std::ptr::null_mut();
    // SAFETY: `id` is a known folder's id and `path` is ours to fill.
    let done = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut path) };
    // SAFETY: null or a string the call allocated, taken once.
    let units = unsafe { taken(path) };
    (done >= 0 && !units.is_empty()).then(|| PathBuf::from(std::ffi::OsString::from_wide(&units)))
}

/// Whether `path` is inside `folder`, names compared whatever their case.
fn within(path: &Path, folder: &Path) -> bool {
    let mut names = path.components();
    folder.components().all(|own| {
        names.next().is_some_and(|name| match (own, name) {
            (Component::Normal(own), Component::Normal(name)) => {
                own.to_string_lossy().to_lowercase() == name.to_string_lossy().to_lowercase()
            }
            (own, name) => own == name,
        })
    }) && path.components().count() > folder.components().count()
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}
