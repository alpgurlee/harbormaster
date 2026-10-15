use std::io;

use super::process::{bsd_info, key_of};
use crate::{ProcessKey, Signal, SignalOutcome};

pub(crate) fn signal(key: ProcessKey, signal: Signal) -> SignalOutcome {
    let Some(pid) = signal_target(key.pid.0) else {
        return SignalOutcome::Denied;
    };
    // The last identity check before the signal. macOS has no pidfd, so a
    // pid reused within this instant is the one race left.
    if !is_running(key) {
        return SignalOutcome::Gone;
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

/// PROC_PIDTBSDINFO fails for zombies, so an exited but unreaped process
/// doesn't count.
pub(crate) fn is_running(key: ProcessKey) -> bool {
    let Ok(pid) = libc::pid_t::try_from(key.pid.0) else {
        return false;
    };
    bsd_info(pid).is_ok_and(|info| key_of(pid, &info) == key)
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
