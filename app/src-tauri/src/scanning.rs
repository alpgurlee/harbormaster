use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use harbormaster_engine::apps::AppUsage;
use harbormaster_engine::{Classification, Scanner, Service, Snapshot, live};
use harbormaster_platform::{ProcessKey, Thermal};
use serde::{Serialize, Serializer};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::settings::Store;
use crate::tray;

/// Scans come this often while the panel shows, and less often while it doesn't.
const SHOWN_EVERY: Duration = Duration::from_secs(2);
const HIDDEN_EVERY: Duration = Duration::from_secs(5);

/// The panel names this many apps by memory and sums up the rest.
const APPS_SHOWN: usize = 8;

/// One scan, the way the panel reads it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Update {
    services: Vec<Service>,
    memory: Option<Memory>,
    /// Read only while the panel shows.
    apps: Option<Vec<AppUsage>>,
    /// Milliseconds since the Unix epoch.
    scanned_at: u64,
    #[serde(serialize_with = "named")]
    thermal: Thermal,
}

/// How hard the Mac is cooling down, by the panel's name for it.
fn named<S: Serializer>(thermal: &Thermal, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(match thermal {
        Thermal::Nominal => "nominal",
        Thermal::Fair => "fair",
        Thermal::Serious => "serious",
        Thermal::Critical => "critical",
        Thermal::Unknown => "unknown",
    })
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
        let mut shown = None;
        loop {
            match live::scan(&mut scanner, &home) {
                Ok(mut snapshot) => {
                    if let Some(store) = app.try_state::<Store>() {
                        hide(&mut snapshot.services, &store.get().hidden);
                    }
                    let count = snapshot
                        .services
                        .iter()
                        .filter(|service| service.classification == Classification::Dev)
                        .count();
                    // Drawn again when the count changes, or on Windows the taskbar's colour.
                    let face = tray::face(count);
                    if shown != Some(face) {
                        tray::show(app, face);
                        shown = Some(face);
                    }
                    self.publish(snapshot);
                }
                Err(error) => eprintln!("harbormaster: the scan failed: {error}"),
            }
            self.pause();
        }
    }

    fn publish(&self, snapshot: Snapshot) {
        let shown = self.lock().shown;
        let update = Update {
            services: snapshot.services.clone(),
            memory: harbormaster_platform::memory().map(|memory| Memory {
                used: memory.used,
                total: memory.total,
            }),
            apps: shown.then(|| apps(&snapshot.services)).flatten(),
            scanned_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| {
                    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
                }),
            thermal: harbormaster_platform::thermal(),
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

/// Takes out of the dev servers those any of whose processes runs a
/// program the user hid; they are listed with the other listeners.
fn hide(services: &mut [Service], hidden: &[String]) {
    for service in services {
        if service.classification == Classification::Dev
            && service
                .processes
                .iter()
                .any(|member| hidden.contains(&member.name))
        {
            service.classification = Classification::Other;
        }
    }
}

/// The dev servers' processes, which the breakdown counts on their own.
pub(crate) fn servers(services: &[Service]) -> HashSet<ProcessKey> {
    services
        .iter()
        .filter(|service| service.classification == Classification::Dev)
        .flat_map(|service| service.processes.iter().map(|member| member.key))
        .collect()
}

/// The apps using the most memory, the dev servers left out.
fn apps(services: &[Service]) -> Option<Vec<AppUsage>> {
    match live::apps(&servers(services)) {
        Ok(mut apps) => {
            apps.truncate(APPS_SHOWN);
            Some(apps)
        }
        Err(error) => {
            eprintln!("harbormaster: reading memory by app failed: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use harbormaster_engine::apps::AppUsage;
    use harbormaster_engine::{Classification, Member, Service};
    use harbormaster_platform::{Pid, ProcessKey, StartTime, Thermal};

    use super::{Memory, Update, hide};

    /// A service whose processes are named `names`, its root first.
    fn service(classification: Classification, names: &[&str]) -> Service {
        Service {
            id: format!("{}-1", names.len()),
            classification,
            protected: None,
            ports: Vec::new(),
            project: None,
            command: None,
            started_at: 0,
            memory_bytes: None,
            cpu_percent: None,
            processes: names
                .iter()
                .zip(1..)
                .map(|(name, pid)| Member {
                    key: ProcessKey {
                        pid: Pid(pid),
                        start: StartTime(1),
                        exec: 0,
                    },
                    pid,
                    name: (*name).into(),
                    command: None,
                    memory_bytes: None,
                    cpu_percent: None,
                })
                .collect(),
            holders: Vec::new(),
            ancestors: Vec::new(),
        }
    }

    #[test]
    fn lists_what_a_hidden_program_runs_with_the_other_listeners() {
        let mut services = vec![
            service(Classification::Dev, &["npm", "node"]),
            service(Classification::Dev, &["python3"]),
            service(Classification::System, &["node"]),
        ];

        hide(&mut services, &["node".into()]);

        let classes: Vec<Classification> = services
            .iter()
            .map(|service| service.classification)
            .collect();
        assert_eq!(
            classes,
            [
                Classification::Other,
                Classification::Dev,
                Classification::System
            ]
        );
    }

    #[test]
    fn tells_the_panel_what_it_expects() {
        let update = Update {
            services: Vec::new(),
            memory: Some(Memory { used: 1, total: 2 }),
            apps: Some(vec![AppUsage {
                id: "user/app:Google Chrome".into(),
                name: "Google Chrome".into(),
                memory_bytes: 3,
                system: false,
                members: Vec::new(),
            }]),
            scanned_at: 42,
            thermal: Thermal::Serious,
        };

        assert_eq!(
            serde_json::to_value(update).unwrap(),
            serde_json::json!({
                "services": [],
                "memory": { "used": 1, "total": 2 },
                "apps": [{
                    "id": "user/app:Google Chrome",
                    "name": "Google Chrome",
                    "memoryBytes": 3,
                    "system": false
                }],
                "scannedAt": 42,
                "thermal": "serious"
            })
        );
    }

    #[test]
    fn names_each_thermal_state_as_the_panel_reads_it() {
        let named = |thermal| {
            let update = Update {
                services: Vec::new(),
                memory: None,
                apps: None,
                scanned_at: 0,
                thermal,
            };
            serde_json::to_value(update).unwrap()["thermal"].clone()
        };

        assert_eq!(
            [
                Thermal::Nominal,
                Thermal::Fair,
                Thermal::Serious,
                Thermal::Critical,
                Thermal::Unknown,
            ]
            .map(named),
            ["nominal", "fair", "serious", "critical", "unknown"]
        );
    }
}
