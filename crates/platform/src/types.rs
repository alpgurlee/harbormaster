use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pid(pub u32);

/// When a process started, in nanoseconds since the Unix epoch. Together with
/// the pid it identifies a process even after the OS reuses the pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StartTime(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessKey {
    pub pid: Pid,
    pub start: StartTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    CurrentUser,
    OtherUser,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEntry {
    pub key: ProcessKey,
    pub parent: Option<Pid>,
    pub name: String,
    pub owner: Owner,
    /// Physical footprint on macOS, private working set on Windows.
    pub memory_bytes: Option<u64>,
    pub cpu_time: Option<Duration>,
}

/// A value the OS may or may not let us read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field<T> {
    Known(T),
    Denied,
    Unsupported,
    Gone,
}

impl<T> Field<T> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Field::Known(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDetails {
    pub exe: Field<PathBuf>,
    pub args: Field<Vec<String>>,
    pub cwd: Field<PathBuf>,
}

/// A TCP socket in the LISTEN state and the process that owns it. The owner's
/// identity was confirmed before and after the socket was read, so a reused
/// pid can never be blamed for someone else's port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub owner: ProcessKey,
    pub address: IpAddr,
    pub port: u16,
}
