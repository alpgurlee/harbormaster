use std::sync::OnceLock;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont, point};
use tauri::image::Image;
use tauri::menu::{Menu, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};

use crate::panel;
use crate::settings::{Language, Store};

pub(crate) const ID: &str = "harbormaster";

/// Drawn at twice the menu bar's point size, so it stays sharp on Retina.
const HEIGHT: u32 = 36;
const PLATE_HEIGHT: f32 = 26.0;
const STROKE: f32 = 2.6;
const RADIUS: f32 = 7.0;
const TEXT_SIZE: f32 = 19.0;

/// A click opens the panel; the icon's menu only quits.
pub(crate) fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = menu(app, Language::System)?;
    TrayIconBuilder::with_id(ID)
        .icon(image(0))
        .icon_as_template(true)
        .tooltip("Harbormaster")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                panel::toggle(tray.app_handle(), rect);
            }
        })
        .build(app)?;
    Ok(())
}

/// Keeps the icon's menu in the language the settings choose.
pub(crate) fn speak(app: &AppHandle, store: &Store) {
    let app = app.clone();
    store.subscribe("tray", move |settings| {
        let language = settings.language;
        let handle = app.clone();
        // Told under the settings' lock: the menu changes on the main thread.
        let _ = app.run_on_main_thread(move || {
            if let (Some(tray), Ok(menu)) = (handle.tray_by_id(ID), menu(&handle, language)) {
                let _ = tray.set_menu(Some(menu));
            }
        });
        true
    });
}

fn menu(app: &AppHandle, language: Language) -> tauri::Result<Menu<Wry>> {
    let system = harbormaster_platform::preferred_language();
    let quit = PredefinedMenuItem::quit(app, Some(quit_text(language, system.as_deref())))?;
    Menu::with_items(app, &[&quit])
}

/// The quit item in the language the windows speak: the one chosen, or else
/// the user's first preferred `system` language.
fn quit_text(language: Language, system: Option<&str>) -> &'static str {
    let turkish = match language {
        Language::Tr => true,
        Language::En => false,
        Language::System => system.is_some_and(|tag| tag.to_ascii_lowercase().starts_with("tr")),
    };
    if turkish {
        "Harbormaster'dan Çık"
    } else {
        "Quit Harbormaster"
    }
}

pub(crate) fn show_count(app: &AppHandle, count: usize) {
    if let Some(tray) = app.tray_by_id(ID) {
        let _ = tray.set_icon_with_as_template(Some(image(count)), true);
    }
}

fn image(count: usize) -> Image<'static> {
    let (rgba, width, height) = draw(count, font());
    Image::new_owned(rgba, width, height)
}

/// The menu bar icon: a port plate like those in the panel, holding the
/// number of dev servers in `font`, or empty and dimmed when there are none.
/// Without a font only the plate shows. Black with alpha, for macOS to tint
/// as a template image.
fn draw(count: usize, font: Option<&FontVec>) -> (Vec<u8>, u32, u32) {
    let label = match count {
        0 => None,
        1..=9 => Some(count.to_string()),
        _ => Some("9+".to_string()),
    };
    let width: u32 = if label.as_ref().is_some_and(|label| label.len() > 1) {
        46
    } else {
        34
    };
    let mut alpha = vec![0f32; (width * HEIGHT) as usize];
    let (center_x, center_y) = (width as f32 / 2.0, HEIGHT as f32 / 2.0);
    let (half_width, half_height) = (center_x - STROKE, PLATE_HEIGHT / 2.0);
    for y in 0..HEIGHT {
        for x in 0..width {
            let distance = rounded_rect(
                x as f32 + 0.5 - center_x,
                y as f32 + 0.5 - center_y,
                half_width,
                half_height,
            );
            alpha[(y * width + x) as usize] = (STROKE / 2.0 + 0.5 - distance.abs()).clamp(0.0, 1.0);
        }
    }
    if let (Some(label), Some(font)) = (label, font) {
        draw_text(&mut alpha, width, font, &label);
    }
    let dim = if count == 0 { 0.45 } else { 1.0 };
    let rgba = alpha
        .iter()
        .flat_map(|&coverage| [0, 0, 0, (coverage * dim * 255.0).round() as u8])
        .collect();
    (rgba, width, HEIGHT)
}

/// Signed distance from a point to a rounded rectangle centred on the origin.
fn rounded_rect(x: f32, y: f32, half_width: f32, half_height: f32) -> f32 {
    let qx = x.abs() - (half_width - RADIUS);
    let qy = y.abs() - (half_height - RADIUS);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    outside + qx.max(qy).min(0.0) - RADIUS
}

fn draw_text(alpha: &mut [f32], width: u32, font: &FontVec, label: &str) {
    let scale = PxScale::from(TEXT_SIZE);
    let scaled = font.as_scaled(scale);
    let mut caret = 0.0;
    let glyphs: Vec<_> = label
        .chars()
        .map(|c| {
            let mut glyph = scaled.scaled_glyph(c);
            glyph.position = point(caret, 0.0);
            caret += scaled.h_advance(glyph.id);
            glyph
        })
        .filter_map(|glyph| font.outline_glyph(glyph))
        .collect();
    let Some(bounds) = glyphs
        .iter()
        .map(|glyph| glyph.px_bounds())
        .reduce(|a, b| ab_glyph::Rect {
            min: point(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
            max: point(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
        })
    else {
        return;
    };
    // Centre the ink, not the advance box: digits sit evenly in the plate.
    let shift_x = (width as f32 - bounds.width()) / 2.0 - bounds.min.x;
    let shift_y = (HEIGHT as f32 - bounds.height()) / 2.0 - bounds.min.y;
    for glyph in glyphs {
        let glyph_bounds = glyph.px_bounds();
        glyph.draw(|x, y, coverage| {
            let px = (glyph_bounds.min.x + shift_x).round() as i64 + i64::from(x);
            let py = (glyph_bounds.min.y + shift_y).round() as i64 + i64::from(y);
            if px >= 0 && py >= 0 && px < i64::from(width) && py < i64::from(HEIGHT) {
                let slot = &mut alpha[(py as u32 * width + px as u32) as usize];
                *slot = slot.max(coverage);
            }
        });
    }
}

/// A bold system monospace face, read from the OS rather than shipped.
fn font() -> Option<&'static FontVec> {
    static FONT: OnceLock<Option<FontVec>> = OnceLock::new();
    FONT.get_or_init(|| {
        let mut font = FONT_FILES
            .iter()
            .find_map(|path| std::fs::read(path).ok())
            .and_then(|bytes| FontVec::try_from_vec(bytes).ok())?;
        font.set_variation(b"wght", 700.0);
        Some(font)
    })
    .as_ref()
}

#[cfg(target_os = "macos")]
const FONT_FILES: &[&str] = &[
    "/System/Library/Fonts/SFNSMono.ttf",
    "/System/Library/Fonts/SFNS.ttf",
];
#[cfg(windows)]
const FONT_FILES: &[&str] = &[
    "C:\\Windows\\Fonts\\consolab.ttf",
    "C:\\Windows\\Fonts\\segoeuib.ttf",
];

#[cfg(test)]
mod tests {
    use ab_glyph::FontVec;

    use super::{draw, font, quit_text};
    use crate::settings::Language;

    #[test]
    fn says_quit_in_the_language_the_windows_speak() {
        assert_eq!(quit_text(Language::En, Some("tr-TR")), "Quit Harbormaster");
        assert_eq!(
            quit_text(Language::Tr, Some("en-US")),
            "Harbormaster'dan Çık"
        );
        assert_eq!(
            quit_text(Language::System, Some("tr-TR")),
            "Harbormaster'dan Çık"
        );
        assert_eq!(
            quit_text(Language::System, Some("TR")),
            "Harbormaster'dan Çık"
        );
        assert_eq!(
            quit_text(Language::System, Some("en-TR")),
            "Quit Harbormaster"
        );
        assert_eq!(quit_text(Language::System, None), "Quit Harbormaster");
    }

    fn alpha(icon: &(Vec<u8>, u32, u32), x: u32, y: u32) -> u8 {
        icon.0[((y * icon.1 + x) * 4 + 3) as usize]
    }

    fn max_alpha(icon: &(Vec<u8>, u32, u32)) -> u8 {
        icon.0
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[3])
            .max()
            .unwrap_or(0)
    }

    /// The face the tray reads from the system, which a supported OS has.
    fn system_font() -> &'static FontVec {
        font().expect("the system should have a face for the tray's digits")
    }

    #[test]
    fn shows_a_dim_empty_plate_when_nothing_runs() {
        let icon = draw(0, Some(system_font()));

        assert!(max_alpha(&icon) > 0, "the plate should show");
        assert!(max_alpha(&icon) < 128, "and be dimmed");
        let center = (icon.1 / 2, icon.2 / 2);
        assert_eq!(alpha(&icon, center.0, center.1), 0, "with nothing inside");
        assert_eq!(alpha(&icon, 0, 0), 0);
    }

    #[test]
    fn writes_the_count_inside_the_plate() {
        let five = draw(5, Some(system_font()));

        assert_eq!(max_alpha(&five), 255);
        let center_rows = (five.2 / 2 - 4)..(five.2 / 2 + 4);
        let inked = center_rows
            .flat_map(|y| (0..five.1).map(move |x| (x, y)))
            .filter(|&(x, y)| x > 8 && x < five.1 - 8 && alpha(&five, x, y) > 128)
            .count();
        assert!(inked > 10, "the digit should be drawn in the middle");
        assert_ne!(five.0, draw(6, Some(system_font())).0);
    }

    #[test]
    fn shows_a_bright_empty_plate_for_running_servers_without_a_font() {
        let five = draw(5, None);

        assert_eq!(max_alpha(&five), 255, "the plate should show servers run");
        let center = (five.1 / 2, five.2 / 2);
        assert_eq!(alpha(&five, center.0, center.1), 0, "with nothing inside");
        assert_eq!(five.1, draw(5, Some(system_font())).1);
    }

    #[test]
    fn widens_the_plate_for_ten_or_more() {
        let font = Some(system_font());
        assert!(draw(12, font).1 > draw(5, font).1);
        assert_eq!(draw(12, font).0, draw(40, font).0, "both read 9+");
    }
}
