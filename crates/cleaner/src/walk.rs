//! Folders opened one step at a time from the root, never through a link.
//! Each step opens the next name relative to the folder already open, so a
//! folder swapped for a link in the meantime is refused, not followed.

use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

const OPEN_FOLDER: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
/// A pipe in a file's place would hold up an ordinary open; this opens it
/// without waiting, so it can be refused.
const OPEN_FILE: libc::c_int =
    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
/// Opens a file only to look at it: nothing is read, so a file kept in the
/// cloud isn't fetched for it.
const LOOK_AT_FILE: libc::c_int =
    libc::O_EVTONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;

/// An open folder.
#[derive(Debug)]
pub(crate) struct Dir(OwnedFd);

/// A file opened only to look at it.
#[derive(Debug)]
pub(crate) struct Item(OwnedFd);

impl Item {
    /// How this file is stored; it may be something else than a file by now.
    pub(crate) fn metadata(&self) -> io::Result<libc::stat> {
        stat_of(self.0.as_fd())
    }

    /// Where this file is now, as the disk spells it.
    pub(crate) fn path(&self) -> io::Result<PathBuf> {
        path_of(self.0.as_fd())
    }
}

impl Dir {
    /// The folder at `path`: absolute, and a real folder at every step.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let mut components = path.components();
        if components.next() != Some(Component::RootDir) {
            return Err(plain_path());
        }
        // SAFETY: a valid C string and flags; the result is checked by `own`.
        let mut dir = Self(own(unsafe { libc::open(c"/".as_ptr(), OPEN_FOLDER) })?);
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
        self.open_at(name, OPEN_FOLDER).map(Self)
    }

    /// What is called `name` inside this folder, opened only to look at it;
    /// a link is refused.
    pub(crate) fn item(&self, name: &OsStr) -> io::Result<Item> {
        self.open_at(name, LOOK_AT_FILE).map(Item)
    }

    /// Up to `most` bytes from the start of the regular file `name` inside
    /// this folder; a link, a folder or a pipe in its place is refused.
    pub(crate) fn read_start(&self, name: &OsStr, most: u64) -> io::Result<Vec<u8>> {
        let file = self.open_at(name, OPEN_FILE)?;
        if !is_file(&stat_of(file.as_fd())?) {
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
        path_of(self.0.as_fd())
    }

    /// Removes `name` inside this folder: a file, a link itself, or an
    /// empty folder.
    pub(crate) fn unlink(&self, name: &OsStr, folder: bool) -> io::Result<()> {
        let name = CString::new(name.as_bytes())?;
        let flags = if folder { libc::AT_REMOVEDIR } else { 0 };
        // SAFETY: `self.0` is an open folder and `name` a valid C string.
        if unsafe { libc::unlinkat(self.0.as_raw_fd(), name.as_ptr(), flags) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn open_at(&self, name: &OsStr, flags: libc::c_int) -> io::Result<OwnedFd> {
        let name = CString::new(name.as_bytes())?;
        // SAFETY: `self.0` is an open folder and `name` a valid C string.
        own(unsafe { libc::openat(self.0.as_raw_fd(), name.as_ptr(), flags) })
    }

    /// Down `relative`, a real folder at every step.
    pub(crate) fn descend(&self, relative: &Path) -> io::Result<Self> {
        let mut dir = self.child(OsStr::new("."))?;
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
        mut keep_going: impl FnMut() -> bool,
    ) -> io::Result<Vec<(OsString, libc::stat)>> {
        // Reading moves a handle's position, so the listing gets a handle of its own.
        let raw = self.child(OsStr::new("."))?.0.into_raw_fd();
        // SAFETY: `raw` is an open folder; on success the stream owns it.
        let stream = unsafe { libc::fdopendir(raw) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            // SAFETY: the stream didn't take `raw`, so it is still ours to close.
            unsafe { libc::close(raw) };
            return Err(error);
        }
        let listed = self.read_all(stream, &mut keep_going);
        // SAFETY: closes the stream and the handle it owns, once.
        unsafe { libc::closedir(stream) };
        listed
    }

    /// The entries `stream` lists. An entry that vanished meanwhile is left
    /// out; any other failure fails the listing, so it is never taken as whole.
    fn read_all(
        &self,
        stream: *mut libc::DIR,
        keep_going: &mut impl FnMut() -> bool,
    ) -> io::Result<Vec<(OsString, libc::stat)>> {
        let mut found = Vec::new();
        while keep_going() {
            // readdir reports a failure only through errno, so clear it first.
            // SAFETY: errno() points at this thread's errno.
            unsafe { *errno() = 0 };
            // SAFETY: `stream` is open; the caller closes it after this returns.
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                // SAFETY: as above, read right after readdir.
                let code = unsafe { *errno() };
                if code != 0 {
                    return Err(io::Error::from_raw_os_error(code));
                }
                break;
            }
            // SAFETY: readdir returned an entry whose name is NUL-terminated.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            match self.stat(name) {
                Ok(stat) => found.push((OsString::from_vec(name.to_bytes().to_vec()), stat)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(found)
    }

    /// The folder `name` inside this one, if it is still the folder `seen`
    /// described when it was listed: on the same disk, with the same inode.
    pub(crate) fn child_if_same(
        &self,
        name: &OsStr,
        seen: &libc::stat,
    ) -> io::Result<Option<Self>> {
        let child = self.child(name)?;
        let now = child.metadata()?;
        Ok((now.st_dev == seen.st_dev && now.st_ino == seen.st_ino).then_some(child))
    }

    /// How this folder itself is stored.
    pub(crate) fn metadata(&self) -> io::Result<libc::stat> {
        stat_of(self.0.as_fd())
    }

    /// How `name` inside this folder is stored, without following a link.
    fn stat(&self, name: &CStr) -> io::Result<libc::stat> {
        let mut stat = MaybeUninit::<libc::stat>::uninit();
        // SAFETY: `self.0` is open, `name` is NUL-terminated, `stat` has room for one stat.
        let status = unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if status != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fstatat succeeded, so it filled `stat`.
        Ok(unsafe { stat.assume_init() })
    }
}

/// What `open` or `openat` returned, owned, or the error they left.
fn own(fd: libc::c_int) -> io::Result<OwnedFd> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a descriptor the kernel just handed out, owned by nothing else.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// How the open file or folder `fd` is stored.
/// Where what `fd` names is now, as the disk spells it.
fn path_of(fd: BorrowedFd<'_>) -> io::Result<PathBuf> {
    let mut buffer = vec![0u8; libc::PATH_MAX as usize];
    // SAFETY: `fd` is open and `buffer` has the PATH_MAX bytes F_GETPATH writes at most.
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let path = CStr::from_bytes_until_nul(&buffer).map_err(io::Error::other)?;
    Ok(PathBuf::from(OsStr::from_bytes(path.to_bytes())))
}

fn stat_of(fd: BorrowedFd<'_>) -> io::Result<libc::stat> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `fd` is open and `stat` has room for one stat.
    if unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstat succeeded, so it filled `stat`.
    Ok(unsafe { stat.assume_init() })
}

/// Whether `stat` describes a folder.
pub(crate) fn is_folder(stat: &libc::stat) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFDIR
}

/// Whether `stat` describes a regular file.
pub(crate) fn is_file(stat: &libc::stat) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFREG
}

/// Whether `stat` describes a link.
pub(crate) fn is_link(stat: &libc::stat) -> bool {
    stat.st_mode & libc::S_IFMT == libc::S_IFLNK
}

#[cfg(target_os = "macos")]
fn errno() -> *mut libc::c_int {
    // SAFETY: __error has no preconditions; it returns this thread's errno.
    unsafe { libc::__error() }
}

#[cfg(target_os = "linux")]
fn errno() -> *mut libc::c_int {
    // SAFETY: __errno_location has no preconditions; it returns this thread's errno.
    unsafe { libc::__errno_location() }
}

fn plain_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "expected an absolute path of plain names",
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::Dir;

    #[test]
    fn refuses_a_folder_swapped_for_a_link_after_its_parent_was_opened() {
        let place = tempfile::tempdir().unwrap();
        let real = place.path().canonicalize().unwrap();
        fs::create_dir(real.join("cache")).unwrap();
        fs::create_dir(real.join("elsewhere")).unwrap();
        let parent = Dir::open(&real).unwrap();
        assert!(parent.child(OsStr::new("cache")).is_ok());

        fs::remove_dir(real.join("cache")).unwrap();
        symlink(real.join("elsewhere"), real.join("cache")).unwrap();

        assert!(parent.child(OsStr::new("cache")).is_err());
    }

    #[test]
    fn stops_listing_when_told() {
        let place = tempfile::tempdir().unwrap();
        let real = place.path().canonicalize().unwrap();
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
    fn opens_a_listed_folder_only_while_it_is_the_same_one() {
        let place = tempfile::tempdir().unwrap();
        let real = place.path().canonicalize().unwrap();
        fs::create_dir(real.join("cache")).unwrap();
        let parent = Dir::open(&real).unwrap();
        let (name, seen) = parent.entries_while(|| true).unwrap().pop().unwrap();
        assert!(parent.child_if_same(&name, &seen).unwrap().is_some());

        fs::remove_dir(real.join("cache")).unwrap();
        fs::create_dir(real.join("cache")).unwrap();

        assert!(parent.child_if_same(&name, &seen).unwrap().is_none());
    }

    #[test]
    fn lists_a_link_as_a_link() {
        let place = tempfile::tempdir().unwrap();
        let real = place.path().canonicalize().unwrap();
        fs::create_dir(real.join("folder")).unwrap();
        symlink(real.join("folder"), real.join("link")).unwrap();

        let entries = Dir::open(&real).unwrap().entries_while(|| true).unwrap();

        let kind = |name: &str| {
            entries
                .iter()
                .find(|(entry, _)| entry == name)
                .map(|(_, stat)| (super::is_folder(stat), super::is_link(stat)))
        };
        assert_eq!(kind("folder"), Some((true, false)));
        assert_eq!(kind("link"), Some((false, true)));
    }

    #[test]
    fn reads_the_start_of_a_regular_file_only() {
        let place = tempfile::tempdir().unwrap();
        let real = place.path().canonicalize().unwrap();
        fs::write(
            real.join("tag"),
            b"Signature: 8a477f597d28d172789f06886806bc55\n# cache",
        )
        .unwrap();
        symlink(real.join("tag"), real.join("link")).unwrap();
        fs::create_dir(real.join("folder")).unwrap();
        let pipe = std::ffi::CString::new(real.join("pipe").into_os_string().into_encoded_bytes())
            .unwrap();
        // SAFETY: `pipe` is a valid C string naming a path in a folder we own.
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        let dir = Dir::open(&real).unwrap();
        let start = |name: &str| dir.read_start(OsStr::new(name), 10);

        assert_eq!(start("tag").unwrap(), b"Signature:");
        assert!(start("link").is_err());
        assert!(start("folder").is_err());
        assert!(start("pipe").is_err());
        assert!(start("missing").is_err());
    }
}
