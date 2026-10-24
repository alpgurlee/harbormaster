//! The Recycle Bin, through the shell, so that File Explorer can put an
//! item back. The shell deletes for good what the bin can't take, for its
//! size or because the drive keeps no bin. What it says it won't recycle is
//! stopped before anything goes, and a file larger than the bin isn't
//! handed to it, as for one of those it says it will recycle the file and
//! then deletes it; either stays where it was.

use std::cell::Cell;
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::{Mutex, PoisonError};

use windows::Win32::Foundation::E_ABORT;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NO_UI, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation,
    IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem,
    SHCreateItemFromParsingName, TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows_core::{ComObject, HRESULT, PCWSTR, Ref, implement};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetDiskFreeSpaceExW, GetFileInformationByHandle, GetFinalPathNameByHandleW,
    GetVolumeNameForVolumeMountPointW, OPEN_EXISTING, VOLUME_NAME_DOS,
};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

/// The folder at the root of each drive that holds its Recycle Bin.
const BIN: &str = "$Recycle.Bin";
/// Where this user's settings for each drive's Recycle Bin are, by volume,
/// as Recycle Bin Properties writes them.
const SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume";
const MIB: u64 = 1024 * 1024;
/// The least room each drive's Recycle Bin was found to have since this
/// program started, by drive letter: the shell reads its settings once, so
/// a bin made larger since may still be the smaller one to it.
static ROOM: Mutex<[Option<u64>; 26]> = Mutex::new([None; 26]);

/// Moves the file or folder at `path`, on a drive, to that drive's Recycle
/// Bin, and says where it is there. A link is refused rather than moved.
pub(crate) fn trash(path: &Path) -> io::Result<PathBuf> {
    let Some(drive) = drive_of(path) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "only what is on a drive goes to the Recycle Bin",
        ));
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // Held throughout, so that afterwards the kernel can say where this very
    // item went, whatever the shell made of the path.
    let item = open(&wide)?;
    let information = information(&item)?;
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a link isn't moved to the Recycle Bin",
        ));
    }
    let folder = information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    let bytes = u64::from(information.nFileSizeHigh) << 32 | u64::from(information.nFileSizeLow);
    if !folder && bytes > room(drive)? {
        return Err(left_out());
    }
    // The shell needs COM set up its way on the thread it runs on, and the
    // thread calling may be one that is lent out with COM set up otherwise.
    std::thread::scope(|scope| {
        scope
            .spawn(|| recycle(&wide))
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("the shell's move failed unexpectedly")))
    })?;
    let landed = super::plain(final_path(&item)?);
    if !in_bin(&landed, drive) {
        return Err(io::Error::other("the Recycle Bin didn't take it"));
    }
    Ok(landed)
}

/// Asks the shell to recycle `path`, which ends in a nul, on a thread of
/// its own.
fn recycle(path: &[u16]) -> io::Result<()> {
    let _com = Apartment::enter()?;
    let sink = ComObject::new(Recycling::default());
    // SAFETY: COM is set up on this thread while `_com` lives, and `path`
    // ends in a nul.
    let done = unsafe {
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        operation.SetOperationFlags(
            FOF_NO_UI | FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE,
        )?;
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None)?;
        operation.DeleteItem(&item, &sink.to_interface::<IFileOperationProgressSink>())?;
        operation
            .PerformOperations()
            .and_then(|()| operation.GetAnyOperationsAborted())
    };
    if sink.refused.get() {
        return Err(left_out());
    }
    if sink.destroyed.get() {
        return Err(io::Error::other(
            "Windows deleted it for good rather than move it to the Recycle Bin",
        ));
    }
    match done {
        Ok(aborted) if aborted.as_bool() => Err(io::Error::other(
            "the move to the Recycle Bin was cancelled",
        )),
        Ok(_) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn left_out() -> io::Error {
    io::Error::other(
        "it is too large for the Recycle Bin, or the drive keeps none, so it was left where it was",
    )
}

/// The largest file the Recycle Bin of `drive` takes, by this user's
/// settings for it, or by the least it was found to take before.
fn room(drive: u8) -> io::Result<u64> {
    let root = format!("{}:\\", char::from(drive));
    let key = format!(r"{SETTINGS}\{}", volume_of(&root)?);
    // The shell says it won't recycle anything here, which `Recycling` stops.
    if setting(&key, "NukeOnDelete")? == Some(1) {
        return Ok(0);
    }
    let now = match setting(&key, "MaxCapacity")? {
        Some(megabytes) => u64::from(megabytes) * MIB,
        None => default_room(total_of(&root)?),
    };
    let mut rooms = ROOM.lock().unwrap_or_else(PoisonError::into_inner);
    let least = &mut rooms[usize::from(drive.to_ascii_uppercase() - b'A')];
    let room = least.map_or(now, |least| least.min(now));
    *least = Some(room);
    Ok(room)
}

/// How large the Recycle Bin of a drive with `total` bytes for this user is
/// where the user never set its size: a tenth of the first 40 GB and a
/// twentieth of the rest, in whole megabytes.
fn default_room(total: u64) -> u64 {
    const FIRST: u64 = 40 * 1024 * MIB;
    let bytes = total.min(FIRST) / 10 + total.saturating_sub(FIRST) / 20;
    bytes / MIB * MIB
}

/// The `{GUID}` naming the volume at `root`, `X:\`.
fn volume_of(root: &str) -> io::Result<String> {
    let root: Vec<u16> = root.encode_utf16().chain([0]).collect();
    let mut name = [0u16; 64];
    // SAFETY: the root ends in a nul and the buffer holds as many units as said.
    if unsafe {
        GetVolumeNameForVolumeMountPointW(root.as_ptr(), name.as_mut_ptr(), name.len() as u32)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let name = String::from_utf16_lossy(&name);
    name.find('{')
        .zip(name.find('}'))
        .map(|(start, end)| name[start..=end].to_owned())
        .ok_or_else(|| io::Error::other("the drive's volume has no GUID"))
}

/// How many bytes the drive at `root` holds for this user, quotas counted.
fn total_of(root: &str) -> io::Result<u64> {
    let root: Vec<u16> = root.encode_utf16().chain([0]).collect();
    let mut total = 0u64;
    // SAFETY: the root ends in a nul; what isn't asked for is null.
    if unsafe {
        GetDiskFreeSpaceExW(
            root.as_ptr(),
            std::ptr::null_mut(),
            &mut total,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(total)
}

/// The number `name` under `key` in this user's settings, if it is there.
fn setting(key: &str, name: &str) -> io::Result<Option<u32>> {
    let key: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let mut value = 0u32;
    let mut size = 4u32;
    // SAFETY: the key and name end in a nul; `value` has room for the DWORD asked for.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut value).cast(),
            &mut size,
        )
    };
    match read {
        ERROR_SUCCESS => Ok(Some(value)),
        ERROR_FILE_NOT_FOUND => Ok(None),
        error => Err(io::Error::from_raw_os_error(error as i32)),
    }
}

/// COM set up for the shell on this thread, and taken down again.
struct Apartment;

impl Apartment {
    fn enter() -> io::Result<Self> {
        // SAFETY: no reserved pointer; each success is balanced in `drop`.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.ok()?;
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: `enter` succeeded on this thread.
        unsafe { CoUninitialize() };
    }
}

/// Watches a recycling, item by item, and stops it before anything would
/// be deleted for good.
#[implement(IFileOperationProgressSink)]
#[derive(Default)]
struct Recycling {
    /// Something was about to be deleted for good, and the shell was stopped.
    refused: Cell<bool>,
    /// Something was deleted for good all the same.
    destroyed: Cell<bool>,
}

impl IFileOperationProgressSink_Impl for Recycling_Impl {
    fn PreDeleteItem(&self, flags: u32, _: Ref<IShellItem>) -> windows_core::Result<()> {
        // The shell leaves this out when it won't recycle the item.
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            self.refused.set(true);
            return Err(E_ABORT.into());
        }
        Ok(())
    }

    fn PostDeleteItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        done: HRESULT,
        recycled: Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        // What was deleted is named in the Recycle Bin, unless it is gone for good.
        if done.is_ok() && recycled.is_null() {
            self.destroyed.set(true);
        }
        Ok(())
    }

    fn StartOperations(&self) -> windows_core::Result<()> {
        Ok(())
    }

    fn FinishOperations(&self, _: HRESULT) -> windows_core::Result<()> {
        Ok(())
    }

    fn PreRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows_core::Result<()> {
        Ok(())
    }

    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> windows_core::Result<()> {
        Ok(())
    }

    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn UpdateProgress(&self, _: u32, _: u32) -> windows_core::Result<()> {
        Ok(())
    }

    fn ResetTimer(&self) -> windows_core::Result<()> {
        Ok(())
    }

    fn PauseTimer(&self) -> windows_core::Result<()> {
        Ok(())
    }

    fn ResumeTimer(&self) -> windows_core::Result<()> {
        Ok(())
    }
}

/// The drive letter of `X:\...`, the only form a path to recycle may take.
fn drive_of(path: &Path) -> Option<u8> {
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Prefix(prefix)), Some(Component::RootDir)) => match prefix.kind() {
            Prefix::Disk(letter) => Some(letter),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `path` is inside the Recycle Bin of `drive`.
fn in_bin(path: &Path, drive: u8) -> bool {
    drive_of(path).is_some_and(|letter| letter.eq_ignore_ascii_case(&drive))
        && path
            .components()
            .nth(2)
            .is_some_and(|name| name.as_os_str().eq_ignore_ascii_case(OsStr::new(BIN)))
        && path.components().count() > 3
}

/// The item at `path`, which ends in a nul, held without standing in the
/// way of anything done to it: reading its attributes takes no part in
/// sharing. A link is opened as itself.
fn open(path: &[u16]) -> io::Result<OwnedHandle> {
    // SAFETY: `path` ends in a nul; no security attributes or template.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW gave a handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn information(item: &OwnedHandle) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open and `information` is ours to fill.
    if unsafe { GetFileInformationByHandle(item.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information)
}

/// Where the item is now, as the kernel names it, with `\\?\` in front.
fn final_path(item: &OwnedHandle) -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: the handle is open and the buffer holds as many units as said.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                item.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length < buffer.len() {
            buffer.truncate(length);
            return Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer)));
        }
        buffer.resize(length, 0);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{MIB, default_room, drive_of, in_bin};

    #[test]
    fn sizes_a_recycle_bin_never_given_a_size_as_windows_does() {
        // As a 325 GB drive's bin was found to be, with no size set.
        assert_eq!(default_room(348_915_757_056), 18_685 * MIB);
        assert_eq!(default_room(10 * 1024 * MIB), 1024 * MIB);
        assert_eq!(default_room(0), 0);
    }

    #[test]
    fn takes_only_a_path_on_a_drive() {
        assert_eq!(drive_of(Path::new(r"C:\Users\dev\clip.mov")), Some(b'C'));
        assert_eq!(drive_of(Path::new(r"\\?\C:\Users\dev\clip.mov")), None);
        assert_eq!(drive_of(Path::new(r"\\server\share\clip.mov")), None);
        assert_eq!(drive_of(Path::new(r"C:clip.mov")), None);
        assert_eq!(drive_of(Path::new(r"Users\dev\clip.mov")), None);
    }

    #[test]
    fn tells_an_item_in_the_drives_recycle_bin() {
        let bin = r"C:\$Recycle.Bin\S-1-5-21-1-2-3-1001\$R2K1Z9S.mov";
        assert!(in_bin(Path::new(bin), b'C'));
        assert!(in_bin(Path::new(&bin.to_lowercase()), b'C'));
        assert!(!in_bin(Path::new(bin), b'D'));
        assert!(!in_bin(Path::new(r"C:\$Recycle.Bin"), b'C'));
        assert!(!in_bin(Path::new(r"C:\Users\dev\$Recycle.Bin\a"), b'C'));
        assert!(!in_bin(Path::new(r"C:\$Recycle.Binary\a\b"), b'C'));
    }
}
