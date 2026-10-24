mod bind;
/// Read on Windows; their tests run everywhere.
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod command_line;
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod peb;
mod types;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as sys;
/// Ending a process, which Windows does through a held handle.
#[cfg(windows)]
pub use self::windows::control::{Ended, Held, hold, watch};
/// The programs installed on Windows, as their uninstall entries list them.
#[cfg(windows)]
pub use self::windows::installed::{Installed, Registry, Scope, entry, installed};
#[cfg(windows)]
pub use self::windows::window::has_windows;

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

/// `path` with every link in it resolved, spelled as the disk spells it and
/// in the form processes report paths in.
pub fn canonical(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    sys::canonical(path)
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

/// The pids of the processes the system would end along with the process
/// `key` names: on Windows, those of each job it holds that ends them all
/// once its last handle closes, as Node's for the programs it starts does.
/// None on a Mac.
pub fn bound_to(key: ProcessKey) -> Result<Vec<u32>, Error> {
    Ok(sys::bound_to(key)?)
}

/// Whether this process runs as the system's own account: root, as its
/// real or effective user, or Windows' LocalSystem. What a stop or a removal
/// may reach is judged by the user it runs as, so those checks hold only
/// for an ordinary one.
pub fn runs_as_root() -> bool {
    sys::runs_as_root()
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

/// Where Windows is installed, the folder its own programs live under;
/// `None` on a Mac.
pub fn windows_folder() -> Option<std::path::PathBuf> {
    sys::windows_folder()
}

/// What a program's file says it is, as Task Manager names it: on Windows,
/// the description in its version resource, read once per file. `None` on a
/// Mac, whose apps go by their bundles, or where the file says nothing.
pub fn description(exe: &std::path::Path) -> Option<String> {
    sys::description(exe)
}

/// The pid of the process behind Windows' desktop shell window, the
/// session's Explorer; `None` on a Mac or without a desktop.
pub fn shell_process() -> Option<u32> {
    sys::shell_process()
}

/// Whether Windows' taskbar is light, for an icon to stand out on it. `None`
/// where the OS tints the icon itself, as macOS does, or doesn't say.
pub fn light_taskbar() -> Option<bool> {
    sys::light_taskbar()
}

/// The language the user prefers first, as a tag such as `tr-TR`, if the
/// OS says.
pub fn preferred_language() -> Option<String> {
    sys::preferred_language()
}
