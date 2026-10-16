mod bind;
mod types;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as sys;

pub use bind::{BindProbe, probe_bind};
pub use types::{
    Field, Listener, Memory, Owner, Pid, Presence, ProcessDetails, ProcessEntry, ProcessKey,
    Signal, SignalOutcome, SocketId, StartTime,
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

/// The program file of the process `key` names; cheaper than `details`.
pub fn executable(key: ProcessKey) -> Field<std::path::PathBuf> {
    sys::executable_of(key)
}

/// Sends `signal` only if `key` still names the same process.
pub fn signal(key: ProcessKey, signal: Signal) -> SignalOutcome {
    sys::signal(key, signal)
}

/// Whether the process `key` names is still there.
pub fn presence(key: ProcessKey) -> Presence {
    sys::presence(key)
}

/// Whether a `ProcessKey` also changes when its process execs another
/// program. Without that, the pid and start time are all that tell two
/// processes apart.
pub fn tracks_exec() -> bool {
    sys::tracks_exec()
}

/// Used and total memory of the machine, if the OS says.
pub fn memory() -> Option<Memory> {
    sys::memory()
}
