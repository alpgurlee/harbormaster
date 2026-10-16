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
    /// Changes when the process execs another program, which keeps both the
    /// pid and the start time. 0 where the OS doesn't track it.
    pub exec: u32,
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
    /// Tells processes that share one socket (a reloader and its worker)
    /// from unrelated ones that bound the same port with SO_REUSEPORT.
    /// `None` where the OS doesn't say.
    pub socket: Option<SocketId>,
}

/// One socket: where the kernel keeps it, and which allocation of that place
/// it is, so a socket freed and replaced mid-scan reads as a different one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketId {
    pub kernel: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Ask the process to exit (SIGTERM).
    Terminate,
    /// End it without asking (SIGKILL).
    Kill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalOutcome {
    Sent,
    /// It already exited, or its pid now belongs to another process.
    Gone,
    Denied,
    Failed(String),
}

/// The machine's memory the way its own monitor counts it: Activity
/// Monitor's "Memory Used", Task Manager's "In use".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub used: u64,
    pub total: u64,
}
