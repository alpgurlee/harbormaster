mod types;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as sys;

pub use types::{Field, Listener, Owner, Pid, ProcessDetails, ProcessEntry, ProcessKey, StartTime};

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
