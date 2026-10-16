mod bind;
mod types;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as sys;

pub use bind::{BindProbe, probe_bind};
pub use types::{
    Field, Listener, Memory, Owner, Pid, ProcessDetails, ProcessEntry, ProcessKey, Signal,
    SignalOutcome, SocketId, StartTime,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the operating system refused the request: {0}")]
    Os(#[from] std::io::Error),
}

/// Every TCP socket in the LISTEN state that this user can see.
pub fn listeners() -> Result<Vec<Listener>, Error> {
    Ok(sys::listeners()?)
}

/// Every process the OS reports, with usage figures where readable.
pub fn processes() -> Result<Vec<ProcessEntry>, Error> {
    Ok(sys::processes()?)
}

/// Launch details of one process. Fields come back `Gone` if the pid now
/// belongs to a different process than `key`.
pub fn details(key: ProcessKey) -> ProcessDetails {
    sys::details(key)
}

/// Sends `signal` only if `key` still names the same process.
pub fn signal(key: ProcessKey, signal: Signal) -> SignalOutcome {
    sys::signal(key, signal)
}

/// Whether the process `key` names is still running. A zombie isn't.
pub fn is_running(key: ProcessKey) -> bool {
    sys::is_running(key)
}

/// Used and total memory of the machine, if the OS says.
pub fn memory() -> Option<Memory> {
    sys::memory()
}
