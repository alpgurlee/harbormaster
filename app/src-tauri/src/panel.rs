use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, LogicalSize, Manager, Monitor, Rect, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::scanning::Scanning;
use crate::tray;

const LABEL: &str = "panel";
const WIDTH: f64 = 340.0;
const MIN_HEIGHT: f64 = 120.0;
const MAX_HEIGHT: f64 = 600.0;
/// Between the panel and the icon or the screen's edge, in points.
const GAP: f64 = 6.0;
/// A click on the icon takes the panel's focus and so closes it; that same
/// click must not open it again.
const REOPEN_AFTER: Duration = Duration::from_millis(250);

/// The tray icon's last place, which the panel hangs from.
#[derive(Debug, Default)]
pub(crate) struct Anchor(Mutex<Hold>);

#[derive(Debug, Default)]
struct Hold {
    icon: Option<Rect>,
    hidden_at: Option<Instant>,
    /// The user dragged the panel off its icon, to another screen perhaps:
    /// it opens where they left it, and stays open when it loses focus.
    placed: bool,
}

impl Anchor {
    fn lock(&self) -> MutexGuard<'_, Hold> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The icon the panel hangs from, unless the user put it elsewhere.
    fn hung_from(&self) -> Option<Rect> {
        let hold = self.lock();
        hold.icon.filter(|_| !hold.placed)
    }
}

/// The panel exists from the start, hidden, so it shows at once.
pub(crate) fn create(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::default())
        .title("Harbormaster")
        .inner_size(WIDTH, 420.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?;
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState, apply_vibrancy};
        // Without it the panel is just opaque.
        let _ = apply_vibrancy(
            &window,
            NSVisualEffectMaterial::Popover,
            Some(NSVisualEffectState::Active),
            Some(10.0),
        );
    }
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event
            && !handle.state::<Anchor>().lock().placed
        {
            hide(&handle);
        }
    });
    Ok(())
}

/// The user drags the panel away from its icon. It stays where they put it.
pub(crate) fn drag(app: &AppHandle, window: &WebviewWindow) -> tauri::Result<()> {
    app.state::<Anchor>().lock().placed = true;
    window.start_dragging()
}

/// Hangs the panel from its icon again.
pub(crate) fn anchor(app: &AppHandle, window: &WebviewWindow) {
    app.state::<Anchor>().lock().placed = false;
    place(app, window);
}

/// Whether the user put the panel somewhere of their own.
pub(crate) fn placed(app: &AppHandle) -> bool {
    app.state::<Anchor>().lock().placed
}

/// The tray icon was clicked: open the panel under it, or close it.
pub(crate) fn toggle(app: &AppHandle, icon: Rect) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let just_hidden = {
        let anchor = app.state::<Anchor>();
        let mut hold = anchor.lock();
        hold.icon = Some(icon);
        hold.hidden_at.is_some_and(|at| at.elapsed() < REOPEN_AFTER)
    };
    if window.is_visible().unwrap_or(false) {
        hide(app);
    } else if !just_hidden {
        reveal(app, &window);
    }
}

/// Shows the panel under the tray icon, as when Harbormaster is launched again.
pub(crate) fn show(app: &AppHandle) {
    // Before any click there is no anchor, and the screens may have changed since.
    if let Some(icon) = app
        .tray_by_id(tray::ID)
        .and_then(|tray| tray.rect().ok().flatten())
    {
        app.state::<Anchor>().lock().icon = Some(icon);
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        reveal(app, &window);
    }
}

fn reveal(app: &AppHandle, window: &WebviewWindow) {
    // Put on a screen that is gone since, it would open out of reach.
    let anchor = app.state::<Anchor>();
    if anchor.lock().placed && !on_a_screen(app, window) {
        anchor.lock().placed = false;
    }
    place(app, window);
    let _ = window.show();
    let _ = window.set_focus();
    if let Some(scanning) = app.try_state::<Arc<Scanning>>() {
        scanning.set_shown(true);
    }
}

pub(crate) fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    app.state::<Anchor>().lock().hidden_at = Some(Instant::now());
    if let Some(scanning) = app.try_state::<Arc<Scanning>>() {
        scanning.set_shown(false);
    }
}

/// The panel is as tall as its content, and stays where it hangs or was put.
pub(crate) fn resize(app: &AppHandle, window: &WebviewWindow, height: f64) -> tauri::Result<()> {
    window.set_size(LogicalSize::new(
        WIDTH,
        height.clamp(MIN_HEIGHT, MAX_HEIGHT),
    ))?;
    place(app, window);
    Ok(())
}

fn place(app: &AppHandle, window: &WebviewWindow) {
    let Some(icon) = app.state::<Anchor>().hung_from() else {
        return;
    };
    // Tray icons report their place in physical pixels.
    let position = icon.position.to_physical::<f64>(1.0);
    let size = icon.size.to_physical::<f64>(1.0);
    // The screen whose physical bounds hold the icon: on a Mac, monitor_from_point
    // reads points, not pixels, and finds none.
    let monitor = app.available_monitors().ok().and_then(|monitors| {
        monitors
            .into_iter()
            .find(|monitor| holds(screen(monitor), position.x, position.y))
    });
    let (Some(monitor), Ok(panel), Ok(scale)) =
        (monitor, window.outer_size(), window.scale_factor())
    else {
        return;
    };
    let target = monitor.scale_factor();
    let work = monitor.work_area();
    let (x, y) = spot(
        Area {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        },
        Area {
            x: f64::from(work.position.x),
            y: f64::from(work.position.y),
            width: f64::from(work.size.width),
            height: f64::from(work.size.height),
        },
        rescale(
            (f64::from(panel.width), f64::from(panel.height)),
            scale,
            target,
        ),
        GAP * target,
    );
    // A Mac moves windows in points, which every screen shares.
    #[cfg(target_os = "macos")]
    let _ = window.set_position(tauri::LogicalPosition::new(x / target, y / target));
    #[cfg(not(target_os = "macos"))]
    let _ = window.set_position(tauri::PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    ));
}

fn holds(area: Area, x: f64, y: f64) -> bool {
    (area.x..area.x + area.width).contains(&x) && (area.y..area.y + area.height).contains(&y)
}

/// Whether any of the window shows on one of the screens there are now.
/// Not knowing counts as yes: the window is left where it is.
fn on_a_screen(app: &AppHandle, window: &WebviewWindow) -> bool {
    let (Ok(position), Ok(size), Ok(monitors)) = (
        window.outer_position(),
        window.outer_size(),
        app.available_monitors(),
    ) else {
        return true;
    };
    let panel = Area {
        x: f64::from(position.x),
        y: f64::from(position.y),
        width: f64::from(size.width),
        height: f64::from(size.height),
    };
    monitors
        .iter()
        .any(|monitor| overlaps(panel, screen(monitor)))
}

fn overlaps(a: Area, b: Area) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// A screen's whole extent, in physical pixels.
fn screen(monitor: &Monitor) -> Area {
    let (origin, extent) = (monitor.position(), monitor.size());
    Area {
        x: f64::from(origin.x),
        y: f64::from(origin.y),
        width: f64::from(extent.width),
        height: f64::from(extent.height),
    }
}

/// A size in one screen's pixels, in another's: the panel may be about to move.
fn rescale((width, height): (f64, f64), from: f64, to: f64) -> (f64, f64) {
    (width / from * to, height / from * to)
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Area {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Below an icon in the menu bar or a top taskbar, above it in a bottom
/// taskbar, beside it in a side one, and always inside the work area.
fn spot(icon: Area, work: Area, (width, height): (f64, f64), gap: f64) -> (f64, f64) {
    let (center_x, center_y) = (icon.x + icon.width / 2.0, icon.y + icon.height / 2.0);
    let (x, y) = if center_x < work.x {
        (icon.x + icon.width + gap, center_y - height / 2.0)
    } else if center_x > work.x + work.width {
        (icon.x - width - gap, center_y - height / 2.0)
    } else if center_y < work.y + work.height / 2.0 {
        (center_x - width / 2.0, icon.y + icon.height + gap)
    } else {
        (center_x - width / 2.0, icon.y - height - gap)
    };
    let (left, top) = (work.x + gap, work.y + gap);
    (
        x.clamp(left, (work.x + work.width - width - gap).max(left)),
        y.clamp(top, (work.y + work.height - height - gap).max(top)),
    )
}

#[cfg(test)]
mod tests {
    use super::{Area, holds, rescale, spot};

    const PANEL: (f64, f64) = (340.0, 400.0);
    const GAP: f64 = 6.0;

    fn area(x: f64, y: f64, width: f64, height: f64) -> Area {
        Area {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn counts_a_window_as_on_a_screen_while_any_of_it_shows_there() {
        let screen = area(0.0, 0.0, 2560.0, 1440.0);
        assert!(super::overlaps(area(100.0, 100.0, 680.0, 800.0), screen));
        assert!(super::overlaps(area(2500.0, -700.0, 680.0, 800.0), screen));
        assert!(!super::overlaps(area(2560.0, 0.0, 680.0, 800.0), screen));
        assert!(!super::overlaps(area(-680.0, 0.0, 680.0, 800.0), screen));
        assert!(!super::overlaps(area(0.0, 1440.0, 680.0, 800.0), screen));
    }

    #[test]
    fn finds_the_icon_on_a_retina_screen_by_its_pixels() {
        let retina = area(0.0, 0.0, 3024.0, 1964.0);
        let beside = area(3024.0, 0.0, 1920.0, 1080.0);

        assert!(holds(retina, 2400.0, 0.0));
        assert!(!holds(beside, 2400.0, 0.0));
        assert!(holds(beside, 3024.0, 10.0));
    }

    #[test]
    fn sizes_the_panel_for_the_screen_it_moves_to() {
        assert_eq!(rescale((340.0, 400.0), 1.0, 2.0), (680.0, 800.0));
        assert_eq!(rescale((680.0, 800.0), 2.0, 1.0), (340.0, 400.0));
    }

    #[test]
    fn hangs_below_an_icon_in_the_menu_bar() {
        let work = area(0.0, 25.0, 1512.0, 957.0);
        let icon = area(1200.0, 0.0, 30.0, 24.0);

        assert_eq!(spot(icon, work, PANEL, GAP), (1045.0, 31.0));
    }

    #[test]
    fn sits_above_an_icon_in_a_bottom_taskbar() {
        let work = area(0.0, 0.0, 1920.0, 1032.0);
        let icon = area(1700.0, 1040.0, 24.0, 40.0);

        assert_eq!(spot(icon, work, PANEL, GAP), (1542.0, 626.0));
    }

    #[test]
    fn hangs_below_an_icon_in_a_top_taskbar() {
        let work = area(0.0, 48.0, 1920.0, 1032.0);
        let icon = area(1700.0, 4.0, 24.0, 40.0);

        assert_eq!(spot(icon, work, PANEL, GAP), (1542.0, 54.0));
    }

    #[test]
    fn sits_beside_an_icon_in_a_side_taskbar() {
        let left = area(60.0, 0.0, 1860.0, 1080.0);
        assert_eq!(
            spot(area(10.0, 900.0, 40.0, 24.0), left, PANEL, GAP),
            (66.0, 674.0)
        );

        let right = area(0.0, 0.0, 1860.0, 1080.0);
        assert_eq!(
            spot(area(1870.0, 900.0, 40.0, 24.0), right, PANEL, GAP),
            (1514.0, 674.0)
        );
    }

    #[test]
    fn stays_inside_the_screen_near_its_edge() {
        let work = area(0.0, 0.0, 1920.0, 1032.0);
        let icon = area(1900.0, 1040.0, 20.0, 40.0);

        assert_eq!(spot(icon, work, PANEL, GAP), (1574.0, 626.0));
    }

    #[test]
    fn keeps_to_the_top_left_of_a_screen_too_small_for_it() {
        let work = area(0.0, 25.0, 300.0, 200.0);
        let icon = area(150.0, 0.0, 24.0, 24.0);

        assert_eq!(spot(icon, work, PANEL, GAP), (6.0, 31.0));
    }
}
