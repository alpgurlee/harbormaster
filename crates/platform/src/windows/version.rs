//! What a program's file says it is: the description in its version
//! resource, which Task Manager names it by.

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};

/// The most a version resource may take; real ones take a few KiB.
const MOST_BYTES: u32 = 1024 * 1024;
/// How many files' descriptions are kept before starting over.
const MOST_KEPT: usize = 512;
/// How many of the languages a file lists are looked in; real files list one or two.
const MOST_LANGUAGES: usize = 16;
/// US English in Unicode and in Windows' Western code page, the languages
/// a file that lists none most often has its strings in.
const USUAL: [(u16, u16); 2] = [(0x0409, 0x04B0), (0x0409, 0x04E4)];

/// The description in the version resource of `exe`, read again only once
/// the file has changed, as an update changes it.
pub(crate) fn description(exe: &Path) -> Option<String> {
    type Known = HashMap<PathBuf, (SystemTime, Option<String>)>;
    static KNOWN: LazyLock<Mutex<Known>> = LazyLock::new(Mutex::default);
    let changed = std::fs::metadata(exe).and_then(|file| file.modified()).ok();
    if let Some(changed) = changed
        && let Some((seen, known)) = KNOWN
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(exe)
        && *seen == changed
    {
        return known.clone();
    }
    let read = read(exe);
    if let Some(changed) = changed {
        let mut known = KNOWN.lock().unwrap_or_else(PoisonError::into_inner);
        if known.len() >= MOST_KEPT {
            known.clear();
        }
        known.insert(exe.to_path_buf(), (changed, read.clone()));
    }
    read
}

fn read(exe: &Path) -> Option<String> {
    let name: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let mut ignored = 0;
    // SAFETY: the name ends in a nul.
    let size = unsafe { GetFileVersionInfoSizeW(name.as_ptr(), &mut ignored) };
    if size == 0 || size > MOST_BYTES {
        return None;
    }
    // u64s keep the resource's fields aligned.
    let mut words = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: the words hold at least `size` bytes, as many as may be written.
    if unsafe { GetFileVersionInfoW(name.as_ptr(), 0, size, words.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let block = Block {
        words: &words,
        bytes: size as usize,
    };
    let listed: Vec<(u16, u16)> = block
        .value(r"\VarFileInfo\Translation")
        .map(|(rest, bytes)| {
            rest[..bytes.min(rest.len())]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&[a, b, c, d]| (u16::from_le_bytes([a, b]), u16::from_le_bytes([c, d])))
                .collect()
        })
        .unwrap_or_default();
    listed
        .into_iter()
        .take(MOST_LANGUAGES)
        .chain(USUAL)
        .find_map(|(language, page)| {
            let path = format!(r"\StringFileInfo\{language:04x}{page:04x}\FileDescription");
            let (rest, units) = block.value(&path)?;
            let text: Vec<u16> = rest[..units.saturating_mul(2).min(rest.len())]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&unit| u16::from_le_bytes(unit))
                .take_while(|&unit| unit != 0)
                .collect();
            let text = String::from_utf16_lossy(&text);
            (!text.trim().is_empty()).then_some(text)
        })
}

/// A version resource as Windows read it.
struct Block<'a> {
    words: &'a [u64],
    bytes: usize,
}

impl Block<'_> {
    /// The block from where the value at `path` starts to its end, and the
    /// value's length as Windows gives it: in bytes for data, in UTF-16
    /// units for text. `None` if it isn't there or lies outside the block.
    fn value(&self, path: &str) -> Option<(&[u8], usize)> {
        let path: Vec<u16> = path.encode_utf16().chain([0]).collect();
        let start = self.words.as_ptr().cast::<u8>();
        let mut at: *mut c_void = std::ptr::null_mut();
        let mut length = 0u32;
        // SAFETY: the block is what GetFileVersionInfoW wrote; the path ends in a nul.
        let found = unsafe { VerQueryValueW(start.cast(), path.as_ptr(), &mut at, &mut length) };
        if found == 0 || at.is_null() {
            return None;
        }
        let offset = (at as usize).checked_sub(start as usize)?;
        // SAFETY: the words hold `self.bytes` bytes, and the slice starts inside them.
        let whole = unsafe { std::slice::from_raw_parts(start, self.bytes) };
        whole.get(offset..).map(|rest| (rest, length as usize))
    }
}
