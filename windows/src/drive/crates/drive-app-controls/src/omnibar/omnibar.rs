//! `Omnibar` control (mirror of `Files.App.Controls/Omnibar/Omnibar.cs`).
//! The WinUI control itself (`OnApplyTemplate`/`PopulateModes`/popup) is not
//! ported; this file holds the PORTABLE layout math from `ChangeMode`
//! (content padding, rects of mode buttons packed to the right), the
//! dimensions from `Omnibar.xaml`, the [`OmnibarView`] snapshot, and [`draw`].

use crate::geometry::Rect;
use crate::Canvas;

use super::omnibar_mode::{MODE_BUTTON_MARGIN, MODE_CLICK_AREA_WIDTH, MODE_HEIGHT, MODE_SLOT_WIDTH};
use super::omnibar_mode_separator::SEPARATOR_SLOT_WIDTH;

/// ✕ clear glyph (small close, Segoe Fluent). Counterpart of
/// `GLYPH_CLOSE_SMALL` on the app side (`ui/metrics.rs`).
const GLYPH_CLOSE_SMALL: &str = "\u{E894}";

// ── Dimensions (Omnibar.xaml) ───────────────────────────────────────────────
/// `OmnibarDefaultHeight`. The web search field is 48 tall; the port keeps 38
/// because the toolbar strip is only 48 (see `ui/layout.rs`).
pub const HEIGHT: f32 = 38.0;
/// Pill, like the web search field (`rounded-full`) — `shape::pill(HEIGHT)`.
pub const CORNER_RADIUS: f32 = HEIGHT / 2.0;
/// `OmnibarBorderThicknessUnfocused` / `Focused`. Focus = a 2 DIP accent ring,
/// the web's `focus:border-primary`.
pub const BORDER_UNFOCUSED: f32 = 1.0;
pub const BORDER_FOCUSED: f32 = 2.0;
/// `PART_ModesHostGrid` `Padding="2,0"`.
pub const MODES_HOST_PADDING_X: f32 = 2.0;

/// Right margin added to the TextBox's padding (`ChangeMode`, the `+ 8`).
pub const CONTENT_RIGHT_MARGIN: f32 = 8.0;

/// Rects of the mode buttons, packed to the RIGHT of the bar (`ChangeMode`
/// behavior: the active column becomes `*` and pushes inactive modes against
/// the right edge). `bar` = the Omnibar's full rect.
pub fn mode_button_rects(bar: &Rect, count: usize) -> Vec<Rect> {
    let top = bar.top + ((bar.bottom - bar.top) - MODE_HEIGHT) / 2.0;
    let bottom = top + MODE_HEIGHT;
    let group_right = bar.right - MODES_HOST_PADDING_X;
    let n = count as f32;
    let group_width = n * MODE_SLOT_WIDTH + (n - 1.0).max(0.0) * SEPARATOR_SLOT_WIDTH;
    let mut x = group_right - group_width;
    let mut rects = Vec::with_capacity(count);
    for i in 0..count {
        let left = x + MODE_BUTTON_MARGIN;
        rects.push(Rect::new(left, top, left + MODE_CLICK_AREA_WIDTH, bottom));
        x += MODE_SLOT_WIDTH;
        if i + 1 < count {
            x += SEPARATOR_SLOT_WIDTH;
        }
    }
    rects
}

/// Left/right padding of the TextBox so the text doesn't overlap the mode
/// buttons — transcription of `Omnibar.cs::ChangeMode`.
pub fn content_padding(count: usize, active_index: usize) -> (f32, f32) {
    let (c, i) = (count as f32, active_index as f32);
    let left = (i + 1.0) * MODE_SLOT_WIDTH + SEPARATOR_SLOT_WIDTH * i;
    let right =
        (c - i - 1.0) * MODE_SLOT_WIDTH + SEPARATOR_SLOT_WIDTH * (c - i - 1.0) + CONTENT_RIGHT_MARGIN;
    (left, right)
}

// ── Drawing view ─────────────────────────────────────────────────────────────

/// Drawing snapshot of the Omnibar, in PRIMITIVES: enough to paint the
/// address pill and its mode buttons without knowing `UiState`/`Layout`. The
/// EDITABLE text box (`edit_box::draw_text`) and suggestions are NOT here:
/// the application draws them AROUND this call.
pub struct OmnibarView {
    /// Address pill: fill + border, and the focus ring if `focused`.
    pub address_bar: Rect,
    /// Active text mode: accent focus ring of [`BORDER_FOCUSED`] DIP.
    pub focused: bool,
    /// Active mode's icon to the left of the input, in text mode.
    pub active_mode_icon: Option<&'static str>,
    /// ✕ text-clear button (present in active text mode).
    pub clear_button: Option<Rect>,
    /// INACTIVE mode buttons packed to the right: `(rect, icon, hovered)`.
    pub mode_buttons: Vec<(Rect, &'static str, bool)>,
    /// Vertical separator (1 DIP) between two mode buttons.
    pub separator: Option<Rect>,
}

/// Draws the Omnibar through a [`Canvas`] — the control's DRAWING now lives
/// WITH the control (mirror of `Files.App.Controls`'s `Omnibar`).
pub fn draw(c: &dyn Canvas, v: &OmnibarView) {
    let t = c.theme();
    let f = c.formats();

    // Pill of `--color-surface-1` inside `--color-border`, like the web
    // search field at rest.
    c.fill_rounded(&v.address_bar, CORNER_RADIUS, &t.card_background);
    c.stroke_rounded(&v.address_bar, CORNER_RADIUS, &t.card_stroke);

    // Focus: a SINGLE 2 DIP accent border (`--color-primary`), no outer glow.
    if v.focused {
        c.stroke_rounded_w(&v.address_bar, CORNER_RADIUS, &t.accent, BORDER_FOCUSED);
    }

    // Mode icon to the left of the input.
    if let Some(icon) = v.active_mode_icon {
        let icon_rect = Rect::new(v.address_bar.left + 8.0, v.address_bar.top, v.address_bar.left + 30.0, v.address_bar.bottom);
        c.vector_icon(icon, &icon_rect, 16.0, &t.text_secondary);
    }

    // ✕ clears the text.
    if let Some(clear) = &v.clear_button {
        c.text(GLYPH_CLOSE_SMALL, clear, &f.icon_small, &t.text_secondary, true);
    }

    // Inactive mode buttons on the right: `shape::pill` hover, like the round
    // icon buttons of the web's search field.
    for (rect, icon, hot) in &v.mode_buttons {
        if *hot {
            let pill = crate::themes::shape::pill(rect.bottom - rect.top);
            c.fill_rounded(rect, pill, &t.control_fill_hover);
        }
        c.vector_icon(icon, rect, 16.0, &t.text_secondary);
    }

    // Vertical separator (1 DIP, card_stroke).
    if let Some(sep) = &v.separator {
        c.fill_rounded(sep, 0.0, &t.card_stroke);
    }
}
