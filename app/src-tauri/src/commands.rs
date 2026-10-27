use std::net::IpAddr;
use std::sync::Arc;

use harbormaster_engine::Port;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

use crate::activity::Activity;
use crate::quitting::{QuitPreview, Quits};
use crate::scanning::{self, Scanning, Update};
use crate::settings::{Invalid, Settings, Store};
use crate::stopping::{Approvals, Preview, Reason, Stopped};
use crate::updates::{Status, Updates};
use crate::{panel, settings};

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
    let url =
        url(listening).ok_or("that server answers only on a network interface it doesn't name")?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

/// A listener on loopback or on every address answers on localhost. One on
/// a link-local IPv6 address answers only on the interface it named, which
/// isn't known here, so no URL reaches it.
fn url(port: &Port) -> Option<String> {
    match port.address {
        address if address.is_loopback() || address.is_unspecified() => {
            Some(format!("http://localhost:{}", port.port))
        }
        IpAddr::V4(address) => Some(format!("http://{address}:{}", port.port)),
        IpAddr::V6(address) if address.is_unicast_link_local() => None,
        IpAddr::V6(address) => Some(format!("http://[{address}]:{}", port.port)),
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
    blocking(move || {
        let _doing = doing(&app)?;
        app.state::<Approvals>().stop(token)
    })
    .await
}

/// Only a Mac stops in two rounds; on Windows the stop is the forced one.
#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn force_stop(token: u64, app: AppHandle) -> Result<Stopped, Reason> {
    blocking(move || {
        let _doing = doing(&app)?;
        app.state::<Approvals>().force(token)
    })
    .await
}

/// `id` names an app as the breakdown lists it.
#[tauri::command]
pub(crate) async fn quit_app_preview(id: String, app: AppHandle) -> Result<QuitPreview, Reason> {
    blocking(move || {
        Ok(match app.state::<Arc<Scanning>>().snapshot() {
            Some(snapshot) => app
                .state::<Quits>()
                .preview(&scanning::servers(&snapshot.services), &id),
            None => QuitPreview::Refused {
                reason: Reason::NotFound,
            },
        })
    })
    .await
}

/// What ending every process of the app `id` names at once would do, for
/// Windows to offer once asking it to close left some of it open.
#[cfg(windows)]
#[tauri::command]
pub(crate) async fn force_quit_app_preview(
    id: String,
    app: AppHandle,
) -> Result<QuitPreview, Reason> {
    blocking(move || {
        Ok(match app.state::<Arc<Scanning>>().snapshot() {
            Some(snapshot) => app
                .state::<Quits>()
                .force_preview(&scanning::servers(&snapshot.services), &id),
            None => QuitPreview::Refused {
                reason: Reason::NotFound,
            },
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn quit_app(token: u64, app: AppHandle) -> Result<Stopped, Reason> {
    blocking(move || {
        let _doing = doing(&app)?;
        app.state::<Quits>().quit(token)
    })
    .await
}

/// Counts a stop or a quit as under way, so an update waits for it; none
/// starts while an update installs.
pub(crate) fn doing(app: &AppHandle) -> Result<crate::activity::Doing, Reason> {
    app.state::<Arc<Activity>>()
        .begin()
        .map_err(|_| Reason::Failed {
            message: UPDATING.into(),
        })
}

/// Why nothing starts while an update installs.
pub(crate) const UPDATING: &str = "Harbormaster is updating.";

/// Tells the window what is known of updates now and each time it changes.
#[tauri::command]
pub(crate) fn update_subscribe(
    channel: Channel<Status>,
    window: WebviewWindow,
    updates: State<'_, Updates>,
) {
    updates.subscribe(window.label(), move |status| {
        channel.send(status.clone()).is_ok()
    });
}

#[tauri::command]
pub(crate) fn update_check(updates: State<'_, Updates>) {
    updates.check_now();
}

#[tauri::command]
pub(crate) fn update_install(updates: State<'_, Updates>) {
    updates.install();
}

/// Opens the release page of the version found in the browser.
#[tauri::command]
pub(crate) fn update_whats_new(app: AppHandle) -> Result<(), String> {
    app.state::<Updates>().whats_new(&app)
}

/// Opens System Settings at App Management, where macOS lets Harbormaster
/// replace itself once it refused to.
#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) fn update_open_app_management(app: AppHandle) -> Result<(), String> {
    Updates::open_app_management(&app)
}

/// Async, as a window Windows builds in a synchronous command deadlocks and
/// stays white.
#[tauri::command]
pub(crate) async fn open_settings(app: AppHandle) -> Result<(), String> {
    settings::show(&app).map_err(|error| error.to_string())
}

/// What the user has set.
#[tauri::command]
pub(crate) fn settings_get(store: State<'_, Store>) -> Settings {
    store.get()
}

/// Tells the window what the user has set now and each time it changes.
#[tauri::command]
pub(crate) fn settings_subscribe(
    channel: Channel<Settings>,
    window: WebviewWindow,
    store: State<'_, Store>,
) {
    store.subscribe(window.label(), move |settings| {
        channel.send(settings.clone()).is_ok()
    });
}

/// Checks and keeps `settings`; every window that subscribed is told.
#[tauri::command]
pub(crate) fn settings_set(
    settings: Settings,
    store: State<'_, Store>,
) -> Result<Settings, Invalid> {
    store.set(settings)
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

/// The user drags the panel off its icon; it stays where they put it.
#[tauri::command]
pub(crate) fn drag_panel(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    panel::drag(&app, &window).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn anchor_panel(app: AppHandle, window: WebviewWindow) {
    panel::anchor(&app, &window);
}

/// Whether the panel sits where the user put it rather than by its icon.
#[tauri::command]
pub(crate) fn panel_placed(app: AppHandle) -> bool {
    panel::placed(&app)
}

#[tauri::command]
pub(crate) fn quit(app: AppHandle) {
    app.exit(0);
}

/// Scans and waits for processes to exit run off the thread that draws.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Reason> + Send + 'static,
) -> Result<T, Reason> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| Reason::Failed {
            message: error.to_string(),
        })?
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
        let url = |address, port| url(&at(address, port));
        assert_eq!(url("127.0.0.1", 3000).unwrap(), "http://localhost:3000");
        assert_eq!(url("::", 5173).unwrap(), "http://localhost:5173");
        assert_eq!(url("192.168.1.5", 8080).unwrap(), "http://192.168.1.5:8080");
        assert_eq!(url("fd00::5", 8080).unwrap(), "http://[fd00::5]:8080");
    }

    #[test]
    fn opens_no_link_local_listener_whose_interface_it_cant_name() {
        assert_eq!(url(&at("fe80::1", 3000)), None);
        assert_eq!(url(&at("febf::a:1", 3000)), None);
    }
}
