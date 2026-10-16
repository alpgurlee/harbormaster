#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod panel;
mod quitting;
mod scanning;
mod stopping;
mod tray;

use tauri::Manager;

fn main() {
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
            panel::create(app.handle())?;
            tray::create(app.handle())?;
            app.manage(scanning::start(app.handle().clone()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Harbormaster should start");
}
