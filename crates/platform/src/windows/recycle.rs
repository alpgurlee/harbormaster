//! The Recycle Bin, through the shell, so that File Explorer can put an
//! item back. The shell deletes for good what the bin can't take, for its
//! size or because the drive keeps no bin; it is stopped before it does,
//! and the item stays where it was.

use std::cell::Cell;
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, PathBuf, Prefix};

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
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GetFileInformationByHandle, GetFinalPathNameByHandleW, OPEN_EXISTING, VOLUME_NAME_DOS,
};

/// The folder at the root of each drive that holds its Recycle Bin.
const BIN: &str = "$Recycle.Bin";

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
    if attributes(&item)? & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a link isn't moved to the Recycle Bin",
        ));
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
        return Err(io::Error::other(
            "it is too large for the Recycle Bin, or the drive keeps none, so it was left where it was",
        ));
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

fn attributes(item: &OwnedHandle) -> io::Result<u32> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open and `information` is ours to fill.
    if unsafe { GetFileInformationByHandle(item.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information.dwFileAttributes)
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

    use super::{drive_of, in_bin};

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
