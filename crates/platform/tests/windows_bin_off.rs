#![cfg(windows)]
//! A process of its own: the test turns this user's Recycle Bin off for a
//! drive while it runs, which any test moving things there would meet.

use std::path::{Path, PathBuf};

use harbormaster_platform::{canonical, trash};
use windows_sys::Win32::Storage::FileSystem::GetVolumeNameForVolumeMountPointW;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};

/// The value Recycle Bin Properties sets for "Don't move files to the
/// Recycle Bin. Remove files immediately when deleted."
const REMOVE_AT_ONCE: &str = "NukeOnDelete";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// The Recycle Bin of a drive turned off for this user until dropped.
struct BinOff {
    key: Vec<u16>,
    before: Option<u32>,
}

impl BinOff {
    fn on_drive_of(path: &Path) -> Self {
        let root: PathBuf = path.components().take(2).collect();
        let root = wide(root.to_str().expect("a drive root is text"));
        let mut volume = [0u16; 64];
        // SAFETY: the root ends in a nul and the buffer holds as many units as said.
        let named = unsafe {
            GetVolumeNameForVolumeMountPointW(
                root.as_ptr(),
                volume.as_mut_ptr(),
                volume.len() as u32,
            )
        };
        assert_ne!(named, 0, "{}", std::io::Error::last_os_error());
        let volume = String::from_utf16_lossy(&volume);
        let guid = volume
            .find('{')
            .zip(volume.find('}'))
            .map(|(start, end)| &volume[start..=end])
            .expect("a volume name holds its GUID");
        let key = wide(&format!(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume\{guid}"
        ));
        let name = wide(REMOVE_AT_ONCE);
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
        let off = Self {
            key,
            before: (read == 0).then_some(value),
        };
        off.set(1);
        off
    }

    fn set(&self, value: u32) {
        let name = wide(REMOVE_AT_ONCE);
        // SAFETY: the key and name end in a nul; the data is the DWORD it says.
        let written = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                self.key.as_ptr(),
                name.as_ptr(),
                REG_DWORD,
                (&raw const value).cast(),
                4,
            )
        };
        assert_eq!(
            written,
            0,
            "{}",
            std::io::Error::from_raw_os_error(written as i32)
        );
    }
}

impl Drop for BinOff {
    fn drop(&mut self) {
        match self.before {
            Some(value) => self.set(value),
            None => {
                let name = wide(REMOVE_AT_ONCE);
                // SAFETY: the key and name end in a nul.
                unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, self.key.as_ptr(), name.as_ptr()) };
            }
        }
    }
}

#[test]
fn leaves_what_the_recycle_bin_wont_take_where_it_was() {
    let folder = tempfile::tempdir().unwrap();
    let file = canonical(folder.path()).unwrap().join("clip.mov");
    std::fs::write(&file, b"a film").unwrap();
    let _off = BinOff::on_drive_of(&file);

    let refused = trash(&file);

    assert!(file.exists(), "{refused:?}");
    let error = refused.unwrap_err().to_string();
    assert!(error.contains("left where it was"), "{error}");
}
