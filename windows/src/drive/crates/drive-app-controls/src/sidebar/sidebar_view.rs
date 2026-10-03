//! `SidebarView` (mirrors `Files.App.Controls/Sidebar/SidebarView.xaml.cs` +
//! `SidebarView.Properties.cs` + `SidebarView.xaml`).
//!
//! The WinUI `UserControl` (VSM, resizer manipulation, event wiring) is
//! IGNORED machinery. This file houses the PURE core: pane width thresholds,
//! mode calculation (`UpdateDisplayModeForPaneWidth`), and the port's
//! rendering snapshot (`SidebarView` + `draw`, invented by the port for lack
//! of an equivalent `.cs` — WinUI renders via the template).

use super::sidebar_display_mode::SidebarMode;
use super::sidebar_item::{RowIcon, SidebarRowView};

/// `Constants.UI.MinimumSidebarWidth` / `MaximumSidebarWidth`.
pub const SIDEBAR_MIN_WIDTH: f32 = 180.0;
pub const SIDEBAR_MAX_WIDTH: f32 = 500.0;
/// `SidebarView.COMPACT_MAX_WIDTH`: below this, the pane switches to Compact.
pub const SIDEBAR_COMPACT_MAX_WIDTH: f32 = 200.0;
/// `SidebarCompactOpenPaneLength`: the icon rail of Compact mode.
pub const SIDEBAR_COMPACT_WIDTH: f32 = 56.0;
/// `SidebarOpenPaneLength`: the floating pane's width in Minimal mode.
pub const SIDEBAR_OPEN_PANE_LENGTH: f32 = 300.0;
/// `MainPage` `SidebarStates` `MinWindowWidth="641"`: below this threshold,
/// the window forces Minimal mode (overlay pane + hamburger).
pub const SIDEBAR_MINIMAL_MAX_WINDOW: f32 = 641.0;
/// `SidebarResizer`: `MinWidth="4"`.
pub const RESIZER_WIDTH: f32 = 4.0;
/// Duration of mode transitions (`KeyTime="0:0:0.35"`).
pub const SIDEBAR_ANIM_MS: f32 = 350.0;

/// Is the window too narrow for a docked pane? (`MinWindowWidth=641`).
pub fn window_forces_minimal(window_width: f32) -> bool {
    window_width < SIDEBAR_MINIMAL_MAX_WINDOW
}

/// The current mode. PURE: the Compact/Expanded preference
/// (`SidebarViewModel.SidebarDisplayMode`) is passed by the caller,
/// otherwise the window width forces Minimal (mirrors `SidebarStates`).
pub fn sidebar_mode(window_width: f32, compact_preference: bool) -> SidebarMode {
    if window_forces_minimal(window_width) {
        SidebarMode::Minimal
    } else if compact_preference {
        SidebarMode::Compact
    } else {
        SidebarMode::Expanded
    }
}

/// The mode resulting from dragging the resizer to `pane_width`
/// (`UpdateDisplayModeForPaneWidth`): below 200 → Compact, otherwise Expanded.
pub fn display_mode_for_pane_width(pane_width: f32) -> SidebarMode {
    if pane_width < SIDEBAR_COMPACT_MAX_WIDTH {
        SidebarMode::Compact
    } else {
        SidebarMode::Expanded
    }
}

/// The width the pane occupies in a given mode. `stored_width` is the
/// Expanded preference (`SidebarWidth`), clamped to `[min, max]`.
pub fn sidebar_pane_width(mode: SidebarMode, stored_width: f32) -> f32 {
    match mode {
        SidebarMode::Minimal => SIDEBAR_OPEN_PANE_LENGTH,
        SidebarMode::Compact => SIDEBAR_COMPACT_WIDTH,
        SidebarMode::Expanded => stored_width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH),
    }
}

// ── Control rendering (migrated from `Painter::draw_sidebar`) ────────────────

use super::sidebar_item::{ROW_ICON_SIZE, ROW_ICON_TEXT_GAP, ROW_TEXT_RIGHT_MARGIN};
use crate::geometry::Rect;
use crate::themes::shape::{pill, radius};
use crate::Canvas;

// Fallback glyphs (Segoe Fluent Icons) — a control carries its own glyphs.
const GLYPH_CHEVRON_DOWN: &str = "\u{E70D}";
const GLYPH_CHEVRON_RIGHT: &str = "\u{E76C}";

/// Drawing snapshot of the sidebar, in PRIMITIVES. The scrollbar is NOT
/// included: the app draws it AFTER the call (depends on the `Scrollbar` type).
pub struct SidebarView<'a> {
    /// Pane clipping (replaces the raw `PushAxisAlignedClip`).
    pub clip: Rect,
    /// Minimal mode's floating acrylic card (already filtered by the builder).
    pub overlay: Option<Rect>,
    /// Compact rail: centered icons, no labels or chevrons.
    pub compact: bool,
    /// Horizontal content offset (scrolling); 0 in Compact.
    pub dx: f32,
    pub rows: Vec<SidebarRowView<'a>>,
}

/// Draws the sidebar through a [`Canvas`] — direct port of `draw_sidebar`.
pub fn draw(c: &dyn Canvas, v: &SidebarView) {
    let t = c.theme();
    let f = c.formats();

    // Floating card (Minimal): painted BEFORE the clip, rows drawn on top.
    if let Some(o) = v.overlay {
        c.draw_card_shadow(&o, radius::XL);
        c.fill_rounded(&o, radius::XL, &t.flyout_background);
        c.stroke_rounded(&o, radius::XL, &t.card_stroke);
    }

    c.push_clip(&v.clip);

    for row in &v.rows {
        // Web nav row: a full pill (`rounded-full`) — `SIDEBAR_ROW_CORNER`,
        // recomputed from the row's own height so the Compact rail (40x36)
        // still reads as a centered stadium.
        let corner = pill(row.rect.bottom - row.rect.top);
        // The ACTIVE row wears the `accent_light` pastille and keeps it on
        // hover (the two fills never stack). Section headers are inert.
        if row.selected {
            c.fill_rounded(&row.rect, corner, &t.accent_light);
        } else if row.hot && !row.is_section {
            c.fill_rounded(&row.rect, corner, &t.control_fill_hover);
        }

        let icon_left = if v.compact {
            row.rect.left + ((row.rect.right - row.rect.left) - ROW_ICON_SIZE) / 2.0
        } else {
            row.rect.left + row.indent + v.dx
        };
        let icon_rect = Rect::new(icon_left, row.rect.top, icon_left + ROW_ICON_SIZE, row.rect.bottom);

        match &row.icon {
            RowIcon::Bitmap(b) => c.image(b, &icon_rect, 16.0),
            // A `Vector` icon arrives with its final colour — either DATA
            // colour (a tag's own colour) or the active/rest state already
            // resolved by the caller — so it is never re-tinted here.
            RowIcon::Vector { name, color } => c.vector_icon(name, &icon_rect, 16.0, color),
            RowIcon::Glyph { text, color } => {
                let color = if row.selected { &t.accent } else { color };
                c.text(text, &icon_rect, &f.icon_small, color, true)
            }
        }

        if v.compact {
            continue;
        }

        let label_rect = Rect::new(
            icon_rect.right + ROW_ICON_TEXT_GAP,
            row.rect.top,
            row.rect.right - ROW_TEXT_RIGHT_MARGIN,
            row.rect.bottom,
        );
        // Section header: 12px semi-bold UPPERCASE in `text_tertiary`.
        // Nav row: 14px, `text_nav_active` when active, else `text_primary`.
        let (format, color) = if row.is_section {
            (&f.caption_strong, &t.text_tertiary)
        } else if row.selected {
            (&f.body, &t.text_nav_active)
        } else {
            (&f.body, &t.text_primary)
        };
        let label: std::borrow::Cow<str> = if row.is_section {
            row.label.to_uppercase().into()
        } else {
            row.label.as_str().into()
        };
        c.text_ellipsis(&label, &label_rect, format, color);

        if let Some(expanded) = row.chevron {
            let glyph = if expanded { GLYPH_CHEVRON_DOWN } else { GLYPH_CHEVRON_RIGHT };
            let chevron_rect = Rect::new(
                row.rect.left + row.indent - 24.0 + v.dx,
                row.rect.top,
                row.rect.left + row.indent - 4.0 + v.dx,
                row.rect.bottom,
            );
            let chevron_color = if row.is_section { &t.text_tertiary } else { &t.text_secondary };
            c.text(glyph, &chevron_rect, &f.icon_small, chevron_color, true);
        }
    }

    c.pop_clip();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes() {
        assert_eq!(sidebar_mode(640.0, false), SidebarMode::Minimal);
        assert_eq!(sidebar_mode(1000.0, true), SidebarMode::Compact);
        assert_eq!(sidebar_mode(1000.0, false), SidebarMode::Expanded);
        assert_eq!(display_mode_for_pane_width(199.0), SidebarMode::Compact);
        assert_eq!(display_mode_for_pane_width(240.0), SidebarMode::Expanded);
    }

    #[test]
    fn pane_widths() {
        assert_eq!(sidebar_pane_width(SidebarMode::Compact, 300.0), SIDEBAR_COMPACT_WIDTH);
        assert_eq!(sidebar_pane_width(SidebarMode::Minimal, 300.0), SIDEBAR_OPEN_PANE_LENGTH);
        assert_eq!(sidebar_pane_width(SidebarMode::Expanded, 999.0), SIDEBAR_MAX_WIDTH);
        assert_eq!(sidebar_pane_width(SidebarMode::Expanded, 50.0), SIDEBAR_MIN_WIDTH);
    }
}
