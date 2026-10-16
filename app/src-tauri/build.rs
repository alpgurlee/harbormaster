fn main() {
    // Each window gets only the commands its capability file grants; without
    // this list every command would be open to every window.
    let commands = tauri_build::AppManifest::new().commands(&[
        "subscribe",
        "open_in_browser",
        "reveal_folder",
        "stop_preview",
        "stop",
        "force_stop",
        "quit_app_preview",
        "quit_app",
        "open_cleaner",
        "open_settings",
        "settings_get",
        "settings_set",
        "clean_scan",
        "clean_hunt",
        "clean_apps",
        "clean_map",
        "clean_run",
        "clean_recheck",
        "clean_stop",
        "clean_disk",
        "clean_reveal",
        "clean_reveal_area",
        "resize_panel",
        "hide_panel",
        "quit",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(commands))
        .expect("the Tauri configuration should be valid");
}
