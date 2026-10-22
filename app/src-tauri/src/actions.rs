//! What the panel and the cleaner window may do to this Mac besides
//! stopping servers: quit apps, and find and remove what could go. macOS
//! only for now.

use std::path::PathBuf;
use std::sync::Arc;

use harbormaster_cleaner::Finding;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::cleaner;
use crate::cleaning::{Cleaning, MapEvent, Refusal, RunEvent, ScanEvent};
use crate::commands::blocking;
use crate::daemons::{DaemonPreview, Daemons};
use crate::quitting::{QuitPreview, Quits};
use crate::scanning::{self, Scanning};
use crate::settings::Store;
use crate::stopping::{Reason, Stopped};

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

/// Of the items listed under `token` and in use, those only known dev
/// daemons keep so, which the window may offer to stop.
#[tauri::command]
pub(crate) async fn clean_daemons(token: u64, app: AppHandle) -> Result<Vec<PathBuf>, Reason> {
    blocking(move || {
        let listed = app
            .state::<Arc<Cleaning>>()
            .shown(token)
            .map_err(|_| Reason::Expired)?;
        app.state::<Daemons>().stoppable(&listed)
    })
    .await
}

/// What stopping the daemons that keep the item at `path` in use would do.
#[tauri::command]
pub(crate) async fn clean_daemon_preview(
    token: u64,
    path: PathBuf,
    app: AppHandle,
) -> Result<DaemonPreview, Reason> {
    blocking(move || {
        let listed = app
            .state::<Arc<Cleaning>>()
            .shown(token)
            .map_err(|_| Reason::Expired)?;
        Ok(app.state::<Daemons>().preview(token, &path, &listed))
    })
    .await
}

/// Stops the daemons the window was shown and the user agreed to stop.
#[tauri::command]
pub(crate) async fn clean_daemon_stop(approval: u64, app: AppHandle) -> Result<Stopped, Reason> {
    blocking(move || {
        let cleaning = app.state::<Arc<Cleaning>>();
        app.state::<Daemons>()
            .stop(approval, |token| cleaning.shown(token).ok())
    })
    .await
}

#[tauri::command]
pub(crate) fn open_cleaner(app: AppHandle) -> Result<(), String> {
    cleaner::show(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn clean_scan(
    channel: Channel<ScanEvent>,
    cleaning: State<'_, Arc<Cleaning>>,
    store: State<'_, Store>,
) -> Result<(), Refusal> {
    let settings = store.get();
    // Taken before the work is queued, so a stop from here on holds.
    let working = cleaning.begin()?;
    tauri::async_runtime::spawn_blocking(move || {
        working.scan(&settings.roots, settings.idle_days, &|event| {
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
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    let working = cleaning.begin()?;
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
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    let working = cleaning.begin()?;
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

/// Lists this user's own apps, telling the window as it goes; they come
/// last with what else is shown, under the token removing some takes.
#[tauri::command]
pub(crate) async fn clean_apps(
    channel: Channel<ScanEvent>,
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    let working = cleaning.begin()?;
    tauri::async_runtime::spawn_blocking(move || {
        working.apps(&|event| {
            let _ = channel.send(event);
        })
    })
    .await
    .map_err(|error| Refusal::Failed {
        message: error.to_string(),
    })?
}

/// Maps this user's home folder, telling the window as it goes; the map
/// comes last, under the token showing its places in Finder takes.
#[tauri::command]
pub(crate) async fn clean_map(
    others: bool,
    channel: Channel<MapEvent>,
    cleaning: State<'_, Arc<Cleaning>>,
) -> Result<(), Refusal> {
    let working = cleaning.begin()?;
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
