//! A newer version: asked about now and then, told about when one is out,
//! and installed on a click. Nothing is downloaded before that click, and
//! nothing installed that isn't signed with the key the app was built with.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Manager, Url};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Error, Update, UpdaterExt};

use crate::activity::Activity;
use crate::settings::Store;

/// Left alone after the start, so the panel is up first.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(30);
/// Asked soon after automatic checks are turned on.
const SOON: Duration = Duration::from_secs(5);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// With nothing due, the thread wakes this often to look again.
const LONGEST_SLEEP: Duration = Duration::from_secs(60 * 60);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How long an install waits for a stop or a clean-up under way.
const PATIENCE: Duration = Duration::from_secs(30);
const RELEASES: &str = "https://github.com/alpgurlee/harbormaster/releases/tag/v";
/// Where a debug build may be told to ask instead, to try an update out.
const ENDPOINT_VAR: &str = "HARBORMASTER_UPDATE_ENDPOINT";
#[cfg(target_os = "macos")]
const APP_MANAGEMENT: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AppBundles";

/// What the windows are told of updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Status {
    /// Automatic checks are off, and none was asked for since.
    Off,
    /// Nothing newer, as of `checked_at` in seconds since the Unix epoch;
    /// never checked, without one.
    Idle {
        checked_at: Option<u64>,
    },
    Checking,
    Available {
        version: String,
    },
    Downloading {
        version: String,
        percent: Option<u8>,
    },
    Installing {
        version: String,
    },
    Failed {
        version: Option<String>,
        why: Why,
        message: String,
    },
}

impl Default for Status {
    fn default() -> Self {
        Self::Idle { checked_at: None }
    }
}

/// Why an update didn't come through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Why {
    /// The check itself failed, as without a network.
    Check,
    /// Something a restart would cut short was still under way.
    Busy,
    /// The system wouldn't let Harbormaster replace itself.
    Permission,
    /// The user didn't let it, at the password prompt.
    Cancelled,
    Install,
}

enum Wake {
    /// Automatic checks were turned on or off.
    Settings(bool),
    Check,
    Install,
}

/// What is known of updates, told to each window that asked.
#[derive(Default)]
pub(crate) struct Updates {
    state: Mutex<State>,
}

/// A window's ear, kept for as long as it says it heard.
type Told = Box<dyn Fn(&Status) -> bool + Send>;

#[derive(Default)]
struct State {
    status: Status,
    found: Option<Update>,
    wake: Option<Sender<Wake>>,
    told: Vec<(String, Told)>,
}

impl Updates {
    /// Tells the subscriber identified by `key` what is known now and then
    /// each change, for as long as `told` says it could be told. One that
    /// subscribes again, as a window that reloaded does, is told the new
    /// way only.
    pub(crate) fn subscribe(&self, key: &str, told: impl Fn(&Status) -> bool + Send + 'static) {
        let mut state = self.lock();
        state.told.retain(|(subscriber, _)| subscriber != key);
        if told(&state.status) {
            state.told.push((key.to_owned(), Box::new(told)));
        }
    }

    /// Asks for a check now, whatever the schedule.
    pub(crate) fn check_now(&self) {
        self.wake(Wake::Check);
    }

    /// Asks for the version found to be downloaded and installed.
    pub(crate) fn install(&self) {
        self.wake(Wake::Install);
    }

    /// Opens the release page of the version found in the browser.
    pub(crate) fn whats_new(&self, app: &AppHandle) -> Result<(), String> {
        let version = match &self.lock().status {
            Status::Available { version }
            | Status::Downloading { version, .. }
            | Status::Installing { version } => version.clone(),
            Status::Failed {
                version: Some(version),
                ..
            } => version.clone(),
            _ => return Err("no newer version is known".into()),
        };
        app.opener()
            .open_url(release_page(&version), None::<&str>)
            .map_err(|error| error.to_string())
    }

    /// Opens System Settings where macOS lets an app change apps.
    #[cfg(target_os = "macos")]
    pub(crate) fn open_app_management(app: &AppHandle) -> Result<(), String> {
        app.opener()
            .open_url(APP_MANAGEMENT, None::<&str>)
            .map_err(|error| error.to_string())
    }

    fn status(&self) -> Status {
        self.lock().status.clone()
    }

    /// Puts `status` in force and tells every subscriber, under the one
    /// lock, so none falls between.
    fn set(&self, status: Status) {
        let mut state = self.lock();
        state.told.retain(|(_, told)| told(&status));
        state.status = status;
    }

    fn found(&self) -> Option<Update> {
        self.lock().found.clone()
    }

    fn keep(&self, found: Option<Update>) {
        self.lock().found = found;
    }

    fn wake(&self, wake: Wake) {
        if let Some(sender) = &self.lock().wake {
            let _ = sender.send(wake);
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Starts the thread that checks and installs, following the settings.
pub(crate) fn start(app: AppHandle) {
    let (sender, receiver) = mpsc::channel();
    app.state::<Updates>().lock().wake = Some(sender.clone());
    // Told under the settings' lock, with what is in force first.
    app.state::<Store>().subscribe("updates", move |settings| {
        sender.send(Wake::Settings(settings.check_updates)).is_ok()
    });
    std::thread::Builder::new()
        .name("updater".into())
        .spawn(move || run(&app, &receiver))
        .expect("the updater thread should start");
}

fn run(app: &AppHandle, wakes: &Receiver<Wake>) {
    let mut automatic = false;
    let mut checked = false;
    let mut next: Option<Instant> = None;
    loop {
        let wait = next.map_or(LONGEST_SLEEP, |at| {
            at.saturating_duration_since(Instant::now())
        });
        match wakes.recv_timeout(wait) {
            Ok(Wake::Settings(on)) => {
                automatic = on;
                let updates = app.state::<Updates>();
                if on {
                    if next.is_none() {
                        next =
                            Some(Instant::now() + if checked { SOON } else { FIRST_CHECK_AFTER });
                    }
                    if updates.status() == Status::Off {
                        updates.set(Status::default());
                    }
                } else {
                    next = None;
                    if matches!(updates.status(), Status::Idle { .. }) {
                        updates.set(Status::Off);
                    }
                }
            }
            Ok(Wake::Check) => {
                check(app);
                checked = true;
                if automatic {
                    next = Some(Instant::now() + CHECK_EVERY);
                }
            }
            Ok(Wake::Install) => install(app),
            Err(RecvTimeoutError::Timeout) => {
                if automatic && next.is_some_and(|at| at <= Instant::now()) {
                    check(app);
                    checked = true;
                    next = Some(Instant::now() + CHECK_EVERY);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn check(app: &AppHandle) {
    let updates = app.state::<Updates>();
    if matches!(
        updates.status(),
        Status::Downloading { .. } | Status::Installing { .. }
    ) {
        return;
    }
    updates.set(Status::Checking);
    match newer(app) {
        Ok(Some(update)) => {
            let version = update.version.clone();
            updates.keep(Some(update));
            updates.set(Status::Available { version });
        }
        Ok(None) => {
            updates.keep(None);
            updates.set(Status::Idle {
                checked_at: Some(now()),
            });
        }
        Err(error) => updates.set(Status::Failed {
            version: None,
            why: Why::Check,
            message: error.to_string(),
        }),
    }
}

/// The newer version the endpoint names, if any.
fn newer(app: &AppHandle) -> Result<Option<Update>, Error> {
    let mut builder = app.updater_builder().timeout(CHECK_TIMEOUT);
    if let Some(endpoint) = endpoint_override(
        std::env::var(ENDPOINT_VAR).ok().as_deref(),
        cfg!(debug_assertions),
    ) {
        builder = builder.endpoints(vec![endpoint])?;
    }
    let updater = builder.build()?;
    tauri::async_runtime::block_on(updater.check())
}

/// Downloads the version found, waits for what a restart would cut short,
/// installs it and starts the new version. On Windows the installer takes
/// over and this process ends inside `install`.
fn install(app: &AppHandle) {
    let updates = app.state::<Updates>();
    if matches!(
        updates.status(),
        Status::Downloading { .. } | Status::Installing { .. }
    ) {
        return;
    }
    let Some(mut update) = updates.found() else {
        return;
    };
    let version = update.version.clone();
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    updates.set(Status::Downloading {
        version: version.clone(),
        percent: None,
    });
    let mut received = 0;
    let mut shown = None;
    let downloaded = tauri::async_runtime::block_on(update.download(
        |chunk, total| {
            received += chunk as u64;
            let now = percent(received, total);
            if now != shown {
                shown = now;
                updates.set(Status::Downloading {
                    version: version.clone(),
                    percent: now,
                });
            }
        },
        || {},
    ));
    let failed = |why: Why, message: String| Status::Failed {
        version: Some(version.clone()),
        why,
        message,
    };
    let bytes = match downloaded {
        Ok(bytes) => bytes,
        Err(error) => return updates.set(failed(Why::Install, error.to_string())),
    };
    let Ok(_quiet) = app.state::<Arc<Activity>>().quiesce(PATIENCE) else {
        return updates.set(failed(Why::Busy, String::new()));
    };
    updates.set(Status::Installing {
        version: version.clone(),
    });
    match update.install(bytes) {
        Ok(()) => app.restart(),
        Err(error) => updates.set(failed(why(&error), error.to_string())),
    }
}

/// Whether an update is coming: downloading, or installing, when the
/// password prompt or the installer takes the focus from the panel.
pub(crate) fn installing(app: &AppHandle) -> bool {
    app.try_state::<Updates>()
        .is_some_and(|updates| keeps_panel(&updates.status()))
}

/// Whether the panel stays up through a lost focus, to show how the update
/// goes and how it went.
fn keeps_panel(status: &Status) -> bool {
    matches!(
        status,
        Status::Downloading { .. } | Status::Installing { .. }
    )
}

/// Why an install failed, as far as the error says.
fn why(error: &Error) -> Why {
    match error {
        Error::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied => Why::Permission,
        Error::AuthenticationFailed => Why::Cancelled,
        _ => Why::Install,
    }
}

/// How much of `total` the `received` bytes are, once the total is known.
fn percent(received: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|&total| total > 0)?;
    Some((received.saturating_mul(100) / total).min(100) as u8)
}

/// Where a debug build is told to ask, if it is told; a release build asks
/// only where it was built to.
fn endpoint_override(told: Option<&str>, debug: bool) -> Option<Url> {
    if !debug {
        return None;
    }
    Url::parse(told?).ok()
}

fn release_page(version: &str) -> String {
    format!("{RELEASES}{version}")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tauri_plugin_updater::Error;

    use super::{Status, Updates, Why, endpoint_override, keeps_panel, percent, release_page, why};

    #[test]
    fn asks_elsewhere_only_when_a_debug_build_is_told_to() {
        let told = Some("http://127.0.0.1:8000/latest.json");

        assert_eq!(
            endpoint_override(told, true).map(|url| url.to_string()),
            Some("http://127.0.0.1:8000/latest.json".into())
        );
        assert_eq!(endpoint_override(told, false), None);
        assert_eq!(endpoint_override(Some("not a url"), true), None);
        assert_eq!(endpoint_override(None, true), None);
    }

    #[test]
    fn tells_a_refusal_by_the_system_from_a_dismissed_prompt_and_the_rest() {
        let refused = Error::Io(io::Error::from(io::ErrorKind::PermissionDenied));
        let dismissed = Error::AuthenticationFailed;
        let other = Error::Io(io::Error::from(io::ErrorKind::StorageFull));

        assert_eq!(why(&refused), Why::Permission);
        assert_eq!(why(&dismissed), Why::Cancelled);
        assert_eq!(why(&other), Why::Install);
    }

    #[test]
    fn counts_the_download_in_whole_percents_once_the_total_is_known() {
        assert_eq!(percent(50, Some(200)), Some(25));
        assert_eq!(percent(300, Some(200)), Some(100));
        assert_eq!(percent(50, None), None);
        assert_eq!(percent(50, Some(0)), None);
    }

    #[test]
    fn keeps_the_panel_up_only_while_an_update_comes() {
        assert!(keeps_panel(&Status::Downloading {
            version: "0.1.1".into(),
            percent: Some(42),
        }));
        assert!(keeps_panel(&Status::Installing {
            version: "0.1.1".into(),
        }));
        assert!(!keeps_panel(&Status::Available {
            version: "0.1.1".into(),
        }));
        assert!(!keeps_panel(&Status::Failed {
            version: Some("0.1.1".into()),
            why: Why::Cancelled,
            message: String::new(),
        }));
    }

    #[test]
    fn names_the_release_page_by_the_version() {
        assert_eq!(
            release_page("0.1.1"),
            "https://github.com/alpgurlee/harbormaster/releases/tag/v0.1.1"
        );
    }

    #[test]
    fn tells_a_window_what_is_known_and_then_each_change() {
        let updates = Updates::default();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let ear = Arc::clone(&heard);
        updates.subscribe("panel", move |status| {
            ear.lock().unwrap().push(status.clone());
            true
        });

        updates.set(Status::Checking);
        updates.set(Status::Available {
            version: "0.1.1".into(),
        });

        assert_eq!(
            *heard.lock().unwrap(),
            [
                Status::Idle { checked_at: None },
                Status::Checking,
                Status::Available {
                    version: "0.1.1".into()
                }
            ]
        );
    }

    #[test]
    fn stops_telling_a_window_that_cant_be_told() {
        let updates = Updates::default();
        let heard = Arc::new(Mutex::new(0));
        let ear = Arc::clone(&heard);
        updates.subscribe("panel", move |_| {
            *ear.lock().unwrap() += 1;
            false
        });

        updates.set(Status::Checking);

        assert_eq!(*heard.lock().unwrap(), 1);
    }
}
