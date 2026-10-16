#![cfg(target_os = "macos")]

use std::ffi::CStr;
use std::fs::{self, File};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

/// Where the open file `file` is now, as the kernel tells it.
fn now_at(file: &File) -> PathBuf {
    let mut buffer = vec![0u8; libc::PATH_MAX as usize];
    // SAFETY: the descriptor is open and `buffer` has the PATH_MAX bytes F_GETPATH writes at most.
    let status = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) };
    assert_eq!(status, 0, "{}", std::io::Error::last_os_error());
    let path = CStr::from_bytes_until_nul(&buffer).expect("a path");
    PathBuf::from(path.to_str().expect("a path in text"))
}

#[test]
fn moves_a_folder_to_the_trash_and_says_where_it_went() {
    let place = tempfile::tempdir().unwrap();
    let folder = place
        .path()
        .canonicalize()
        .unwrap()
        .join("harbormaster trash test");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("file"), b"x").unwrap();
    let open = File::open(&folder).unwrap();

    let landed = harbormaster_platform::trash(&folder).unwrap();

    // Should any of this fail, the test's folder stays in the Trash rather
    // than anything else in it being taken out.
    let followed = now_at(&open);
    assert_eq!(followed, landed);
    assert!(!folder.exists());
    assert!(landed.starts_with(Path::new(&std::env::var("HOME").unwrap())));
    // Only the folder this test put in the user's Trash is taken out again,
    // found through its own descriptor.
    fs::remove_dir_all(&followed).unwrap();
}
