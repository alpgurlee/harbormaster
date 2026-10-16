//! The cleaner's own window, over the same pages as the panel.

use std::sync::Arc;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::cleaning::Cleaning;

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
        .inner_size(760.0, 560.0)
        .min_inner_size(640.0, 480.0)
        .center();
    // The page runs up under the title bar, as a Mac app's own content does.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    let window = builder.build()?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        // Nobody is left to see a scan or removal through.
        if let WindowEvent::Destroyed = event {
            handle.state::<Arc<Cleaning>>().stop();
        }
    });
    window.set_focus()
}
