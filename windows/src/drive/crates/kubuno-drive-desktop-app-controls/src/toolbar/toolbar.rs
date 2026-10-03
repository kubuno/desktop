//! `Toolbar` level (mirrors `Files.App.Controls/Toolbar/Toolbar.cs`).
//!
//! The WinUI `Control` class is not portable; this file houses the port's
//! control rendering ([`ToolbarView`], [`draw`]) and the size constants
//! pulled from `Toolbar.ThemeResources.xaml`.

use super::toolbar_button::toolbar_button::GLYPH_CHEVRON_DOWN;
use super::toolbar_button::{ButtonVisual, ToolbarButton};
use crate::geometry::Rect;
use crate::themes::shape::pill;
use crate::Canvas;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

/// `ToolbarInnerPadding` = 4. Card's inner margin.
pub const INNER_PADDING: f32 = 4.0;
/// `ToolbarBorderThickness` = 1.
pub const BORDER_THICKNESS: f32 = 1.0;
/// `ToolbarItemSpacing` = 4 (StackLayout Spacing).
pub const ITEM_SPACING: f32 = 4.0;
/// `ToolbarCornerRadius` = OverlayCornerRadius = 8. Kept for reference: the
/// Kubuno command bar is FLAT on the module panel, so nothing rounds it.
pub const CORNER_RADIUS: f32 = 8.0;
/// `ToolbarButtonPadding` = 4.
pub const BUTTON_PADDING: f32 = 4.0;
/// Glyph size (overflow `IconSize="16"`).
pub const ICON_SIZE: f32 = 16.0;

/// Drawing snapshot of the command bar, in PRIMITIVES. Built by the
/// application, then passed to [`draw`]. Counterpart of `Toolbar.xaml`.
pub struct ToolbarView {
    /// Background card (root Grid); `None` if there's no bar.
    pub card: Option<Rect>,
    /// Buttons, left→right, as laid out by the layout pass.
    pub buttons: Vec<ToolbarButton>,
}

/// Opacity of a disabled command (`disabled:opacity-40` in the web).
const DISABLED_OPACITY: f32 = 0.40;

/// A color at reduced opacity — a disabled command keeps its hue and only
/// fades (the web dims the whole button, it does not recolor it).
fn dimmed(color: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: color.a * DISABLED_OPACITY, ..*color }
}

/// The web's `Plus` icon (2 DIP stroke, rounded caps) built from primitives:
/// `Canvas` exposes no path API, and a plus is just two rounded bars.
fn draw_plus(c: &dyn Canvas, rect: &Rect, size: f32, thickness: f32, color: &D2D1_COLOR_F) {
    let cx = (rect.left + rect.right) / 2.0;
    let cy = (rect.top + rect.bottom) / 2.0;
    let (arm, half) = (size / 2.0, thickness / 2.0);
    c.fill_rounded(&Rect::new(cx - arm, cy - half, cx + arm, cy + half), half, color);
    c.fill_rounded(&Rect::new(cx - half, cy - arm, cx + half, cy + arm), half, color);
}

/// Draws the command bar through a [`Canvas`] — DRAWING now lives WITH the
/// control (mirrors `Toolbar` from `Files.App.Controls`).
pub fn draw(c: &dyn Canvas, v: &ToolbarView) {
    let t = c.theme();
    let f = c.formats();

    // The web has NO toolbar card: the command bar sits straight on the white
    // module panel (`--color-surface-0`) — no border, no shadow, no radius.
    // The content card right below carries the separation, so no bottom rule
    // is drawn here (it would double that card's own top border).
    if let Some(card) = &v.card {
        c.fill_rounded(card, 0.0, &t.layer_background);
    }

    for b in &v.buttons {
        let rect = &b.rect;
        match &b.visual {
            ButtonVisual::Separator => {
                let mid = (rect.left + rect.right) / 2.0;
                let bar = Rect::new(mid, rect.top + 6.0, mid + 1.0, rect.bottom - 6.0);
                c.fill_rounded(&bar, 0.0, &t.divider);
            }
            ButtonVisual::NewButton { label, hot } => {
                // A command-bar button like its neighbours, not the web's
                // free-standing sidebar pill — that shape only reads right
                // floating over the page, which is not where it lives here.
                // Kubuno styling all the same: accent « + », 14px label at
                // normal weight, chevron for the menu it opens.
                if *hot {
                    c.fill_rounded(rect, pill(rect.bottom - rect.top), &t.control_fill_hover);
                }
                let icon = Rect::new(rect.left + 8.0, rect.top, rect.left + 30.0, rect.bottom);
                draw_plus(c, &icon, ICON_SIZE, 2.0, &t.accent);
                let text_rect = Rect::new(rect.left + 36.0, rect.top, rect.right - 22.0, rect.bottom);
                c.text(label, &text_rect, &f.body, &t.text_primary, false);
                let chevron = Rect::new(rect.right - 22.0, rect.top, rect.right - 6.0, rect.bottom);
                c.text(GLYPH_CHEVRON_DOWN, &chevron, &f.icon_small, &t.text_tertiary, false);
            }
            ButtonVisual::Icon { name, hot, enabled } => {
                if *hot && *enabled {
                    c.fill_rounded(rect, pill(rect.bottom - rect.top), &t.control_fill_hover);
                }
                // Command icons are secondary text; disabled only fades them.
                let fg = if *enabled { t.text_secondary } else { dimmed(&t.text_secondary) };
                c.vector_icon(name, rect, ICON_SIZE, &fg);
            }
            ButtonVisual::IconLabel { name, label, hot, enabled } => {
                if *hot && *enabled {
                    c.fill_rounded(rect, pill(rect.bottom - rect.top), &t.control_fill_hover);
                }
                let fg = if *enabled { t.text_secondary } else { dimmed(&t.text_secondary) };
                let icon = Rect::new(rect.left + 8.0, rect.top, rect.left + 30.0, rect.bottom);
                c.vector_icon_layered(name, &icon, ICON_SIZE, &fg, &fg);
                let text_rect = Rect::new(rect.left + 36.0, rect.top, rect.right - 4.0, rect.bottom);
                c.text(label, &text_rect, &f.body_small, &fg, false);
            }
            ButtonVisual::IconAccent { name, hot } => {
                if *hot {
                    c.fill_rounded(rect, pill(rect.bottom - rect.top), &t.control_fill_hover);
                }
                // MONOCHROME, like the web's lucide toolbar icons: tinting the
                // accent layer separately made the bar read as two-tone and
                // busy, the accent fighting the actual selection state.
                c.vector_icon_layered(name, rect, ICON_SIZE, &t.text_secondary, &t.text_secondary);
            }
            ButtonVisual::IconChevron { name, accent_layer, hot } => {
                if *hot {
                    c.fill_rounded(rect, pill(rect.bottom - rect.top), &t.control_fill_hover);
                }
                let icon = Rect::new(rect.left, rect.top, rect.right - 16.0, rect.bottom);
                if *accent_layer {
                    c.vector_icon_layered(name, &icon, ICON_SIZE, &t.text_secondary, &t.text_secondary);
                } else {
                    c.vector_icon(name, &icon, ICON_SIZE, &t.text_secondary);
                }
                let chevron = Rect::new(rect.right - 18.0, rect.top, rect.right - 2.0, rect.bottom);
                c.text(GLYPH_CHEVRON_DOWN, &chevron, &f.icon_small, &t.text_tertiary, true);
            }
            ButtonVisual::Toggle { name, on, hot } => {
                if *on {
                    // Active toggle = the web's `bg-primary-light text-primary`
                    // pill, NOT a solid accent block with a white glyph.
                    c.fill_rounded(rect, pill(rect.bottom - rect.top), &t.accent_light);
                    c.vector_icon(name, rect, ICON_SIZE, &t.accent);
                } else {
                    if *hot {
                        c.fill_rounded(rect, pill(rect.bottom - rect.top), &t.control_fill_hover);
                    }
                    c.vector_icon(name, rect, ICON_SIZE, &t.text_secondary);
                }
            }
        }
    }
}
