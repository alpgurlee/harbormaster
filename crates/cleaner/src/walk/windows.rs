//! The walker on Windows: each name is opened with `NtCreateFile` relative
//! to the folder already open, as itself rather than through what it may
//! point to, and refused when it is a reparse point of any kind: a link, a
//! junction, a mount point, a file kept in the cloud.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
    FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
};
use windows_sys::Win32::Foundation::{
    ERROR_DIRECTORY, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER,
    ERROR_NO_MORE_FILES, ERROR_NOT_SUPPORTED, ERROR_PATH_NOT_FOUND,
    ERROR_REPARSE_POINT_ENCOUNTERED, HANDLE, INVALID_HANDLE_VALUE, NTSTATUS, OBJ_CASE_INSENSITIVE,
    RtlNtStatusToDosError, UNICODE_STRING,
};
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO,
    FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_EXTD_DIR_INFO, FILE_ID_INFO,
    FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO,
    FileBasicInfo, FileDispositionInfo, FileDispositionInfoEx, FileIdExtdDirectoryInfo,
    FileIdExtdDirectoryRestartInfo, FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx,
    GetFinalPathNameByHandleW, OPEN_EXISTING, SYNCHRONIZE, SetFileInformationByHandle,
    VOLUME_NAME_DOS,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::{FileType, Stat, is_file, plain_path};

/// What a folder is opened with: enough to list it and to open names inside it.
const FOLDER: u32 = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
/// What a file opened only to look at it is opened with: nothing is read, so a
/// file kept in the cloud isn't fetched for it.
const LOOK: u32 = FILE_READ_ATTRIBUTES | SYNCHRONIZE;
/// Others may read, write, rename and delete what is open here meanwhile, as
/// they may on the Mac: a scan must never make the user's own tools fail.
const SHARED: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
/// Room for one batch of a listing.
const LISTING: usize = 64 * 1024;
/// The 100-nanosecond ticks from 1601, where Windows counts time from, to 1970.
const TICKS_TO_UNIX_EPOCH: u64 = 116_444_736_000_000_000;

/// An open folder.
#[derive(Debug)]
pub(crate) struct Dir(OwnedHandle);

/// A file opened only to look at it.
#[derive(Debug)]
pub(crate) struct Item(OwnedHandle);

impl Item {
    /// How this file is stored; it may be something else than a file by now.
    pub(crate) fn metadata(&self) -> io::Result<Stat> {
        stat_of(&self.0)
    }

    /// Where this file is now, as the disk spells it.
    pub(crate) fn path(&self) -> io::Result<PathBuf> {
        path_of(&self.0)
    }
}

impl Dir {
    /// The folder at `path`: absolute, on a drive, and a real folder at every step.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let mut components = path.components();
        let letter = match components.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "only folders on a drive are looked through, not on a network share",
                    ));
                }
            },
            _ => return Err(plain_path()),
        };
        if components.next() != Some(Component::RootDir) {
            return Err(plain_path());
        }
        let root: Vec<u16> = format!(r"\\?\{}:\", char::from(letter))
            .encode_utf16()
            .chain([0])
            .collect();
        // SAFETY: the name ends in a nul; no security attributes or template are passed.
        let handle = unsafe {
            CreateFileW(
                root.as_ptr(),
                FOLDER,
                SHARED,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateFileW succeeded, so the handle is open and ours alone.
        let mut dir = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        refuse_reparse_point(&dir.0)?;
        for component in components {
            let Component::Normal(name) = component else {
                return Err(plain_path());
            };
            dir = dir.child(name)?;
        }
        Ok(dir)
    }

    /// The folder `name` inside this one, refused when it is a link or not a folder.
    pub(crate) fn child(&self, name: &OsStr) -> io::Result<Self> {
        let child = open_at(&self.0, name, FOLDER, FILE_DIRECTORY_FILE)?;
        refuse_reparse_point(&child)?;
        Ok(Self(child))
    }

    /// What is called `name` inside this folder, opened only to look at it;
    /// a link is refused.
    pub(crate) fn item(&self, name: &OsStr) -> io::Result<Item> {
        let item = open_at(&self.0, name, LOOK, 0)?;
        refuse_reparse_point(&item)?;
        Ok(Item(item))
    }

    /// Up to `most` bytes from the start of the regular file `name` inside
    /// this folder; a link or a folder in its place is refused.
    pub(crate) fn read_start(&self, name: &OsStr, most: u64) -> io::Result<Vec<u8>> {
        let file = open_at(
            &self.0,
            name,
            FILE_READ_DATA | LOOK,
            FILE_NON_DIRECTORY_FILE,
        )?;
        if !is_file(&stat_of(&file)?) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        let mut start = Vec::new();
        File::from(file).take(most).read_to_end(&mut start)?;
        Ok(start)
    }

    /// Where this folder is now, as the disk spells it, wherever it has
    /// moved since it was opened.
    pub(crate) fn path(&self) -> io::Result<PathBuf> {
        path_of(&self.0)
    }

    /// Removes `name` inside this folder: a file, a link itself, or an
    /// empty folder, whichever `folder` says it is. The name goes at once,
    /// even while others have it open.
    pub(crate) fn unlink(&self, name: &OsStr, folder: bool) -> io::Result<()> {
        let options = if folder { FILE_DIRECTORY_FILE } else { 0 };
        let target = open_at(&self.0, name, DELETE | LOOK, options)?;
        let basic: FILE_BASIC_INFO = query(&target, FileBasicInfo)?;
        // A link to a folder is a folder to Windows, opened as itself here.
        if (kind_of(basic.FileAttributes, false) == FileType::Folder) != folder {
            return Err(io::Error::other(if folder {
                "a link stands where a folder was"
            } else {
                "a folder stands where a file was"
            }));
        }
        let posix = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
        };
        let error = match set(&target, FileDispositionInfoEx, &posix) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        // A disk without POSIX deletion, such as FAT, takes the name away
        // once the last handle closes; a read-only file stays there.
        let unsupported = [ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED]
            .map(|code| Some(code as i32))
            .contains(&error.raw_os_error());
        if !unsupported {
            return Err(error);
        }
        set(
            &target,
            FileDispositionInfo,
            &FILE_DISPOSITION_INFO { DeleteFile: true },
        )
    }

    /// Down `relative`, a real folder at every step.
    pub(crate) fn descend(&self, relative: &Path) -> io::Result<Self> {
        let mut dir = self.reopened()?;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(plain_path());
            };
            dir = dir.child(name)?;
        }
        Ok(dir)
    }

    /// Each name inside, with how it is stored, asking `keep_going` before
    /// each; a link is reported as a link.
    pub(crate) fn entries_while(
        &self,
        keep_going: impl FnMut() -> bool,
    ) -> io::Result<Vec<(OsString, Stat)>> {
        let mut found = Vec::new();
        self.listed(keep_going, true, &mut |name, stat| {
            found.push((name.to_os_string(), *stat));
        })?;
        Ok(found)
    }

    /// Hands `seen` each name inside, as it is read, asking `keep_going`
    /// before each, and says whether the listing broke off before the end.
    pub(crate) fn each_seen(
        &self,
        keep_going: impl FnMut() -> bool,
        seen: &mut dyn FnMut(&OsStr, &Stat),
    ) -> io::Result<bool> {
        self.listed(keep_going, false, seen)
    }

    /// The folder `name` inside this one, if it is still the folder `seen`
    /// described when it was listed: on the same disk, with the same file id.
    pub(crate) fn child_if_same(&self, name: &OsStr, seen: &Stat) -> io::Result<Option<Self>> {
        let child = self.child(name)?;
        let (device, inode) = id_of(&child.0)?;
        Ok((device == seen.device && inode == seen.inode).then_some(child))
    }

    /// How this folder itself is stored.
    pub(crate) fn metadata(&self) -> io::Result<Stat> {
        stat_of(&self.0)
    }

    /// This folder opened again: a listing reads through a handle's own
    /// place in the folder, so each gets a handle of its own.
    fn reopened(&self) -> io::Result<Self> {
        open_relative(&self.0, &[], FOLDER, FILE_DIRECTORY_FILE).map(Self)
    }

    /// The entries a listing of its own reads in batches. Windows lists a
    /// folder whole, each entry with how it is stored, so no entry fails on
    /// its own; a failure partway fails a listing that must be `whole`, and
    /// otherwise ends it, said to be short.
    fn listed(
        &self,
        mut keep_going: impl FnMut() -> bool,
        whole: bool,
        seen: &mut dyn FnMut(&OsStr, &Stat),
    ) -> io::Result<bool> {
        let listing = self.reopened()?;
        // Mount points are reparse points, never entered, so what is listed
        // is on this folder's disk.
        let (device, _) = id_of(&listing.0)?;
        // u64s keep the entries aligned.
        let mut batch = vec![0u64; LISTING / 8];
        let mut class = FileIdExtdDirectoryRestartInfo;
        while keep_going() {
            // SAFETY: the batch holds LISTING bytes, as many as may be written.
            let read = unsafe {
                GetFileInformationByHandleEx(
                    listing.0.as_raw_handle(),
                    class,
                    batch.as_mut_ptr().cast(),
                    LISTING as u32,
                )
            };
            class = FileIdExtdDirectoryInfo;
            if read == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    return Ok(false);
                }
                if whole {
                    return Err(error);
                }
                return Ok(true);
            }
            // SAFETY: the batch is LISTING initialized bytes.
            let bytes = unsafe { std::slice::from_raw_parts(batch.as_ptr().cast::<u8>(), LISTING) };
            for (name, stat) in entries(bytes, device) {
                if !keep_going() {
                    return Ok(false);
                }
                if name != *"." && name != *".." {
                    seen(&name, &stat);
                }
            }
        }
        Ok(false)
    }
}

/// The entries of one batch, each as a name and how it is stored.
fn entries(bytes: &[u8], device: u64) -> Vec<(OsString, Stat)> {
    let name_at = offset_of!(FILE_ID_EXTD_DIR_INFO, FileName);
    let mut found = Vec::new();
    let mut at = 0;
    while at + size_of::<FILE_ID_EXTD_DIR_INFO>() <= bytes.len() {
        // SAFETY: a whole entry's fixed part lies within the bytes.
        let entry: FILE_ID_EXTD_DIR_INFO =
            unsafe { ptr::read_unaligned(bytes[at..].as_ptr().cast()) };
        let start = at + name_at;
        let name = start
            .checked_add(entry.FileNameLength as usize)
            .and_then(|end| bytes.get(start..end));
        let Some(name) = name else {
            break;
        };
        let name: Vec<u16> = name
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&unit| u16::from_le_bytes(unit))
            .collect();
        found.push((
            OsString::from_wide(&name),
            Stat {
                kind: kind_of(entry.FileAttributes, false),
                device,
                inode: u128::from_le_bytes(entry.FileId.Identifier),
                // The listing doesn't say.
                links: None,
                bytes: u64::try_from(entry.AllocationSize).unwrap_or(0),
                modified: from_ticks(entry.LastWriteTime),
            },
        ));
        // The next entry starts past this one's fixed part, or there is none.
        let next = entry.NextEntryOffset as usize;
        if next < name_at {
            break;
        }
        at += next;
    }
    found
}

/// Whether `error`, from opening a folder listed a moment ago, says it went
/// or became something else since: gone, a link, or no longer a folder.
pub(crate) fn changed(error: &io::Error) -> bool {
    [
        ERROR_FILE_NOT_FOUND,
        ERROR_PATH_NOT_FOUND,
        ERROR_DIRECTORY,
        ERROR_REPARSE_POINT_ENCOUNTERED,
    ]
    .map(|code| Some(code as i32))
    .contains(&error.raw_os_error())
}

/// Whether the names `a` and `b` name the same file, as Windows compares
/// file names: ordinally, regardless of case.
pub(crate) fn same_name(a: &OsStr, b: &OsStr) -> bool {
    let a: Vec<u16> = a.encode_wide().collect();
    let b: Vec<u16> = b.encode_wide().collect();
    let (Ok(a_length), Ok(b_length)) = (i32::try_from(a.len()), i32::try_from(b.len())) else {
        return false;
    };
    // SAFETY: each pointer and length describe one of the vectors.
    unsafe { CompareStringOrdinal(a.as_ptr(), a_length, b.as_ptr(), b_length, 1) == CSTR_EQUAL }
}

/// `path` without the `\\?\` that keeps Windows from rewriting it, as people
/// and other programs write it: `C:\Users\dev`, `\\server\share`. Any other
/// kind of path is left as it is.
pub(crate) fn plain(path: PathBuf) -> PathBuf {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path;
    };
    let mut plain = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:", char::from(letter))),
        Prefix::VerbatimUNC(server, share) => {
            let mut root = OsString::from(r"\\");
            root.push(server);
            root.push(r"\");
            root.push(share);
            PathBuf::from(root)
        }
        _ => return path,
    };
    for component in components {
        plain.push(component);
    }
    plain
}

/// `name` inside `parent`, opened as itself with `access` and `options`. A
/// name is one plain name, never a path, so that only it can be a reparse
/// point, and FILE_OPEN_REPARSE_POINT opens that as itself.
fn open_at(
    parent: &OwnedHandle,
    name: &OsStr,
    access: u32,
    options: u32,
) -> io::Result<OwnedHandle> {
    let name: Vec<u16> = name.encode_wide().collect();
    let separator = |unit: &u16| [b'\\', b'/', b':', 0].map(u16::from).contains(unit);
    let dots = name == [u16::from(b'.')] || name == [u16::from(b'.'); 2];
    if name.is_empty() || dots || name.iter().any(separator) {
        return Err(plain_path());
    }
    open_relative(parent, &name, access, options)
}

/// `name` relative to `parent`, or `parent` itself again for no name.
fn open_relative(
    parent: &OwnedHandle,
    name: &[u16],
    access: u32,
    options: u32,
) -> io::Result<OwnedHandle> {
    let length = u16::try_from(name.len() * 2).map_err(|_| plain_path())?;
    let text = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name.as_ptr().cast_mut(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &text,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    // SAFETY: an all-zero status block is valid; NtCreateFile fills it.
    let mut status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    let mut handle: HANDLE = ptr::null_mut();
    // SAFETY: the attributes point at a name that outlives the call, and
    // `parent` is an open folder; no extended attributes are passed.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status,
            ptr::null(),
            0,
            SHARED,
            FILE_OPEN,
            options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            ptr::null(),
            0,
        )
    };
    if result < 0 {
        return Err(nt_error(result));
    }
    // SAFETY: NtCreateFile succeeded, so the handle is open and ours alone.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Refuses what `handle` opened when it is a reparse point.
fn refuse_reparse_point(handle: &OwnedHandle) -> io::Result<()> {
    let basic: FILE_BASIC_INFO = query(handle, FileBasicInfo)?;
    if basic.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::from_raw_os_error(
            ERROR_REPARSE_POINT_ENCOUNTERED as i32,
        ));
    }
    Ok(())
}

/// The disk and file id of what `handle` opened.
fn id_of(handle: &OwnedHandle) -> io::Result<(u64, u128)> {
    let id: FILE_ID_INFO = query(handle, FileIdInfo)?;
    Ok((
        id.VolumeSerialNumber,
        u128::from_le_bytes(id.FileId.Identifier),
    ))
}

/// How what `handle` opened is stored.
fn stat_of(handle: &OwnedHandle) -> io::Result<Stat> {
    let (device, inode) = id_of(handle)?;
    let basic: FILE_BASIC_INFO = query(handle, FileBasicInfo)?;
    let standard: FILE_STANDARD_INFO = query(handle, FileStandardInfo)?;
    Ok(Stat {
        kind: kind_of(basic.FileAttributes, standard.Directory),
        device,
        inode,
        links: Some(u64::from(standard.NumberOfLinks)),
        bytes: u64::try_from(standard.AllocationSize).unwrap_or(0),
        modified: from_ticks(basic.LastWriteTime),
    })
}

fn kind_of(attributes: u32, directory: bool) -> FileType {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        FileType::Link
    } else if directory || attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        FileType::Folder
    } else {
        FileType::File
    }
}

/// The time `ticks` of 100 nanoseconds after 1601 names.
fn from_ticks(ticks: i64) -> SystemTime {
    let ticks = u64::try_from(ticks).unwrap_or(0);
    let unix = |ticks: u64| Duration::new(ticks / 10_000_000, (ticks % 10_000_000) as u32 * 100);
    match ticks.checked_sub(TICKS_TO_UNIX_EPOCH) {
        Some(after) => UNIX_EPOCH.checked_add(unix(after)),
        None => UNIX_EPOCH.checked_sub(unix(TICKS_TO_UNIX_EPOCH - ticks)),
    }
    .unwrap_or(UNIX_EPOCH)
}

/// What Windows says of what `handle` opened, as the `T` that `class` fills.
fn query<T>(handle: &OwnedHandle, class: FILE_INFO_BY_HANDLE_CLASS) -> io::Result<T> {
    let mut info = std::mem::MaybeUninit::<T>::uninit();
    // SAFETY: `info` has room for one T, the structure `class` fills.
    let read = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            class,
            info.as_mut_ptr().cast(),
            size_of::<T>() as u32,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetFileInformationByHandleEx succeeded, so it filled `info`.
    Ok(unsafe { info.assume_init() })
}

/// Tells Windows `info` about what `handle` opened, as `class` describes it.
fn set<T>(handle: &OwnedHandle, class: FILE_INFO_BY_HANDLE_CLASS, info: &T) -> io::Result<()> {
    // SAFETY: `info` is one T, the structure `class` reads.
    let done = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle(),
            class,
            ptr::from_ref(info).cast(),
            size_of::<T>() as u32,
        )
    };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Where what `handle` opened is now, as the disk spells it.
fn path_of(handle: &OwnedHandle) -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: the buffer holds as many units as passed.
        let written = unsafe {
            GetFinalPathNameByHandleW(
                handle.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        // Too small, it says how much room the path and its nul need.
        if written >= buffer.len() {
            if written > u16::MAX as usize {
                return Err(io::Error::from_raw_os_error(
                    ERROR_INSUFFICIENT_BUFFER as i32,
                ));
            }
            buffer.resize(written, 0);
            continue;
        }
        buffer.truncate(written);
        return Ok(plain(PathBuf::from(OsString::from_wide(&buffer))));
    }
}

fn nt_error(status: NTSTATUS) -> io::Error {
    // SAFETY: RtlNtStatusToDosError has no preconditions.
    io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{Dir, plain, same_name};
    use crate::walk::{is_folder, is_link};

    /// A temp folder, under its real path as people write it.
    fn place() -> (tempfile::TempDir, PathBuf) {
        let place = tempfile::tempdir().unwrap();
        let real = plain(place.path().canonicalize().unwrap());
        (place, real)
    }

    /// A junction at `link` to the folder `target`: a link to a folder that
    /// needs no privilege to make.
    fn junction(link: &Path, target: &Path) {
        let made = Command::new("cmd")
            .raw_arg(format!(
                r#"/c mklink /J "{}" "{}""#,
                link.display(),
                target.display()
            ))
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
    }

    #[test]
    fn refuses_a_folder_swapped_for_a_junction_after_its_parent_was_opened() {
        let (_place, real) = place();
        fs::create_dir(real.join("cache")).unwrap();
        fs::create_dir(real.join("elsewhere")).unwrap();
        let parent = Dir::open(&real).unwrap();
        assert!(parent.child(OsStr::new("cache")).is_ok());

        fs::remove_dir(real.join("cache")).unwrap();
        junction(&real.join("cache"), &real.join("elsewhere"));

        let refused = parent.child(OsStr::new("cache")).unwrap_err();
        assert!(super::changed(&refused), "{refused:?}");
        assert!(Dir::open(&real.join("cache")).is_err());
    }

    #[test]
    fn refuses_a_symbolic_link_where_a_folder_was() {
        let (_place, real) = place();
        fs::create_dir(real.join("elsewhere")).unwrap();
        // Making one takes a privilege or developer mode, which CI's runner has.
        if let Err(error) =
            std::os::windows::fs::symlink_dir(real.join("elsewhere"), real.join("cache"))
        {
            assert_eq!(error.raw_os_error(), Some(1314), "{error:?}");
            return;
        }
        let parent = Dir::open(&real).unwrap();

        assert!(parent.child(OsStr::new("cache")).is_err());
        assert!(parent.item(OsStr::new("cache")).is_err());
    }

    #[test]
    fn opens_one_plain_name_at_a_time() {
        let (_place, real) = place();
        fs::create_dir_all(real.join("a/b")).unwrap();
        let dir = Dir::open(&real).unwrap();

        assert!(dir.child(OsStr::new("a")).is_ok());
        for name in [r"a\b", "a/b", "..", ".", "a:b", ""] {
            assert!(dir.child(OsStr::new(name)).is_err(), "{name:?}");
        }
    }

    #[test]
    fn opens_a_folder_written_either_way() {
        let (place, real) = place();
        fs::create_dir(real.join("inner")).unwrap();

        let verbatim = Dir::open(&place.path().canonicalize().unwrap()).unwrap();
        let plain = Dir::open(&real).unwrap();

        assert_eq!(verbatim.path().unwrap(), real);
        assert_eq!(plain.path().unwrap(), real);
        assert!(Dir::open(Path::new(r"inner")).is_err());
    }

    #[test]
    fn stops_listing_when_told() {
        let (_place, real) = place();
        for index in 0..50 {
            fs::write(real.join(format!("file{index}")), b"x").unwrap();
        }
        let mut asked = 0;

        let entries = Dir::open(&real)
            .unwrap()
            .entries_while(|| {
                asked += 1;
                asked <= 10
            })
            .unwrap();

        assert!(entries.len() <= 10, "{}", entries.len());
    }

    #[test]
    fn lists_a_folder_too_large_for_one_batch_whole() {
        let (_place, real) = place();
        let names: Vec<String> = (0..1500)
            .map(|index| format!("a file with a long enough name to fill batches {index:04}"))
            .collect();
        for name in &names {
            fs::write(real.join(name), b"x").unwrap();
        }
        let dir = Dir::open(&real).unwrap();

        let mut listed: Vec<String> = dir
            .entries_while(|| true)
            .unwrap()
            .into_iter()
            .map(|(name, _)| name.into_string().unwrap())
            .collect();
        listed.sort();

        assert_eq!(listed, names);
        // Each listing reads through a handle of its own.
        assert_eq!(dir.entries_while(|| true).unwrap().len(), names.len());
    }

    #[test]
    fn opens_a_listed_folder_only_while_it_is_the_same_one() {
        let (_place, real) = place();
        fs::create_dir(real.join("cache")).unwrap();
        let parent = Dir::open(&real).unwrap();
        let (name, seen) = parent.entries_while(|| true).unwrap().pop().unwrap();
        assert!(parent.child_if_same(&name, &seen).unwrap().is_some());

        fs::rename(real.join("cache"), real.join("cache old")).unwrap();
        fs::create_dir(real.join("cache")).unwrap();

        assert!(parent.child_if_same(&name, &seen).unwrap().is_none());
    }

    #[test]
    fn lists_a_junction_as_a_link() {
        let (_place, real) = place();
        fs::create_dir(real.join("folder")).unwrap();
        fs::write(real.join("file"), vec![1; 8192]).unwrap();
        junction(&real.join("link"), &real.join("folder"));

        let entries = Dir::open(&real).unwrap().entries_while(|| true).unwrap();

        let kind = |name: &str| {
            entries
                .iter()
                .find(|(entry, _)| entry == name)
                .map(|(_, stat)| (is_folder(stat), is_link(stat)))
        };
        assert_eq!(kind("folder"), Some((true, false)));
        assert_eq!(kind("link"), Some((false, true)));
        assert_eq!(kind("file"), Some((false, false)));
        let (_, file) = entries.iter().find(|(entry, _)| entry == "file").unwrap();
        assert!(file.bytes >= 8192, "{file:?}");
        assert_eq!(file.links, None);
    }

    #[test]
    fn counts_the_names_a_file_has_once_it_is_opened() {
        let (_place, real) = place();
        fs::write(real.join("one"), b"x").unwrap();
        fs::hard_link(real.join("one"), real.join("two")).unwrap();
        let dir = Dir::open(&real).unwrap();

        let stat = dir.item(OsStr::new("one")).unwrap().metadata().unwrap();
        let other = dir.item(OsStr::new("two")).unwrap().metadata().unwrap();

        assert_eq!(stat.links, Some(2));
        assert_eq!((stat.device, stat.inode), (other.device, other.inode));
    }

    #[test]
    fn reads_the_start_of_a_regular_file_only() {
        let (_place, real) = place();
        fs::write(
            real.join("tag"),
            b"Signature: 8a477f597d28d172789f06886806bc55\n# cache",
        )
        .unwrap();
        fs::create_dir(real.join("folder")).unwrap();
        junction(&real.join("link"), &real.join("folder"));
        let dir = Dir::open(&real).unwrap();
        let start = |name: &str| dir.read_start(OsStr::new(name), 10);

        assert_eq!(start("tag").unwrap(), b"Signature:");
        assert!(start("link").is_err());
        assert!(start("folder").is_err());
        assert!(start("missing").is_err());
    }

    #[test]
    fn removes_a_junction_itself_and_leaves_what_it_points_to() {
        let (_place, real) = place();
        fs::create_dir(real.join("folder")).unwrap();
        fs::write(real.join("folder/file"), b"keep me").unwrap();
        junction(&real.join("link"), &real.join("folder"));
        let dir = Dir::open(&real).unwrap();

        // A link is never a folder to remove, nor a folder a file.
        assert!(dir.unlink(OsStr::new("link"), true).is_err());
        assert!(dir.unlink(OsStr::new("folder"), false).is_err());
        dir.unlink(OsStr::new("link"), false).unwrap();

        assert!(!real.join("link").exists());
        assert!(real.join("folder/file").exists());
    }

    #[test]
    fn removes_a_read_only_file_and_an_empty_folder() {
        let (_place, real) = place();
        fs::write(real.join("file"), b"x").unwrap();
        let mut read_only = fs::metadata(real.join("file")).unwrap().permissions();
        read_only.set_readonly(true);
        fs::set_permissions(real.join("file"), read_only).unwrap();
        fs::create_dir(real.join("empty")).unwrap();
        fs::create_dir(real.join("full")).unwrap();
        fs::write(real.join("full/a"), b"x").unwrap();
        let dir = Dir::open(&real).unwrap();

        dir.unlink(OsStr::new("file"), false).unwrap();
        dir.unlink(OsStr::new("empty"), true).unwrap();

        assert!(!real.join("file").exists());
        assert!(!real.join("empty").exists());
        assert!(dir.unlink(OsStr::new("full"), true).is_err());
        assert!(real.join("full/a").exists());
    }

    #[test]
    fn finds_where_an_open_folder_went() {
        let (_place, real) = place();
        fs::create_dir(real.join("before")).unwrap();
        let moved = Dir::open(&real)
            .unwrap()
            .child(OsStr::new("before"))
            .unwrap();

        fs::rename(real.join("before"), real.join("after")).unwrap();

        assert_eq!(moved.path().unwrap(), real.join("after"));
    }

    #[test]
    fn compares_names_as_windows_does() {
        let same = |a: &str, b: &str| same_name(OsStr::new(a), OsStr::new(b));

        assert!(same("node_modules", "Node_Modules"));
        assert!(same("Café", "CAFÉ"));
        assert!(!same("cache", "caches"));
        assert!(!same("a", "b"));
    }

    #[test]
    fn writes_a_path_as_people_do() {
        let plain = |path: &str| plain(PathBuf::from(path));

        assert_eq!(plain(r"\\?\C:\Users\dev"), Path::new(r"C:\Users\dev"));
        assert_eq!(plain(r"\\?\C:\"), Path::new(r"C:\"));
        assert_eq!(
            plain(r"\\?\UNC\server\share\dev"),
            Path::new(r"\\server\share\dev")
        );
        assert_eq!(plain(r"C:\Users\dev"), Path::new(r"C:\Users\dev"));
        assert_eq!(plain(r"\\?\Volume{0}\dev"), Path::new(r"\\?\Volume{0}\dev"));
    }
}
