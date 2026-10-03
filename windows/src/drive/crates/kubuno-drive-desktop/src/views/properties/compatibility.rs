//! "Compatibility" tab of the properties sheet — mirror of
//! `Views/Properties/CompatibilityPage.xaml` (the DRAW). The view-model
//! (`CompatibilityModel`/`gather`, registry read + labels,
//! `CompatibilityViewModel` + `WindowsCompatibilityService`) now lives in
//! `crate::view_models::properties::compatibility_view_model` and is
//! re-exported below to preserve the API
//! `properties::compatibility::CompatibilityModel`.
//!
//! The tab is only offered for an executable (or a shortcut whose target is
//! one). READ-ONLY: the combos/switches are shown read-only (state read from
//! the registry) — writing is a TODO.

use crate::styles::theme::Theme;
use crate::ui::{Painter, Rect};

// Re-export of the view-model (mirror of `CompatibilityViewModel`), moved to
// `view_models/properties/compatibility_view_model.rs`. Preserves the
// `crate::views::properties::compatibility::CompatibilityModel` path used by
// `window.rs`.
pub use crate::view_models::properties::compatibility_view_model::CompatibilityModel;

/// Draws the tab in `content`, offset by `scroll`. Returns the total height.
/// Clipped by the caller. Each option is a `SettingsCard` (header on the
/// left, control on the right) in READ-ONLY mode.
pub fn draw(p: &Painter, theme: &Theme, content: &Rect, model: &CompatibilityModel, scroll: f32) -> f32 {
    let pad = 12.0;
    let left = content.left + pad;
    let right = content.right - pad;
    let top0 = content.top + pad - scroll;
    let mut y = top0;

    if !model.loaded {
        return 0.0;
    }

    // Clickable "Run compatibility troubleshooter" card (`RunTroubleshooter`
    // Command) — shown for fidelity, action = TODO.
    y = draw_action_card(p, theme, left, right, y, kubuno_drive_desktop_localization::tr("CompatibilityRunTroubleshooter")) + 4.0;

    // Combos (value shown read-only).
    y = draw_combo_card(p, theme, left, right, y, kubuno_drive_desktop_localization::tr("CompatibilityMode"), &model.compat_mode) + 4.0;
    y = draw_combo_card(p, theme, left, right, y, kubuno_drive_desktop_localization::tr("CompatibilityReducedColorMode"), &model.reduced_color) + 4.0;

    // Switches.
    y = draw_toggle_card(p, theme, left, right, y, kubuno_drive_desktop_localization::tr("CompatibilityRunIn640x480Resolution"), model.run_640) + 4.0;
    y = draw_toggle_card(p, theme, left, right, y, kubuno_drive_desktop_localization::tr("CompatibilityDisableFullscreenOptimizations"), model.disable_fullscreen) + 4.0;
    y = draw_toggle_card(p, theme, left, right, y, kubuno_drive_desktop_localization::tr("RunAsAdministrator"), model.run_as_admin) + 4.0;
    y = draw_toggle_card(p, theme, left, right, y, kubuno_drive_desktop_localization::tr("CompatibilityRegisterThisProgramForRestart"), model.register_restart) + 4.0;

    // DPI options.
    y = draw_combo_card(p, theme, left, right, y, kubuno_drive_desktop_localization::tr("CompatibilityUseDPISettings"), &model.high_dpi_option) + 4.0;
    y = draw_combo_card(p, theme, left, right, y, kubuno_drive_desktop_localization::tr("CompatibilityOverrideHighDPIBehavior"), &model.high_dpi_override) + 4.0;

    // TODO (writing): `SetCompatibilityOptionsForPath` → the original writes
    // `HKCU:\…\Layers` via an ELEVATED PowerShell command (`New-ItemProperty` /
    // `Remove-ItemProperty`, `Elevated | Hidden` options). Not ported here.
    y - top0 + pad
}

/// `SettingsCard` with header + "combo" control showing `value` (read-only).
fn draw_combo_card(p: &Painter, theme: &Theme, left: f32, right: f32, top: f32, header: &str, value: &str) -> f32 {
    let f = &p.renderer.formats;
    let card = card_rect(left, right, top);
    fill_card(p, theme, &card);
    p.text(
        header,
        &Rect::new(card.left + 16.0, card.top, card.left + (card.right - card.left) * 0.5, card.bottom),
        &f.body,
        &theme.text_primary,
        false,
    );
    // Combo box on the right (bordered, with down chevron).
    let combo = Rect::new(card.right - 200.0, card.top + 8.0, card.right - 16.0, card.bottom - 8.0);
    p.fill_rounded(&combo, 4.0, &theme.toolbar_background);
    p.stroke_rounded(&combo, 4.0, &theme.card_stroke);
    p.text_ellipsis(
        value,
        &Rect::new(combo.left + 10.0, combo.top, combo.right - 28.0, combo.bottom),
        &f.body,
        &theme.text_primary,
    );
    p.text(
        "\u{E70D}",
        &Rect::new(combo.right - 26.0, combo.top, combo.right - 6.0, combo.bottom),
        &f.icon_small,
        &theme.text_secondary,
        true,
    );
    card.bottom
}

/// `SettingsCard` with header + `ToggleSwitch` (read-only).
fn draw_toggle_card(p: &Painter, theme: &Theme, left: f32, right: f32, top: f32, header: &str, on: bool) -> f32 {
    let f = &p.renderer.formats;
    let card = card_rect(left, right, top);
    fill_card(p, theme, &card);
    p.text(
        header,
        &Rect::new(card.left + 16.0, card.top, card.right - 80.0, card.bottom),
        &f.body,
        &theme.text_primary,
        false,
    );
    // Switch (40x20 track, 16 knob). Accent color if on.
    let cy = (card.top + card.bottom) / 2.0;
    let track = Rect::new(card.right - 56.0, cy - 10.0, card.right - 16.0, cy + 10.0);
    let fill = if on { theme.accent } else { theme.toolbar_background };
    p.fill_rounded(&track, 10.0, &fill);
    if !on {
        p.stroke_rounded(&track, 10.0, &theme.card_stroke);
    }
    let knob = if on {
        Rect::new(track.right - 18.0, track.top + 2.0, track.right - 2.0, track.bottom - 2.0)
    } else {
        Rect::new(track.left + 2.0, track.top + 2.0, track.left + 18.0, track.bottom - 2.0)
    };
    let knob_color = if on { theme.accent_foreground } else { theme.text_secondary };
    p.fill_rounded(&knob, 8.0, &knob_color);
    card.bottom
}

/// `SettingsCard IsClickEnabled` with header + action glyph (action = TODO).
fn draw_action_card(p: &Painter, theme: &Theme, left: f32, right: f32, top: f32, header: &str) -> f32 {
    let f = &p.renderer.formats;
    let card = card_rect(left, right, top);
    fill_card(p, theme, &card);
    p.text(
        header,
        &Rect::new(card.left + 16.0, card.top, card.right - 48.0, card.bottom),
        &f.body,
        &theme.text_primary,
        false,
    );
    p.text(
        "\u{E8A7}",
        &Rect::new(card.right - 40.0, card.top, card.right - 12.0, card.bottom),
        &f.icon_small,
        &theme.text_secondary,
        true,
    );
    card.bottom
}

fn card_rect(left: f32, right: f32, top: f32) -> Rect {
    Rect::new(left, top, right, top + 56.0)
}

fn fill_card(p: &Painter, theme: &Theme, card: &Rect) {
    p.fill_rounded(card, 4.0, &theme.card_background);
    p.stroke_rounded(card, 4.0, &theme.card_stroke);
}
