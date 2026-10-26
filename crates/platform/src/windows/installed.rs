//! The programs installed for this user or for the whole PC, as the
//! registry's uninstall entries list them.

use std::io;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};

/// Whose a program is: this user's alone, or the whole PC's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    User,
    /// Uninstalling it takes an administrator.
    Machine,
}

/// Which list of the registry an entry is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Registry {
    pub scope: Scope,
    /// The 64-bit view rather than the one 32-bit programs see.
    pub wide: bool,
}

/// A program as its uninstall entry lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub registry: Registry,
    /// The entry's key, below the uninstall entries.
    pub key: String,
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    /// Where its installer says it is.
    pub location: Option<PathBuf>,
    /// How much room its installer says it takes, if it says.
    pub estimated: Option<u64>,
    /// The file its icon is in, as written, often the program itself.
    pub icon: Option<PathBuf>,
    /// The command its installer left to uninstall it, with the environment
    /// variables it names filled in; empty for an entry that isn't listed to
    /// be uninstalled.
    pub uninstall: String,
}

/// Where each installer leaves how to uninstall what it installed.
const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
/// Where programs register the names they are started by, such as `code.exe`.
const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths";
/// The lists of the registry an entry may be in: this user's, and the whole
/// PC's as both the 64-bit and the 32-bit registry list them. This user's
/// entries are the same in both views.
const REGISTRIES: [Registry; 3] = [
    Registry {
        scope: Scope::User,
        wide: true,
    },
    Registry {
        scope: Scope::Machine,
        wide: true,
    },
    Registry {
        scope: Scope::Machine,
        wide: false,
    },
];
/// What an entry is when it is an update to another program, not one itself.
const UPDATES: &[&str] = &[
    "Update",
    "Hotfix",
    "Security Update",
    "Update Rollup",
    "Service Pack",
];

/// Every program installed for this user, then for the whole PC as both
/// the 64-bit and the 32-bit registry list them, but for parts of Windows,
/// updates and what can't be uninstalled.
pub fn installed() -> io::Result<Vec<Installed>> {
    gathered(Entries::Shown)
}

/// Every uninstall entry with a name, parts of Windows, updates and what
/// can't be uninstalled included: what is installed, rather than what is
/// listed to be uninstalled.
pub fn every_installed() -> io::Result<Vec<Installed>> {
    gathered(Entries::Every)
}

fn gathered(which: Entries) -> io::Result<Vec<Installed>> {
    let mut found = Vec::new();
    for registry in REGISTRIES {
        let Some(entries) = Key::open(registry, UNINSTALL)? else {
            continue;
        };
        for key in entries.subkeys()? {
            if let Some(program) = read(&entries, registry, &key, which)? {
                found.push(program);
            }
        }
    }
    Ok(found)
}

/// The entry `key` of `registry`'s uninstall entries as it is now, if it is
/// still there and still a program to list.
pub fn entry(registry: Registry, key: &str) -> io::Result<Option<Installed>> {
    match Key::open(registry, UNINSTALL)? {
        Some(entries) => read(&entries, registry, key, Entries::Shown),
        None => Ok(None),
    }
}

/// Whether the entry `key` of `registry`'s uninstall entries is still
/// there at all, listed or not.
pub fn still_there(registry: Registry, key: &str) -> io::Result<bool> {
    match Key::open(registry, UNINSTALL)? {
        Some(entries) => entries.has(key),
        None => Ok(false),
    }
}

/// The names programs are started by, as App Paths lists them: `code.exe`.
pub fn app_paths() -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for registry in REGISTRIES {
        if let Some(entries) = Key::open(registry, APP_PATHS)? {
            names.extend(entries.subkeys()?);
        }
    }
    Ok(names)
}

/// Which uninstall entries are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entries {
    /// Those a user may uninstall from: no parts of Windows, no updates.
    Shown,
    /// Every one with a name.
    Every,
}

fn read(
    entries: &Key,
    registry: Registry,
    key: &str,
    which: Entries,
) -> io::Result<Option<Installed>> {
    let text = |name: &str| entries.text(key, name);
    let number = |name: &str| entries.number(key, name);
    let hidden = which == Entries::Shown
        && (number("SystemComponent")? == Some(1)
            || number("NoRemove")? == Some(1)
            || text("ParentKeyName")?.is_some()
            || text("ReleaseType")?.is_some_and(|kind| UPDATES.contains(&kind.as_str())));
    let Some(name) = text("DisplayName")? else {
        return Ok(None);
    };
    let uninstall = text("UninstallString")?.unwrap_or_default();
    if hidden || name.trim().is_empty() || (which == Entries::Shown && uninstall.trim().is_empty())
    {
        return Ok(None);
    }
    Ok(Some(Installed {
        registry,
        key: key.to_owned(),
        name: name.trim().to_owned(),
        publisher: text("Publisher")?.filter(|publisher| !publisher.trim().is_empty()),
        version: text("DisplayVersion")?.filter(|version| !version.trim().is_empty()),
        location: text("InstallLocation")?
            .map(|location| location.trim().trim_matches('"').to_owned())
            .filter(|location| !location.is_empty())
            .map(PathBuf::from),
        estimated: number("EstimatedSize")?
            .filter(|kilobytes| *kilobytes > 0)
            .map(|kilobytes| u64::from(kilobytes) * 1024),
        icon: text("DisplayIcon")?
            .map(|icon| icon.trim().trim_matches('"').to_owned())
            .filter(|icon| !icon.is_empty())
            .map(PathBuf::from),
        uninstall,
    }))
}

/// An open registry key, closed when dropped.
struct Key(HKEY);

impl Key {
    /// `path` in `registry`, if it is there.
    fn open(registry: Registry, path: &str) -> io::Result<Option<Self>> {
        let hive = match registry.scope {
            Scope::User => HKEY_CURRENT_USER,
            Scope::Machine => HKEY_LOCAL_MACHINE,
        };
        let view = if registry.wide {
            KEY_WOW64_64KEY
        } else {
            KEY_WOW64_32KEY
        };
        let path = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: the path ends in a nul and `key` is ours to fill.
        let opened = unsafe { RegOpenKeyExW(hive, path.as_ptr(), 0, KEY_READ | view, &mut key) };
        match opened {
            ERROR_SUCCESS => Ok(Some(Self(key))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            error => Err(failure(error)),
        }
    }

    /// Whether the key `below` this one is there.
    fn has(&self, below: &str) -> io::Result<bool> {
        let below = wide(below);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: the key is open, the name ends in a nul and `key` is ours to fill.
        let opened = unsafe { RegOpenKeyExW(self.0, below.as_ptr(), 0, KEY_READ, &mut key) };
        match opened {
            ERROR_SUCCESS => {
                drop(Self(key));
                Ok(true)
            }
            ERROR_FILE_NOT_FOUND => Ok(false),
            error => Err(failure(error)),
        }
    }

    /// The names of the keys right below this one.
    fn subkeys(&self) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        let mut name = vec![0u16; 256];
        for index in 0.. {
            let mut length = name.len() as u32;
            // SAFETY: the key is open and the buffer holds `length` units.
            let done = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            match done {
                ERROR_SUCCESS => names.push(String::from_utf16_lossy(&name[..length as usize])),
                ERROR_NO_MORE_ITEMS => break,
                // A key name is at most 255 units; one said to be longer
                // isn't an entry, and the next is read.
                ERROR_MORE_DATA => continue,
                error => return Err(failure(error)),
            }
        }
        Ok(names)
    }

    /// The text `name` of the key `below` this one, if it is there, with the
    /// environment variables it names filled in when it is written to have
    /// them, as `%ProgramFiles%`.
    fn text(&self, below: &str, name: &str) -> io::Result<Option<String>> {
        let (below, name) = (wide(below), wide(name));
        // Text written with variables is filled in, and then counts as plain text.
        let flags = RRF_RT_REG_SZ;
        let mut size = 0u32;
        // SAFETY: the key is open, the names end in a nul; no data is asked for.
        let sized = unsafe {
            RegGetValueW(
                self.0,
                below.as_ptr(),
                name.as_ptr(),
                flags,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        match sized {
            ERROR_SUCCESS => {}
            ERROR_FILE_NOT_FOUND => return Ok(None),
            error => return Err(failure(error)),
        }
        loop {
            let mut text = vec![0u16; (size as usize).div_ceil(2)];
            let mut read = (text.len() * 2) as u32;
            // SAFETY: the buffer holds `read` bytes.
            let done = unsafe {
                RegGetValueW(
                    self.0,
                    below.as_ptr(),
                    name.as_ptr(),
                    flags,
                    std::ptr::null_mut(),
                    text.as_mut_ptr().cast(),
                    &mut read,
                )
            };
            match done {
                ERROR_SUCCESS => {
                    let units = (read as usize / 2).min(text.len());
                    let end = text[..units]
                        .iter()
                        .position(|&unit| unit == 0)
                        .unwrap_or(units);
                    return Ok(Some(String::from_utf16_lossy(&text[..end])));
                }
                // It grew since it was sized: `read` says how much room it takes now.
                ERROR_MORE_DATA => size = read.max(size.saturating_mul(2)),
                ERROR_FILE_NOT_FOUND => return Ok(None),
                error => return Err(failure(error)),
            }
        }
    }

    /// The number `name` of the key `below` this one, if it is there as one.
    fn number(&self, below: &str, name: &str) -> io::Result<Option<u32>> {
        let (below, name) = (wide(below), wide(name));
        let mut value = 0u32;
        let mut size = 4u32;
        // SAFETY: the key is open, the names end in a nul; `value` has room for a DWORD.
        let read = unsafe {
            RegGetValueW(
                self.0,
                below.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&raw mut value).cast(),
                &mut size,
            )
        };
        match read {
            ERROR_SUCCESS => Ok(Some(value)),
            // Missing, or not a number, which says nothing either.
            _ => Ok(None),
        }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by RegOpenKeyExW and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn failure(error: WIN32_ERROR) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}
