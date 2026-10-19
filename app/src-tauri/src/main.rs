#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cleaner;
mod cleaning;
mod commands;
mod daemons;
mod panel;
mod quitting;
mod scanning;
mod settings;
mod stopping;
mod tray;

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
    tauri::Builder::default()
        // A second launch shows the first one's panel instead.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            panel::show(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::subscribe,
            commands::open_in_browser,
            commands::reveal_folder,
            commands::stop_preview,
            commands::stop,
            commands::force_stop,
            commands::quit_app_preview,
            commands::quit_app,
            commands::open_cleaner,
            commands::open_settings,
            commands::settings_get,
            commands::settings_subscribe,
            commands::settings_set,
            commands::clean_scan,
            commands::clean_hunt,
            commands::clean_apps,
            commands::clean_map,
            commands::clean_run,
            commands::clean_recheck,
            commands::clean_stop,
            commands::clean_disk,
            commands::clean_reveal,
            commands::clean_reveal_area,
            commands::clean_daemons,
            commands::clean_daemon_preview,
            commands::clean_daemon_stop,
            commands::resize_panel,
            commands::hide_panel,
            commands::quit,
        ])
        .setup(|app| {
            // A menu bar app: no Dock icon and no app menu.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            app.manage(panel::Anchor::default());
            app.manage(stopping::Approvals::default());
            app.manage(quitting::Quits::default());
            app.manage(daemons::Daemons::default());
            app.manage(std::sync::Arc::new(cleaning::Cleaning::default()));
            app.manage(settings::Store::open(
                app.path().app_config_dir()?.join("settings.json"),
                harbormaster_cleaner::account_home()?,
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
