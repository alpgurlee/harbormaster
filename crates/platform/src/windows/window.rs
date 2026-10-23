//! The windows a process shows on the desktop, and asking it to close them.

use std::io;

use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetWindow, GetWindowLongW,
    GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};
use windows_sys::core::BOOL;

/// A console's window. Closing it ends every program the console serves,
/// whichever of them Windows names as its owner, so it is never asked to.
const CONSOLE: &str = "ConsoleWindowClass";

/// Whether `pid` shows a window of its own on the taskbar.
pub fn has_windows(pid: u32) -> bool {
    !shown(pid).is_empty()
}

/// Asks `pid` to close each window it shows on the taskbar, as their close
/// buttons do. The process may ask the user first, or stay open.
pub(crate) fn close(pid: u32) -> io::Result<()> {
    let mut asked = false;
    let mut refused = None;
    for window in shown(pid) {
        // Closed since it was listed, and its handle maybe another's now.
        if owner(window) != pid {
            continue;
        }
        // SAFETY: posting takes any window handle; a stale one only fails.
        if unsafe { PostMessageW(window, WM_CLOSE, 0, 0) } != 0 {
            asked = true;
        } else {
            refused = Some(io::Error::last_os_error());
        }
    }
    match refused {
        _ if asked => Ok(()),
        Some(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Windows wouldn't let its windows be asked to close; it may run as administrator",
            ))
        }
        Some(error) => Err(error),
        None => Err(io::Error::other("it shows no window to close")),
    }
}

/// The windows of `pid` the taskbar shows.
fn shown(pid: u32) -> Vec<HWND> {
    let mut found = Found {
        pid,
        windows: Vec::new(),
    };
    // SAFETY: the callback runs only during the call, while `found` lives.
    unsafe { EnumWindows(Some(each), (&raw mut found) as LPARAM) };
    found.windows
}

struct Found {
    pid: u32,
    windows: Vec<HWND>,
}

unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
    // SAFETY: `shown` passes its own `Found`, held until EnumWindows returns.
    let found = unsafe { &mut *(found as *mut Found) };
    if owner(window) == found.pid && on_taskbar(window) {
        found.windows.push(window);
    }
    // Go on to the next window.
    1
}

fn owner(window: HWND) -> u32 {
    let mut pid = 0;
    // SAFETY: any window handle may be asked; `pid` is ours to fill.
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    pid
}

/// As the taskbar picks its buttons: a visible window that asks for one, or
/// a top window that isn't a tool palette. A console's window never counts.
fn on_taskbar(window: HWND) -> bool {
    // SAFETY: each of these takes any window handle.
    let (visible, extended, owned) = unsafe {
        (
            IsWindowVisible(window) != 0,
            GetWindowLongW(window, GWL_EXSTYLE) as u32,
            !GetWindow(window, GW_OWNER).is_null(),
        )
    };
    visible
        && (extended & WS_EX_APPWINDOW != 0 || (!owned && extended & WS_EX_TOOLWINDOW == 0))
        && class(window) != CONSOLE
}

fn class(window: HWND) -> String {
    let mut name = [0u16; 64];
    // SAFETY: the buffer holds as many units as the call is told.
    let length = unsafe { GetClassNameW(window, name.as_mut_ptr(), name.len() as i32) };
    String::from_utf16_lossy(&name[..usize::try_from(length).unwrap_or(0)])
}
