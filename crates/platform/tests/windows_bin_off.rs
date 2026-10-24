#![cfg(windows)]
//! A process of its own: each test changes this user's Recycle Bin
//! settings for a drive while it runs, which any test moving things there
//! would meet. They take turns, and each setting is put back as it was.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use harbormaster_platform::{canonical, trash};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows_sys::Win32::Storage::FileSystem::GetVolumeNameForVolumeMountPointW;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};

/// Set by Recycle Bin Properties' "Don't move files to the Recycle Bin.
/// Remove files immediately when deleted."
const REMOVE_AT_ONCE: &str = "NukeOnDelete";
/// Recycle Bin Properties' "Maximum size (MB)".
const LARGEST: &str = "MaxCapacity";

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn take_turn() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Where this user's Recycle Bin settings for the drive holding `path` are.
fn settings_of(path: &Path) -> Vec<u16> {
    let root: PathBuf = path.components().take(2).collect();
    let root = wide(root.to_str().expect("a drive root is text"));
    let mut volume = [0u16; 64];
    // SAFETY: the root ends in a nul and the buffer holds as many units as said.
    let named = unsafe {
        GetVolumeNameForVolumeMountPointW(root.as_ptr(), volume.as_mut_ptr(), volume.len() as u32)
    };
    assert_ne!(named, 0, "{}", std::io::Error::last_os_error());
    let volume = String::from_utf16_lossy(&volume);
    let guid = volume
        .find('{')
        .zip(volume.find('}'))
        .map(|(start, end)| &volume[start..=end])
        .expect("a volume name holds its GUID");
    wide(&format!(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume\{guid}"
    ))
}

/// One Recycle Bin setting of a drive, changed until dropped.
struct Setting {
    key: Vec<u16>,
    name: Vec<u16>,
    before: Option<u32>,
}

impl Setting {
    fn changed(path: &Path, name: &str, value: u32) -> Self {
        let key = settings_of(path);
        let name = wide(name);
        let mut was = 0u32;
        let mut size = 4u32;
        // SAFETY: the key and name end in a nul; `was` has room for the DWORD asked for.
        let read = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&raw mut was).cast(),
                &mut size,
            )
        };
        let before = match read {
            0 => Some(was),
            ERROR_FILE_NOT_FOUND => None,
            error => panic!("{}", failure(error)),
        };
        let setting = Self { key, name, before };
        let written = setting.set(value);
        assert_eq!(written, 0, "{}", failure(written));
        setting
    }

    fn set(&self, value: u32) -> WIN32_ERROR {
        // SAFETY: the key and name end in a nul; the data is the DWORD it says.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                self.key.as_ptr(),
                self.name.as_ptr(),
                REG_DWORD,
                (&raw const value).cast(),
                4,
            )
        }
    }
}

impl Drop for Setting {
    fn drop(&mut self) {
        let restored = match self.before {
            Some(value) => self.set(value),
            // SAFETY: the key and name end in a nul.
            None => unsafe {
                RegDeleteKeyValueW(HKEY_CURRENT_USER, self.key.as_ptr(), self.name.as_ptr())
            },
        };
        if !std::thread::panicking() {
            assert_eq!(restored, 0, "{}", failure(restored));
        }
    }
}

fn failure(error: WIN32_ERROR) -> std::io::Error {
    std::io::Error::from_raw_os_error(error as i32)
}

/// A file of `bytes` in a folder of its own, kept for as long as the
/// folder is.
fn film(bytes: usize) -> (tempfile::TempDir, PathBuf) {
    let folder = tempfile::tempdir().expect("a temp folder");
    let file = canonical(folder.path())
        .expect("its real path")
        .join("clip.mov");
    std::fs::write(&file, vec![1; bytes]).expect("the file");
    (folder, file)
}

fn left_where_it_was(file: &Path) {
    let refused = trash(file);

    assert!(file.exists(), "{refused:?}");
    let error = refused.expect_err("nothing should go").to_string();
    assert!(error.contains("left where it was"), "{error}");
}

#[test]
fn leaves_where_it_was_what_the_recycle_bin_is_turned_off_for() {
    let _turn = take_turn();
    let (_folder, file) = film(1024);
    let _off = Setting::changed(&file, REMOVE_AT_ONCE, 1);

    left_where_it_was(&file);
}

#[test]
fn leaves_where_it_was_what_is_too_large_for_the_recycle_bin() {
    let _turn = take_turn();
    let (_folder, file) = film(2 * 1024 * 1024);
    let _small = Setting::changed(&file, LARGEST, 1);

    left_where_it_was(&file);
}
