#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Stopping, quitting apps and cleaning are macOS's alone for now: on Windows
// the panel only shows what runs.
#[cfg(target_os = "macos")]
mod actions;
#[cfg(target_os = "macos")]
mod cleaner;
#[cfg(target_os = "macos")]
mod cleaning;
mod commands;
#[cfg(target_os = "macos")]
mod daemons;
mod panel;
#[cfg(target_os = "macos")]
mod quitting;
mod scanning;
mod settings;
#[cfg(target_os = "macos")]
mod stopping;
mod tray;

use std::io;
use std::path::PathBuf;

use tauri::Manager;

fn main() {
    if harbormaster_platform::runs_as_root() {
        eprintln!(
            "Harbormaster won't run as root, since it judges what is safe to stop or remove by your own user."
        );
        std::process::exit(1);
    }
    // More open files let the cleaner measure more places at once; without, fewer are.
    let _ = harbormaster_platform::allow_more_open_files();
    let builder = tauri::Builder::default()
        // A second launch shows the first one's panel instead.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            panel::show(app);
        }))
        .plugin(tauri_plugin_opener::init());
    #[cfg(target_os = "macos")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::subscribe,
        commands::open_in_browser,
        commands::reveal_folder,
        actions::stop_preview,
        actions::stop,
        actions::force_stop,
        actions::quit_app_preview,
        actions::quit_app,
        actions::open_cleaner,
        commands::open_settings,
        commands::settings_get,
        commands::settings_subscribe,
        commands::settings_set,
        actions::clean_scan,
        actions::clean_hunt,
        actions::clean_apps,
        actions::clean_map,
        actions::clean_run,
        actions::clean_recheck,
        actions::clean_stop,
        actions::clean_disk,
        actions::clean_reveal,
        actions::clean_reveal_area,
        actions::clean_daemons,
        actions::clean_daemon_preview,
        actions::clean_daemon_stop,
        commands::resize_panel,
        commands::hide_panel,
        commands::drag_panel,
        commands::anchor_panel,
        commands::panel_placed,
        commands::quit,
    ]);
    #[cfg(windows)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::subscribe,
        commands::open_in_browser,
        commands::reveal_folder,
        commands::open_settings,
        commands::settings_get,
        commands::settings_subscribe,
        commands::settings_set,
        commands::resize_panel,
        commands::hide_panel,
        commands::drag_panel,
        commands::anchor_panel,
        commands::panel_placed,
        commands::quit,
    ]);
    builder
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                // A menu bar app: no Dock icon and no app menu.
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                app.manage(stopping::Approvals::default());
                app.manage(quitting::Quits::default());
                app.manage(daemons::Daemons::default());
                app.manage(std::sync::Arc::new(cleaning::Cleaning::default()));
            }
            app.manage(panel::Anchor::default());
            app.manage(settings::Store::open(
                app.path().app_config_dir()?.join("settings.json"),
                home()?,
            ));
            panel::create(app.handle())?;
            tray::create(app.handle())?;
            tray::speak(app.handle(), &app.state::<settings::Store>());
            app.manage(scanning::start(app.handle().clone()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Harbormaster should start");
}

/// The home folder of the account this runs as, read from the account
/// itself rather than from $HOME, which whatever starts us may set.
#[cfg(target_os = "macos")]
fn home() -> io::Result<PathBuf> {
    harbormaster_cleaner::account_home()
}

/// The user's profile folder.
#[cfg(windows)]
fn home() -> io::Result<PathBuf> {
    std::env::home_dir().ok_or_else(|| io::ErrorKind::NotFound.into())
}
