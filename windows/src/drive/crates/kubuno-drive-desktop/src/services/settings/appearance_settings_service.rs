//! AppearanceSettingsService (mirror of `AppearanceSettingsService.cs`).
//!
//! Helpers derived from appearance settings (theme background color,
//! custom font); the state itself remains carried by the single
//! `AppSettings` store (see `mod.rs`).

use super::{get, STANDARD_FONT};

/// Parses "#AARRGGBB" / "#RRGGBB" into non-premultiplied float RGBA.
pub fn parse_color(hex: &str) -> Option<windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F> {
    let hex = hex.strip_prefix('#')?;
    let (a, rgb) = match hex.len() {
        8 => (u8::from_str_radix(&hex[0..2], 16).ok()?, &hex[2..]),
        6 => (0xFF, hex),
        _ => return None,
    };
    let r = u8::from_str_radix(&rgb[0..2], 16).ok()?;
    let g = u8::from_str_radix(&rgb[2..4], 16).ok()?;
    let b = u8::from_str_radix(&rgb[4..6], 16).ok()?;
    Some(windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    })
}

/// The custom font to inject into the `Renderer` (`AppThemeFontFamily`), or
/// `None` if the user keeps the standard font. Centralizes the rule so
/// that `Renderer` (in `kubuno-drive-desktop-app-controls`) stays independent from settings.
pub fn app_theme_font_override() -> Option<String> {
    let f = get().app_theme_font_family;
    (f != STANDARD_FONT && !f.trim().is_empty()).then_some(f)
}
