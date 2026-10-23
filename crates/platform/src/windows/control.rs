//! Ending a process on Windows: held through one handle from the check of
//! who it is to the wait for its exit, so that no other process can take
//! its pid in between.

use std::io;
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
    WaitForSingleObject,
};

use super::process::{Handle, hold_with};
use crate::{Presence, ProcessKey};

/// Exit code given to a process that is ended without being asked.
const ENDED: u32 = 1;

/// A process held open, so that it is the one ended and the one waited for.
#[derive(Debug)]
pub struct Held {
    handle: Handle,
    pub key: ProcessKey,
}

/// How a held process stands once it was ended, or the wait was over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// It is gone.
    Exited,
    /// Still there when the wait ran out.
    StillRunning,
    /// The system wouldn't say.
    Unknown,
}

/// Holds the process `key` names, if its pid still names it and it is
/// still running. `Presence::Running` is never returned.
pub fn hold(key: ProcessKey) -> Result<Held, Presence> {
    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE;
    let handle = hold_with(key, access)?;
    let held = Held { handle, key };
    // An exited process stays openable while anyone holds a handle to it.
    match held.wait(Duration::ZERO) {
        Ended::StillRunning => Ok(held),
        Ended::Exited => Err(Presence::Gone),
        Ended::Unknown => Err(Presence::Unknown),
    }
}

impl Held {
    /// Ends the process without asking: it gets no chance to clean up. A
    /// process that exited meanwhile counts as ended.
    pub fn terminate(&self) -> io::Result<()> {
        // SAFETY: the handle is open with the right to terminate.
        if unsafe { TerminateProcess(self.handle.0, ENDED) } != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
            && self.wait(Duration::ZERO) == Ended::Exited
        {
            return Ok(());
        }
        Err(error)
    }

    /// Asks the process to close each window it shows on the taskbar, as
    /// their close buttons do. It may ask the user first, or stay open; the
    /// handle keeps its pid from naming another process meanwhile.
    pub fn close(&self) -> io::Result<()> {
        super::window::close(self.key.pid.0)
    }

    /// Waits up to `grace` for the process to be gone; a wait longer than
    /// Windows counts is cut short rather than made endless.
    pub fn wait(&self, grace: Duration) -> Ended {
        // u32::MAX is INFINITE to the system.
        let millis = u32::try_from(grace.as_millis())
            .unwrap_or(u32::MAX)
            .min(u32::MAX - 1);
        // SAFETY: the handle is open with the right to wait on it.
        match unsafe { WaitForSingleObject(self.handle.0, millis) } {
            WAIT_OBJECT_0 => Ended::Exited,
            WAIT_TIMEOUT => Ended::StillRunning,
            _ => Ended::Unknown,
        }
    }
}
