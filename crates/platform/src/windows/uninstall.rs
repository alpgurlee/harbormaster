//! Starting a program's own uninstaller, as its uninstall entry names it.
//! Nothing runs that the entry doesn't name, and an administrator is asked
//! for only for an uninstaller of the whole PC's kept where only
//! administrators can change it.

use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, ERROR_ELEVATION_REQUIRED, ERROR_SUCCESS, HWND, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    AccessCheck, DACL_SECURITY_INFORMATION, DuplicateToken, GENERIC_MAPPING,
    GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PRIVILEGE_SET, PSECURITY_DESCRIPTOR,
    SecurityImpersonation, TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_ALL_ACCESS, FILE_APPEND_DATA,
    FILE_DELETE_CHILD, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_WRITE_DATA, WRITE_DAC, WRITE_OWNER,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, OpenProcessToken, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows_sys::Win32::UI::Shell::{
    FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC,
    SHELLEXECUTEINFOW, SHEvaluateSystemCommandTemplate, SHGetKnownFolderPath, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, SW_SHOWNORMAL};
use windows_sys::core::{GUID, PWSTR};

use super::installed::{Installed, Scope, entry};

/// Why an uninstaller wasn't started.
#[derive(Debug, thiserror::Error)]
pub enum NotStarted {
    #[error("it is no longer installed")]
    Gone,
    #[error("its uninstall entry changed since it was listed")]
    Changed,
    #[error("Windows can't tell for sure which program its uninstaller is")]
    Unsure,
    /// Only an uninstaller of the whole PC's is started as an administrator.
    #[error("it asks for an administrator, which an app of yours shouldn't")]
    WantsAdministrator,
    #[error("it asks for an administrator, yet isn't kept where only administrators can change it")]
    Unprotected,
    #[error("the administrator was declined")]
    Declined,
    #[error(transparent)]
    Failed(#[from] io::Error),
}

/// Starts the uninstaller of `listed`, if its entry is still as listed.
/// Windows' prompt for an administrator, when it asks, belongs to the
/// window `owner`, a handle to it. Whether the program then goes is the
/// uninstaller's to say.
pub fn start_uninstaller(listed: &Installed, owner: Option<isize>) -> Result<(), NotStarted> {
    let now = entry(listed.registry, &listed.key)?.ok_or(NotStarted::Gone)?;
    if now.uninstall != listed.uninstall {
        return Err(NotStarted::Changed);
    }
    let (program, command, parameters) = evaluated(&now.uninstall).ok_or(NotStarted::Unsure)?;
    let own = program.parent().ok_or(NotStarted::Unsure)?;
    // Some uninstallers count on starting in their own folder.
    let folder = now
        .location
        .filter(|location| location.is_dir())
        .unwrap_or_else(|| own.to_path_buf());
    match created(&program, &command, &folder) {
        Err(error) if error.raw_os_error() == Some(ERROR_ELEVATION_REQUIRED as i32) => {
            if now.registry.scope == Scope::User {
                return Err(NotStarted::WantsAdministrator);
            }
            // What starts is the program where it was found protected, in its
            // own folder, where no one else can leave a library for it to load.
            let program = protected(&program).ok_or(NotStarted::Unprotected)?;
            let own = program.parent().ok_or(NotStarted::Unprotected)?;
            elevated(&program, &parameters, own, owner)
        }
        started => Ok(started?),
    }
}

/// The program the uninstall entry of `listed` starts, as Windows reads the
/// entry, unless Windows can't be sure which it is.
pub fn uninstaller(listed: &Installed) -> Option<PathBuf> {
    evaluated(&listed.uninstall).map(|(program, ..)| program)
}

/// The program `template` names, the command line to start it with, and
/// the parameters alone, as Windows reads the command; none when Windows
/// can't be sure of the program, as for an unquoted path with spaces.
fn evaluated(template: &str) -> Option<(PathBuf, Vec<u16>, Vec<u16>)> {
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
        return None;
    }
    let program = PathBuf::from(std::ffi::OsString::from_wide(&program));
    Some((
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
/// Windows has asked the user in a prompt belonging to `owner`.
fn elevated(
    program: &Path,
    parameters: &[u16],
    folder: &Path,
    owner: Option<isize>,
) -> Result<(), NotStarted> {
    // The shell needs COM set up its way on the thread it runs on, and the
    // thread calling may be one that is lent out with COM set up otherwise.
    std::thread::scope(|scope| {
        scope
            .spawn(|| asked(program, parameters, folder, owner))
            .join()
            .unwrap_or_else(|_| {
                Err(io::Error::other("the shell's start failed unexpectedly").into())
            })
    })
}

fn asked(
    program: &Path,
    parameters: &[u16],
    folder: &Path,
    owner: Option<isize>,
) -> Result<(), NotStarted> {
    let _com = super::Apartment::enter()?;
    let verb: Vec<u16> = "runas".encode_utf16().chain([0]).collect();
    let program = wide(program);
    let folder = wide(folder);
    // A window closed since leaves the prompt to stand on its own.
    let owner = owner
        .map(|owner| owner as HWND)
        // SAFETY: IsWindow takes any handle, a stale one included.
        .filter(|&owner| unsafe { IsWindow(owner) } != 0)
        .unwrap_or(std::ptr::null_mut());
    let mut asking = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        // What goes wrong is told by the error, not by a box of the shell's.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        hwnd: owner,
        lpVerb: verb.as_ptr(),
        lpFile: program.as_ptr(),
        lpParameters: parameters.as_ptr(),
        lpDirectory: folder.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // SAFETY: COM is set up on this thread while `_com` lives, and every
    // string ends in a nul and outlives the call.
    if unsafe { ShellExecuteExW(&mut asking) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
            return Err(NotStarted::Declined);
        }
        return Err(error.into());
    }
    Ok(())
}

/// What changes a file, or swaps it for another: writing it, deleting it,
/// or changing who may.
const CHANGES_A_FILE: u32 = FILE_WRITE_DATA | FILE_APPEND_DATA | DELETE | WRITE_DAC | WRITE_OWNER;
/// What changes what a folder holds: adding a file or a folder to it, such
/// as a library for a program to load, deleting one or the folder itself,
/// or changing who may.
const CHANGES_A_FOLDER: u32 =
    FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY | FILE_DELETE_CHILD | DELETE | WRITE_DAC | WRITE_OWNER;

/// Where `program` really is, if it is kept where only administrators can
/// change it: in Program Files or in the Windows folder, with none of it,
/// the folders from it up to there, or the files beside it, where Windows
/// first looks for its libraries, changeable by this user's programs, which
/// run without an administrator's rights.
fn protected(program: &Path) -> Option<PathBuf> {
    let program = super::canonical(program).ok()?;
    let root = [FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86]
        .iter()
        .filter_map(known_folder)
        .chain(super::windows_folder())
        .find(|folder| within(&program, folder))?;
    let token = Checker::of_this_process().ok()?;
    let unchanged =
        |path: &Path, changes: u32| token.rights(path).is_ok_and(|rights| rights & changes == 0);
    let own = program.parent()?;
    let mut beside = std::fs::read_dir(own).ok()?;
    let mut folders = program
        .ancestors()
        .skip(1)
        .take_while(|folder| folder.components().count() >= root.components().count());
    let protected = unchanged(&program, CHANGES_A_FILE)
        && folders.all(|folder| unchanged(folder, CHANGES_A_FOLDER))
        && beside.all(|entry| {
            entry.is_ok_and(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_dir())
                    || unchanged(&entry.path(), CHANGES_A_FILE)
            })
        });
    protected.then_some(program)
}

/// This process's token, as one to check access by, closed when dropped.
struct Checker(OwnedHandle);

impl Checker {
    fn of_this_process() -> io::Result<Self> {
        let mut own = std::ptr::null_mut();
        // SAFETY: the pseudo handle needs no closing; `own` is ours to fill.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut own) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the token was just opened, and is closed once, by this.
        let own = unsafe { OwnedHandle::from_raw_handle(own) };
        let mut checking = std::ptr::null_mut();
        // SAFETY: the token is open, and `checking` is ours to fill.
        if unsafe { DuplicateToken(own.as_raw_handle(), SecurityImpersonation, &mut checking) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(checking) }))
    }

    /// The rights this token has over the file or folder at `path`, as its
    /// owner and the access control list it has say.
    fn rights(&self, path: &Path) -> io::Result<u32> {
        let path = wide(path);
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: the path ends in a nul; the descriptor is the call's to allocate.
        let read = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if read != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(read as i32));
        }
        let mapping = GENERIC_MAPPING {
            GenericRead: FILE_GENERIC_READ,
            GenericWrite: FILE_GENERIC_WRITE,
            GenericExecute: FILE_GENERIC_EXECUTE,
            GenericAll: FILE_ALL_ACCESS,
        };
        let mut privileges = PRIVILEGE_SET::default();
        let mut length = size_of::<PRIVILEGE_SET>() as u32;
        let (mut granted, mut allowed) = (0, 0);
        // SAFETY: the descriptor is the one just read, the token is open,
        // and every out pointer is ours to fill.
        let checked = unsafe {
            AccessCheck(
                descriptor,
                self.0.as_raw_handle(),
                MAXIMUM_ALLOWED,
                &mapping,
                &mut privileges,
                &mut length,
                &mut granted,
                &mut allowed,
            )
        };
        let error = io::Error::last_os_error();
        // SAFETY: the descriptor was allocated for this call, and is freed once.
        unsafe { LocalFree(descriptor) };
        if checked == 0 {
            return Err(error);
        }
        Ok(if allowed != 0 { granted } else { 0 })
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_what_this_user_can_change_from_what_only_windows_can() {
        let checker = Checker::of_this_process().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let mine = folder.path().join("uninstall.exe");
        std::fs::write(&mine, b"MZ").unwrap();
        // Windows' own programs are its installer's to change, even for an administrator.
        let system = PathBuf::from(std::env::var("SystemRoot").unwrap());
        let windows = system.join(r"System32\whoami.exe");

        assert_ne!(checker.rights(&mine).unwrap() & CHANGES_A_FILE, 0);
        assert_ne!(checker.rights(folder.path()).unwrap() & CHANGES_A_FOLDER, 0);
        assert_eq!(checker.rights(&windows).unwrap() & CHANGES_A_FILE, 0);
        assert_eq!(protected(&mine), None);
    }

    #[test]
    fn takes_a_program_to_be_in_a_folder_only_below_it_whatever_the_case() {
        let folder = Path::new(r"C:\Program Files");

        assert!(within(
            Path::new(r"c:\program files\Tool\uninstall.exe"),
            folder
        ));
        assert!(!within(folder, folder));
        assert!(!within(
            Path::new(r"C:\Program Files Tool\uninstall.exe"),
            folder
        ));
        assert!(!within(
            Path::new(r"D:\Program Files\Tool\uninstall.exe"),
            folder
        ));
    }
}
