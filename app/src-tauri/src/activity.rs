//! What a restart must not cut short: a stop, a quit, an uninstaller's
//! start, a scan or a removal. An update waits for them to end, and keeps
//! new ones out while it installs.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub(crate) struct Activity {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct State {
    under_way: usize,
    installing: bool,
}

/// An update is being installed, so nothing new starts.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Updating;

/// Something was still under way when the update's patience ran out.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StillBusy;

/// Work under way, counted until it is dropped.
#[derive(Debug)]
pub(crate) struct Doing(Arc<Activity>);

impl Drop for Doing {
    fn drop(&mut self) {
        self.0.lock().under_way -= 1;
        self.0.changed.notify_all();
    }
}

/// The quiet an install needs: nothing under way, nothing starting until
/// it is dropped.
#[derive(Debug)]
pub(crate) struct Quiet(Arc<Activity>);

impl Drop for Quiet {
    fn drop(&mut self) {
        self.0.lock().installing = false;
    }
}

impl Activity {
    /// Starts work that a restart must not cut short.
    pub(crate) fn begin(self: &Arc<Self>) -> Result<Doing, Updating> {
        let mut state = self.lock();
        if state.installing {
            return Err(Updating);
        }
        state.under_way += 1;
        Ok(Doing(Arc::clone(self)))
    }

    /// Keeps new work out and waits up to `patience` for what is under way
    /// to end. Work goes on as before if it hasn't ended by then.
    pub(crate) fn quiesce(self: &Arc<Self>, patience: Duration) -> Result<Quiet, StillBusy> {
        let until = Instant::now() + patience;
        let mut state = self.lock();
        state.installing = true;
        while state.under_way > 0 {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                state.installing = false;
                return Err(StillBusy);
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        Ok(Quiet(Arc::clone(self)))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{Activity, StillBusy, Updating};

    #[test]
    fn keeps_new_work_out_only_while_an_update_installs() {
        let activity = Arc::new(Activity::default());

        let quiet = activity.quiesce(Duration::ZERO).unwrap();
        assert_eq!(activity.begin().map(drop), Err(Updating));

        drop(quiet);
        assert!(activity.begin().is_ok());
    }

    #[test]
    fn waits_for_work_under_way_to_end() {
        let activity = Arc::new(Activity::default());
        let doing = activity.begin().unwrap();
        let ending = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            drop(doing);
        });

        let started = Instant::now();
        let quiet = activity.quiesce(Duration::from_secs(5));

        assert!(quiet.is_ok());
        assert!(started.elapsed() >= Duration::from_millis(50));
        ending.join().unwrap();
    }

    #[test]
    fn gives_up_on_work_that_goes_on_and_lets_more_start() {
        let activity = Arc::new(Activity::default());
        let _doing = activity.begin().unwrap();

        assert_eq!(
            activity.quiesce(Duration::from_millis(20)).map(drop),
            Err(StillBusy)
        );
        assert!(activity.begin().is_ok());
    }
}
