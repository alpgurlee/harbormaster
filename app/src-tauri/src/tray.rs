use std::path::PathBuf;
use std::sync::OnceLock;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont, point};
use tauri::image::Image;
use tauri::menu::{Menu, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};

use crate::panel;
use crate::settings::{Language, Store};

pub(crate) const ID: &str = "harbormaster";

/// A click opens the panel; the icon's menu only quits.
pub(crate) fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = menu(app, Language::System)?;
    TrayIconBuilder::with_id(ID)
        .icon(image(face(0)))
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

/// What the icon shows: how many dev servers run, in a colour that stands
/// out where the icon sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Face {
    count: usize,
    ink: [u8; 3],
}

pub(crate) fn face(count: usize) -> Face {
    Face { count, ink: ink() }
}

/// Black, for macOS to tint as a template image.
#[cfg(target_os = "macos")]
fn ink() -> [u8; 3] {
    [0; 3]
}

/// Windows shows the icon as drawn: black on a light taskbar, white on a
/// dark one, which is also how a taskbar that doesn't say starts out.
#[cfg(windows)]
fn ink() -> [u8; 3] {
    if harbormaster_platform::light_taskbar() == Some(true) {
        [0; 3]
    } else {
        [255; 3]
    }
}

pub(crate) fn show(app: &AppHandle, face: Face) {
    if let Some(tray) = app.tray_by_id(ID) {
        let _ = tray.set_icon_with_as_template(Some(image(face)), true);
    }
}

fn image(face: Face) -> Image<'static> {
    let (rgba, width, height) = draw(face, font());
    Image::new_owned(rgba, width, height)
}

/// How large the icon is drawn, and the plate in it.
#[derive(Debug, Clone, Copy)]
struct Shape {
    width: u32,
    height: u32,
    plate_height: f32,
    stroke: f32,
    radius: f32,
    text_size: f32,
}

/// Twice the menu bar's point size, so it stays sharp on Retina, and wider
/// for two characters.
#[cfg(target_os = "macos")]
fn shape(characters: usize) -> Shape {
    Shape {
        width: if characters > 1 { 46 } else { 34 },
        height: 36,
        plate_height: 26.0,
        stroke: 2.6,
        radius: 7.0,
        text_size: 19.0,
    }
}

/// Windows shows a tray icon square, 16 pixels a side at 100 % and 32 at
/// 200 %, so it is drawn at 32, with smaller text for two characters.
#[cfg(windows)]
fn shape(characters: usize) -> Shape {
    Shape {
        width: 32,
        height: 32,
        plate_height: 26.0,
        stroke: 2.4,
        radius: 6.0,
        text_size: if characters > 1 { 15.0 } else { 19.0 },
    }
}

/// The tray icon: a port plate like those in the panel, holding the number
/// of dev servers in `font`, or empty and dimmed when there are none.
/// Without a font only the plate shows.
fn draw(face: Face, font: Option<&FontVec>) -> (Vec<u8>, u32, u32) {
    let label = match face.count {
        0 => None,
        1..=9 => Some(face.count.to_string()),
        _ => Some("9+".to_string()),
    };
    let shape = shape(label.as_ref().map_or(0, String::len));
    let (width, height) = (shape.width, shape.height);
    let mut alpha = vec![0f32; (width * height) as usize];
    let (center_x, center_y) = (width as f32 / 2.0, height as f32 / 2.0);
    let (half_width, half_height) = (center_x - shape.stroke, shape.plate_height / 2.0);
    for y in 0..height {
        for x in 0..width {
            let distance = rounded_rect(
                x as f32 + 0.5 - center_x,
                y as f32 + 0.5 - center_y,
                half_width,
                half_height,
                shape.radius,
            );
            alpha[(y * width + x) as usize] =
                (shape.stroke / 2.0 + 0.5 - distance.abs()).clamp(0.0, 1.0);
        }
    }
    if let (Some(label), Some(font)) = (label, font) {
        draw_text(&mut alpha, shape, font, &label);
    }
    let dim = if face.count == 0 { 0.45 } else { 1.0 };
    let [red, green, blue] = face.ink;
    let rgba = alpha
        .iter()
        .flat_map(|&coverage| [red, green, blue, (coverage * dim * 255.0).round() as u8])
        .collect();
    (rgba, width, height)
}

/// Signed distance from a point to a rounded rectangle centred on the origin.
fn rounded_rect(x: f32, y: f32, half_width: f32, half_height: f32, radius: f32) -> f32 {
    let qx = x.abs() - (half_width - radius);
    let qy = y.abs() - (half_height - radius);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    outside + qx.max(qy).min(0.0) - radius
}

fn draw_text(alpha: &mut [f32], shape: Shape, font: &FontVec, label: &str) {
    let (width, height) = (shape.width, shape.height);
    let scale = PxScale::from(shape.text_size);
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
    let shift_y = (height as f32 - bounds.height()) / 2.0 - bounds.min.y;
    for glyph in glyphs {
        let glyph_bounds = glyph.px_bounds();
        glyph.draw(|x, y, coverage| {
            let px = (glyph_bounds.min.x + shift_x).round() as i64 + i64::from(x);
            let py = (glyph_bounds.min.y + shift_y).round() as i64 + i64::from(y);
            if px >= 0 && py >= 0 && px < i64::from(width) && py < i64::from(height) {
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
        let mut font = font_files()
            .iter()
            .find_map(|path| std::fs::read(path).ok())
            .and_then(|bytes| FontVec::try_from_vec(bytes).ok())?;
        font.set_variation(b"wght", 700.0);
        Some(font)
    })
    .as_ref()
}

#[cfg(target_os = "macos")]
fn font_files() -> Vec<PathBuf> {
    ["SFNSMono.ttf", "SFNS.ttf"]
        .map(|name| PathBuf::from("/System/Library/Fonts").join(name))
        .to_vec()
}

#[cfg(windows)]
fn font_files() -> Vec<PathBuf> {
    let windows =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    ["consolab.ttf", "segoeuib.ttf"]
        .map(|name| windows.join("Fonts").join(name))
        .to_vec()
}

#[cfg(test)]
mod tests {
    use ab_glyph::FontVec;

    use super::{Face, draw, font, quit_text};
    use crate::settings::Language;

    fn black(count: usize) -> Face {
        Face { count, ink: [0; 3] }
    }

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
        let icon = draw(black(0), Some(system_font()));

        assert!(max_alpha(&icon) > 0, "the plate should show");
        assert!(max_alpha(&icon) < 128, "and be dimmed");
        let center = (icon.1 / 2, icon.2 / 2);
        assert_eq!(alpha(&icon, center.0, center.1), 0, "with nothing inside");
        assert_eq!(alpha(&icon, 0, 0), 0);
    }

    #[test]
    fn writes_the_count_inside_the_plate() {
        let five = draw(black(5), Some(system_font()));

        assert_eq!(max_alpha(&five), 255);
        let center_rows = (five.2 / 2 - 4)..(five.2 / 2 + 4);
        let inked = center_rows
            .flat_map(|y| (0..five.1).map(move |x| (x, y)))
            .filter(|&(x, y)| x > 8 && x < five.1 - 8 && alpha(&five, x, y) > 128)
            .count();
        assert!(inked > 10, "the digit should be drawn in the middle");
        assert_ne!(five.0, draw(black(6), Some(system_font())).0);
    }

    #[test]
    fn shows_a_bright_empty_plate_for_running_servers_without_a_font() {
        let five = draw(black(5), None);

        assert_eq!(max_alpha(&five), 255, "the plate should show servers run");
        let center = (five.1 / 2, five.2 / 2);
        assert_eq!(alpha(&five, center.0, center.1), 0, "with nothing inside");
        assert_eq!(five.1, draw(black(5), Some(system_font())).1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn widens_the_plate_for_ten_or_more() {
        let font = Some(system_font());
        assert!(draw(black(12), font).1 > draw(black(5), font).1);
        assert_eq!(
            draw(black(12), font).0,
            draw(black(40), font).0,
            "both read 9+"
        );
    }

    #[cfg(windows)]
    #[test]
    fn keeps_the_icon_square_for_ten_or_more() {
        let font = Some(system_font());
        let twelve = draw(black(12), font);
        assert_eq!((twelve.1, twelve.2), (32, 32));
        assert_eq!((twelve.1, twelve.2), {
            let five = draw(black(5), font);
            (five.1, five.2)
        });
        assert_eq!(twelve.0, draw(black(40), font).0, "both read 9+");
    }

    #[test]
    fn draws_in_the_ink_it_is_given() {
        let white = Face {
            count: 5,
            ink: [255; 3],
        };

        let icon = draw(white, Some(system_font()));

        let pixels = icon.0.as_chunks::<4>().0;
        assert!(pixels.iter().all(|pixel| pixel[..3] == [255; 3]));
        assert!(pixels.iter().any(|pixel| pixel[3] == 255));
    }
}
