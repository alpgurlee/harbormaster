//! The cleaner's own window, over the same pages as the panel.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const LABEL: &str = "cleaner";

/// Brings the cleaner window up, making it the first time.
pub(crate) fn show(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::default())
        .title("Harbormaster")
        .inner_size(880.0, 600.0)
        .min_inner_size(760.0, 520.0)
        .center();
    // The page runs up under the title bar, as a Mac app's own content does.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    let window = builder.build()?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        // Closing only hides the window: a scan or removal under way goes
        // on, and the window comes back to it, and to its list, as it was.
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(window) = handle.get_webview_window(LABEL) {
                let _ = window.hide();
            }
        }
    });
    window.set_focus()
}
