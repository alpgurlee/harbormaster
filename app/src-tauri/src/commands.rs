use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use harbormaster_cleaner::Finding;
use harbormaster_engine::Port;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

use crate::cleaning::{Cleaning, MapEvent, Refusal, RunEvent, ScanEvent};
use crate::quitting::{QuitPreview, Quits};
use crate::scanning::{self, Scanning, Update};
use crate::stopping::{Approvals, Preview, Reason, Stopped};
use crate::{cleaner, panel};

#[tauri::command]
pub(crate) fn subscribe(channel: Channel<Update>, scanning: State<'_, Arc<Scanning>>) {
    scanning.subscribe(channel);
}

/// Opens a port the service listens on, never an address the page made up.
#[tauri::command]
pub(crate) fn open_in_browser(
    id: String,
    port: u16,
    app: AppHandle,
    scanning: State<'_, Arc<Scanning>>,
) -> Result<(), String> {
    let snapshot = scanning.snapshot().ok_or("nothing has been scanned yet")?;
    let listening = snapshot
        .services
        .iter()
        .filter(|service| service.id == id)
        .flat_map(|service| &service.ports)
        .find(|candidate| candidate.port == port)
        .ok_or("that server no longer listens there")?;
    app.opener()
        .open_url(url(listening), None::<&str>)
        .map_err(|error| error.to_string())
}

/// A listener on loopback or on every address answers on localhost.
fn url(port: &Port) -> String {
    match port.address {
        address if address.is_loopback() || address.is_unspecified() => {
            format!("http://localhost:{}", port.port)
        }
        IpAddr::V4(address) => format!("http://{address}:{}", port.port),
        IpAddr::V6(address) => format!("http://[{address}]:{}", port.port),
    }
}

#[tauri::command]
pub(crate) fn reveal_folder(
    id: String,
    app: AppHandle,
    scanning: State<'_, Arc<Scanning>>,
) -> Result<(), String> {
    let snapshot = scanning.snapshot().ok_or("nothing has been scanned yet")?;
    let root = snapshot
        .services
        .iter()
        .find(|service| service.id == id)
        .and_then(|service| service.project.as_ref())
        .map(|project| project.root.clone())
        .ok_or("that server has no project folder")?;
    app.opener()
        .reveal_item_in_dir(root)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn stop_preview(id: String, app: AppHandle) -> Result<Preview, Reason> {
    blocking(move || {
        Ok(match app.state::<Arc<Scanning>>().snapshot() {
            Some(snapshot) => app.state::<Approvals>().preview(&snapshot, &id),
            None => Preview::Refused {
                reason: Reason::NotFound,
            },
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn stop(token: u64, app: AppHandle) -> Result<Stopped, Reason> {
    blocking(move || app.state::<Approvals>().stop(token)).await
}

#[tauri::command]
pub(crate) async fn force_stop(token: u64, app: AppHandle) -> Result<Stopped, Reason> {
    blocking(move || app.state::<Approvals>().force(token)).await
}

/// `name` is an app as the breakdown lists it.
#[tauri::command]
pub(crate) async fn quit_app_preview(name: String, app: AppHandle) -> Result<QuitPreview, Reason> {
    blocking(move || {
        Ok(match app.state::<Arc<Scanning>>().snapshot() {
            Some(snapshot) => app
                .state::<Quits>()
                .preview(&scanning::servers(&snapshot.services), &name),
            None => QuitPreview::Refused {
                reason: Reason::NotFound,
            },
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn quit_app(token: u64, app: AppHandle) -> Result<Stopped, Reason> {
    blocking(move || app.state::<Quits>().quit(token)).await
}

#[tauri::command]
pub(crate) fn open_cleaner(app: AppHandle) -> Result<(), String> {
    cleaner::show(&app).map_err(|error| error.to_string())
}

/// Looks through this user's home for what could go; the findings come
/// last, under the token removing some of them takes.
#[tauri::command]
pub(crate) async fn clean_scan(
    channel: Channel<ScanEvent>,
    window: WebviewWindow,
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    // Taken before the work is queued, so closing the window stops it from here on.
    let working = cleaning.begin(|| open(&window))?;
    tauri::async_runtime::spawn_blocking(move || {
        working.scan(&|event| {
            // A closed window stops the scan itself.
            let _ = channel.send(event);
        })
    })
    .await
    .map_err(|error| Refusal::Failed {
        message: error.to_string(),
    })?
}

/// Hunts for this user's large files, telling the window as it goes.
#[tauri::command]
pub(crate) async fn clean_hunt(
    channel: Channel<ScanEvent>,
    window: WebviewWindow,
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    let working = cleaning.begin(|| open(&window))?;
    tauri::async_runtime::spawn_blocking(move || {
        working.hunt(&|event| {
            let _ = channel.send(event);
        })
    })
    .await
    .map_err(|error| Refusal::Failed {
        message: error.to_string(),
    })?
}

/// Removes the findings at `paths`, which the scan under `token` listed;
/// any other path refuses the whole request.
#[tauri::command]
pub(crate) async fn clean_run(
    token: u64,
    paths: Vec<PathBuf>,
    channel: Channel<RunEvent>,
    window: WebviewWindow,
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    let working = cleaning.begin(|| open(&window))?;
    tauri::async_runtime::spawn_blocking(move || {
        working.run(token, &paths, &mut |event| {
            let _ = channel.send(event);
        })
    })
    .await
    .map_err(|error| Refusal::Failed {
        message: error.to_string(),
    })?
}

/// Whether `window` is still there to see the work through; a closed one
/// can't be asked, and a minimized one still counts.
fn open(window: &WebviewWindow) -> bool {
    window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false)
}

/// Maps this user's home folder, telling the window as it goes; the map
/// comes last, under the token showing its places in Finder takes.
#[tauri::command]
pub(crate) async fn clean_map(
    others: bool,
    channel: Channel<MapEvent>,
    window: WebviewWindow,
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    let working = cleaning.begin(|| open(&window))?;
    tauri::async_runtime::spawn_blocking(move || {
        working.map(others, &mut |event| {
            let _ = channel.send(event);
        })
    })
    .await
    .map_err(|error| Refusal::Failed {
        message: error.to_string(),
    })?
}

/// The findings the scan under `token` listed, as they stand now.
#[tauri::command]
pub(crate) async fn clean_recheck(
    token: u64,
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<Vec<Finding>, Refusal> {
    let cleaning = Arc::clone(&cleaning);
    tauri::async_runtime::spawn_blocking(move || cleaning.recheck(token))
        .await
        .map_err(|error| Refusal::Failed {
            message: error.to_string(),
        })?
}

#[tauri::command]
pub(crate) fn clean_stop(cleaning: State<'_, Arc<Cleaning>>) {
    cleaning.stop();
}

/// The home folder's disk: how large it is and how much is free.
#[tauri::command]
pub(crate) fn clean_disk() -> Result<harbormaster_cleaner::Disk, Refusal> {
    let home = harbormaster_cleaner::account_home().map_err(|error| Refusal::Failed {
        message: error.to_string(),
    })?;
    harbormaster_cleaner::disk(&home).map_err(|error| Refusal::Failed {
        message: error.to_string(),
    })
}

/// Shows a finding in Finder; only one the scan listed.
#[tauri::command]
pub(crate) fn clean_reveal(
    path: PathBuf,
    app: AppHandle,
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    if !cleaning.lists(&path) {
        return Err(Refusal::Unlisted);
    }
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|error| Refusal::Failed {
            message: error.to_string(),
        })
}

/// Shows a place of the last map in Finder, if it is still what the map found.
#[tauri::command]
pub(crate) async fn clean_reveal_area(
    token: u64,
    id: u32,
    app: AppHandle,
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    let cleaning = Arc::clone(&cleaning);
    let path = tauri::async_runtime::spawn_blocking(move || cleaning.mapped(token, id))
        .await
        .map_err(|error| Refusal::Failed {
            message: error.to_string(),
        })??;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|error| Refusal::Failed {
            message: error.to_string(),
        })
}

/// Scans and waits for processes to exit run off the thread that draws.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Reason> + Send + 'static,
) -> Result<T, Reason> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| Reason::Failed {
            message: error.to_string(),
        })?
}

#[tauri::command]
pub(crate) fn resize_panel(
    height: f64,
    app: AppHandle,
    window: WebviewWindow,
) -> Result<(), String> {
    if !height.is_finite() {
        return Err("the panel's height must be a number".into());
    }
    panel::resize(&app, &window, height).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn hide_panel(app: AppHandle) {
    panel::hide(&app);
}

#[tauri::command]
pub(crate) fn quit(app: AppHandle) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use harbormaster_engine::Port;

    use super::url;

    fn at(address: &str, port: u16) -> Port {
        Port {
            port,
            address: address.parse::<IpAddr>().unwrap(),
        }
    }

    #[test]
    fn opens_a_listener_where_it_answers() {
        assert_eq!(url(&at("127.0.0.1", 3000)), "http://localhost:3000");
        assert_eq!(url(&at("::", 5173)), "http://localhost:5173");
        assert_eq!(url(&at("192.168.1.5", 8080)), "http://192.168.1.5:8080");
        assert_eq!(url(&at("fd00::5", 8080)), "http://[fd00::5]:8080");
    }
}
