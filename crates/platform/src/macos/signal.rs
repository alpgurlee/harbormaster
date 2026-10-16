use std::io;

use super::process::identify;
use crate::{Presence, ProcessKey, Signal, SignalOutcome};

pub(crate) fn signal(key: ProcessKey, signal: Signal) -> SignalOutcome {
    let Some(pid) = signal_target(key.pid.0) else {
        return SignalOutcome::Denied;
    };
    // The last identity check before the signal. macOS has no pidfd, so a
    // pid reused within this instant is the one race left.
    match presence(key) {
        Presence::Running => {}
        Presence::Gone => return SignalOutcome::Gone,
        Presence::Replaced => return SignalOutcome::Replaced,
        Presence::Unknown => {
            return SignalOutcome::Failed("could not confirm it is the same process".into());
        }
    }
    let number = match signal {
        Signal::Terminate => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };
    // SAFETY: kill has no memory-safety preconditions.
    if unsafe { libc::kill(pid, number) } == 0 {
        return SignalOutcome::Sent;
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => SignalOutcome::Gone,
        Some(libc::EPERM) => SignalOutcome::Denied,
        _ => SignalOutcome::Failed(error.to_string()),
    }
}

/// proc_pidinfo answers ESRCH for zombies too, so an exited but unreaped
/// process counts as gone.
pub(crate) fn presence(key: ProcessKey) -> Presence {
    let Ok(pid) = libc::pid_t::try_from(key.pid.0) else {
        return Presence::Gone;
    };
    match identify(pid) {
        Ok((_, found)) if found == key => Presence::Running,
        Ok(_) => Presence::Replaced,
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Presence::Gone,
        Err(_) => Presence::Unknown,
    }
}

/// kill(0) would reach our whole process group and kill(1) launchd, so
/// neither is ever a target.
fn signal_target(pid: u32) -> Option<libc::pid_t> {
    libc::pid_t::try_from(pid).ok().filter(|&pid| pid > 1)
}

#[cfg(test)]
mod tests {
    use super::signal_target;

    #[test]
    fn never_targets_the_process_group_or_launchd() {
        assert_eq!(signal_target(0), None);
        assert_eq!(signal_target(1), None);
        assert_eq!(signal_target(u32::MAX), None);
        assert_eq!(signal_target(4242), Some(4242));
    }
}
