use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use harbormaster_engine::{Classification, Scanner, Service, Snapshot, live};
use serde::Serialize;
use tauri::AppHandle;
use tauri::ipc::Channel;

use crate::tray;

/// Scans come this often while the panel shows, and less often while it doesn't.
const SHOWN_EVERY: Duration = Duration::from_secs(2);
const HIDDEN_EVERY: Duration = Duration::from_secs(5);

/// One scan, the way the panel reads it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Update {
    services: Vec<Service>,
    memory: Option<Memory>,
    /// Milliseconds since the Unix epoch.
    scanned_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct Memory {
    used: u64,
    total: u64,
}

/// The scanner's thread and what it last found, for the panel and for stops.
#[derive(Default)]
pub(crate) struct Scanning {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    snapshot: Option<Arc<Snapshot>>,
    update: Option<Update>,
    panel: Option<Channel<Update>>,
    shown: bool,
    hurry: bool,
}

/// Where project lookups stop; without a home folder, at the root.
pub(crate) fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

pub(crate) fn start(app: AppHandle) -> Arc<Scanning> {
    let scanning = Arc::new(Scanning::default());
    let running = Arc::clone(&scanning);
    std::thread::Builder::new()
        .name("scanner".into())
        .spawn(move || running.run(&app))
        .expect("the scanner thread should start");
    scanning
}

impl Scanning {
    fn run(&self, app: &AppHandle) {
        let home = home();
        let mut scanner = Scanner::default();
        let mut counted = None;
        loop {
            match live::scan(&mut scanner, &home) {
                Ok(snapshot) => {
                    let count = snapshot
                        .services
                        .iter()
                        .filter(|service| service.classification == Classification::Dev)
                        .count();
                    if counted != Some(count) {
                        tray::show_count(app, count);
                        counted = Some(count);
                    }
                    self.publish(snapshot);
                }
                Err(error) => eprintln!("harbormaster: the scan failed: {error}"),
            }
            self.pause();
        }
    }

    fn publish(&self, snapshot: Snapshot) {
        let update = Update {
            services: snapshot.services.clone(),
            memory: harbormaster_platform::memory().map(|memory| Memory {
                used: memory.used,
                total: memory.total,
            }),
            scanned_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| {
                    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
                }),
        };
        let mut state = self.lock();
        // A panel that reloaded subscribes again; its old channel goes.
        if let Some(panel) = &state.panel
            && panel.send(update.clone()).is_err()
        {
            state.panel = None;
        }
        state.snapshot = Some(Arc::new(snapshot));
        state.update = Some(update);
    }

    fn pause(&self) {
        let state = self.lock();
        let every = if state.shown {
            SHOWN_EVERY
        } else {
            HIDDEN_EVERY
        };
        let (mut state, _) = self
            .wake
            .wait_timeout_while(state, every, |state| !state.hurry)
            .unwrap_or_else(PoisonError::into_inner);
        state.hurry = false;
    }

    /// The panel gets the latest scan now and every one after it.
    pub(crate) fn subscribe(&self, panel: Channel<Update>) {
        let mut state = self.lock();
        if let Some(update) = &state.update {
            let _ = panel.send(update.clone());
        }
        state.panel = Some(panel);
    }

    /// While the panel shows, scans come faster, the first one right away.
    pub(crate) fn set_shown(&self, shown: bool) {
        let mut state = self.lock();
        state.shown = shown;
        if shown {
            state.hurry = true;
            self.wake.notify_one();
        }
    }

    pub(crate) fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.lock().snapshot.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::{Memory, Update};

    #[test]
    fn tells_the_panel_what_it_expects() {
        let update = Update {
            services: Vec::new(),
            memory: Some(Memory { used: 1, total: 2 }),
            scanned_at: 42,
        };

        assert_eq!(
            serde_json::to_value(update).unwrap(),
            serde_json::json!({ "services": [], "memory": { "used": 1, "total": 2 }, "scannedAt": 42 })
        );
    }
}
