//! This user's Recycle Bins, one on each fixed drive, which only the shell
//! looks into and empties: their folders are never touched.

use std::io;
use std::path::{Component, Path, Prefix};

use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;
use windows_sys::Win32::UI::Shell::{
    SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW, SHQUERYRBINFO,
    SHQueryRecycleBinW,
};

use crate::{Finding, Group, Identity, Kind, Outcome};

/// The folder at a drive's root that holds its Recycle Bin.
const BIN: &str = "$Recycle.Bin";

/// What this user's Recycle Bins hold together, as one finding at the bin
/// of the drive `home` is on, if they hold anything.
pub(crate) fn finding(home: &Path) -> Option<Finding> {
    let (bytes, items) = held();
    if items == 0 {
        return None;
    }
    let Some(drive @ Component::Prefix(_)) = home.components().next() else {
        return None;
    };
    Some(Finding {
        group: Group::Trash,
        kind: Kind::Discarded,
        name: "Recycle Bin".into(),
        path: [drive, Component::RootDir, Component::Normal(BIN.as_ref())]
            .iter()
            .collect(),
        bytes,
        picked: false,
        in_use: None,
        touched: None,
        project: None,
        emptied: true,
        identity: Identity::default(),
    })
}

/// Whether the bins still hold anything, so that a finding of them stands.
pub(crate) fn holds_anything() -> bool {
    held().1 > 0
}

/// Empties the Recycle Bin of each fixed drive that holds anything, for good.
pub(crate) fn emptied() -> Outcome {
    for root in fixed_drives() {
        if !held_at(&root).is_ok_and(|(_, items)| items > 0) {
            continue;
        }
        // SAFETY: no window; the root ends in a nul.
        let done = unsafe {
            SHEmptyRecycleBinW(
                std::ptr::null_mut(),
                root.as_ptr(),
                SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
            )
        };
        if done < 0 {
            return Outcome::Failed {
                why: io::Error::from_raw_os_error(done).to_string(),
            };
        }
    }
    Outcome::Deleted
}

/// Whether `path` is where a drive keeps its Recycle Bin, `X:\$Recycle.Bin`.
pub(crate) fn is(path: &Path) -> bool {
    let mut components = path.components();
    matches!(
        components.next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))
    ) && components.next() == Some(Component::RootDir)
        && components
            .next()
            .is_some_and(|name| crate::walk::same_name(name.as_os_str(), BIN.as_ref()))
        && components.next().is_none()
}

/// The bytes and items the bins of all fixed drives hold.
fn held() -> (u64, u64) {
    fixed_drives()
        .iter()
        .filter_map(|root| held_at(root).ok())
        .fold((0, 0), |(bytes, items), (more, others)| {
            (bytes.saturating_add(more), items.saturating_add(others))
        })
}

/// The bytes and items the bin of the drive at `root`, which ends in a nul,
/// holds.
fn held_at(root: &[u16]) -> io::Result<(u64, u64)> {
    let mut held = SHQUERYRBINFO {
        cbSize: size_of::<SHQUERYRBINFO>() as u32,
        ..SHQUERYRBINFO::default()
    };
    // SAFETY: the root ends in a nul, and `held` says how large it is.
    let done = unsafe { SHQueryRecycleBinW(root.as_ptr(), &mut held) };
    if done < 0 {
        return Err(io::Error::from_raw_os_error(done));
    }
    Ok((
        u64::try_from(held.i64Size).unwrap_or(0),
        u64::try_from(held.i64NumItems).unwrap_or(0),
    ))
}

/// The roots of the fixed drives, `X:\` each, ending in a nul.
fn fixed_drives() -> Vec<Vec<u16>> {
    // SAFETY: GetLogicalDrives takes nothing.
    let drives = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|letter| drives & (1 << letter) != 0)
        .map(|letter| {
            format!("{}:\\", char::from(b'A' + letter))
                .encode_utf16()
                .chain([0])
                .collect::<Vec<u16>>()
        })
        // SAFETY: each root ends in a nul.
        .filter(|root| unsafe { GetDriveTypeW(root.as_ptr()) } == DRIVE_FIXED)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::is;

    #[test]
    fn tells_where_a_drive_keeps_its_recycle_bin() {
        assert!(is(Path::new(r"C:\$Recycle.Bin")));
        assert!(is(Path::new(r"d:\$RECYCLE.BIN")));
        assert!(!is(Path::new(r"C:\$Recycle.Bin\S-1-5-21-1")));
        assert!(!is(Path::new(r"C:\Users\dev\$Recycle.Bin")));
        assert!(!is(Path::new(r"\\server\share\$Recycle.Bin")));
    }
}
