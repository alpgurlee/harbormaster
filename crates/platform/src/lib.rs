mod bind;
mod types;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as sys;

pub use bind::{BindProbe, probe_bind};
pub use types::{
    Field, Listener, Memory, Owner, Pid, Presence, ProcessDetails, ProcessEntry, ProcessKey,
    Signal, SignalOutcome, SocketId, StartTime, Thermal, Whereabouts,
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

/// Where each of this user's processes is, for telling whether a folder is
/// in use. Processes that can't be read are left out.
pub fn whereabouts() -> Result<Vec<Whereabouts>, Error> {
    Ok(sys::whereabouts()?)
}

/// Moves the file or folder at `path` to the Trash, as Finder's Move to
/// Trash does, so it can be put back, and says where it went. The path is
/// followed as given, links in it too.
pub fn trash(path: &std::path::Path) -> Result<std::path::PathBuf, Error> {
    Ok(sys::trash(path)?)
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

/// Whether the process `key` names started with the environment variable
/// `name` set to something. Nothing else in its environment is read out.
pub fn sets_variable(key: ProcessKey, name: &str) -> Field<bool> {
    sys::sets(key, name)
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

/// Raises how many files this process may hold open as far as the system
/// allows, since measuring a folder tree holds one per level and launchd
/// hands an app only 256. Says how many it may hold now.
pub fn allow_more_open_files() -> Result<u64, Error> {
    Ok(sys::allow_more_open_files()?)
}

/// Used and total memory of the machine, if the OS says.
pub fn memory() -> Option<Memory> {
    sys::memory()
}

/// How hard the system is working to cool the machine down right now.
pub fn thermal() -> Thermal {
    sys::thermal()
}

/// The language the user prefers first, as a tag such as `tr-TR`, if the
/// OS says.
pub fn preferred_language() -> Option<String> {
    sys::preferred_language()
}
