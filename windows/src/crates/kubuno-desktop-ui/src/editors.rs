//! Kubuno primitives — **editors**: the controls that pick or edit a piece of
//! text's *form* rather than a value in a form.
//!
//! [`Dropdown`], [`Editable`], [`FontPicker`], [`FontSizeField`] and
//! [`RichTextToolbar`].
//!
//! # What this family assembles, and what it refuses to write
//!
//! Almost nothing here is new code. Three neighbouring families already own the
//! hard parts, and this one composes them:
//!
//! | need | where it already lives |
//! |---|---|
//! | caret, selection, horizontal scroll | [`kubuno_drive_desktop_app_controls::edit_box::EditView`], reached through [`crate::text::TextField`] |
//! | a label's nine-cell placement and its line box | [`crate::display::Label`] |
//! | a select's model (`Items`, `SelectedIndex`, `Sorted`, `DropDownHeight`…) | [`kubuno_desktop_controls::lists::ComboBox`] |
//! | a spinner's range, and the « set the maximum before the value » trap | [`crate::range::NumericField`] |
//! | a strip of commands with a `Checked` state | [`kubuno_desktop_controls::toolstrip::ToolStrip`] / [`ToolStripButton`] |
//! | the floating-panel grid (padding, row, gutters, shadow) | [`crate::lists::menu_metrics`] and [`crate::lists::list_metrics`] |
//!
//! **No second caret is written here.** [`Editable`] is a [`crate::display::Label`]
//! at rest and a [`crate::text::TextField`] in edit, and that is the whole of it.
//!
//! # What is NOT ported: the rich-text editing area
//!
//! `core/frontend/src/ui/RichText.tsx` is a `contenteditable` div driven by
//! `document.execCommand`. Everything that makes it work — an inline formatting
//! model (a tree of spans, not a string), a selection that spans elements,
//! `queryCommandState`, `createLink`, `removeFormat`, undo — is the browser's,
//! and nothing under this crate has any of it: [`EditView`] is a **single-line,
//! single-run** view (`crate::text` already records that a multi-row selection
//! is out of its reach), and the [`Canvas`] draws one run in one
//! `IDWriteTextFormat` at a time.
//!
//! So the editing area is deliberately absent. What ships is its **toolbar**
//! ([`RichTextToolbar`]) — a real primitive, reusable by any editor, whose
//! active states are the replica's `ToolStripButton::Checked`.
//!
//! [`EditView`]: kubuno_drive_desktop_app_controls::edit_box::EditView
//!
//! # Where the numbers come from
//!
//! None of these five has a hand-written predecessor in `kubuno-drive-desktop-app-controls`,
//! so — as `docs/UI_BRIEF.md` requires — the reference is the **web** design
//! system, *read* from the source (this machine cannot run the web app, so no
//! number below was measured):
//!
//! | primitive | web source |
//! |---|---|
//! | [`Dropdown`] | `core/frontend/src/ui/Dropdown.tsx` |
//! | [`Editable`] | `core/frontend/src/ui/Editable.tsx` (which re-uses `@ui/Input`'s class string) |
//! | [`FontPicker`] | `core/frontend/src/ui/FontPicker.tsx` |
//! | [`FontSizeField`] | `core/frontend/src/ui/FontSizeField.tsx` (its `SizeCombo`) |
//! | [`RichTextToolbar`] | `core/frontend/src/ui/RichText.tsx` (its toolbar row) |
//!
//! # `Dropdown` vs `ComboBox` — why both ship
//!
//! [`crate::lists::ComboBox`] is already delivered, and the first question this
//! family had to answer was whether [`Dropdown`] is the same control under a
//! second name. It is not, and the two web files say so in six places:
//!
//! | | `@ui/Combobox` → [`crate::lists::ComboBox`] | `@ui/Dropdown` → [`Dropdown`] |
//! |---|---|---|
//! | trigger | `h-9 … rounded-md border px-3 gap-2`, fixed | `height` **prop** (36 default, 28 in toolbars), `padding: '0 4px 0 8px'`, `gap-1` |
//! | indicator | lucide `ChevronDown`, `w-4` | `@ui/CaretDown` — a **solid** triangle, whose own doc says « not lucide's `ChevronDown` … and not the `▼` character » |
//! | variants | one | `default` / `ghost` (borderless toolbar selector) / `dark` |
//! | options | label only | label **plus an optional icon**, with one shared gutter for the whole list |
//! | popup | the `role="listbox"` panel: `rounded-lg border bg-white`, `p-1`, 32 DIP rows with a `w-4` tick gutter | the **frosted float** panel: `padding: 5`, `5px 10px` rows at `borderRadius: 6`, a `width: 14` check gutter — i.e. the `MenuDropdown` grid |
//! | selected row | accent-coloured label + a `Check` glyph | tinted ground + `fontWeight: 600` + a check |
//!
//! Different trigger, different indicator, different popup grid, an extra
//! gutter and two extra variants: shipping one as an alias of the other would
//! not be de-duplication, it would be a regression in whichever call site lost
//! its look. What the two DO share is their **model**, and that is shared for
//! real: [`Dropdown`] owns the same [`kubuno_desktop_controls::lists::ComboBox`]
//! replica, so `Items`, `SelectedIndex`, `Sorted`, `DropDownWidth` and
//! `MaxDropDownItems` are stored once, in the toolkit's own state machine, and
//! the two primitives cannot disagree about what is selected. It also borrows
//! [`crate::lists`]' already-sourced grids rather than restating them —
//! `DROP_DOWN_MAX`, `PANEL_PAD`, `ICON_GLYPH` and the anchor offset are that
//! module's constants, imported.
//!
//! The `dark` variant is **not** ported, and that is a rule-3 decision rather
//! than an omission: it is a hard-coded palette (`#cccccc`, `#3c3c3c`,
//! `rgb(37 37 38 / 72%)`) for a page whose surrounding app is light. The
//! desktop resolves light and dark through [`Canvas::theme`], so a `ghost`
//! dropdown in a dark theme already paints dark — and a colour literal in a
//! paint body is forbidden here.

use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::sync::OnceLock;

use kubuno_drive_desktop_app_controls::{icon_name, Canvas, Rect};
use kubuno_desktop_controls::enums::{ContentAlignment, Padding, Size};
use kubuno_desktop_controls::host::{self, vk, Modifiers};
use kubuno_desktop_controls::lists as replica;
use kubuno_desktop_controls::range::OutOfRange;
use kubuno_desktop_controls::toolstrip::{StripItem, ToolStrip, ToolStripButton, ToolStripSeparator};
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::display::{Label, Role};
use crate::lists::{list_metrics, menu_metrics};
use crate::metrics::{control, height, radius, space, SHADOW_GREY, SHADOW_MENU};
use crate::range::NumericField;
use crate::text::{TextField, FOCUS_RING};
use crate::graphics::owner_draw::{self, DrawItemEventArgs, DrawItemState, DrawMode};
use crate::graphics::Graphics;
use crate::widget::{Widget, WidgetState};

// ═════════════════════════════════════════════════════════════════════════════
// Family metrics
//
// `crate::metrics` is the crate's ONE table, and everything it already answers
// is taken from it: `radius::SM` for `--radius-md`, `radius::MENU_ITEM` for the
// 6 a floating row is rounded by, `radius::FLOAT` for a float panel,
// `height::MENU_ITEM` for a `5px + 20px line + 5px` row, `height::BUTTON_MD`
// for `h-9`, and the whole spacing scale. `crate::lists` publishes the two
// floating grids this family reuses. What is left below is what the five web
// files state literally and nothing names — each constant quotes its line, and
// the three that have NO source say so in as many words.
// ═════════════════════════════════════════════════════════════════════════════

/// `Dropdown.tsx`, verbatim.
pub mod dropdown_metrics {
    use crate::metrics::{height, radius, space};

    /// `height = 36` — the prop's default. Toolbar call sites pass 28; the
    /// height is a field here for that reason ([`super::Dropdown::height`]).
    pub const HEIGHT: f32 = height::BUTTON_MD;
    /// `padding: '0 4px 0 8px'` on the trigger.
    pub const PAD_L: f32 = space::SM;
    pub const PAD_R: f32 = space::XS;
    /// `gap-1` between the label and the caret.
    pub const GAP: f32 = space::XS;
    /// `<CaretDown>`: a 10 × 10 solid triangle with `marginRight: 4`.
    pub const CARET: f32 = 10.0;
    pub const CARET_GAP: f32 = 4.0;
    /// `borderRadius: 'var(--radius-md)'`, which this system resolves to 4.
    pub const RADIUS: f32 = radius::SM;
    /// `boxShadow: 0 0 0 2px PRIMARY` on an open or focused trigger — the same
    /// 2 DIP ring `@ui/Input` wears (`focus:ring-2`).
    pub const FOCUS_RING: f32 = 2.0;
    /// `opacity: disabled ? 0.5 : 1`. Note this is **0.5**, not the 0.6
    /// `@ui/Input` fades to: the two web files really do differ.
    pub const DISABLED_ALPHA: f32 = 0.5;

    /// A popup row: `padding: '5px 10px'`.
    pub const ROW_PAD: f32 = 10.0;
    /// The check gutter: `width: 14`, `textAlign: 'center'`.
    pub const CHECK_CELL: f32 = 14.0;
    /// The icon gutter, reserved for the whole list as soon as ONE option
    /// carries an icon: `width: 18`.
    pub const ICON_CELL: f32 = 18.0;
    /// `gap-2` between the gutters and the label.
    pub const CELL_GAP: f32 = space::SM;
    /// `borderRadius: 6` on an option — [`radius::MENU_ITEM`], the same 6 a
    /// `MenuDropdown` row is rounded by.
    pub const ROW_RADIUS: f32 = radius::MENU_ITEM;
}

/// `FontPicker.tsx`, verbatim.
pub mod font_metrics {
    use crate::metrics::{height, radius, space};

    /// `height = 36`, `width = 150` — the props' defaults.
    pub const HEIGHT: f32 = height::BUTTON_MD;
    pub const WIDTH: f32 = 150.0;
    /// `padding: '0 6px 0 10px'` on the trigger.
    pub const PAD_L: f32 = 10.0;
    pub const PAD_R: f32 = 6.0;
    /// `<CaretDown size={11} />`, with the component's own `marginRight: 4`.
    pub const CARET: f32 = 11.0;
    pub const CARET_GAP: f32 = 4.0;

    /// `top: r.bottom + 4` — the popup's anchor offset. NOT the 2 a
    /// `Dropdown` uses; the two files differ.
    pub const OFFSET: f32 = 4.0;
    /// `minWidth: Math.max(248, r.width)` and `maxWidth: 360`.
    pub const MIN_WIDTH: f32 = 248.0;
    pub const MAX_WIDTH: f32 = 360.0;
    /// `borderRadius: 10` on the panel — [`radius::FLOAT`].
    pub const RADIUS: f32 = radius::FLOAT;

    /// The search row: `height: 40`, `px-2.5`, `gap-2`, `<Search size={15} />`.
    pub const SEARCH_H: f32 = 40.0;
    pub const SEARCH_PAD: f32 = 10.0;
    pub const SEARCH_GAP: f32 = space::SM;
    pub const SEARCH_GLYPH: f32 = 15.0;

    /// The list: `maxHeight: 340`, `padding: '4px 0'`.
    pub const LIST_MAX: f32 = 340.0;
    pub const LIST_PAD_V: f32 = space::XS;

    /// A category header: `padding: '8px 12px 4px'` at `fontSize: 11`.
    /// The 11 px face does not exist in `TextFormats`, so the row is measured
    /// with the **meta** line box (16) the way [`crate::display::Role::Micro`]
    /// already collapses — 8 + 16 + 4 = 28.
    pub const HEADER_PAD_T: f32 = space::SM;
    pub const HEADER_PAD_B: f32 = space::XS;
    pub const HEADER_PAD_L: f32 = space::MD;

    /// An option row: `padding: '7px 10px 7px 12px'`.
    pub const ROW_PAD_T: f32 = 7.0;
    pub const ROW_PAD_L: f32 = space::MD;
    pub const ROW_PAD_R: f32 = 10.0;
    /// The check gutter (`width: 16`) and its `<Check size={15} />`.
    pub const CHECK_CELL: f32 = 16.0;
    pub const CHECK_GLYPH: f32 = 15.0;
    /// `gap-2` between the cells, and the sample's own `marginLeft: 8`.
    pub const CELL_GAP: f32 = space::SM;
    /// `maxWidth: 96` on the glyph sample.
    pub const SAMPLE_MAX: f32 = 96.0;
    /// The empty state: `className="px-4 py-6"` at `fontSize: 12`.
    pub const EMPTY_PAD_V: f32 = 24.0;
    pub const EMPTY_PAD_H: f32 = space::LG;
    /// The search row's « Effacer » button: `text-xs px-1.5 py-0.5 rounded`.
    pub const CLEAR_PAD_X: f32 = 6.0;
    pub const CLEAR_PAD_Y: f32 = space::XXS;

    /// **NUMBER WITHOUT A SOURCE.** The option row is `fontSize: 15` and the
    /// web never states its line height (the browser's `normal` applies).
    /// `TextFormats` carries no 15 px face either, so the row is painted in the
    /// **body** face and given the body line box (20) the whole design system
    /// is set on: 7 + 20 + 7 = 34. It is written down here, once, rather than
    /// hiding as a literal in a paint body.
    pub const ROW_LINE: f32 = 20.0;
    pub const ROW_H: f32 = ROW_PAD_T * 2.0 + ROW_LINE;
    /// The header row, from the paddings above and the meta line box.
    pub const HEADER_H: f32 = HEADER_PAD_T + 16.0 + HEADER_PAD_B;
}

/// `FontSizeField.tsx`'s `SizeCombo`, verbatim.
pub mod size_metrics {
    use crate::metrics::{height, radius, space};

    /// `height = 30`, `sizeWidth = 62` — the props' defaults. The 30 is the
    /// toolbar height the whole `FontSizeField` is glued at, not `h-9`.
    pub const HEIGHT: f32 = 30.0;
    pub const WIDTH: f32 = 62.0;
    /// The input's `padding: '0 2px 0 8px'`.
    pub const PAD_L: f32 = space::SM;
    pub const PAD_R: f32 = space::XXS;
    /// The caret button: `width: 18`, `<CaretDown size={10} />`.
    pub const CARET_CELL: f32 = 18.0;
    pub const CARET: f32 = 10.0;
    /// `minSize = 1`, `maxSize = 999` — the props' defaults.
    pub const MIN: f64 = 1.0;
    pub const MAX: f64 = 999.0;

    /// The popup: `top: r.bottom + 4`, `minWidth: Math.max(56, r.width)`,
    /// `maxHeight: 280`, `borderRadius: 8`, `padding: '4px 0'`.
    pub const OFFSET: f32 = space::XS;
    pub const MIN_WIDTH: f32 = 56.0;
    pub const RADIUS: f32 = radius::XL;
    pub const LIST_PAD_V: f32 = space::XS;
    /// A preset row: `padding: '5px 12px'` over a 20 DIP line — the same
    /// arithmetic [`height::MENU_ITEM`] is documented with.
    pub const ROW_H: f32 = height::MENU_ITEM;
    pub const ROW_PAD_L: f32 = space::MD;
}

/// `RichText.tsx`'s toolbar row, verbatim.
pub mod rich_metrics {
    use crate::metrics::{radius, space};

    /// The row: `px-1.5 py-1` around `w-8 h-8` buttons.
    pub const PAD_X: f32 = 6.0;
    pub const PAD_Y: f32 = space::XS;
    pub const BUTTON: f32 = 32.0;
    /// `gap-0.5` between the cells.
    pub const GAP: f32 = space::XXS;
    /// `rounded` on a button, which this system resolves to 4.
    pub const RADIUS: f32 = radius::SM;
    /// `<Bold size={15} />` — every glyph in the bar is 15.
    pub const GLYPH: f32 = 15.0;
    /// `<span className="w-px h-5 bg-border mx-1" />`: a 1 DIP rule, 20 tall,
    /// with 4 of margin on each side.
    pub const RULE: f32 = 1.0;
    pub const RULE_H: f32 = 20.0;
    pub const RULE_MARGIN: f32 = space::XS;
    /// The cell a separator occupies: the rule plus both margins.
    pub const SEPARATOR_CELL: f32 = RULE + 2.0 * RULE_MARGIN;
    /// `border-b border-border` under the row.
    pub const RULE_UNDER: f32 = 1.0;
    /// The whole bar: `py-1` twice around a `h-8` button, plus the rule.
    pub const HEIGHT: f32 = BUTTON + 2.0 * PAD_Y + RULE_UNDER;
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

/// The same colour at a fraction of its alpha — how this design system dims.
///
/// The web fades with CSS `opacity`, for which there is no token; alpha over
/// the surface composites the same way. The hue still comes from the theme, so
/// this is not a colour literal.
fn faded(colour: &D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: colour.a * alpha, ..*colour }
}

/// The layered shadow under a floating panel — [`crate::lists`]' own, so a
/// dropdown and a menu are lifted off the page by the same recipe. It follows
/// the panel's own corner, so a 8-rounded size list does not wear the shadow
/// of a 10-rounded one.
fn drop_shadow(c: &dyn Canvas, panel: &Rect, corner: f32) {
    c.draw_shadow(panel, corner, &SHADOW_MENU, SHADOW_GREY);
}

/// The opaque face of a floating panel.
///
/// `layer_background`, **not** `flyout_background`: that token is half-opaque
/// because the web's float lives over a `backdrop-filter` and the desktop's
/// over a compositor blur. A primitive painted INSIDE a window has neither, and
/// the half-opaque colour would let the page read straight through it — the
/// finding [`crate::lists::Menu`] already recorded, reused here so the two
/// floats agree.
fn paint_float_panel(c: &dyn Canvas, panel: Rect, corner: f32) {
    let t = c.theme();
    drop_shadow(c, &panel, corner);
    c.fill_rounded(&panel, corner, &t.layer_background);
    c.stroke_rounded(&panel, corner, &t.card_stroke);
}

// ─────────────────────────────────────────────────────────────────────────────
// Floating surfaces: placement, shadow margin, scrolling, type-ahead
// ─────────────────────────────────────────────────────────────────────────────

/// The numbers the family's three popups share, all from their `reposition`
/// effects and keyboard handlers.
pub mod float_metrics {
    /// `const M = 8` in `Dropdown.tsx`, `FontPicker.tsx` and
    /// `FontSizeField.tsx`: the margin a popup keeps from the viewport edge.
    pub const VIEWPORT_MARGIN: f32 = 8.0;
    /// `now - t0.at < 600` in `Dropdown.tsx`'s `typeAhead`: letters typed
    /// closer together than this extend the search, a longer pause restarts it.
    pub const TYPEAHEAD_MS: u64 = 600;
    /// `PageDown` / `PageUp` in `Dropdown.tsx`: `clamp(h ± 10)`.
    pub const DROPDOWN_PAGE: usize = 10;
    /// `PageDown` / `PageUp` in `FontPicker.tsx`: `i ± 8`.
    pub const FONT_PAGE: usize = 8;
}

/// How far [`SHADOW_MENU`] reaches past the panel it lifts: the largest
/// `blur + spread + |dy|` of its layers (2 + 6 + 2 = 10 DIP). A popup window
/// that hosts a panel must be this much larger on every side, or the shadow is
/// cut at the window's edge.
pub fn shadow_margin() -> f32 {
    SHADOW_MENU.iter().map(|l| l.blur + l.spread + l.dy.abs()).fold(0.0_f32, f32::max).ceil()
}

/// `r` grown by `by` on every side.
pub fn inflate(r: Rect, by: f32) -> Rect {
    Rect::new(r.left - by, r.top - by, r.right + by, r.bottom + by)
}

/// `r` expressed relative to `origin`'s top-left corner — how a rectangle in
/// client coordinates is handed to a `host::popup` paint closure, whose canvas
/// starts at the popup's own top-left.
pub fn rebase(r: Rect, origin: Rect) -> Rect {
    Rect::new(r.left - origin.left, r.top - origin.top, r.right - origin.left, r.bottom - origin.top)
}

/// What a popup does when it does not fit under its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    /// Open ABOVE the anchor instead — `Dropdown.tsx`
    /// (`tr.top - 2 - r.height`) and `FontSizeField.tsx`
    /// (`pos.top - r.height - height - 8`, i.e. 4 above the field).
    Flip,
    /// Slide up just enough to fit — `FontPicker.tsx`
    /// (`innerHeight - M - r.height`).
    Slide,
}

/// The web's `reposition`, for every popup of the family: under the anchor at
/// `gap`, pulled back inside `area` (the monitor work area, in the same space
/// as `anchor`) by [`float_metrics::VIEWPORT_MARGIN`] horizontally, and flipped
/// or slid vertically when it would run off the bottom.
pub fn place_floating(anchor: Rect, width: f32, height: f32, gap: f32, area: Rect, overflow: Overflow) -> Rect {
    let m = float_metrics::VIEWPORT_MARGIN;
    let mut l = anchor.left;
    let mut t = anchor.bottom + gap;
    if l + width > area.right - m {
        l = area.right - m - width;
    }
    if t + height > area.bottom - m {
        t = match overflow {
            Overflow::Flip => anchor.top - gap - height,
            Overflow::Slide => area.bottom - m - height,
        };
    }
    l = l.max(area.left + m);
    t = t.max(area.top + m);
    Rect::new(l, t, l + width, t + height)
}

/// A thin scroll thumb on the right edge of `track`: `first` of `total`
/// content units scrolled away, `visible` of them on screen. Painted only when
/// the content overflows — the web's `overflowY: auto`, which shows no bar
/// when everything fits. The width is the web's `::-webkit-scrollbar` (8), the
/// minimum length the shared scroll bar's.
///
/// `panel` and `corner` are the floating panel's outline: the thumb stays
/// inside its border and clear of its rounded corners.
fn paint_scroll_thumb(c: &dyn Canvas, panel: Rect, corner: f32, track: Rect, first: f32, visible: f32, total: f32) {
    if total <= visible || total <= 0.0 {
        return;
    }
    let w = control::SCROLLBAR_THUMB;
    let (frame, r) = crate::range::inside_border(panel, corner);
    let right = track.right.min(frame.right);
    let rail = crate::range::fit_rail(Rect::new(right - w, track.top, right, track.bottom), frame, r);
    let h = rail.bottom - rail.top;
    if h <= 0.0 {
        return;
    }
    let len = (h * visible / total).clamp(control::SCROLLBAR_THUMB_MIN.min(h), h);
    let reach = (total - visible).max(1.0);
    let top = rail.top + (h - len) * (first / reach).clamp(0.0, 1.0);
    let thumb = Rect::new(rail.left, top, rail.right, top + len);
    c.push_clip_rounded(&frame, r);
    c.fill_rounded(&thumb, w / 2.0, &c.theme().scrollbar_thumb);
    c.pop_clip_rounded();
}

/// The web's type-ahead (`Dropdown.tsx`, `typeAhead`): letters typed in quick
/// succession are matched against the START of the labels — « b », « bo »,
/// « boo » — and forgotten after [`float_metrics::TYPEAHEAD_MS`]. A single
/// letter repeated (« b », « b », « b ») walks through the matches instead of
/// looking for « bbb ».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeAhead {
    text: String,
    at_ms: u64,
}

impl TypeAhead {
    /// The typed prefix currently remembered (lower case).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Feeds one character typed at `now_ms` and returns the label it lands on,
    /// searched from the row after `from` (`None` = before the first row) so
    /// repeated letters cycle. `None` when nothing matches.
    pub fn find<S: AsRef<str>>(&mut self, ch: char, now_ms: u64, from: Option<usize>, labels: &[S]) -> Option<usize> {
        let fresh = now_ms.saturating_sub(self.at_ms) >= float_metrics::TYPEAHEAD_MS || self.text.is_empty();
        if fresh {
            self.text.clear();
        }
        self.text.extend(ch.to_lowercase());
        self.at_ms = now_ms;
        let n = labels.len();
        if n == 0 {
            return None;
        }
        let q = self.text.clone();
        let mut chars = q.chars();
        let first = chars.next()?;
        let count = q.chars().count();
        // « A single repeated letter cycles ».
        let single = count > 1 && q.chars().all(|c| c == first);
        let needle: String = if single { first.to_string() } else { q.clone() };
        // `from + 1` for a fresh letter or a repeat, `from` while a word grows
        // — so « bo » stays on the « bo… » row the « b » found.
        let from = from.map_or(-1, |f| f as i64);
        let start = if single || count == 1 { from + 1 } else { from.max(0) };
        (0..n as i64)
            .map(|k| ((start + k) % n as i64 + n as i64) as usize % n)
            .find(|&i| labels[i].as_ref().to_lowercase().starts_with(&needle))
    }

    /// Forgets the typed prefix (the list closed).
    pub fn reset(&mut self) {
        self.text.clear();
    }
}

/// Which edge of a selector is squared off so it can be glued to a neighbour
/// — `FontSizeField.tsx`'s `buttonStyle` / `boxStyle`: « the joined edge is
/// squared (only the outer corners stay rounded) and the middle borders
/// overlap into one divider line ».
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SquaredEdge {
    #[default]
    None,
    /// The left corners are square (the control's left neighbour is glued).
    Left,
    /// The right corners are square.
    Right,
}

/// Fills and strokes a box whose `joined` edge is square. `Canvas` rounds all
/// four corners or none, so the rounded shape is drawn one radius wider than
/// the box on the squared side, clipped back to the box, and the squared
/// edge's own border is drawn as a straight 1 DIP line.
fn paint_box(
    c: &dyn Canvas,
    r: Rect,
    corner: f32,
    joined: SquaredEdge,
    fill: Option<&D2D1_COLOR_F>,
    line: Option<&D2D1_COLOR_F>,
) {
    let wide = match joined {
        SquaredEdge::None => r,
        SquaredEdge::Left => Rect::new(r.left - corner - 1.0, r.top, r.right, r.bottom),
        SquaredEdge::Right => Rect::new(r.left, r.top, r.right + corner + 1.0, r.bottom),
    };
    if joined != SquaredEdge::None {
        c.push_clip(&r);
    }
    if let Some(fill) = fill {
        c.fill_rounded(&wide, corner, fill);
    }
    if let Some(line) = line {
        c.stroke_rounded(&wide, corner, line);
        let edge = control::SEPARATOR;
        match joined {
            SquaredEdge::None => {}
            SquaredEdge::Left => c.fill_rounded(&Rect::new(r.left, r.top, r.left + edge, r.bottom), 0.0, line),
            SquaredEdge::Right => c.fill_rounded(&Rect::new(r.right - edge, r.top, r.right, r.bottom), 0.0, line),
        }
    }
    if joined != SquaredEdge::None {
        c.pop_clip();
    }
}

/// The accent focus ring every selector of the family wears on
/// `:focus-visible` — the same 2 DIP ring `@ui/Input` draws
/// ([`crate::text::FOCUS_RING`]).
fn paint_focus_ring(c: &dyn Canvas, r: Rect, corner: f32) {
    c.stroke_rounded_w(&r, corner, &c.theme().accent, FOCUS_RING);
}

/// What a keystroke did to a selector with a dropped list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    /// Not a key this control uses — leave it for someone else.
    Ignored,
    /// The list opened.
    Opened,
    /// The highlight moved (or the query changed) inside the open list.
    Moved,
    /// Row / option `i` was chosen; the list is closed.
    Committed(usize),
    /// The list closed without a choice.
    Closed,
}

impl ListKey {
    /// Whether the key was used (anything but [`ListKey::Ignored`]).
    pub fn handled(self) -> bool {
        self != ListKey::Ignored
    }
}

/// Clamps `i + delta` into `0 .. n` — the web's `clamp`.
fn step_index(i: Option<usize>, delta: i64, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let base = i.map_or(if delta >= 0 { -1 } else { n as i64 }, |i| i as i64);
    Some((base + delta).clamp(0, n as i64 - 1) as usize)
}

/// The solid drop-down caret every list-style selector in this family wears,
/// centred in `cell`.
///
/// `@ui/CaretDown` is a **geometry**, not a character — its own doc comment
/// rejects the `▼` character « whose shape and baseline change from one font to
/// the next », which is rule 6 of the brief stated in the web's own words. The
/// path lives in `assets/lucide-icons.txt` under `CaretDown`.
fn paint_caret(c: &dyn Canvas, cell: Rect, size: f32, colour: &D2D1_COLOR_F) {
    c.vector_icon("CaretDown", &cell, size, colour);
}

/// The installed font families, enumerated **once** per process.
///
/// This is [`kubuno_drive_desktop_app_controls::Renderer::system_font_families`] — the very
/// function the shell's appearance settings already list fonts with — reached
/// through a detached renderer, because that method needs a `Renderer` and a
/// [`Canvas`] does not expose one. The renderer is dropped as soon as the names
/// are read; the `OnceLock` is what keeps a paint body from building a D3D
/// device per frame.
///
/// An empty slice means the enumeration failed (no device, no font
/// collection). Callers show what they are given — a picker with no families is
/// visibly empty, which is the truth, rather than a plausible invented list.
pub fn system_font_families() -> &'static [String] {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(found) = FAMILIES.get() {
        return found;
    }
    // Only a SUCCESS is kept: a call made before the thread has a COM
    // apartment (an app declaring its UI before the host starts) fails, and
    // caching that failure would leave every font list empty for the life of
    // the process.
    let found = kubuno_drive_desktop_app_controls::Renderer::new_detached(1, 1, 96.0, None)
        .map(|r| r.system_font_families())
        .unwrap_or_default();
    if found.is_empty() {
        return &[];
    }
    FAMILIES.get_or_init(|| found)
}

// ═════════════════════════════════════════════════════════════════════════════
// Dropdown
// ═════════════════════════════════════════════════════════════════════════════

/// Which of `Dropdown.tsx`'s looks a selector wears.
///
/// `dark` is absent on purpose — see the module documentation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DropdownVariant {
    /// `default`: a bordered field that takes the accent ring when open.
    #[default]
    Default,
    /// `ghost`: `border: transparent` — the toolbar selector, which must not
    /// draw a box inside a bar that is already a box.
    Ghost,
}

/// A selector with optional per-option icons — `@ui/Dropdown`.
///
/// The **model is the toolkit's**: `Items`, `SelectedIndex`, `Sorted`,
/// `DropDownWidth` and `MaxDropDownItems` all live in the
/// [`kubuno_desktop_controls::lists::ComboBox`] this owns and are reached through
/// [`Deref`] — the same replica [`crate::lists::ComboBox`] owns, so the two
/// primitives cannot hold two different ideas of what is selected. What this
/// type adds is what .NET has no concept of: a [`DropdownVariant`], a free
/// trigger [`height`](Dropdown::height), a `placeholder`, the per-option icon
/// gutter, and `open` / `hot_index` (runtime state, not designer properties).
///
/// ```ignore
/// let mut d = Dropdown::new();
/// d.add_option("Trier par nom", Some("Type"));   // model + icon, together
/// d.add_option("Trier par date", None);
/// d.set_selected_index(0);                       // the replica's own setter
/// d.height = 28.0;                               // the toolbar size
/// d.variant = DropdownVariant::Ghost;
/// ```
#[derive(Clone, Default)]
pub struct Dropdown {
    inner: replica::ComboBox,
    /// One entry per item — the web's `DropdownOption.icon`, as a geometry name
    /// from `assets/lucide-icons.txt`.
    ///
    /// It is a **parallel** vector rather than a field on the item because the
    /// item IS the replica's `String`: giving options a struct of their own here
    /// would put the label in two places, which rule 1 forbids. It is kept in
    /// step by [`Dropdown::add_option`], and read through [`Dropdown::icon_of`],
    /// which answers `None` for anything it does not cover — so a caller that
    /// pushes into `items` directly gets an icon-less option, never a panic.
    pub icons: Vec<Option<String>>,
    pub variant: DropdownVariant,
    /// `height` — 36 by default, 28 in a toolbar.
    pub height: f32,
    /// The greyed text shown when nothing is selected. .NET's `ComboBox` has no
    /// `PlaceholderText` (only `TextBox` does), so it lives here.
    pub placeholder: String,
    /// `DroppedDown` — runtime state the replica does not model.
    pub open: bool,
    /// The row under the pointer inside the open list.
    pub hot_index: Option<usize>,
    /// The trigger holds the keyboard focus (`focusable` + `focused` in the
    /// web, which is what turns the border accent even while closed).
    pub focused: bool,
    /// The first option row shown when the list is longer than
    /// `maxHeight: 280` — the list's scroll position, in rows. Kept in range by
    /// [`Dropdown::scroll_by`] and [`Dropdown::ensure_visible`].
    pub scroll: usize,
    /// The type-ahead buffer (`typed` in the web).
    pub type_ahead: TypeAhead,
    /// Squares one edge to glue the trigger to a neighbour (`buttonStyle`).
    pub joined: SquaredEdge,
    /// Where [`Dropdown::place_drop_down`] put the open list — `None` means
    /// « under the trigger », the historical behaviour.
    placed: Option<Rect>,
}

impl Deref for Dropdown {
    type Target = replica::ComboBox;
    fn deref(&self) -> &replica::ComboBox {
        &self.inner
    }
}
impl DerefMut for Dropdown {
    fn deref_mut(&mut self) -> &mut replica::ComboBox {
        &mut self.inner
    }
}

impl Dropdown {
    pub fn new() -> Self {
        Self { height: dropdown_metrics::HEIGHT, ..Self::default() }
    }

    /// Adds an option and its icon in one call — the only mutator that keeps
    /// [`Dropdown::icons`] aligned with the replica's `Items`.
    ///
    /// Returns the index the replica stored it at, which is **not** necessarily
    /// the end: `Sorted` inserts in order, and this follows it.
    pub fn add_option(&mut self, label: impl Into<String>, icon: Option<&str>) -> usize {
        let at = self.inner.add_item(label);
        let icon = icon.map(str::to_string);
        if at >= self.icons.len() {
            self.icons.resize(at + 1, None);
            self.icons[at] = icon;
        } else {
            self.icons.insert(at, icon);
        }
        at
    }

    /// The geometry name of option `i`, if it has one.
    pub fn icon_of(&self, i: usize) -> Option<&str> {
        self.icons.get(i).and_then(Option::as_deref)
    }

    /// Whether ANY option carries an icon — the web's `anyIcon`, which reserves
    /// one gutter for the whole list « so their labels stay in one column ».
    pub fn has_icon_gutter(&self) -> bool {
        self.icons.iter().any(Option::is_some)
    }

    /// The text the trigger shows: the selected label, else the placeholder,
    /// else the control's own `Text` — `selected?.label ?? placeholder ?? value`.
    pub fn display_text(&self) -> &str {
        match self.inner.selected_item() {
            Some(s) => s,
            None if !self.placeholder.is_empty() => &self.placeholder,
            None => &self.inner.control().text,
        }
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open_with(self.selected());
        }
    }

    /// The selected row, as an index (`selectedIdx`, `-1` → `None`).
    pub fn selected(&self) -> Option<usize> {
        usize::try_from(self.inner.selected_index).ok().filter(|&i| i < self.inner.items.len())
    }

    /// `openWith(i)`: drops the list with row `i` highlighted and scrolled
    /// into view. A disabled control refuses, as the web's `if (disabled)
    /// return` does.
    pub fn open_with(&mut self, i: Option<usize>) {
        if !self.inner.control().enabled {
            return;
        }
        self.open = true;
        self.hot_index = i.filter(|&i| i < self.inner.items.len());
        if let Some(i) = self.hot_index {
            self.ensure_visible(i);
        }
    }

    /// Closes the list without choosing.
    pub fn close(&mut self) {
        self.open = false;
        self.hot_index = None;
        self.type_ahead.reset();
    }

    /// `commit(i)`: selects row `i` through the replica and closes.
    pub fn commit(&mut self, i: usize) -> ListKey {
        if i >= self.inner.items.len() {
            self.close();
            return ListKey::Closed;
        }
        self.inner.set_selected_index(i as i32);
        self.close();
        ListKey::Committed(i)
    }

    /// What leaving the trigger with the list open does: « leaving takes the
    /// highlighted row with it, as a native list does » (the web's `Tab`).
    pub fn commit_highlight(&mut self) -> ListKey {
        match self.hot_index {
            Some(i) if self.open => self.commit(i),
            _ if self.open => {
                self.close();
                ListKey::Closed
            }
            _ => ListKey::Ignored,
        }
    }

    /// The labels, for type-ahead.
    fn labels(&self) -> &[String] {
        &self.inner.items
    }

    /// `onTriggerKeyDown`, key by key — what a native `<select>` does. The
    /// trigger keeps the focus and the list follows it; `Escape` closes.
    ///
    /// Closed: `↓` `↑` `Enter` `Space` (and `Alt+↓`) open on the selected row,
    /// `Home` / `End` open on the first / last. Open: `↑` `↓` move, `Home`
    /// `End`, `PageUp` `PageDown` (±10), `Enter` / `Space` / `Alt+↑` choose
    /// the highlighted row, `Escape` closes.
    pub fn key_down(&mut self, key: u16, mods: Modifiers) -> ListKey {
        if !self.inner.control().enabled {
            return ListKey::Ignored;
        }
        let n = self.inner.items.len();
        let plain = mods.matches(Modifiers::NONE);
        let alt = mods.matches(Modifiers::ALT);
        if !self.open {
            let at = match key {
                vk::DOWN | vk::UP if plain || alt => Some(self.selected().unwrap_or(0)),
                vk::ENTER | vk::SPACE if plain => Some(self.selected().unwrap_or(0)),
                vk::HOME if plain => Some(0),
                vk::END if plain => Some(n.saturating_sub(1)),
                _ => return ListKey::Ignored,
            };
            self.open_with(at);
            return ListKey::Opened;
        }
        let hi = self.hot_index;
        let moved = match key {
            vk::UP if alt => return self.commit_highlight(),
            vk::DOWN if plain => step_index(hi, 1, n),
            vk::UP if plain => step_index(hi, -1, n),
            vk::HOME if plain => step_index(None, 1, n),
            vk::END if plain => step_index(None, -1, n),
            vk::PAGE_DOWN if plain => step_index(hi, float_metrics::DROPDOWN_PAGE as i64, n),
            vk::PAGE_UP if plain => step_index(hi, -(float_metrics::DROPDOWN_PAGE as i64), n),
            vk::ENTER | vk::SPACE if plain => return self.commit_highlight(),
            vk::ESCAPE if plain => {
                self.close();
                return ListKey::Closed;
            }
            _ => return ListKey::Ignored,
        };
        self.hot_index = moved;
        if let Some(i) = moved {
            self.ensure_visible(i);
        }
        ListKey::Moved
    }

    /// Typed characters — the type-ahead. Closed, a match opens the list on
    /// it; open, it moves the highlight. A space is never part of a search:
    /// the web's `switch` takes `' '` first (it opens or commits).
    pub fn type_text(&mut self, text: &str, now_ms: u64) -> ListKey {
        if !self.inner.control().enabled {
            return ListKey::Ignored;
        }
        let mut result = ListKey::Ignored;
        for ch in text.chars().filter(|c| !c.is_whitespace() && !c.is_control()) {
            let from = if self.open { self.hot_index } else { self.selected() };
            let labels = self.labels().to_vec();
            if let Some(i) = self.type_ahead.find(ch, now_ms, from, &labels) {
                if self.open {
                    self.hot_index = Some(i);
                    self.ensure_visible(i);
                    result = ListKey::Moved;
                } else {
                    self.open_with(Some(i));
                    result = ListKey::Opened;
                }
            }
        }
        result
    }

    /// Reads this frame's keys and typed text from the host queue and applies
    /// them — for a caller whose trigger holds the focus. Keys this control
    /// does not use are left in the queue for the next reader. Escape is taken
    /// only while the list is open (the web's capture listener), so a closed
    /// dropdown never swallows the Escape a dialog is waiting for.
    pub fn take_input(&mut self, now_ms: u64) -> ListKey {
        let mut result = ListKey::Ignored;
        let mut keys: Vec<(u16, Modifiers)> = Vec::new();
        let open = self.open;
        host::consume(|e| match e {
            host::InputEvent::Key { vk: k, down: true, mods, .. } => {
                let wanted = matches!(
                    *k,
                    vk::DOWN | vk::UP | vk::HOME | vk::END | vk::PAGE_DOWN | vk::PAGE_UP | vk::ENTER | vk::SPACE
                ) || (open && *k == vk::ESCAPE);
                if wanted && !mods.ctrl {
                    keys.push((*k, *mods));
                }
                wanted && !mods.ctrl
            }
            _ => false,
        });
        for (k, m) in keys {
            let r = self.key_down(k, m);
            if r.handled() {
                result = r;
            }
        }
        let typed = host::take_text();
        let r = self.type_text(&typed, now_ms);
        if r.handled() {
            result = r;
        }
        result
    }

    /// The trigger's rectangle inside `bounds` — the first [`Dropdown::height`]
    /// DIP of it, so a caller can hand the whole control one rectangle and the
    /// list still drops below the button rather than below the rectangle.
    pub fn trigger_rect(&self, bounds: Rect) -> Rect {
        Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + self.height)
    }

    /// How many rows fit before the list scrolls: `maxHeight: 280` over the
    /// row height, further bounded by `MaxDropDownItems` (the replica's).
    pub fn visible_rows(&self) -> usize {
        let room = ((list_metrics::DROP_DOWN_MAX - 2.0 * menu_metrics::PANEL_PAD)
            / height::MENU_ITEM)
            .floor()
            .max(1.0) as usize;
        self.inner
            .items
            .len()
            .min(room)
            .min(self.inner.max_drop_down_items.max(1) as usize)
    }

    /// Whether the list is longer than its window — `overflowY: 'auto'` then
    /// shows a bar, and the rows give up its width.
    pub fn scrolls(&self) -> bool {
        self.inner.items.len() > self.visible_rows()
    }

    /// The furthest [`Dropdown::scroll`] can go.
    pub fn max_scroll(&self) -> usize {
        self.inner.items.len().saturating_sub(self.visible_rows())
    }

    /// Scrolls the list by `rows` (positive = down), clamped.
    pub fn scroll_by(&mut self, rows: i32) {
        let s = (self.scroll as i64 + rows as i64).clamp(0, self.max_scroll() as i64);
        self.scroll = s as usize;
    }

    /// Scrolls by a wheel travel in DIP (`Frame::wheel_dip`, `.1 > 0` = down),
    /// one row per [`height::MENU_ITEM`], at least one row per notch.
    pub fn scroll_by_dip(&mut self, dy: f32) {
        if dy == 0.0 {
            return;
        }
        let rows = (dy / height::MENU_ITEM).round() as i32;
        let rows = if rows == 0 { dy.signum() as i32 } else { rows };
        self.scroll_by(rows);
    }

    /// `scrollIntoView({ block: 'nearest' })` for row `i`.
    pub fn ensure_visible(&mut self, i: usize) {
        let v = self.visible_rows().max(1);
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + v {
            self.scroll = i + 1 - v;
        }
        self.scroll = self.scroll.min(self.max_scroll());
    }

    /// The popup's height: its rows plus the panel's own `padding: 5`.
    pub fn drop_down_height(&self) -> f32 {
        self.visible_rows() as f32 * height::MENU_ITEM + 2.0 * menu_metrics::PANEL_PAD
    }

    /// Where the open list is drawn.
    ///
    /// `openDropdown`: `top: r.bottom + 2`, `left: r.left`,
    /// `minWidth: Math.max(dropdownMinWidth ?? 0, r.width)` — `DropDownWidth` is
    /// the replica's spelling of `dropdownMinWidth`, and its `0` default means
    /// « match the control », exactly as the web's `?? 0` does. Once
    /// [`Dropdown::place_drop_down`] has run, the placed rectangle instead.
    pub fn drop_down_rect(&self, bounds: Rect) -> Rect {
        if let Some(p) = self.placed {
            return p;
        }
        let trigger = self.trigger_rect(bounds);
        let top = trigger.bottom + DROP_DOWN_OFFSET;
        let width = (trigger.right - trigger.left).max(self.inner.drop_down_width.max(0) as f32);
        Rect::new(trigger.left, top, trigger.left + width, top + self.drop_down_height())
    }

    /// The list's `width: max-content`: the widest label plus both gutters and
    /// the row and panel paddings (and the bar when it scrolls).
    pub fn natural_drop_down_width(&self, canvas: &dyn Canvas) -> f32 {
        let f = canvas.formats();
        let widest = self
            .inner
            .items
            .iter()
            .fold(0.0_f32, |w, s| w.max(canvas.measure(s, &f.body_strong)));
        let bar = if self.scrolls() { control::SCROLLBAR_THUMB } else { 0.0 };
        widest + self.label_inset() + dropdown_metrics::ROW_PAD + 2.0 * menu_metrics::PANEL_PAD + bar
    }

    /// The web's `placeAt` + `reposition`: sizes the list to its content
    /// (never narrower than the trigger or `DropDownWidth`, never wider than
    /// the work area), then places it under the trigger — or ABOVE it when the
    /// monitor has no room below — inside `area` (the monitor work area, in
    /// the same space as `bounds`: `Frame::screen_area`). Every later geometry
    /// call ([`Dropdown::drop_down_rect`], [`Dropdown::row_rect`],
    /// [`Dropdown::item_at`]) follows the placement.
    pub fn place_drop_down(&mut self, canvas: &dyn Canvas, bounds: Rect, area: Rect) {
        let trigger = self.trigger_rect(bounds);
        let room = (area.right - area.left - 2.0 * float_metrics::VIEWPORT_MARGIN).max(0.0);
        let width = (trigger.right - trigger.left)
            .max(self.inner.drop_down_width.max(0) as f32)
            .max(self.natural_drop_down_width(canvas))
            .min(room);
        let p = place_floating(trigger, width, self.drop_down_height(), DROP_DOWN_OFFSET, area, Overflow::Flip);
        self.placed = Some(p);
    }

    /// Forgets a [`Dropdown::place_drop_down`] placement.
    pub fn clear_placement(&mut self) {
        self.placed = None;
    }

    /// The rectangle a `host::popup` hosting the open list must cover: the
    /// panel plus its shadow ([`shadow_margin`]). Only the list — the trigger
    /// stays painted in the owner window ([`Dropdown::paint_field`]).
    pub fn drop_down_paint_bounds(&self, bounds: Rect) -> Rect {
        inflate(self.drop_down_rect(bounds), shadow_margin())
    }

    /// The rows' column inside a panel: its `padding: 5`, minus the bar when
    /// the list scrolls.
    fn rows_column(&self, panel: Rect) -> Rect {
        let bar = if self.scrolls() { control::SCROLLBAR_THUMB } else { 0.0 };
        Rect::new(
            panel.left + menu_metrics::PANEL_PAD,
            panel.top + menu_metrics::PANEL_PAD,
            (panel.right - menu_metrics::PANEL_PAD - bar).max(panel.left + menu_metrics::PANEL_PAD),
            panel.bottom - menu_metrics::PANEL_PAD,
        )
    }

    /// Where option `i` is drawn inside the panel `panel` — `None` when it is
    /// scrolled out of the window.
    pub fn row_rect_in(&self, panel: Rect, i: usize) -> Option<Rect> {
        if i < self.scroll || i >= (self.scroll + self.visible_rows()).min(self.inner.items.len()) {
            return None;
        }
        let col = self.rows_column(panel);
        let y = col.top + (i - self.scroll) as f32 * height::MENU_ITEM;
        Some(Rect::new(col.left, y, col.right, y + height::MENU_ITEM))
    }

    /// Where option `i` of the open list is drawn (`None` when scrolled out).
    pub fn row_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        self.row_rect_in(self.drop_down_rect(bounds), i)
    }

    /// Which option `(x, y)` lands on, inside the panel `panel`.
    pub fn item_at_in(&self, panel: Rect, x: f32, y: f32) -> Option<usize> {
        if !self.open {
            return None;
        }
        let last = (self.scroll + self.visible_rows()).min(self.inner.items.len());
        (self.scroll..last).find(|&i| self.row_rect_in(panel, i).is_some_and(|r| r.contains(x, y)))
    }

    /// Which option `(x, y)` lands on. `None` when the list is closed, on the
    /// panel's padding, or past the last visible row.
    pub fn item_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        self.item_at_in(self.drop_down_rect(bounds), x, y)
    }

    /// Where the label starts inside a popup row: the row's `padding-left`, the
    /// check gutter, the gap, and — when the list has any icon — the icon
    /// gutter and a second gap.
    pub fn label_inset(&self) -> f32 {
        let m = dropdown_metrics::ROW_PAD;
        let base = m + dropdown_metrics::CHECK_CELL + dropdown_metrics::CELL_GAP;
        if self.has_icon_gutter() {
            base + dropdown_metrics::ICON_CELL + dropdown_metrics::CELL_GAP
        } else {
            base
        }
    }

    /// Paints the trigger alone, in its open / focused / hovered / disabled
    /// look — what the owner window draws while the list floats in a popup.
    pub fn paint_field(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let dead = state.disabled || !self.inner.control().enabled;
        self.paint_trigger(canvas, self.trigger_rect(bounds), state, dead);
    }

    fn paint_trigger(&self, c: &dyn Canvas, trigger: Rect, state: WidgetState, dead: bool) {
        let t = c.theme();
        let f = c.formats();
        let m = dropdown_metrics::RADIUS;

        // `focusBorder = variant === 'default' && (open || focused)`, where
        // `focused` is `:focus-visible` — a trigger clicked with the mouse must
        // not stay lit once its list is closed. A ghost trigger never rings: it
        // must not draw a box in a bar — it shows the keyboard focus with the
        // shared ring instead, like any other button.
        let keyboard = self.focused || state.show_focus_ring();
        let ringed = self.variant == DropdownVariant::Default && (self.open || keyboard) && !dead;

        // `background: open && !focusBorder ? activeBg : undefined`, and the
        // hover handler's `hoverBg` (not while ringed).
        if !dead && !ringed {
            if self.open {
                paint_box(c, trigger, m, self.joined, Some(&t.control_fill_pressed), None);
            } else if state.hot {
                paint_box(c, trigger, m, self.joined, Some(&t.control_fill_hover), None);
            }
        }

        // `border: 1px solid ${focusBorder ? PRIMARY : t.border}`, with
        // `transparent` for the ghost variant.
        if ringed {
            paint_box(c, trigger, m, self.joined, None, Some(&t.accent));
            paint_focus_ring(c, trigger, m);
        } else if self.variant == DropdownVariant::Default {
            let line = if dead { faded(&t.card_stroke, dropdown_metrics::DISABLED_ALPHA) } else { t.card_stroke };
            paint_box(c, trigger, m, self.joined, None, Some(&line));
        } else if keyboard && !dead {
            paint_focus_ring(c, trigger, m);
        }

        let alpha = if dead { dropdown_metrics::DISABLED_ALPHA } else { 1.0 };
        let caret = Rect::new(
            trigger.right - dropdown_metrics::PAD_R - dropdown_metrics::CARET_GAP - dropdown_metrics::CARET,
            trigger.top,
            trigger.right - dropdown_metrics::PAD_R - dropdown_metrics::CARET_GAP,
            trigger.bottom,
        );
        let label = Rect::new(
            trigger.left + dropdown_metrics::PAD_L,
            trigger.top,
            caret.left - dropdown_metrics::GAP,
            trigger.bottom,
        );

        // `<span className="truncate flex-1 text-left">{label}</span>`, greyed
        // when it is standing in for an empty selection. The ghost variant's
        // `text: '#5f6368'` / `chevron: '#80868b'` are the secondary and
        // tertiary inks; the default's are primary and secondary.
        let showing_placeholder = self.inner.selected_item().is_none() && !self.placeholder.is_empty();
        let (ink, chevron) = match self.variant {
            DropdownVariant::Default => (t.text_primary, t.text_secondary),
            DropdownVariant::Ghost => (t.text_secondary, t.text_tertiary),
        };
        let ink = if showing_placeholder { t.text_tertiary } else { ink };
        c.push_clip(&trigger);
        // Owner-draw: the selected item in the field (`DrawItemState.ComboBoxEdit`).
        let mut drawn = false;
        if self.inner.draw_mode != DrawMode::Normal && owner_draw::has_handler() {
            let g = Graphics::new(c);
            let index = usize::try_from(self.inner.selected_index).ok();
            let st = DrawItemState::COMBO_BOX_EDIT.with(DrawItemState::FOCUS, keyboard).with(DrawItemState::DISABLED, dead);
            let mut e = DrawItemEventArgs::new(&g, "Dropdown", index, label, st, self.display_text());
            drawn = owner_draw::draw_item(&mut e);
        }
        if !drawn {
            c.text_ellipsis(self.display_text(), &label, &f.body, &faded(&ink, alpha));
        }
        paint_caret(c, caret, dropdown_metrics::CARET, &faded(&chevron, alpha));
        c.pop_clip();
    }

    /// Records the visible rows of the open list, laid out in `panel` (the popup's coordinates),
    /// through the owner-draw `handler` — for a list painted later in its popup, which lends the
    /// recording ([`owner_draw::RecordedItems`]) around [`Dropdown::paint_drop_down_at`].
    pub fn record_drop_down_items(&self, panel: Rect, handler: &mut dyn owner_draw::OwnerDrawHandler) -> owner_draw::RecordedItems {
        let mut out = owner_draw::RecordedItems::new();
        if self.inner.draw_mode == DrawMode::Normal {
            return out;
        }
        let last = (self.scroll + self.visible_rows()).min(self.inner.items.len());
        for i in self.scroll..last {
            let (Some(row), Some(label)) = (self.row_rect_in(panel, i), self.inner.items.get(i)) else { break };
            let hot = self.hot_index == Some(i);
            let st = DrawItemState::NONE.with(DrawItemState::SELECTED, self.inner.selected_index == i as i32).with(DrawItemState::HOT_LIGHT, hot).with(DrawItemState::FOCUS, hot);
            out.record(handler, "Dropdown", i, row, st, label);
        }
        out
    }

    /// Paints the open list into the panel `panel` — `drop_down_rect` in the
    /// owner's space, or that rectangle [`rebase`]d into a popup's. The
    /// shadow reaches [`shadow_margin`] past `panel`.
    pub fn paint_drop_down_at(&self, c: &dyn Canvas, panel: Rect) {
        let t = c.theme();
        let f = c.formats();
        paint_float_panel(c, panel, radius::FLOAT);

        // Rows are clipped to the ROUNDED panel, so a hovered first row does
        // not square the panel's corner.
        c.push_clip_rounded(&panel, radius::FLOAT);
        let inset = self.label_inset();
        let last = (self.scroll + self.visible_rows()).min(self.inner.items.len());
        let owner = (self.inner.draw_mode != DrawMode::Normal && owner_draw::has_handler()).then(|| Graphics::new(c));
        for i in self.scroll..last {
            let (Some(row), Some(label)) = (self.row_rect_in(panel, i), self.inner.items.get(i)) else {
                break;
            };
            let selected = self.inner.selected_index == i as i32;
            // Owner-draw (`DrawMode`): the lent handler draws the row first.
            if let Some(g) = &owner {
                let hot = self.hot_index == Some(i);
                let st = DrawItemState::NONE.with(DrawItemState::SELECTED, selected).with(DrawItemState::HOT_LIGHT, hot).with(DrawItemState::FOCUS, hot);
                let mut e = DrawItemEventArgs::new(g, "Dropdown", Some(i), row, st, label.as_str());
                if owner_draw::draw_item(&mut e) {
                    continue;
                }
            }
            // `background: isHi ? (isSel ? selHoverBg : itemHover) : isSel ?
            // selBg : undefined` — the selection is an ACCENT tint
            // (`rgba(26,115,232,0.12)` = `--color-primary-light`), and a
            // highlighted selected row darkens it rather than swapping colour,
            // which two token fills compose exactly as the web's two rgba do.
            if selected {
                c.fill_rounded(&row, dropdown_metrics::ROW_RADIUS, &t.accent_light);
            }
            if self.hot_index == Some(i) {
                c.fill_rounded(&row, dropdown_metrics::ROW_RADIUS, &t.row_hover);
            }

            // The fixed gutters, « always the same width whether the check and
            // the icon are there or not: labels of the same level must line up »
            // — the file's own comment, recording a bug where a negative margin
            // shifted the selected row out of column.
            let check = Rect::new(
                row.left + dropdown_metrics::ROW_PAD,
                row.top,
                row.left + dropdown_metrics::ROW_PAD + dropdown_metrics::CHECK_CELL,
                row.bottom,
            );
            if selected {
                c.vector_icon("Check", &check, dropdown_metrics::CHECK_CELL, &t.accent);
            }
            if self.has_icon_gutter() {
                let cell = Rect::new(
                    check.right + dropdown_metrics::CELL_GAP,
                    row.top,
                    check.right + dropdown_metrics::CELL_GAP + dropdown_metrics::ICON_CELL,
                    row.bottom,
                );
                if let Some(name) = self.icon_of(i).and_then(icon_name) {
                    c.vector_icon(name, &cell, menu_metrics::ICON_GLYPH, &t.text_secondary);
                }
            }

            let text = Rect::new(
                row.left + inset,
                row.top,
                (row.right - dropdown_metrics::ROW_PAD).max(row.left + inset),
                row.bottom,
            );
            // `fontWeight: o.value === value ? 600 : undefined`.
            let fmt = if selected { &f.body_strong } else { &f.body };
            c.text_ellipsis(label, &text, fmt, &t.text_primary);
        }
        c.pop_clip_rounded();

        // The bar `overflowY: auto` shows, inside the panel's padding.
        let track = Rect::new(
            panel.left,
            panel.top + menu_metrics::PANEL_PAD,
            panel.right - menu_metrics::PANEL_PAD / 2.0,
            panel.bottom - menu_metrics::PANEL_PAD,
        );
        paint_scroll_thumb(
            c,
            panel,
            radius::FLOAT,
            track,
            self.scroll as f32,
            self.visible_rows() as f32,
            self.inner.items.len() as f32,
        );
    }

    /// Paints the open list where [`Dropdown::drop_down_rect`] puts it.
    pub fn paint_drop_down(&self, c: &dyn Canvas, bounds: Rect) {
        self.paint_drop_down_at(c, self.drop_down_rect(bounds));
    }
}

/// `openDropdown`: `top: r.bottom + 2`. The same 2 [`crate::lists::ComboBox`]
/// took from this very file.
const DROP_DOWN_OFFSET: f32 = 2.0;

impl Widget for Dropdown {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// The trigger's natural size: its widest label between the two paddings,
    /// the gap, and the caret with its own right margin. The web sizes the
    /// trigger from a `width` prop and lets the label `truncate`, so this is
    /// what « omit for natural sizing » comes to.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let f = canvas.formats();
        let widest = self
            .inner
            .items
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(self.placeholder.as_str()))
            .fold(0.0_f32, |w, s| w.max(canvas.measure(s, &f.body)));
        let chrome = dropdown_metrics::PAD_L
            + dropdown_metrics::GAP
            + dropdown_metrics::CARET
            + dropdown_metrics::CARET_GAP
            + dropdown_metrics::PAD_R;
        Size::new((widest + chrome).ceil(), self.height)
    }

    /// The trigger, and — while `open` — the list too, INLINE, under it: the
    /// historical one-call paint, still right for a static exposition or a
    /// caller that draws the whole control inside a popup of its own. A live
    /// caller floats the list instead: [`Dropdown::paint_field`] in the
    /// window, [`Dropdown::paint_drop_down_at`] in a `host::popup` covering
    /// [`Dropdown::drop_down_paint_bounds`].
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_field(canvas, bounds, state);
        let dead = state.disabled || !self.inner.control().enabled;
        if self.open && !dead {
            self.paint_drop_down(canvas, bounds);
        }
    }

    /// The trigger, plus the open list — a click in the dropped list is a click
    /// on the dropdown, not on whatever is painted behind it.
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.trigger_rect(bounds).contains(x, y)
            || (self.open && self.drop_down_rect(bounds).contains(x, y))
    }

    fn type_name(&self) -> &'static str {
        "Dropdown"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Editable
// ═════════════════════════════════════════════════════════════════════════════

/// A label that becomes a field — `@ui/Editable`.
///
/// It is an **assembly**, not a third editor: at rest it is a
/// [`crate::display::Label`], in edit it is a [`crate::text::TextField`], and
/// the caret, the selection band and the horizontal scroll are therefore
/// `kubuno_drive_desktop_app_controls`' [`EditView`](kubuno_drive_desktop_app_controls::edit_box::EditView),
/// reached through that field. Nothing about editing is written here.
///
/// # One model, one text
///
/// The [`TextField`] is the **only** storage: the rest-state label is built from
/// it at paint time (see [`Editable::label`]) rather than kept beside it, which
/// is why the two can never show different strings. Everything a caller sets —
/// `set_text`, `placeholder_text`, `read_only`, `max_length`, `select` — reaches
/// the field and then the `TextBox` replica behind it, through [`Deref`].
///
/// # Why the text does not jump
///
/// The rest label is given `@ui/Input`'s own insets as its **replica padding**
/// (`px-3` / `py-2`) and `MiddleLeft` as its `TextAlign`, so
/// [`Label::band`](crate::display::Label::band) lands on exactly the rectangle
/// [`crate::text::content_rect`] gives the field, vertically centred on the same
/// axis. A test pins the two against each other; without it, clicking into the
/// control would nudge its text by 12 DIP.
///
/// # Divergence from the web, stated
///
/// `@ui/Editable` is a `contenteditable` div that carries `border border-border`
/// **at all times** — it always looks like an input. The desktop's affordance is
/// the click-to-rename one (a row's name that turns into a box), so the resting
/// state here is a bare label and the frame appears on hover. Set
/// [`Editable::always_framed`] to get the web's permanent box back; that is the
/// same chrome either way, only its trigger differs.
#[derive(Clone, Default)]
pub struct Editable {
    field: TextField,
    /// Whether the caret is live. [`Editable::begin_edit`] sets it (a click,
    /// `Enter` or `F2` on a focused label); `Enter` commits and `Escape`
    /// cancels, both clearing it.
    pub editing: bool,
    /// Wear `@ui/Editable`'s permanent `<Input>` frame instead of showing it on
    /// hover only.
    pub always_framed: bool,
    /// The text as it was when the edit began — what `Escape` restores.
    original: String,
    /// The selection's fixed end, in characters. The replica stores a
    /// selection as `start + length` and cannot say which end the caret is
    /// on; `Shift+←` needs to know, so the anchor is kept here and the
    /// replica's selection is re-derived from it after every move.
    anchor: i32,
    /// The caret, in characters (the selection's moving end).
    caret: i32,
}

/// What a keystroke did to an [`Editable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKey {
    /// Not a key the field uses.
    Ignored,
    /// The text changed.
    Edited,
    /// Only the caret or the selection moved (or text was copied).
    Moved,
    /// Editing started (`Enter` / `F2` on the resting label).
    Started,
    /// `Enter`: the edit is kept and the field is back at rest.
    Committed,
    /// `Escape`: the text is back to what it was and the field is at rest.
    Cancelled,
}

impl EditKey {
    /// Whether the key was used.
    pub fn handled(self) -> bool {
        self != EditKey::Ignored
    }
}

/// Number of characters in `s` as the replica counts them.
fn char_count(s: &str) -> i32 {
    s.chars().count() as i32
}

/// The character index of the start of the word before `at` — `Ctrl+←`:
/// skip the spaces, then the word.
fn word_start_before(s: &str, at: i32) -> i32 {
    let chars: Vec<char> = s.chars().collect();
    let mut i = at.clamp(0, chars.len() as i32) as usize;
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    i as i32
}

/// The character index of the end of the word after `at` — `Ctrl+→`: skip
/// the word, then the spaces (Windows lands on the next word's start).
fn word_end_after(s: &str, at: i32) -> i32 {
    let chars: Vec<char> = s.chars().collect();
    let mut i = at.clamp(0, chars.len() as i32) as usize;
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i as i32
}

/// A pasted or typed string made fit for a single-line field: line breaks
/// become spaces, other control characters are dropped.
fn single_line(s: &str) -> String {
    s.replace("\r\n", " ")
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect()
}

impl Deref for Editable {
    type Target = TextField;
    fn deref(&self) -> &TextField {
        &self.field
    }
}
impl DerefMut for Editable {
    fn deref_mut(&mut self) -> &mut TextField {
        &mut self.field
    }
}

impl Editable {
    pub fn new() -> Self {
        Self::default()
    }

    /// The rest-state label, configured so its band is the field's content
    /// rectangle. Built on demand — it holds no text of its own between calls.
    ///
    /// `px-3` / `py-2` ride in the **replica's** `Padding`, which is what
    /// [`crate::display::content_rect`] already deflates by, so this layer adds
    /// no geometry rule of its own.
    pub fn label(&self) -> Label {
        let shown = if self.field.text().is_empty() {
            self.field.placeholder_text.clone()
        } else {
            self.field.display()
        };
        let mut l = Label::new(shown).role(Role::Body).align(ContentAlignment::MiddleLeft);
        l.padding = Padding::new(space::MD, space::SM, space::MD, space::SM);
        // `truncate` on the web's own label surfaces; a name too long for the
        // row ellipsises instead of spilling into the next column.
        l.auto_ellipsis = true;
        l
    }

    /// Whether the resting frame is on screen for this state: always when
    /// framed, on hover, and on a keyboard focus (so the ring has a box).
    pub fn shows_frame(&self, state: WidgetState) -> bool {
        self.always_framed || state.hot || state.show_focus_ring()
    }

    /// The state handed to the inner field while editing: the caret is only
    /// live on a focused field, so `editing` implies focus — and a text field
    /// always shows its ring (`FocusOpts::TEXT`).
    fn edit_state(&self, state: WidgetState) -> WidgetState {
        state.focused(true).focus_visible(true)
    }

    // ── Editing ────────────────────────────────────────────────────────────

    /// Enters edit mode with the whole text selected — the rename idiom (the
    /// web seeds the box once and lets the user type over it).
    pub fn begin_edit(&mut self) {
        if !self.field.enabled {
            return;
        }
        self.editing = true;
        self.original = self.field.text().to_string();
        self.anchor = 0;
        self.caret = char_count(self.field.text());
        self.sync_selection();
    }

    /// Leaves edit mode keeping the text. Returns whether it changed.
    pub fn commit_edit(&mut self) -> bool {
        self.editing = false;
        self.field.text() != self.original
    }

    /// Leaves edit mode restoring the text the edit started from.
    pub fn cancel_edit(&mut self) {
        let original = std::mem::take(&mut self.original);
        self.field.set_text(&original);
        self.editing = false;
    }

    /// Pushes `anchor` / `caret` into the replica's `SelectionStart` /
    /// `SelectionLength`.
    fn sync_selection(&mut self) {
        let n = char_count(self.field.text());
        self.anchor = self.anchor.clamp(0, n);
        self.caret = self.caret.clamp(0, n);
        let start = self.anchor.min(self.caret);
        self.field.select(start, (self.caret - self.anchor).abs());
    }

    /// Re-reads `anchor` / `caret` from the replica when someone changed the
    /// selection behind this layer's back (`set_text`, `select`…).
    fn adopt_selection(&mut self) {
        let (s, l) = (self.field.selection_start(), self.field.selection_length());
        let (a, c) = (self.anchor.min(self.caret), (self.caret - self.anchor).abs());
        if (a, c) != (s, l) {
            self.anchor = s;
            self.caret = s + l;
        }
    }

    /// The caret position, in characters.
    pub fn caret(&self) -> i32 {
        self.caret
    }

    /// Types `text` over the selection (the replica's `SelectedText` setter,
    /// which honours `MaxLength`). Refused when read-only.
    pub fn type_text(&mut self, text: &str) -> bool {
        let text = single_line(text);
        if text.is_empty() || self.field.read_only || !self.editing {
            return false;
        }
        self.adopt_selection();
        self.field.set_selected_text(&text);
        self.caret = self.field.selection_start();
        self.anchor = self.caret;
        true
    }

    /// Moves the caret to `to`, extending the selection when `extend`.
    fn move_caret(&mut self, to: i32, extend: bool) {
        self.caret = to;
        if !extend {
            self.anchor = to;
        }
        self.sync_selection();
    }

    /// Deletes the selection, or the range `from..to` when nothing is selected.
    fn delete_range(&mut self, from: i32, to: i32) -> bool {
        if self.field.read_only {
            return false;
        }
        if self.field.selection_length() == 0 {
            let (a, b) = (from.min(to), from.max(to));
            if a == b {
                return false;
            }
            self.field.select(a, b - a);
        }
        self.field.set_selected_text("");
        self.caret = self.field.selection_start();
        self.anchor = self.caret;
        true
    }

    /// One keystroke, the way a single-line `<input>` / `contenteditable`
    /// handles it on Windows: `←` `→` `Home` `End` (with `Shift` to select,
    /// `Ctrl` to jump a word), `Backspace` / `Delete` (with `Ctrl`: a word),
    /// `Ctrl+A` `Ctrl+C` `Ctrl+X` `Ctrl+V`, `Enter` commits and `Escape`
    /// cancels. At rest, `Enter` or `F2` starts editing.
    pub fn key_down(&mut self, key: u16, mods: Modifiers) -> EditKey {
        if !self.field.enabled {
            return EditKey::Ignored;
        }
        if !self.editing {
            if (key == vk::ENTER || key == vk::F2) && mods.is_none() {
                self.begin_edit();
                return EditKey::Started;
            }
            return EditKey::Ignored;
        }
        self.adopt_selection();
        let text = self.field.text().to_string();
        let n = char_count(&text);
        let shift = mods.shift;
        let ctrl = mods.ctrl;
        if mods.alt {
            return EditKey::Ignored;
        }
        let (lo, hi) = (self.anchor.min(self.caret), self.anchor.max(self.caret));
        match key {
            vk::LEFT => {
                let to = if ctrl {
                    word_start_before(&text, self.caret)
                } else if !shift && lo != hi {
                    lo
                } else {
                    self.caret - 1
                };
                self.move_caret(to, shift);
                EditKey::Moved
            }
            vk::RIGHT => {
                let to = if ctrl {
                    word_end_after(&text, self.caret)
                } else if !shift && lo != hi {
                    hi
                } else {
                    self.caret + 1
                };
                self.move_caret(to, shift);
                EditKey::Moved
            }
            vk::HOME => {
                self.move_caret(0, shift);
                EditKey::Moved
            }
            vk::END => {
                self.move_caret(n, shift);
                EditKey::Moved
            }
            vk::BACK => {
                let from = if ctrl { word_start_before(&text, self.caret) } else { self.caret - 1 };
                if self.delete_range(from.max(0), self.caret) { EditKey::Edited } else { EditKey::Moved }
            }
            vk::DELETE => {
                let to = if ctrl { word_end_after(&text, self.caret) } else { self.caret + 1 };
                if self.delete_range(self.caret, to.min(n)) { EditKey::Edited } else { EditKey::Moved }
            }
            k if ctrl && !shift && k == vk::letter('a') => {
                self.anchor = 0;
                self.caret = n;
                self.sync_selection();
                EditKey::Moved
            }
            k if ctrl && !shift && (k == vk::letter('c') || k == vk::letter('x')) => {
                let masked = self.field.password_char.is_some() || self.field.use_system_password_char;
                let chosen = self.field.selected_text();
                if masked || chosen.is_empty() {
                    return EditKey::Moved;
                }
                host::set_clipboard_text(&chosen);
                if k == vk::letter('x') && self.delete_range(lo, hi) {
                    return EditKey::Edited;
                }
                EditKey::Moved
            }
            k if ctrl && !shift && k == vk::letter('v') => match host::clipboard_text() {
                Some(t) if self.type_text(&t) => EditKey::Edited,
                _ => EditKey::Moved,
            },
            vk::ENTER if !ctrl && !shift => {
                self.commit_edit();
                EditKey::Committed
            }
            vk::ESCAPE if !ctrl && !shift => {
                self.cancel_edit();
                EditKey::Cancelled
            }
            _ => EditKey::Ignored,
        }
    }

    /// Reads this frame's keys and typed text from the host queue — for a
    /// caller whose field holds the focus. Keys the field does not use stay in
    /// the queue (Tab, Ctrl+Tab…).
    pub fn take_input(&mut self) -> EditKey {
        let editing = self.editing;
        let mut keys: Vec<(u16, Modifiers)> = Vec::new();
        host::consume(|e| match e {
            host::InputEvent::Key { vk: k, down: true, mods, .. } => {
                let wanted = if editing {
                    matches!(
                        *k,
                        vk::LEFT | vk::RIGHT | vk::HOME | vk::END | vk::BACK | vk::DELETE | vk::ENTER | vk::ESCAPE
                    ) || (mods.ctrl
                        && !mods.alt
                        && [vk::letter('a'), vk::letter('c'), vk::letter('x'), vk::letter('v')].contains(k))
                } else {
                    (*k == vk::ENTER || *k == vk::F2) && mods.is_none()
                };
                if wanted {
                    keys.push((*k, *mods));
                }
                wanted
            }
            _ => false,
        });
        let mut result = EditKey::Ignored;
        for (k, m) in keys {
            let r = self.key_down(k, m);
            if r.handled() {
                result = r;
            }
        }
        if self.editing {
            let typed = host::take_text();
            if self.type_text(&typed) {
                result = EditKey::Edited;
            }
        }
        result
    }

    /// The character boundary nearest to `x` in a field painted at `bounds` —
    /// where a click puts the caret. Measured against the text as the field
    /// lays it out (from the content edge); a string wider than the box is
    /// scrolled by the editing view, which this does not model, so a click
    /// there lands at the nearest end.
    pub fn caret_at_x(&self, canvas: &dyn Canvas, bounds: Rect, x: f32) -> i32 {
        let content = self.field.content(bounds);
        let shown = self.field.display();
        let fmt = &canvas.formats().body;
        let full = canvas.measure(&shown, fmt);
        let n = char_count(&shown);
        if full > content.right - content.left {
            return if x < (content.left + content.right) / 2.0 { 0 } else { n };
        }
        let mut prev = 0.0_f32;
        let mut prefix = String::new();
        for (i, ch) in shown.chars().enumerate() {
            prefix.push(ch);
            let w = canvas.measure(&prefix, fmt);
            if x < content.left + (prev + w) / 2.0 {
                return i as i32;
            }
            prev = w;
        }
        n
    }

    /// A click at `x`: starts editing if needed, and places the caret (or,
    /// with `extend`, moves the selection's end) there.
    pub fn click_at(&mut self, canvas: &dyn Canvas, bounds: Rect, x: f32, extend: bool) {
        if !self.editing {
            self.begin_edit();
        }
        self.adopt_selection();
        let at = self.caret_at_x(canvas, bounds, x);
        self.move_caret(at, extend);
    }

    /// A double click: selects the word under `x`.
    pub fn select_word_at(&mut self, canvas: &dyn Canvas, bounds: Rect, x: f32) {
        if !self.editing {
            self.begin_edit();
        }
        let text = self.field.text().to_string();
        let at = self.caret_at_x(canvas, bounds, x);
        let chars: Vec<char> = text.chars().collect();
        let mut a = at.clamp(0, chars.len() as i32) as usize;
        let mut b = a;
        while a > 0 && !chars[a - 1].is_whitespace() {
            a -= 1;
        }
        while b < chars.len() && !chars[b].is_whitespace() {
            b += 1;
        }
        self.anchor = a as i32;
        self.caret = b as i32;
        self.sync_selection();
    }
}

impl Widget for Editable {
    fn model(&self) -> &dyn Control {
        self.field.model()
    }

    /// The field's measurement, unchanged — so a caller laying out an
    /// `Editable` reserves the same box whichever state it is in.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        self.field.measure(canvas)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let dead = state.disabled || !self.field.enabled;
        if self.editing && !dead {
            // The whole of the editing state is the field's: chrome, focus
            // ring, placeholder, caret, selection, scroll.
            self.field.paint(canvas, bounds, self.edit_state(state));
            return;
        }

        let t = canvas.theme();
        if self.shows_frame(state) || dead {
            // `@ui/Editable`: the same surface, border and radius as `<Input>`
            // — `bg-white` (`disabled:bg-surface-2`) and `border-border`.
            let face = if dead { t.surface_2 } else { t.layer_background };
            canvas.fill_rounded(&bounds, radius::SM, &face);
            canvas.stroke_rounded(&bounds, radius::SM, &t.card_stroke);
        }
        // `kb-field-focus`: a label reached with Tab shows the field's ring.
        if state.show_focus_ring() && !dead {
            paint_focus_ring(canvas, bounds, radius::SM);
        }

        let mut label = self.label();
        // `empty:before:text-text-tertiary` for the placeholder,
        // `text-text-primary` otherwise; `disabled:opacity-60` greys the lot,
        // which `Label` already resolves through `Control.Enabled`.
        let showing_placeholder = self.field.text().is_empty();
        label.fore_color = Some(if showing_placeholder { t.text_tertiary } else { t.text_primary });
        label.enabled = !dead;
        canvas.push_clip(&bounds);
        label.paint(canvas, bounds, state);
        canvas.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "Editable"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// FontPicker
// ═════════════════════════════════════════════════════════════════════════════

/// The visual family a font name is filed under — `FontPicker.tsx`'s
/// `classifyFont`, which groups « the way pro pickers (Figma / Google Fonts) do ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontCategory {
    Sans,
    Serif,
    Mono,
    Script,
    Display,
}

impl FontCategory {
    /// `CAT_ORDER`, verbatim.
    pub const ORDER: [FontCategory; 5] =
        [Self::Sans, Self::Serif, Self::Mono, Self::Script, Self::Display];

    /// `CAT_LABEL`, verbatim — and in French, as the web writes it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sans => "Sans Serif",
            Self::Serif => "Serif",
            Self::Mono => "Monospace",
            Self::Script => "Manuscrite",
            Self::Display => "Fantaisie",
        }
    }
}

/// `RE_MONO`, `RE_SCRIPT`, `RE_DISPLAY`, `RE_SERIF` — each web regex is a plain
/// alternation of literals, so it is spelled here as the list it is rather than
/// pulling a regex engine into a paint-time classifier.
const MONO_WORDS: [&str; 18] = [
    "mono", "consol", "courier", "menlo", "monaco", "fixedsys", "terminal", "source code",
    "fira code", "firacode", "jetbrains", "inconsolata", "space mono", "ubuntu mono", "cascadia",
    "hack", "iosevka", "code",
];
const SCRIPT_WORDS: [&str; 19] = [
    "script", "hand", "brush", "comic", "cursive", "calligr", "pacifico", "dancing", "lobster",
    "caveat", "satisfy", "sacramento", "great vibes", "shadows into", "indie flower", "kalam",
    "marck", "allura", "tangerine",
];
const DISPLAY_WORDS: [&str; 19] = [
    "display", "impact", "bebas", "oswald", "anton", "abril", "playbill", "stencil", "bungee",
    "black ops", "fredoka", "lilita", "luckiest", "righteous", "permanent marker", "creepster",
    "monoton", "bangers", "poster",
];
const SERIF_WORDS: [&str; 24] = [
    "serif", "times", "georgia", "garamond", "book antiqua", "palatino", "cambria", "constantia",
    "didot", "bodoni", "minion", "caslon", "merriweather", "playfair", "lora", "crimson",
    "spectral", "slab", "rockwell", "century", "sylfaen", "cardo", "vollkorn", "headline",
];

/// Whether `needle` occurs in `haystack` as a whole word — the `\b…\b` of
/// `/\bsans\b/` and `/\bcode\b/`, which is why « Cascadia Code » is monospace
/// and « Bookman » is not « book antiqua ».
fn contains_word(haystack: &str, needle: &str) -> bool {
    let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
    let mut from = 0usize;
    while let Some(at) = haystack[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        if boundary(haystack[..start].chars().next_back()) && boundary(haystack[end..].chars().next())
        {
            return true;
        }
        from = start + needle.len().max(1);
        if from >= haystack.len() {
            break;
        }
    }
    false
}

/// `classifyFont`, move for move — including the order, where monospace,
/// script and display win over the broad serif test, and `\bsans\b` is asked
/// before it.
/// Which category a family belongs to.
///
/// Memoised per thread: the classification of a family is a pure function of its
/// name (it never changes while the process runs), and it is otherwise recomputed
/// for every one of the ~260 installed families every time the open list is
/// built — dozens of times per paint. The UI is single-threaded, so a
/// `thread_local` map needs no lock.
pub fn classify_font(name: &str) -> FontCategory {
    thread_local! {
        static CACHE: RefCell<std::collections::HashMap<String, FontCategory>> =
            RefCell::new(std::collections::HashMap::new());
    }
    CACHE.with(|c| {
        if let Some(&cat) = c.borrow().get(name) {
            return cat;
        }
        let cat = classify_font_uncached(name);
        c.borrow_mut().insert(name.to_string(), cat);
        cat
    })
}

fn classify_font_uncached(name: &str) -> FontCategory {
    let n = name.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if MONO_WORDS.iter().any(|w| if *w == "code" { contains_word(&n, w) } else { n.contains(w) }) {
        return FontCategory::Mono;
    }
    if any(&SCRIPT_WORDS) || n.contains("segoe script") || n.contains("bradley")
        || n.contains("lucida handwriting")
    {
        return FontCategory::Script;
    }
    if any(&DISPLAY_WORDS) {
        return FontCategory::Display;
    }
    if contains_word(&n, "sans") {
        return FontCategory::Sans;
    }
    if any(&SERIF_WORDS) {
        return FontCategory::Serif;
    }
    FontCategory::Sans
}

/// The memoised open list plus the key it was built for (the query, the
/// family count and the recent list, joined). See [`FontPicker::rows_cache`].
type RowsCache = RefCell<Option<(String, Vec<FontRow>)>>;

/// One line of an open [`FontPicker`] — the web's `Row`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontRow {
    /// A category (or « Récentes ») header. Never a target.
    Header(&'static str),
    /// A selectable family, and its index in the flat option list the keyboard
    /// highlight moves over.
    Option { font: String, index: usize },
}

impl FontRow {
    /// The row's own height: [`font_metrics::HEADER_H`] or
    /// [`font_metrics::ROW_H`].
    pub fn height(&self) -> f32 {
        match self {
            Self::Header(_) => font_metrics::HEADER_H,
            Self::Option { .. } => font_metrics::ROW_H,
        }
    }
}

/// A font-family selector with a search box and category headers —
/// `@ui/FontPicker`.
///
/// The **model is the toolkit's**: the families are the
/// [`kubuno_desktop_controls::lists::ComboBox`]'s `Items` and the chosen one is its
/// `SelectedIndex`, both reached through [`Deref`]. What this adds is what .NET
/// has no concept of: the search `query`, the `recent` list pinned at the top,
/// the glyph `sample`, and `open` / `hot_index`.
///
/// The families themselves come from [`system_font_families`], i.e. from
/// [`kubuno_drive_desktop_app_controls::Renderer::system_font_families`] — the installed
/// families, not a curated list.
///
/// # The preview, and what a [`Canvas`] cannot do
///
/// On the web each option is drawn **in its own face**
/// (`fontFamily: cssFamily(row.font)`). That is not reachable from here, and
/// pretending otherwise would be the only dishonest line in this crate:
/// [`Canvas`] exposes the shared [`kubuno_drive_desktop_app_controls::TextFormats`] and
/// nothing that builds an `IDWriteTextFormat`, so a primitive can pick *which*
/// of the twelve shared formats to draw with, never *which family*. Adding such
/// a method is a change to `canvas.rs`, which this family does not own.
///
/// What ships instead: every family name is drawn in the **UI face**, and the
/// sample column is laid out and reserved exactly as the web lays it out — so
/// [`FontPicker::sample_rect`] hands a host that *does* hold a DirectWrite
/// factory the precise rectangle to overpaint the preview into, and the popup
/// needs no relayout when it does. The trigger is unaffected: the web already
/// paints it in the UI face on purpose (« a serif/script value made the field
/// itself hard to read »).
#[derive(Clone, Default)]
pub struct FontPicker {
    inner: replica::ComboBox,
    /// Families pinned at the top under a « Récentes » header.
    pub recent: Vec<String>,
    /// The search box's content. Empty shows the grouped list; non-empty shows
    /// a flat, prefix-first ranking, as `FontPicker.tsx` does.
    pub query: String,
    /// `sampleText = 'AaBbCc'`; empty hides the column.
    pub sample: String,
    /// Shown greyed when nothing is selected — « e.g. a mixed-font selection ».
    pub placeholder: String,
    pub variant: DropdownVariant,
    pub height: f32,
    pub open: bool,
    /// The highlighted option, as an index into the FLAT option list (not into
    /// [`FontPicker::rows`]).
    pub hot_index: Option<usize>,
    /// The list's scroll offset, in DIP (`maxHeight: 340; overflowY: auto`).
    pub scroll_y: f32,
    /// Whether the search box draws its caret this frame — the host's blink
    /// phase ([`crate::focus::caret_visible`]); the search box holds the
    /// keyboard while the popup is open (`searchRef.current?.focus()`).
    pub search_caret: bool,
    /// Squares one edge to glue the trigger to a neighbour (`buttonStyle`):
    /// `FontSizeField` squares the picker's right edge.
    pub joined: SquaredEdge,
    /// Memoised [`FontPicker::rows`]. Building the grouped list classifies every
    /// installed family (~260 of them) against three word lists, and a single
    /// paint asks for the rows dozens of times (measure, hit-test, and once per
    /// visible row). Recomputing each time made the open picker take ~1 s per
    /// frame in a debug build — a frozen window, since the host repaints on
    /// every pointer move. The cache is keyed on what the rows depend on
    /// (`query`, the families and the recent list), so it is rebuilt exactly
    /// when the list would actually differ.
    rows_cache: RowsCache,
    /// The popup's `width: max-content`, measured once per family count.
    natural_w: Cell<Option<(usize, f32)>>,
    /// Where [`FontPicker::place_popup`] put the popup (`None` = under the
    /// trigger).
    placed: Option<Rect>,
}

impl Deref for FontPicker {
    type Target = replica::ComboBox;
    fn deref(&self) -> &replica::ComboBox {
        &self.inner
    }
}
impl DerefMut for FontPicker {
    fn deref_mut(&mut self) -> &mut replica::ComboBox {
        &mut self.inner
    }
}

/// The empty row list a cache miss borrows.
static NO_ROWS: Vec<FontRow> = Vec::new();

/// One editable line of text — the search box and the size field. The text is
/// left-aligned at `rect.left` and scrolled left just enough to keep the caret
/// in view, clipped to `rect`; the selection `(a, b)` (character indices) is
/// the browser's highlight, `--color-primary` at 35 % like
/// `edit_box::draw_text`, and the caret is 1 DIP of `text_primary` over the
/// 20 DIP line box. `caret` is `None` during the blink's off phase.
#[allow(clippy::too_many_arguments)]
fn paint_edit_line(
    c: &dyn Canvas,
    rect: Rect,
    text: &str,
    fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    ink: &D2D1_COLOR_F,
    selection: (usize, usize),
    caret: Option<usize>,
    caret_at: usize,
) {
    let t = c.theme();
    let byte = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(i, _)| i);
    let inner_w = rect.right - rect.left;
    let caret_w = c.measure(&text[..byte(caret_at)], fmt);
    let origin = rect.left - (caret_w - inner_w + 1.0).max(0.0);
    let cy = (rect.top + rect.bottom) / 2.0;
    let line = Role::Body.line_height();
    let (top, bottom) = ((cy - line / 2.0).max(rect.top), (cy + line / 2.0).min(rect.bottom));
    c.push_clip(&rect);
    let (a, b) = (selection.0.min(selection.1), selection.0.max(selection.1));
    if a != b {
        let x0 = origin + c.measure(&text[..byte(a)], fmt);
        let x1 = origin + c.measure(&text[..byte(b)], fmt);
        let band = D2D1_COLOR_F { a: EDIT_SELECTION_ALPHA, ..t.accent };
        c.fill_rounded(&Rect::new(x0, top, x1, bottom), 0.0, &band);
    }
    let w = c.measure(text, fmt).max(inner_w);
    c.text(text, &Rect::new(origin, rect.top, origin + w + 1.0, rect.bottom), fmt, ink, false);
    if let Some(at) = caret {
        let x = origin + c.measure(&text[..byte(at)], fmt);
        c.fill_rounded(&Rect::new(x, top, x + 1.0, bottom), 0.0, &t.text_primary);
    }
    c.pop_clip();
}

/// The browser's selection highlight over an input — `--color-primary` at
/// 35 %, the value `edit_box::draw_text` already paints.
const EDIT_SELECTION_ALPHA: f32 = 0.35;

impl FontPicker {
    /// An empty picker with the web's defaults.
    pub fn new() -> Self {
        Self {
            sample: "AaBbCc".to_string(),
            height: font_metrics::HEIGHT,
            ..Self::default()
        }
    }

    /// A picker over the families this machine actually has installed.
    pub fn system() -> Self {
        Self::with_fonts(system_font_families().iter().map(String::as_str))
    }

    /// A picker over a caller-supplied list — what a test and a fixed toolbar
    /// want.
    pub fn with_fonts<'a>(fonts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut p = Self::new();
        for f in fonts {
            p.inner.add_item(f);
        }
        p
    }

    /// The selected family, or the placeholder.
    pub fn display_text(&self) -> &str {
        self.inner.selected_item().unwrap_or(&self.placeholder)
    }

    /// What the row list depends on, as one string.
    fn rows_key(&self) -> String {
        format!("{}\u{1}{}\u{1}{}", self.query, self.inner.items.len(), self.recent.join("\u{2}"))
    }

    /// The memoised rows, borrowed — no clone of ~260 names per call.
    fn rows_ref(&self) -> std::cell::Ref<'_, Vec<FontRow>> {
        let key = self.rows_key();
        let fresh = matches!(&*self.rows_cache.borrow(), Some((k, _)) if *k == key);
        if !fresh {
            let rows = self.compute_rows();
            *self.rows_cache.borrow_mut() = Some((key, rows));
        }
        std::cell::Ref::map(self.rows_cache.borrow(), |c| c.as_ref().map_or(&NO_ROWS, |(_, r)| r))
    }

    /// The rows of the open popup — headers and options, in the web's order:
    /// « Récentes » first, then the five categories, each sorted; or, while
    /// searching, one flat list with prefix matches first. Memoised — see
    /// [`FontPicker::rows_cache`].
    ///
    /// `dedupeFontFamilies` is the web's own de-duplication of the list it is
    /// handed; here the replica's `Items` are the list, so the same rule is
    /// applied case-insensitively as they are walked.
    pub fn rows(&self) -> Vec<FontRow> {
        self.rows_ref().clone()
    }

    fn compute_rows(&self) -> Vec<FontRow> {
        let mut rows = Vec::new();
        // A HashSet, not a Vec: the de-dup is checked once per family, and a
        // linear `Vec::contains` inside that loop is O(n²). Over the ~260 system
        // families, in a debug build, and recomputed on every repaint (the host
        // repaints on every pointer move), that O(n²) froze the window. `insert`
        // returning false IS the "already seen" test, in O(1).
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut index = 0usize;
        let mut push = |rows: &mut Vec<FontRow>, seen: &mut std::collections::HashSet<String>, font: &str| {
            if !seen.insert(font.to_lowercase()) {
                return;
            }
            rows.push(FontRow::Option { font: font.to_string(), index });
            index += 1;
        };

        let q = self.query.trim().to_lowercase();
        if !q.is_empty() {
            let mut hits: Vec<&String> =
                self.inner.items.iter().filter(|f| f.to_lowercase().contains(&q)).collect();
            // `sort` on `startsWith` — a stable sort, so ties keep their order.
            hits.sort_by_key(|f| !f.to_lowercase().starts_with(&q));
            for f in hits {
                push(&mut rows, &mut seen, f);
            }
            return rows;
        }

        let is_recent = |f: &str| self.recent.iter().any(|r| r.eq_ignore_ascii_case(f));
        let recent: Vec<&String> = self.inner.items.iter().filter(|f| is_recent(f)).collect();
        if !recent.is_empty() {
            rows.push(FontRow::Header("Récentes"));
            for f in recent {
                push(&mut rows, &mut seen, f);
            }
        }
        for cat in FontCategory::ORDER {
            let mut group: Vec<&String> = self
                .inner
                .items
                .iter()
                .filter(|f| !is_recent(f) && classify_font(f) == cat)
                .collect();
            group.sort_by_key(|f| f.to_lowercase());
            if group.is_empty() {
                continue;
            }
            rows.push(FontRow::Header(cat.label()));
            for f in group {
                push(&mut rows, &mut seen, f);
            }
        }
        rows
    }

    /// The flat list of selectable families, in row order — what `hot_index`
    /// and the arrow keys move over.
    pub fn options(&self) -> Vec<String> {
        self.rows_ref()
            .iter()
            .filter_map(|r| match r {
                FontRow::Option { font, .. } => Some(font.clone()),
                FontRow::Header(_) => None,
            })
            .collect()
    }

    /// How many options the open list offers.
    pub fn option_count(&self) -> usize {
        self.rows_ref().iter().filter(|r| matches!(r, FontRow::Option { .. })).count()
    }

    pub fn trigger_rect(&self, bounds: Rect) -> Rect {
        Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + self.height)
    }

    /// The whole list's height — every row plus `padding: '4px 0'` — or the
    /// empty state's.
    pub fn content_height(&self) -> f32 {
        let rows = self.rows_ref();
        if rows.is_empty() {
            // The empty state is a row of its own: `px-4 py-6` around a meta line.
            return 2.0 * font_metrics::EMPTY_PAD_V + Role::Meta.line_height();
        }
        rows.iter().map(FontRow::height).sum::<f32>() + 2.0 * font_metrics::LIST_PAD_V
    }

    /// The height the row list takes, capped at `maxHeight: 340`.
    pub fn list_height(&self) -> f32 {
        self.content_height().min(font_metrics::LIST_MAX)
    }

    /// Whether the list overflows its 340 and scrolls.
    pub fn scrolls(&self) -> bool {
        self.content_height() > font_metrics::LIST_MAX
    }

    /// The furthest [`FontPicker::scroll_y`] can go.
    pub fn max_scroll(&self) -> f32 {
        (self.content_height() - self.list_height()).max(0.0)
    }

    /// Scrolls the list by `dy` DIP (positive = down), clamped.
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, self.max_scroll());
    }

    /// The top of option `flat` in list coordinates (0 = the list's first
    /// pixel, before scrolling), and its height.
    fn option_offset(&self, flat: usize) -> Option<(f32, f32)> {
        let mut y = font_metrics::LIST_PAD_V;
        for row in self.rows_ref().iter() {
            if matches!(row, FontRow::Option { index, .. } if *index == flat) {
                return Some((y, row.height()));
            }
            y += row.height();
        }
        None
    }

    /// `scrollIntoView({ block: 'nearest' })` for option `flat`.
    pub fn ensure_visible(&mut self, flat: usize) {
        let Some((y, h)) = self.option_offset(flat) else { return };
        let view = self.list_height();
        if y < self.scroll_y {
            self.scroll_y = y;
        } else if y + h > self.scroll_y + view {
            self.scroll_y = y + h - view;
        }
        // Within the list's own `padding: 4px 0` of an end, show the padding
        // too: the first and last options never sit flush against the edge.
        let max = self.max_scroll();
        if self.scroll_y <= font_metrics::LIST_PAD_V {
            self.scroll_y = 0.0;
        } else if self.scroll_y >= max - font_metrics::LIST_PAD_V {
            self.scroll_y = max;
        }
        self.scroll_y = self.scroll_y.clamp(0.0, max);
    }

    /// `scrollIntoView({ block: 'center' })` for option `flat` — what opening
    /// does with the current font.
    pub fn center_on(&mut self, flat: usize) {
        let Some((y, h)) = self.option_offset(flat) else { return };
        self.scroll_y = (y + h / 2.0 - self.list_height() / 2.0).clamp(0.0, self.max_scroll());
    }

    /// The popup's `width: max-content`: the check gutter, the widest name,
    /// the sample and the paddings (and the bar when the list scrolls). The
    /// names are measured once per family count — a system list is ~260 of
    /// them.
    pub fn natural_popup_width(&self, canvas: &dyn Canvas) -> f32 {
        let n = self.inner.items.len();
        if let Some((k, w)) = self.natural_w.get() {
            if k == n {
                return w;
            }
        }
        let f = canvas.formats();
        let widest = self.inner.items.iter().fold(0.0_f32, |w, s| w.max(canvas.measure(s, &f.body)));
        let sample = if self.sample.is_empty() {
            0.0
        } else {
            font_metrics::CELL_GAP
                + canvas.measure(&self.sample, &f.body).min(font_metrics::SAMPLE_MAX)
        };
        let w = font_metrics::ROW_PAD_L
            + font_metrics::CHECK_CELL
            + font_metrics::CELL_GAP
            + widest
            + sample
            + font_metrics::ROW_PAD_R
            + control::SCROLLBAR_THUMB;
        self.natural_w.set(Some((n, w)));
        w
    }

    /// The popup: the search row on top of the list.
    ///
    /// `top: r.bottom + 4`, `left: r.left`,
    /// `minWidth: Math.max(248, r.width)`, `maxWidth: 360` — or, after
    /// [`FontPicker::place_popup`], the placed rectangle.
    pub fn popup_rect(&self, bounds: Rect) -> Rect {
        if let Some(p) = self.placed {
            return Rect::new(p.left, p.top, p.right, p.top + font_metrics::SEARCH_H + self.list_height());
        }
        let trigger = self.trigger_rect(bounds);
        let top = trigger.bottom + font_metrics::OFFSET;
        let width = (trigger.right - trigger.left)
            .clamp(font_metrics::MIN_WIDTH, font_metrics::MAX_WIDTH);
        Rect::new(
            trigger.left,
            top,
            trigger.left + width,
            top + font_metrics::SEARCH_H + self.list_height(),
        )
    }

    /// The web's `useLayoutEffect` placement: `width: max-content` between
    /// `max(248, trigger)` and 360, under the trigger at +4, pulled back
    /// inside `area` (the monitor work area, in the same space as `bounds`) and
    /// SLID up — not flipped — when it would run off the bottom.
    pub fn place_popup(&mut self, canvas: &dyn Canvas, bounds: Rect, area: Rect) {
        let trigger = self.trigger_rect(bounds);
        let min = font_metrics::MIN_WIDTH.max(trigger.right - trigger.left);
        let width = self.natural_popup_width(canvas).min(font_metrics::MAX_WIDTH).max(min);
        let height = font_metrics::SEARCH_H + self.list_height();
        let p = place_floating(trigger, width, height, font_metrics::OFFSET, area, Overflow::Slide);
        self.placed = Some(p);
    }

    /// Forgets a [`FontPicker::place_popup`] placement.
    pub fn clear_placement(&mut self) {
        self.placed = None;
    }

    /// The rectangle a `host::popup` hosting the open picker must cover: the
    /// panel plus its shadow.
    pub fn popup_paint_bounds(&self, bounds: Rect) -> Rect {
        inflate(self.popup_rect(bounds), shadow_margin())
    }

    /// The search row of the open popup.
    pub fn search_rect(&self, bounds: Rect) -> Rect {
        let p = self.popup_rect(bounds);
        Rect::new(p.left, p.top, p.right, p.top + font_metrics::SEARCH_H)
    }

    /// The scrolled list's window inside the panel `panel`.
    pub fn list_rect_in(&self, panel: Rect) -> Rect {
        Rect::new(panel.left, panel.top + font_metrics::SEARCH_H, panel.right, panel.bottom)
    }

    /// The « Effacer » button of the search row — only while a query is typed
    /// (`text-xs px-1.5 py-0.5 rounded`, at the row's right padding).
    pub fn clear_rect_in(&self, canvas: &dyn Canvas, panel: Rect) -> Option<Rect> {
        if self.query.is_empty() {
            return None;
        }
        let w = canvas.measure(CLEAR_LABEL, &canvas.formats().caption) + 2.0 * font_metrics::CLEAR_PAD_X;
        let right = panel.right - font_metrics::SEARCH_PAD;
        let cy = panel.top + font_metrics::SEARCH_H / 2.0;
        let h = Role::Meta.line_height() + 2.0 * font_metrics::CLEAR_PAD_Y;
        Some(Rect::new(right - w, cy - h / 2.0, right, cy + h / 2.0))
    }

    /// Where row `k` of [`FontPicker::rows`] is drawn inside the panel `panel`
    /// — `None` when it is scrolled wholly out of the list's window.
    pub fn row_rect_in(&self, panel: Rect, k: usize) -> Option<Rect> {
        let list = self.list_rect_in(panel);
        let bar = if self.scrolls() { control::SCROLLBAR_THUMB } else { 0.0 };
        let rows = self.rows_ref();
        let mut y = list.top + font_metrics::LIST_PAD_V - self.scroll_y;
        for (i, row) in rows.iter().enumerate() {
            let h = row.height();
            if i == k {
                if y + h <= list.top || y >= list.bottom {
                    return None;
                }
                return Some(Rect::new(panel.left, y, panel.right - bar, y + h));
            }
            y += h;
        }
        None
    }

    /// Where row `k` of [`FontPicker::rows`] is drawn, or `None` once it is
    /// scrolled out of the list's `maxHeight` window.
    pub fn row_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        self.row_rect_in(self.popup_rect(bounds), i)
    }

    /// Which **option** `(x, y)` lands on inside the panel `panel`, as a flat
    /// option index — headers answer `None`, being labels rather than targets.
    pub fn item_at_in(&self, panel: Rect, x: f32, y: f32) -> Option<usize> {
        if !self.open || !self.list_rect_in(panel).contains(x, y) {
            return None;
        }
        let rows = self.rows_ref().clone();
        rows.iter().enumerate().find_map(|(k, row)| {
            let FontRow::Option { index, .. } = row else { return None };
            self.row_rect_in(panel, k).filter(|r| r.contains(x, y)).map(|_| *index)
        })
    }

    /// Which **option** `(x, y)` lands on, as a flat option index.
    pub fn item_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        self.item_at_in(self.popup_rect(bounds), x, y)
    }

    /// Where the **glyph sample** of the row drawn at `row` goes.
    ///
    /// Published so a host holding a DirectWrite factory can paint the preview
    /// in the family's own face — the one thing a [`Canvas`] cannot do (see the
    /// type's documentation). `None` when the sample column is switched off.
    pub fn sample_rect(&self, row: Rect) -> Option<Rect> {
        if self.sample.is_empty() {
            return None;
        }
        let right = row.right - font_metrics::ROW_PAD_R;
        Some(Rect::new((right - font_metrics::SAMPLE_MAX).max(row.left), row.top, right, row.bottom))
    }

    /// Where a family's NAME goes inside its row.
    pub fn name_rect(&self, row: Rect) -> Rect {
        let left = row.left
            + font_metrics::ROW_PAD_L
            + font_metrics::CHECK_CELL
            + font_metrics::CELL_GAP;
        let right = match self.sample_rect(row) {
            Some(s) => s.left - font_metrics::CELL_GAP,
            None => row.right - font_metrics::ROW_PAD_R,
        };
        Rect::new(left, row.top, right.max(left), row.bottom)
    }

    // ── Keyboard ───────────────────────────────────────────────────────────

    /// `openMenu`: clears the search, highlights the current font (or the
    /// first) and centres it in view. Refused when disabled.
    pub fn open_menu(&mut self) {
        if !self.inner.control().enabled {
            return;
        }
        self.open = true;
        self.query.clear();
        let current = self.inner.selected_item().map(str::to_string);
        let idx = current
            .and_then(|v| self.options().iter().position(|o| *o == v))
            .unwrap_or(0);
        self.hot_index = (self.option_count() > 0).then_some(idx);
        self.scroll_y = 0.0;
        self.center_on(idx);
    }

    /// Closes the popup (Escape, a click outside, a choice).
    pub fn close(&mut self) {
        self.open = false;
        self.hot_index = None;
        self.query.clear();
        self.scroll_y = 0.0;
    }

    /// `choose(f)`: selects option `flat` of the open list through the
    /// replica and closes. Returns the chosen family.
    pub fn choose(&mut self, flat: usize) -> Option<String> {
        let font = self.options().get(flat).cloned()?;
        if let Some(pos) = self.inner.items.iter().position(|i| *i == font) {
            self.inner.set_selected_index(pos as i32);
        }
        self.close();
        Some(font)
    }

    /// Characters typed into the search box: `setQuery(…)`, `setHi(0)`.
    pub fn type_text(&mut self, text: &str) -> ListKey {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if !self.open || text.is_empty() {
            return ListKey::Ignored;
        }
        self.query.push_str(&text);
        self.after_query_change();
        ListKey::Moved
    }

    /// The « Effacer » button: `setQuery('')`, `setHi(0)`.
    pub fn clear_query(&mut self) {
        self.query.clear();
        self.after_query_change();
    }

    fn after_query_change(&mut self) {
        self.scroll_y = 0.0;
        self.hot_index = (self.option_count() > 0).then_some(0);
    }

    /// One keystroke. Closed: the trigger is a button — `Enter` / `Space`
    /// (and `Alt+↓`) open it. Open, the search box has the focus and `onKey`
    /// runs: `↑` `↓` `Home` `End` `PageUp` `PageDown` (±8) move the highlight,
    /// `Enter` chooses, `Escape` closes; `Backspace` edits the query
    /// (`Ctrl+Backspace` clears it).
    pub fn key_down(&mut self, key: u16, mods: Modifiers) -> ListKey {
        if !self.inner.control().enabled {
            return ListKey::Ignored;
        }
        let plain = mods.matches(Modifiers::NONE);
        if !self.open {
            let opens = (plain && (key == vk::ENTER || key == vk::SPACE))
                || (mods.matches(Modifiers::ALT) && key == vk::DOWN);
            if opens {
                self.open_menu();
                return ListKey::Opened;
            }
            return ListKey::Ignored;
        }
        let n = self.option_count();
        let hi = self.hot_index;
        let moved = match key {
            vk::DOWN if plain => step_index(hi, 1, n),
            vk::UP if plain => step_index(hi, -1, n),
            vk::HOME if plain => step_index(None, 1, n),
            vk::END if plain => step_index(None, -1, n),
            vk::PAGE_DOWN if plain => step_index(hi, float_metrics::FONT_PAGE as i64, n),
            vk::PAGE_UP if plain => step_index(hi, -(float_metrics::FONT_PAGE as i64), n),
            vk::ENTER if plain => {
                return match hi.and_then(|i| self.choose(i)) {
                    Some(_) => ListKey::Committed(hi.unwrap_or_default()),
                    None => ListKey::Moved,
                };
            }
            vk::ESCAPE if plain => {
                self.close();
                return ListKey::Closed;
            }
            vk::UP if mods.matches(Modifiers::ALT) => {
                self.close();
                return ListKey::Closed;
            }
            vk::BACK if plain || mods.matches(Modifiers::CTRL) => {
                if mods.ctrl {
                    self.query.clear();
                } else {
                    self.query.pop();
                }
                self.after_query_change();
                return ListKey::Moved;
            }
            _ => return ListKey::Ignored,
        };
        self.hot_index = moved;
        if let Some(i) = moved {
            self.ensure_visible(i);
        }
        // Home / End reach the list's very ends, so the first group's header
        // (or the last row's padding) is shown with the option.
        if moved.is_some() && key == vk::HOME {
            self.scroll_y = 0.0;
        } else if moved.is_some() && key == vk::END {
            self.scroll_y = self.max_scroll();
        }
        ListKey::Moved
    }

    /// Reads this frame's keys and typed text from the host queue — for a
    /// caller whose trigger holds the focus. While closed only the opening
    /// keys are taken; while open the search box takes the typing too.
    pub fn take_input(&mut self) -> ListKey {
        let open = self.open;
        let mut keys: Vec<(u16, Modifiers)> = Vec::new();
        host::consume(|e| match e {
            host::InputEvent::Key { vk: k, down: true, mods, .. } => {
                let wanted = if open {
                    matches!(
                        *k,
                        vk::DOWN | vk::UP | vk::HOME | vk::END | vk::PAGE_DOWN | vk::PAGE_UP | vk::ENTER | vk::ESCAPE | vk::BACK
                    ) && !mods.shift
                } else {
                    matches!(*k, vk::ENTER | vk::SPACE | vk::DOWN) && !mods.ctrl && !mods.shift
                };
                if wanted {
                    keys.push((*k, *mods));
                }
                wanted
            }
            _ => false,
        });
        let mut result = ListKey::Ignored;
        for (k, m) in keys {
            let r = self.key_down(k, m);
            if r.handled() {
                result = r;
            }
        }
        if self.open {
            let typed = host::take_text();
            let r = self.type_text(&typed);
            if r.handled() {
                result = r;
            }
        } else {
            // The Space that opened the list also arrives as text: a closed
            // button has no use for it.
            host::consume(|e| matches!(e, host::InputEvent::Text(s) if s.trim().is_empty()));
        }
        result
    }

    // ── Paint ──────────────────────────────────────────────────────────────

    /// Paints the trigger alone — what the owner window draws while the popup
    /// floats.
    pub fn paint_field(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let dead = state.disabled || !self.inner.control().enabled;
        self.paint_trigger(canvas, self.trigger_rect(bounds), state, dead);
    }

    fn paint_trigger(&self, c: &dyn Canvas, trigger: Rect, state: WidgetState, dead: bool) {
        let t = c.theme();
        let f = c.formats();
        let alpha = if dead { dropdown_metrics::DISABLED_ALPHA } else { 1.0 };
        let m = dropdown_metrics::RADIUS;

        // `background: open ? P.active : undefined` (`--color-surface-2`),
        // and the hover handler's `P.hover` (`--color-surface-1`).
        if !dead {
            if self.open {
                paint_box(c, trigger, m, self.joined, Some(&t.surface_2), None);
            } else if state.hot {
                paint_box(c, trigger, m, self.joined, Some(&t.card_background), None);
            }
        }
        // `border: 1px solid ${ghost ? 'transparent' : P.border}`.
        if self.variant == DropdownVariant::Default {
            paint_box(c, trigger, m, self.joined, None, Some(&faded(&t.card_stroke, alpha)));
        }
        if state.show_focus_ring() && !dead {
            paint_focus_ring(c, trigger, m);
        }

        let caret = Rect::new(
            trigger.right - font_metrics::PAD_R - font_metrics::CARET_GAP - font_metrics::CARET,
            trigger.top,
            trigger.right - font_metrics::PAD_R - font_metrics::CARET_GAP,
            trigger.bottom,
        );
        let label = Rect::new(
            trigger.left + font_metrics::PAD_L,
            trigger.top,
            (caret.left - dropdown_metrics::GAP).max(trigger.left + font_metrics::PAD_L),
            trigger.bottom,
        );
        let empty = self.inner.selected_item().is_none();
        let ink = if empty { t.text_tertiary } else { t.text_primary };
        c.push_clip(&trigger);
        c.text_ellipsis(self.display_text(), &label, &f.body, &faded(&ink, alpha));
        paint_caret(c, caret, font_metrics::CARET, &faded(&t.text_secondary, alpha));
        c.pop_clip();
    }

    fn paint_search(&self, c: &dyn Canvas, panel: Rect) {
        let t = c.theme();
        let f = c.formats();
        let row = Rect::new(panel.left, panel.top, panel.right, panel.top + font_metrics::SEARCH_H);
        let glyph = Rect::new(
            row.left + font_metrics::SEARCH_PAD,
            row.top,
            row.left + font_metrics::SEARCH_PAD + font_metrics::SEARCH_GLYPH,
            row.bottom,
        );
        c.vector_icon("Search", &glyph, font_metrics::SEARCH_GLYPH, &t.text_tertiary);
        let clear = self.clear_rect_in(c, panel);
        let right = clear.map_or(row.right - font_metrics::SEARCH_PAD, |r| r.left - font_metrics::SEARCH_GAP);
        let text = Rect::new(glyph.right + font_metrics::SEARCH_GAP, row.top, right.max(glyph.right), row.bottom);
        // `placeholder="Rechercher une police…"` at `fontSize: 11.5`.
        if self.query.is_empty() {
            c.text_ellipsis(SEARCH_PLACEHOLDER, &text, &f.caption, &t.text_tertiary);
            if self.search_caret {
                paint_edit_line(c, text, "", &f.caption, &t.text_primary, (0, 0), Some(0), 0);
            }
        } else {
            let n = self.query.chars().count();
            let caret = self.search_caret.then_some(n);
            paint_edit_line(c, text, &self.query, &f.caption, &t.text_primary, (n, n), caret, n);
        }
        // « Effacer »: `text-xs px-1.5 py-0.5 rounded`, `color: P.sec`.
        if let Some(r) = clear {
            c.text(CLEAR_LABEL, &r, &f.caption, &t.text_secondary, true);
        }
        // `borderBottom: 1px solid ${P.border}` under the search row.
        c.fill_rounded(
            &Rect::new(row.left, row.bottom - control::SEPARATOR, row.right, row.bottom),
            0.0,
            &t.card_stroke,
        );
    }

    /// A family name with the query's first match emphasised — the web's
    /// `highlightMatch` (`color: primary, fontWeight: 600`). When the name does
    /// not fit, the plain ellipsised name is drawn instead: the emphasis must
    /// never push text out of its cell.
    fn paint_name(&self, c: &dyn Canvas, rect: Rect, name: &str) {
        let t = c.theme();
        let f = c.formats();
        let q = self.query.trim();
        let hit = (!q.is_empty())
            .then(|| name.to_lowercase().find(&q.to_lowercase()))
            .flatten()
            .filter(|&i| name.is_char_boundary(i) && name.is_char_boundary(i + q.len()));
        let Some(at) = hit else {
            c.text_ellipsis(name, &rect, &f.body, &t.text_primary);
            return;
        };
        let (before, rest) = name.split_at(at);
        let (matched, after) = rest.split_at(q.len());
        let wb = c.measure(before, &f.body);
        let wm = c.measure(matched, &f.body_strong);
        let wa = c.measure(after, &f.body);
        let avail = rect.right - rect.left;
        // The match itself must be whole; only the tail after it may be
        // ellipsised (the web's `truncate` cuts the end of the whole span).
        let head = wb + wm;
        if head + c.measure("…", &f.body) > avail && head + wa > avail {
            c.text_ellipsis(name, &rect, &f.body, &t.text_primary);
            return;
        }
        let mut x = rect.left;
        for (part, fmt, ink, w) in [
            (before, &f.body, &t.text_primary, wb),
            (matched, &f.body_strong, &t.accent, wm),
        ] {
            if !part.is_empty() {
                c.text(part, &Rect::new(x, rect.top, x + w + 1.0, rect.bottom), fmt, ink, false);
            }
            x += w;
        }
        if !after.is_empty() {
            c.text_ellipsis(after, &Rect::new(x, rect.top, rect.right, rect.bottom), &f.body, &t.text_primary);
        }
    }

    fn paint_rows(&self, c: &dyn Canvas, panel: Rect) {
        let t = c.theme();
        let f = c.formats();
        let list = self.list_rect_in(panel);
        let rows = self.rows_ref().clone();
        c.push_clip(&list);
        for (k, row) in rows.iter().enumerate() {
            let Some(rect) = self.row_rect_in(panel, k) else { continue };
            match row {
                FontRow::Header(label) => {
                    // `fontSize: 11, fontWeight: 600, textTransform: uppercase`
                    // — the caption_strong face, uppercased here because
                    // DirectWrite has no `text-transform`.
                    let r = Rect::new(
                        rect.left + font_metrics::HEADER_PAD_L,
                        rect.top + font_metrics::HEADER_PAD_T,
                        rect.right - font_metrics::HEADER_PAD_L,
                        rect.bottom - font_metrics::HEADER_PAD_B,
                    );
                    c.text_ellipsis(&label.to_uppercase(), &r, &f.caption_strong, &t.text_tertiary);
                }
                FontRow::Option { font, index } => {
                    let selected = self.inner.selected_item() == Some(font.as_str());
                    // `background: row.i === hi ? P.sel : row.font === value ?
                    // P.hover : undefined` — `--color-primary-light` for the
                    // highlight, `--color-surface-1` for the current value.
                    if self.hot_index == Some(*index) {
                        c.fill_rounded(&rect, 0.0, &t.accent_light);
                    } else if selected {
                        c.fill_rounded(&rect, 0.0, &t.card_background);
                    }
                    if selected {
                        let check = Rect::new(
                            rect.left + font_metrics::ROW_PAD_L,
                            rect.top,
                            rect.left + font_metrics::ROW_PAD_L + font_metrics::CHECK_CELL,
                            rect.bottom,
                        );
                        c.vector_icon("Check", &check, font_metrics::CHECK_GLYPH, &t.accent);
                    }
                    self.paint_name(c, self.name_rect(rect), font);
                    // The sample — in the UI face, not the family's own; see
                    // the type's documentation, and `sample_rect` for the way
                    // out.
                    if let Some(sample) = self.sample_rect(rect) {
                        c.text_ellipsis(&self.sample, &sample, &f.body, &t.text_tertiary);
                    }
                }
            }
        }
        c.pop_clip();
        if rows.is_empty() {
            // `Aucune police pour « {q} »`, centred.
            let text = format!("Aucune police pour « {} »", self.query.trim());
            let r = Rect::new(list.left + font_metrics::EMPTY_PAD_H, list.top, list.right - font_metrics::EMPTY_PAD_H, list.bottom);
            c.push_clip(&r);
            c.text_ellipsis_center(&text, &r, &f.caption, &t.text_tertiary);
            c.pop_clip();
        }
        paint_scroll_thumb(
            c,
            panel,
            font_metrics::RADIUS,
            Rect::new(list.left, list.top, list.right, list.bottom),
            self.scroll_y,
            self.list_height(),
            self.content_height(),
        );
    }

    /// Paints the open popup into the panel `panel` — `popup_rect` in the
    /// owner's space, or that rectangle [`rebase`]d into a popup's.
    pub fn paint_popup_at(&self, c: &dyn Canvas, panel: Rect) {
        paint_float_panel(c, panel, font_metrics::RADIUS);
        // `overflow: hidden` on the rounded panel.
        c.push_clip_rounded(&panel, font_metrics::RADIUS);
        self.paint_search(c, panel);
        self.paint_rows(c, panel);
        c.pop_clip_rounded();
    }

    /// Paints the open popup where [`FontPicker::popup_rect`] puts it.
    pub fn paint_popup(&self, c: &dyn Canvas, bounds: Rect) {
        self.paint_popup_at(c, self.popup_rect(bounds));
    }
}

/// The search box's placeholder, verbatim.
const SEARCH_PLACEHOLDER: &str = "Rechercher une police…";
/// The clear button's label, verbatim (`aria-label="Effacer la recherche"`).
const CLEAR_LABEL: &str = "Effacer";

impl Widget for FontPicker {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// `width = 150` is the prop's default and the trigger truncates, so the
    /// natural width is the selected name between the paddings and the caret,
    /// floored at that 150.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let f = canvas.formats();
        let text = canvas.measure(self.display_text(), &f.body);
        let chrome = font_metrics::PAD_L
            + dropdown_metrics::GAP
            + font_metrics::CARET
            + font_metrics::CARET_GAP
            + font_metrics::PAD_R;
        Size::new((text + chrome).max(font_metrics::WIDTH).ceil(), self.height)
    }

    /// The trigger, and — while `open` — the popup INLINE under it (the
    /// historical one-call paint). A live caller floats the popup instead:
    /// [`FontPicker::paint_field`] in the window and
    /// [`FontPicker::paint_popup_at`] in a `host::popup` covering
    /// [`FontPicker::popup_paint_bounds`].
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_field(canvas, bounds, state);
        let dead = state.disabled || !self.inner.control().enabled;
        if self.open && !dead {
            self.paint_popup(canvas, bounds);
        }
    }

    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.trigger_rect(bounds).contains(x, y)
            || (self.open && self.popup_rect(bounds).contains(x, y))
    }

    fn type_name(&self) -> &'static str {
        "FontPicker"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// FontSizeField
// ═════════════════════════════════════════════════════════════════════════════

/// A type-or-pick font size — `@ui/FontSizeField`'s `SizeCombo`.
///
/// # What is wrapped, and why that is the whole point
///
/// The **model is [`crate::range::NumericField`]**, and with it the ordering
/// trap that type exists to remove: `NumericUpDown.Value` *throws* outside
/// `[Minimum, Maximum]` and `Maximum` defaults to **100**, so the obvious
/// « set the value, then widen the range » is wrong for every size above 100 —
/// which is most of a font-size list. [`FontSizeField::new`] goes through
/// [`NumericField::set_range_and_value`], applying `1 … 999` (the web's own
/// `minSize` / `maxSize` defaults) *before* the value, atomically. The clamp a
/// typed value goes through is [`NumericField::clamped`], the replica's, not a
/// second one written here.
///
/// # The pair
///
/// The web's `FontSizeField` is this control **glued to a [`FontPicker`]**:
/// « the joined edge is squared … and the middle borders overlap into one
/// divider line ». That gluing is a two-control layout, not a primitive, so the
/// two ship separately and a caller places them side by side; the gallery shows
/// the pair.
///
/// Note what this is NOT: [`crate::range::NumericField`]'s own paint, which
/// draws `@ui/NumberInput`'s `w-6` up/down spin column. `SizeCombo` has one
/// caret button that opens a preset list, and no spinners at all (it steps with
/// the arrow keys instead). The chrome is therefore this file's; the model,
/// the range and the clamp are not.
pub struct FontSizeField {
    inner: NumericField,
    /// The presets the caret offers — `sizes`. Any value may still be typed.
    pub presets: Vec<f64>,
    /// The trigger's height: 30 in the web's glued toolbar field.
    pub height: f32,
    pub open: bool,
    pub hot_index: Option<usize>,
    /// The input is being typed into (`focused` in the web): the field shows
    /// [`FontSizeField::edit_text`] instead of the value.
    pub editing: bool,
    /// The input's own text while editing (`text` in the web — « mirror the
    /// external value unless the user is mid-edit »).
    pub edit_text: String,
    /// The whole text is selected — `inputRef.current?.select()` on focus —
    /// so the first key typed replaces it.
    pub all_selected: bool,
    /// Whether the caret is drawn this frame (the host's blink phase).
    pub caret_visible: bool,
    /// The first preset row shown when the list scrolls (`maxHeight: 280`).
    pub scroll: usize,
    /// Squares one edge to glue the field to a neighbour (`boxStyle`):
    /// `FontSizeField` squares the size box's left edge.
    pub joined: SquaredEdge,
    /// Where [`FontSizeField::place_drop_down`] put the list.
    placed: Option<Rect>,
}

/// What a keystroke did to a [`FontSizeField`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeKey {
    /// Not a key the field uses.
    Ignored,
    /// The typed text changed (nothing committed yet).
    Edited,
    /// `↑` / `↓` stepped the value to this size.
    Stepped(f64),
    /// `Enter` committed this size (the web then blurs the input).
    Committed(f64),
    /// `Escape` reverted the text (the web then blurs the input).
    Cancelled,
    /// The preset list opened (`Alt+↓`).
    Opened,
    /// The preset list closed.
    Closed,
}

impl SizeKey {
    /// Whether the key was used.
    pub fn handled(self) -> bool {
        self != SizeKey::Ignored
    }
}

/// A snapshot of an open preset list, cheap to move into a `host::popup`
/// paint closure (the field itself owns a replica that is not `Clone`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SizePresetList {
    pub presets: Vec<f64>,
    /// The value shown — its preset row is the selected one.
    pub value: f64,
    pub hot_index: Option<usize>,
    pub scroll: usize,
}

impl SizePresetList {
    /// How many rows the 280 window holds.
    pub fn visible_rows(&self) -> usize {
        let room = ((list_metrics::DROP_DOWN_MAX - 2.0 * size_metrics::LIST_PAD_V) / size_metrics::ROW_H)
            .floor()
            .max(1.0) as usize;
        self.presets.len().min(room)
    }

    /// Whether the list overflows its window.
    pub fn scrolls(&self) -> bool {
        self.presets.len() > self.visible_rows()
    }

    /// Where preset `i` is drawn inside the panel `panel` (`None` when scrolled
    /// out).
    pub fn row_rect_in(&self, panel: Rect, i: usize) -> Option<Rect> {
        if i < self.scroll || i >= (self.scroll + self.visible_rows()).min(self.presets.len()) {
            return None;
        }
        let bar = if self.scrolls() { control::SCROLLBAR_THUMB } else { 0.0 };
        let y = panel.top + size_metrics::LIST_PAD_V + (i - self.scroll) as f32 * size_metrics::ROW_H;
        Some(Rect::new(panel.left, y, panel.right - bar, y + size_metrics::ROW_H))
    }

    /// Which preset `(x, y)` lands on inside `panel`.
    pub fn item_at_in(&self, panel: Rect, x: f32, y: f32) -> Option<usize> {
        let last = (self.scroll + self.visible_rows()).min(self.presets.len());
        (self.scroll..last).find(|&i| self.row_rect_in(panel, i).is_some_and(|r| r.contains(x, y)))
    }

    /// Paints the list into `panel`: the float panel (`borderRadius: 8`),
    /// then the rows clipped to it.
    pub fn paint_at(&self, canvas: &dyn Canvas, panel: Rect) {
        let t = canvas.theme();
        let f = canvas.formats();
        paint_float_panel(canvas, panel, size_metrics::RADIUS);
        canvas.push_clip_rounded(&panel, size_metrics::RADIUS);
        let last = (self.scroll + self.visible_rows()).min(self.presets.len());
        for i in self.scroll..last {
            let (Some(row), Some(preset)) = (self.row_rect_in(panel, i), self.presets.get(i)) else {
                break;
            };
            // `sel = sv === value`; `background: sel ? P.sel : undefined`, and
            // the hover handler's `sel ? P.sel : P.hover`.
            let selected = (preset - self.value).abs() < f64::EPSILON;
            if selected {
                canvas.fill_rounded(&row, 0.0, &t.accent_light);
            } else if self.hot_index == Some(i) {
                canvas.fill_rounded(&row, 0.0, &t.card_background);
            }
            let text = Rect::new(
                row.left + size_metrics::ROW_PAD_L,
                row.top,
                (row.right - size_metrics::ROW_PAD_L).max(row.left + size_metrics::ROW_PAD_L),
                row.bottom,
            );
            // `fontWeight: sel ? 600 : undefined`.
            let fmt = if selected { &f.body_strong } else { &f.body };
            canvas.text_ellipsis(&format_preset(*preset), &text, fmt, &t.text_primary);
        }
        canvas.pop_clip_rounded();
        let track = Rect::new(panel.left, panel.top + size_metrics::LIST_PAD_V, panel.right, panel.bottom - size_metrics::LIST_PAD_V);
        paint_scroll_thumb(canvas, panel, size_metrics::RADIUS, track, self.scroll as f32, self.visible_rows() as f32, self.presets.len() as f32);
    }
}

impl Default for FontSizeField {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for FontSizeField {
    type Target = NumericField;
    fn deref(&self) -> &NumericField {
        &self.inner
    }
}
impl DerefMut for FontSizeField {
    fn deref_mut(&mut self) -> &mut NumericField {
        &mut self.inner
    }
}

impl FontSizeField {
    /// `minSize = 1`, `maxSize = 999`, and no value yet — the range is applied
    /// first, so any later size in it is accepted.
    pub fn new() -> Self {
        let mut inner = NumericField::new();
        inner.set_range(size_metrics::MIN, size_metrics::MAX);
        Self {
            inner,
            presets: Vec::new(),
            height: size_metrics::HEIGHT,
            open: false,
            hot_index: None,
            editing: false,
            edit_text: String::new(),
            all_selected: false,
            caret_visible: false,
            scroll: 0,
            joined: SquaredEdge::None,
            placed: None,
        }
    }

    /// A field over `presets`, showing `value`.
    ///
    /// Fails only if `value` is outside `1 … 999` — the two the web accepts —
    /// and leaves the field untouched when it does, because
    /// [`NumericField::set_range_and_value`] validates before it assigns.
    pub fn with_presets(presets: &[f64], value: f64) -> Result<Self, OutOfRange> {
        let mut f = Self::new();
        f.inner.set_range_and_value(size_metrics::MIN, size_metrics::MAX, value)?;
        f.presets = presets.to_vec();
        Ok(f)
    }

    /// The presets `FontSizeField.tsx`'s call sites pass — the classic word
    /// processor ladder. Offered as a constant because every caller wants it
    /// and none should retype it.
    pub const DEFAULT_PRESETS: [f64; 16] = [
        8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 48.0, 72.0,
        96.0,
    ];

    /// `commit(raw)`: parse (accepting a comma as the decimal point), round,
    /// then clamp into `[min, max]` — and revert on anything unparseable, which
    /// is what keeps a mixed selection mixed.
    ///
    /// Returns the value now shown.
    pub fn commit(&mut self, raw: &str) -> f64 {
        let s = raw.trim().replace(',', ".");
        match s.parse::<f64>() {
            Ok(n) if n.is_finite() => self.inner.clamped(n.round()),
            _ => self.inner.value(),
        }
    }

    /// `step(d)`: the arrow keys, clamped the same way.
    pub fn step(&mut self, delta: f64) -> f64 {
        self.inner.clamped(self.inner.value() + delta)
    }

    /// The text the field shows — the replica's own formatting
    /// (`NumericUpDown::display_text`), so `DecimalPlaces` and the thousands
    /// separator behave as the toolkit says.
    pub fn display_text(&self) -> String {
        self.inner.display_text()
    }

    /// What the input shows right now: the edit buffer while typing, else the
    /// value.
    pub fn shown_text(&self) -> String {
        if self.editing {
            self.edit_text.clone()
        } else {
            self.display_text()
        }
    }

    // ── Typing ─────────────────────────────────────────────────────────────

    /// `onFocus`: the input takes the value's text and selects all of it.
    pub fn begin_edit(&mut self) {
        if !self.inner.model().control().enabled {
            return;
        }
        self.editing = true;
        self.edit_text = self.display_text();
        self.all_selected = true;
    }

    /// `onBlur`: commits what was typed (reverting when it does not parse)
    /// and leaves edit mode. Returns the value now shown.
    pub fn end_edit(&mut self) -> f64 {
        let raw = std::mem::take(&mut self.edit_text);
        let v = if self.editing { self.commit(&raw) } else { self.inner.value() };
        self.editing = false;
        self.all_selected = false;
        v
    }

    /// Characters typed into the input — over the selection when all of it
    /// is selected.
    pub fn type_text(&mut self, text: &str) -> bool {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if !self.editing || text.is_empty() {
            return false;
        }
        if self.all_selected {
            self.edit_text.clear();
            self.all_selected = false;
        }
        self.edit_text.push_str(&text);
        true
    }

    /// Picks preset `i` — `onChange(sv); setText(sv); setOpen(false)`.
    pub fn choose_preset(&mut self, i: usize) -> Option<f64> {
        let v = *self.presets.get(i)?;
        let shown = self.inner.clamped(v);
        if self.editing {
            self.edit_text = format_preset(shown);
            self.all_selected = true;
        }
        self.close();
        Some(shown)
    }

    /// Opens the preset list, highlighting the current size when it is one.
    pub fn open_list(&mut self) {
        if !self.inner.model().control().enabled {
            return;
        }
        self.open = true;
        let v = self.inner.value();
        self.hot_index = self.presets.iter().position(|p| (p - v).abs() < f64::EPSILON);
        if let Some(i) = self.hot_index {
            self.ensure_visible(i);
        }
    }

    /// Closes the preset list.
    pub fn close(&mut self) {
        self.open = false;
        self.hot_index = None;
    }

    /// The input's `onKeyDown`: `Enter` commits, `↑` / `↓` step by one,
    /// `Escape` reverts (or, with the list open, only closes the list),
    /// `Backspace` edits, and — the combobox convention the web leaves to the
    /// mouse — `Alt+↓` / `Alt+↑` open and close the preset list.
    pub fn key_down(&mut self, key: u16, mods: Modifiers) -> SizeKey {
        if !self.inner.model().control().enabled {
            return SizeKey::Ignored;
        }
        let plain = mods.matches(Modifiers::NONE);
        if mods.matches(Modifiers::ALT) && (key == vk::DOWN || key == vk::UP) {
            if self.open || key == vk::UP {
                self.close();
                return SizeKey::Closed;
            }
            self.open_list();
            return SizeKey::Opened;
        }
        if !plain {
            return SizeKey::Ignored;
        }
        match key {
            vk::ENTER => {
                if self.open {
                    if let Some(v) = self.hot_index.and_then(|i| self.choose_preset(i)) {
                        self.end_edit();
                        return SizeKey::Committed(v);
                    }
                }
                let v = self.end_edit();
                SizeKey::Committed(v)
            }
            vk::UP | vk::DOWN => {
                let d = if key == vk::UP { 1.0 } else { -1.0 };
                // `parseInt(text || value || '0')`: the step starts from what is
                // typed, not from the last committed value.
                let base = if self.editing { self.commit(&self.edit_text.clone()) } else { self.inner.value() };
                let v = self.inner.clamped(base + d);
                if self.editing {
                    self.edit_text = format_preset(v);
                    self.all_selected = true;
                }
                SizeKey::Stepped(v)
            }
            vk::ESCAPE => {
                if self.open {
                    self.close();
                    return SizeKey::Closed;
                }
                self.edit_text.clear();
                self.editing = false;
                self.all_selected = false;
                SizeKey::Cancelled
            }
            vk::BACK if self.editing => {
                if self.all_selected {
                    self.edit_text.clear();
                    self.all_selected = false;
                } else {
                    self.edit_text.pop();
                }
                SizeKey::Edited
            }
            _ => SizeKey::Ignored,
        }
    }

    /// Reads this frame's keys and typed text from the host queue — for a
    /// caller whose input holds the focus.
    pub fn take_input(&mut self) -> SizeKey {
        let mut keys: Vec<(u16, Modifiers)> = Vec::new();
        host::consume(|e| match e {
            host::InputEvent::Key { vk: k, down: true, mods, .. } => {
                let wanted = (mods.is_none() && matches!(*k, vk::ENTER | vk::UP | vk::DOWN | vk::ESCAPE | vk::BACK))
                    || (mods.matches(Modifiers::ALT) && matches!(*k, vk::UP | vk::DOWN));
                if wanted {
                    keys.push((*k, *mods));
                }
                wanted
            }
            _ => false,
        });
        let mut result = SizeKey::Ignored;
        for (k, m) in keys {
            let r = self.key_down(k, m);
            if r.handled() {
                result = r;
            }
        }
        if self.editing && self.type_text(&host::take_text()) {
            result = SizeKey::Edited;
        }
        result
    }

    // ── Geometry ───────────────────────────────────────────────────────────

    pub fn field_rect(&self, bounds: Rect) -> Rect {
        Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + self.height)
    }

    /// The caret button: `width: 18`, full height, on the trailing edge.
    pub fn caret_rect(&self, bounds: Rect) -> Rect {
        let f = self.field_rect(bounds);
        Rect::new(f.right - size_metrics::CARET_CELL, f.top, f.right, f.bottom)
    }

    /// The strip the number is typed in — `padding: '0 2px 0 8px'` inside
    /// whatever the caret button leaves.
    pub fn text_rect(&self, bounds: Rect) -> Rect {
        let f = self.field_rect(bounds);
        Rect::new(
            f.left + size_metrics::PAD_L,
            f.top,
            (self.caret_rect(bounds).left - size_metrics::PAD_R).max(f.left + size_metrics::PAD_L),
            f.bottom,
        )
    }

    /// The whole input (text strip and its padding): a click here focuses
    /// the input, a click on [`FontSizeField::caret_rect`] opens the list.
    pub fn input_rect(&self, bounds: Rect) -> Rect {
        let f = self.field_rect(bounds);
        Rect::new(f.left, f.top, self.caret_rect(bounds).left, f.bottom)
    }

    /// The list's height: `presets × 30 + padding: '4px 0'`, capped at 280.
    fn list_height(&self) -> f32 {
        let content = self.presets.len() as f32 * size_metrics::ROW_H + 2.0 * size_metrics::LIST_PAD_V;
        content.min(list_metrics::DROP_DOWN_MAX)
    }

    /// The preset list: `top: r.bottom + 4`, `minWidth: Math.max(56, r.width)`,
    /// `maxHeight: 280`, `padding: '4px 0'` — or, after
    /// [`FontSizeField::place_drop_down`], the placed rectangle.
    pub fn drop_down_rect(&self, bounds: Rect) -> Rect {
        if let Some(p) = self.placed {
            return p;
        }
        let f = self.field_rect(bounds);
        let top = f.bottom + size_metrics::OFFSET;
        let width = (f.right - f.left).max(size_metrics::MIN_WIDTH);
        Rect::new(f.left, top, f.left + width, top + self.list_height())
    }

    /// The web's `useLayoutEffect`: under the field at +4, pulled back inside
    /// `area` (the monitor work area, same space as `bounds`), and flipped
    /// ABOVE the field when it would run off the bottom.
    pub fn place_drop_down(&mut self, bounds: Rect, area: Rect) {
        let f = self.field_rect(bounds);
        let width = (f.right - f.left).max(size_metrics::MIN_WIDTH);
        let p = place_floating(f, width, self.list_height(), size_metrics::OFFSET, area, Overflow::Flip);
        self.placed = Some(p);
    }

    /// Forgets a [`FontSizeField::place_drop_down`] placement.
    pub fn clear_placement(&mut self) {
        self.placed = None;
    }

    /// The rectangle a `host::popup` hosting the open list must cover.
    pub fn drop_down_paint_bounds(&self, bounds: Rect) -> Rect {
        inflate(self.drop_down_rect(bounds), shadow_margin())
    }

    /// How many preset rows the list shows before it scrolls.
    pub fn visible_rows(&self) -> usize {
        self.list_view().visible_rows()
    }

    /// Scrolls the list by `rows`, clamped.
    pub fn scroll_by(&mut self, rows: i32) {
        let max = self.presets.len().saturating_sub(self.visible_rows()) as i64;
        self.scroll = (self.scroll as i64 + rows as i64).clamp(0, max) as usize;
    }

    /// Scrolls by a wheel travel in DIP, at least one row per notch.
    pub fn scroll_by_dip(&mut self, dy: f32) {
        if dy == 0.0 {
            return;
        }
        let rows = (dy / size_metrics::ROW_H).round() as i32;
        self.scroll_by(if rows == 0 { dy.signum() as i32 } else { rows });
    }

    /// Keeps preset `i` in the list's window.
    pub fn ensure_visible(&mut self, i: usize) {
        let v = self.visible_rows().max(1);
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + v {
            self.scroll = i + 1 - v;
        }
    }

    /// The open list as a free-standing snapshot — what a `host::popup`
    /// closure captures.
    pub fn list_view(&self) -> SizePresetList {
        SizePresetList {
            presets: self.presets.clone(),
            value: self.inner.value(),
            hot_index: self.hot_index,
            scroll: self.scroll,
        }
    }

    pub fn row_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        self.list_view().row_rect_in(self.drop_down_rect(bounds), i)
    }

    /// Which preset `(x, y)` lands on.
    pub fn item_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if !self.open {
            return None;
        }
        self.list_view().item_at_in(self.drop_down_rect(bounds), x, y)
    }

    // ── Paint ──────────────────────────────────────────────────────────────

    /// Paints the field alone — what the owner window draws while the list
    /// floats. The web gives the box no focus ring (`outline-none` on the
    /// input): the caret and the selected text ARE the focus indication.
    pub fn paint_field(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let t = canvas.theme();
        let f = canvas.formats();
        let dead = state.disabled || !self.inner.model().control().enabled;
        let field = self.field_rect(bounds);
        let alpha = if dead { dropdown_metrics::DISABLED_ALPHA } else { 1.0 };
        let m = dropdown_metrics::RADIUS;

        // `background: open ? P.active : undefined` + the hover handler
        // (`--color-surface-2` / `--color-surface-1`), then
        // `border: 1px solid ${P.border}`.
        if !dead {
            if self.open {
                paint_box(canvas, field, m, self.joined, Some(&t.surface_2), None);
            } else if state.hot {
                paint_box(canvas, field, m, self.joined, Some(&t.card_background), None);
            }
        }
        paint_box(canvas, field, m, self.joined, None, Some(&faded(&t.card_stroke, alpha)));

        let text = self.text_rect(bounds);
        if self.editing && !dead {
            let n = self.edit_text.chars().count();
            let sel = if self.all_selected { (0, n) } else { (n, n) };
            let caret = (self.caret_visible && state.focused).then_some(n);
            paint_edit_line(canvas, text, &self.edit_text, &f.body, &t.text_primary, sel, caret, n);
        } else {
            canvas.push_clip(&field);
            canvas.text_ellipsis(&self.display_text(), &text, &f.body, &faded(&t.text_primary, alpha));
            canvas.pop_clip();
        }
        paint_caret(canvas, self.caret_rect(bounds), size_metrics::CARET, &faded(&t.text_secondary, alpha));
    }

    /// Paints the open list into the panel `panel` (owner or popup space).
    pub fn paint_drop_down_at(&self, canvas: &dyn Canvas, panel: Rect) {
        self.list_view().paint_at(canvas, panel);
    }

    /// Paints the open list where [`FontSizeField::drop_down_rect`] puts it.
    pub fn paint_drop_down(&self, canvas: &dyn Canvas, bounds: Rect) {
        self.paint_drop_down_at(canvas, self.drop_down_rect(bounds));
    }
}

impl Widget for FontSizeField {
    fn model(&self) -> &dyn Control {
        self.inner.model()
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let f = canvas.formats();
        let text = canvas.measure(&self.display_text(), &f.body);
        let chrome = size_metrics::PAD_L + size_metrics::PAD_R + size_metrics::CARET_CELL;
        Size::new((text + chrome).max(size_metrics::WIDTH).ceil(), self.height)
    }

    /// The field, and — while `open` — the list INLINE under it (the
    /// historical one-call paint). A live caller floats the list:
    /// [`FontSizeField::paint_field`] in the window, and
    /// [`SizePresetList::paint_at`] in a `host::popup` covering
    /// [`FontSizeField::drop_down_paint_bounds`].
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_field(canvas, bounds, state);
        let dead = state.disabled || !self.inner.model().control().enabled;
        if self.open && !dead {
            self.paint_drop_down(canvas, bounds);
        }
    }

    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.field_rect(bounds).contains(x, y)
            || (self.open && self.drop_down_rect(bounds).contains(x, y))
    }

    fn type_name(&self) -> &'static str {
        "FontSizeField"
    }
}

/// A preset as the list prints it — `String(s)`, i.e. no trailing `.0` on a
/// whole number.
fn format_preset(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// RichTextToolbar
// ═════════════════════════════════════════════════════════════════════════════

/// A formatting command a [`RichTextToolbar`] can carry.
///
/// The first seven are `@ui/RichText`'s bar, in its order, with its labels and
/// its lucide icons. The four alignments are **not** in that file: an editor's
/// alignment group is what every host that adopted this toolbar asked for next,
/// and they are declared here so the geometry exists — with lucide's own
/// `align-*` icons — rather than each host inventing a glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RichTextCommand {
    Bold,
    Italic,
    Underline,
    OrderedList,
    BulletList,
    Link,
    ClearFormat,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignJustify,
}

impl RichTextCommand {
    /// The geometry name in `assets/lucide-icons.txt` — the very icon
    /// `RichText.tsx` imports from `lucide-react`.
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Bold => "Bold",
            Self::Italic => "Italic",
            Self::Underline => "Underline",
            Self::OrderedList => "ListOrdered",
            Self::BulletList => "List",
            Self::Link => "Link2",
            Self::ClearFormat => "Eraser",
            Self::AlignLeft => "AlignLeft",
            Self::AlignCenter => "AlignCenter",
            Self::AlignRight => "AlignRight",
            Self::AlignJustify => "AlignJustify",
        }
    }

    /// The `title` / `aria-label` the web gives the button, verbatim.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bold => "Gras",
            Self::Italic => "Italique",
            Self::Underline => "Souligné",
            Self::OrderedList => "Liste numérotée",
            Self::BulletList => "Liste à puces",
            Self::Link => "Insérer un lien",
            Self::ClearFormat => "Effacer la mise en forme",
            Self::AlignLeft => "Aligner à gauche",
            Self::AlignCenter => "Centrer",
            Self::AlignRight => "Aligner à droite",
            Self::AlignJustify => "Justifier",
        }
    }

    /// Every command, in the order [`RichTextToolbar::with_alignments`] lays
    /// them out.
    pub const ALL: [RichTextCommand; 11] = [
        Self::Bold,
        Self::Italic,
        Self::Underline,
        Self::OrderedList,
        Self::BulletList,
        Self::Link,
        Self::ClearFormat,
        Self::AlignLeft,
        Self::AlignCenter,
        Self::AlignRight,
        Self::AlignJustify,
    ];

    /// The command a geometry name stands for — the inverse of
    /// [`RichTextCommand::icon`].
    pub fn from_icon(icon: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.icon() == icon)
    }

    /// Whether the command is a STATE the caret can be in (bold, a list, an
    /// alignment) rather than a one-shot action (insert a link, clear the
    /// formatting). Only states light up.
    pub const fn is_toggle(self) -> bool {
        !matches!(self, Self::Link | Self::ClearFormat)
    }

    /// The group whose members exclude one another: the two lists
    /// (`insertOrderedList` replaces a bulleted list, and back) and the four
    /// alignments (a paragraph has one).
    pub const fn exclusive_group(self) -> Option<u8> {
        match self {
            Self::OrderedList | Self::BulletList => Some(1),
            Self::AlignLeft | Self::AlignCenter | Self::AlignRight | Self::AlignJustify => Some(2),
            _ => None,
        }
    }
}

/// What a keystroke did to a focused [`RichTextToolbar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarKey {
    /// Not a key the bar uses.
    Ignored,
    /// The roving focus moved to cell `i`.
    Moved(usize),
    /// Cell `i` was activated (`Enter` / `Space`).
    Activated(usize),
}

/// The formatting bar of a rich-text editor — `@ui/RichText`'s toolbar row.
///
/// **The editing area is not ported**; see the module documentation for why.
/// This is the half that *is* a primitive, and it is reusable on its own: a
/// strip of `w-8 h-8` icon buttons with `w-px h-5` rules between the groups.
///
/// The model is the toolkit's [`ToolStrip`], and the active state is
/// [`ToolStripButton::checked`] — the replica field that already means « this
/// command is on », the same one [`crate::navigation::Toolbar`] lights its
/// toggles from. There is therefore no `active: Vec<bool>` here; a host reads
/// `queryCommandState` (or its own document model) and sets `checked`.
///
/// The active pixels are `crate::navigation`'s, deliberately: `accent_light`
/// ground with an `accent` glyph — « the web's `bg-primary-light text-primary`
/// pill, NOT a solid accent block with a white glyph ». `@ui/RichText`'s own
/// buttons have **no** active state at all (they only hover), which is a
/// finding rather than an omission: the web bar never reflects the caret's
/// formatting, and this one can.
#[derive(Clone, Default)]
pub struct RichTextToolbar {
    inner: ToolStrip,
    /// The cell under the pointer, as an index into [`ToolStrip::items`]. A
    /// `ToolStripItem` is a `Component`, not a `Control`: it cannot observe the
    /// pointer, which is why the replica's own painter takes this from its host
    /// too.
    pub hot_index: Option<usize>,
    /// Whether the bar draws the editor's `rounded-md border` box and the
    /// `border-b` under itself. A bar hosted inside another chrome turns it off.
    pub framed: bool,
    /// The cell that holds the bar's roving focus — the WAI-ARIA toolbar
    /// pattern: the bar is ONE Tab stop, and `←` / `→` / `Home` / `End` move
    /// between its buttons. It is ringed when the bar is painted with a
    /// focus-visible state.
    pub focus_index: Option<usize>,
    /// The cell the pointer is pressing (`:active`).
    pub pressed_index: Option<usize>,
}

impl Deref for RichTextToolbar {
    type Target = ToolStrip;
    fn deref(&self) -> &ToolStrip {
        &self.inner
    }
}
impl DerefMut for RichTextToolbar {
    fn deref_mut(&mut self) -> &mut ToolStrip {
        &mut self.inner
    }
}

/// A command cell — a `ToolStripButton` whose `Image` is the geometry name and
/// whose `Text` is the tooltip the web puts in `title`.
pub fn command_item(cmd: RichTextCommand, active: bool) -> StripItem {
    let mut b = ToolStripButton::new(cmd.label());
    b.item.image = Some(cmd.icon().to_string());
    b.item.tool_tip_text = cmd.label().to_string();
    b.check_on_click = true;
    b.checked = active;
    StripItem::Button(b)
}

/// A `<span className="w-px h-5 bg-border mx-1" />`.
pub fn rule_item() -> StripItem {
    StripItem::Separator(ToolStripSeparator::default())
}

impl RichTextToolbar {
    pub fn new() -> Self {
        Self { framed: true, ..Self::default() }
    }

    /// `@ui/RichText`'s bar, exactly: bold · italic · underline ┃ ordered ·
    /// bullet ┃ link · clear.
    pub fn standard() -> Self {
        use RichTextCommand::*;
        let mut bar = Self::new();
        for cmd in [Bold, Italic, Underline] {
            bar.push(command_item(cmd, false));
        }
        bar.push(rule_item());
        for cmd in [OrderedList, BulletList] {
            bar.push(command_item(cmd, false));
        }
        bar.push(rule_item());
        for cmd in [Link, ClearFormat] {
            bar.push(command_item(cmd, false));
        }
        bar
    }

    /// [`RichTextToolbar::standard`] plus the alignment group — see
    /// [`RichTextCommand`] for why those four are an extension.
    pub fn with_alignments() -> Self {
        use RichTextCommand::*;
        let mut bar = Self::standard();
        bar.push(rule_item());
        for cmd in [AlignLeft, AlignCenter, AlignRight, AlignJustify] {
            bar.push(command_item(cmd, false));
        }
        bar
    }

    pub fn push(&mut self, item: StripItem) {
        self.inner.items.push(item);
    }

    /// Turns a command on or off — i.e. writes the replica's `Checked`.
    /// Returns whether a matching button was found.
    pub fn set_active(&mut self, cmd: RichTextCommand, on: bool) -> bool {
        let mut found = false;
        for item in &mut self.inner.items {
            if let StripItem::Button(b) = item {
                if b.item.image.as_deref() == Some(cmd.icon()) {
                    b.checked = on;
                    found = true;
                }
            }
        }
        found
    }

    /// Whether a command is currently on — read straight off the replica.
    pub fn is_active(&self, cmd: RichTextCommand) -> bool {
        self.inner.items.iter().any(|item| {
            matches!(item, StripItem::Button(b)
                if b.item.image.as_deref() == Some(cmd.icon()) && b.checked)
        })
    }

    /// The width one cell wants: a button is `w-8`, a rule is its own 1 DIP
    /// plus `mx-1` on each side.
    fn cell_width(item: &StripItem) -> f32 {
        match item {
            StripItem::Separator(_) => rich_metrics::SEPARATOR_CELL,
            _ => rich_metrics::BUTTON,
        }
    }

    /// Where cell `i` is drawn: packed from the left with `gap-0.5` between,
    /// after the row's `px-1.5`, vertically centred in the bar.
    pub fn item_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        let item = self.inner.items.get(i)?;
        let mut x = bounds.left + rich_metrics::PAD_X;
        for prev in self.inner.items.iter().take(i) {
            x += Self::cell_width(prev) + rich_metrics::GAP;
        }
        let cy = bounds.top + rich_metrics::PAD_Y + rich_metrics::BUTTON / 2.0;
        let (w, h) = match item {
            StripItem::Separator(_) => (rich_metrics::SEPARATOR_CELL, rich_metrics::RULE_H),
            _ => (rich_metrics::BUTTON, rich_metrics::BUTTON),
        };
        Some(Rect::new(x, cy - h / 2.0, x + w, cy + h / 2.0))
    }

    /// **Which command** `(x, y)` lands on.
    ///
    /// A separator answers `None` — it is a cell geometrically and never a
    /// target — and so does the `gap-0.5` between two buttons, which belongs to
    /// neither: a bar whose gaps answered would fire the wrong command from a
    /// pixel that is visibly between the two.
    pub fn item_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        (0..self.inner.items.len()).find(|&i| {
            !matches!(self.inner.items[i], StripItem::Separator(_))
                && self.item_rect(bounds, i).is_some_and(|r| r.contains(x, y))
        })
    }

    /// The width every cell and gap comes to, plus the row's `px-1.5`.
    pub fn content_width(&self) -> f32 {
        let cells: f32 = self.inner.items.iter().map(Self::cell_width).sum();
        let gaps = rich_metrics::GAP * self.inner.items.len().saturating_sub(1) as f32;
        cells + gaps + 2.0 * rich_metrics::PAD_X
    }

    /// The command cell `i` carries (`None` for a rule or a foreign item).
    pub fn command_at(&self, i: usize) -> Option<RichTextCommand> {
        match self.inner.items.get(i)? {
            StripItem::Button(b) => b.item.image.as_deref().and_then(RichTextCommand::from_icon),
            _ => None,
        }
    }

    /// The tooltip of cell `i` — the web's `title`.
    pub fn tooltip_of(&self, i: usize) -> Option<&str> {
        match self.inner.items.get(i)? {
            StripItem::Separator(_) => None,
            other => Some(other.item().tool_tip_text.as_str()).filter(|s| !s.is_empty()),
        }
    }

    /// Whether cell `i` is a button a user can reach and press.
    fn is_target(&self, i: usize) -> bool {
        match self.inner.items.get(i) {
            Some(StripItem::Separator(_)) | None => false,
            Some(other) => other.item().enabled,
        }
    }

    /// The indices of every enabled button, left to right.
    pub fn targets(&self) -> Vec<usize> {
        (0..self.inner.items.len()).filter(|&i| self.is_target(i)).collect()
    }

    /// Applies `cmd` to the bar's state, the way the editor's document would
    /// answer `queryCommandState` after `execCommand`: a style flips, a list
    /// or an alignment turns its group's others off, `ClearFormat` drops the
    /// three inline styles, and `Link` changes no state. Returns whether any
    /// button changed.
    pub fn apply(&mut self, cmd: RichTextCommand) -> bool {
        use RichTextCommand::*;
        let before: Vec<bool> = RichTextCommand::ALL.iter().map(|c| self.is_active(*c)).collect();
        match cmd {
            Link => {}
            ClearFormat => {
                for c in [Bold, Italic, Underline] {
                    self.set_active(c, false);
                }
            }
            c if c.exclusive_group().is_some() => {
                let on = if matches!(c, AlignLeft | AlignCenter | AlignRight | AlignJustify) {
                    // A paragraph always has an alignment: pressing the active
                    // one keeps it.
                    true
                } else {
                    !self.is_active(c)
                };
                for other in RichTextCommand::ALL {
                    if other != c && other.exclusive_group() == c.exclusive_group() {
                        self.set_active(other, false);
                    }
                }
                self.set_active(c, on);
            }
            c => {
                let on = !self.is_active(c);
                self.set_active(c, on);
            }
        }
        let after: Vec<bool> = RichTextCommand::ALL.iter().map(|c| self.is_active(*c)).collect();
        before != after
    }

    /// Presses cell `i` — a click or `Enter` / `Space` on the focused cell:
    /// moves the roving focus there, applies the command and returns it.
    /// `None` for a rule, a disabled button or a foreign item.
    pub fn activate(&mut self, i: usize) -> Option<RichTextCommand> {
        if !self.is_target(i) {
            return None;
        }
        self.focus_index = Some(i);
        let cmd = self.command_at(i)?;
        self.apply(cmd);
        Some(cmd)
    }

    /// The cell the roving focus lands on when the bar is entered: the one it
    /// left from, else the first enabled button.
    pub fn entry_index(&self) -> Option<usize> {
        self.focus_index.filter(|&i| self.is_target(i)).or_else(|| self.targets().first().copied())
    }

    /// One keystroke on the focused bar — the WAI-ARIA toolbar: `→` / `←`
    /// move to the next / previous enabled button (wrapping), `Home` / `End`
    /// to the first / last, `Enter` / `Space` press the focused one.
    pub fn key_down(&mut self, key: u16, mods: Modifiers) -> ToolbarKey {
        if !mods.matches(Modifiers::NONE) {
            return ToolbarKey::Ignored;
        }
        let targets = self.targets();
        if targets.is_empty() {
            return ToolbarKey::Ignored;
        }
        let at = self.entry_index().and_then(|f| targets.iter().position(|&t| t == f)).unwrap_or(0);
        let n = targets.len();
        let to = match key {
            vk::RIGHT => targets[(at + 1) % n],
            vk::LEFT => targets[(at + n - 1) % n],
            vk::HOME => targets[0],
            vk::END => targets[n - 1],
            vk::ENTER | vk::SPACE => {
                let i = targets[at];
                self.activate(i);
                return ToolbarKey::Activated(i);
            }
            _ => return ToolbarKey::Ignored,
        };
        self.focus_index = Some(to);
        ToolbarKey::Moved(to)
    }

    /// Reads this frame's navigation keys from the host queue — for a caller
    /// whose bar holds the focus.
    pub fn take_input(&mut self) -> ToolbarKey {
        let mut keys: Vec<u16> = Vec::new();
        host::consume(|e| match e {
            host::InputEvent::Key { vk: k, down: true, mods, .. } => {
                let wanted = mods.is_none()
                    && matches!(*k, vk::RIGHT | vk::LEFT | vk::HOME | vk::END | vk::ENTER | vk::SPACE);
                if wanted {
                    keys.push(*k);
                }
                wanted
            }
            _ => false,
        });
        // The Space that pressed a button also arrives as text.
        if keys.contains(&vk::SPACE) {
            host::consume(|e| matches!(e, host::InputEvent::Text(s) if s.trim().is_empty()));
        }
        let mut result = ToolbarKey::Ignored;
        for k in keys {
            let r = self.key_down(k, Modifiers::NONE);
            if r != ToolbarKey::Ignored {
                result = r;
            }
        }
        result
    }
}

impl Widget for RichTextToolbar {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        Size::new(self.content_width(), rich_metrics::HEIGHT)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let t = canvas.theme();
        if self.framed {
            // The editor's own box: `rounded-md border border-border bg-white`.
            canvas.fill_rounded(&bounds, radius::SM, &t.layer_background);
            canvas.stroke_rounded(&bounds, radius::SM, &t.card_stroke);
            // `border-b border-border` under the toolbar row.
            let y = bounds.top + rich_metrics::HEIGHT - rich_metrics::RULE_UNDER;
            canvas.fill_rounded(
                &Rect::new(bounds.left, y, bounds.right, y + rich_metrics::RULE_UNDER),
                0.0,
                &t.divider,
            );
        }

        // `overflow-hidden` on the editor's box: a bar narrower than its
        // commands cuts the last ones at its own edge instead of painting them
        // over its neighbour.
        if self.framed {
            canvas.push_clip_rounded(&bounds, radius::SM);
        } else {
            canvas.push_clip(&bounds);
        }
        let ring_at = self.focus_index.filter(|_| state.show_focus_ring() && !state.disabled);
        for (i, item) in self.inner.items.iter().enumerate() {
            let Some(cell) = self.item_rect(bounds, i) else { continue };
            match item {
                StripItem::Separator(_) => {
                    // `w-px h-5 bg-border mx-1` — the rule inside its margins.
                    let mid = (cell.left + cell.right) / 2.0;
                    canvas.fill_rounded(
                        &Rect::new(mid, cell.top, mid + rich_metrics::RULE, cell.bottom),
                        0.0,
                        &t.divider,
                    );
                }
                other => {
                    let it = other.item();
                    let enabled = it.enabled && !state.disabled;
                    let on = matches!(other, StripItem::Button(b) if b.checked) && enabled;
                    let hot = self.hot_index == Some(i) && enabled && !on;

                    if on {
                        canvas.fill_rounded(&cell, rich_metrics::RADIUS, &t.accent_light);
                    } else if hot {
                        // `hover:bg-surface-2`.
                        canvas.fill_rounded(&cell, rich_metrics::RADIUS, &t.surface_2);
                    }
                    // `text-text-secondary`, `hover:text-text-primary`, and the
                    // accent when the command is on.
                    let ink = if !enabled {
                        faded(&t.text_secondary, dropdown_metrics::DISABLED_ALPHA)
                    } else if on {
                        t.accent
                    } else if hot {
                        t.text_primary
                    } else {
                        t.text_secondary
                    };
                    if let Some(name) = it.image.as_deref().and_then(icon_name) {
                        canvas.vector_icon(name, &cell, rich_metrics::GLYPH, &ink);
                    }
                    // The roving focus, on `:focus-visible` only.
                    if ring_at == Some(i) && enabled {
                        paint_focus_ring(canvas, cell, rich_metrics::RADIUS);
                    }
                }
            }
        }
        if self.framed {
            canvas.pop_clip_rounded();
        } else {
            canvas.pop_clip();
        }
    }

    fn type_name(&self) -> &'static str {
        "RichTextToolbar"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text;

    fn bounds() -> Rect {
        Rect::new(0.0, 0.0, 240.0, height::BUTTON_MD)
    }

    // ── The Dropdown / ComboBox decision, materialised ────────────────────

    /// The two selectors share ONE model — the same replica, so a selection
    /// cannot mean two things — and differ in every measurement the two web
    /// files state differently. This is the argument of the module doc, pinned:
    /// if a future edit made the two agree everywhere, the alias the brief asks
    /// about would be the right answer and this test would say so by failing.
    #[test]
    fn a_dropdown_shares_the_combos_model_and_none_of_its_geometry() {
        let mut d = Dropdown::new();
        let mut c = crate::lists::ComboBox::new();
        for label in ["Nom", "Date", "Taille"] {
            d.add_option(label, None);
            c.add_item(label);
        }
        // Same replica, same state machine: `Sorted` and `SelectedIndex` are
        // the toolkit's, not two parallel copies.
        d.set_selected_index(1);
        c.set_selected_index(1);
        assert_eq!(d.selected_item(), c.selected_item());
        assert_eq!(d.model().type_name(), c.model().type_name(), "one replica");
        assert_eq!(d.model().type_name(), "ComboBox");

        // …and a different control on screen.
        assert_ne!(d.type_name(), c.type_name());
        // Trigger inset: `padding-left: 8px` against `px-3`.
        assert_eq!(dropdown_metrics::PAD_L, 8.0);
        assert_eq!(dropdown_metrics::PAD_R, 4.0);
        // Popup row: `5px 10px` over a 20 line = 30, against the combo's 32.
        assert_eq!(height::MENU_ITEM, 30.0);
        assert_ne!(height::MENU_ITEM, crate::metrics::control::COMBO_ROW);
        // A free height, which `@ui/Combobox` does not have.
        d.height = 28.0;
        assert_eq!(d.trigger_rect(bounds()).bottom - bounds().top, 28.0);
    }

    /// The icon gutter is the web's `anyIcon`: reserved for the WHOLE list as
    /// soon as one option carries an icon, so the labels stay in one column.
    #[test]
    fn one_option_with_an_icon_indents_every_label() {
        let mut d = Dropdown::new();
        d.add_option("Nom", None);
        assert!(!d.has_icon_gutter());
        let bare = d.label_inset();
        d.add_option("Type", Some("Type"));
        assert!(d.has_icon_gutter());
        assert_eq!(
            d.label_inset() - bare,
            dropdown_metrics::ICON_CELL + dropdown_metrics::CELL_GAP,
            "one gutter for the list, not one per option",
        );
        // The parallel vector stays aligned with the replica's items…
        assert_eq!(d.icon_of(0), None);
        assert_eq!(d.icon_of(1), Some("Type"));
        // …and degrades rather than panicking on an item pushed past it.
        d.items.push("Poids".into());
        assert_eq!(d.icon_of(2), None);
    }

    /// `Sorted` inserts an item in the middle; the icon must travel with it.
    #[test]
    fn a_sorted_dropdown_keeps_each_icon_on_its_own_option() {
        let mut d = Dropdown::new();
        d.sorted = true;
        d.add_option("Zèbre", Some("Star"));
        d.add_option("Alpha", Some("Check"));
        assert_eq!(d.items, vec!["Alpha".to_string(), "Zèbre".to_string()]);
        assert_eq!(d.icon_of(0), Some("Check"));
        assert_eq!(d.icon_of(1), Some("Star"));
    }

    /// The list is anchored at `r.bottom + 2` and is at least as wide as the
    /// trigger, and `item_at` answers only on a row.
    #[test]
    fn the_open_list_anchors_under_the_trigger_and_hit_tests_its_rows() {
        let mut d = Dropdown::new();
        for label in ["Nom", "Date", "Taille"] {
            d.add_option(label, None);
        }
        let b = bounds();
        assert_eq!(d.item_at(b, 10.0, 60.0), None, "closed: nothing to hit");
        d.toggle();
        assert!(d.open);
        let panel = d.drop_down_rect(b);
        assert_eq!(panel.top, d.trigger_rect(b).bottom + 2.0);
        assert_eq!(panel.right - panel.left, b.right - b.left);
        assert_eq!(panel.bottom - panel.top, 3.0 * 30.0 + 2.0 * menu_metrics::PANEL_PAD);

        let row = d.row_rect(b, 1).expect("three rows are visible");
        let (cx, cy) = ((row.left + row.right) / 2.0, (row.top + row.bottom) / 2.0);
        assert_eq!(d.item_at(b, cx, cy), Some(1));
        assert_eq!(d.item_at(b, cx, panel.top + 1.0), None, "the panel's own padding");
        assert_eq!(d.item_at(b, cx, panel.bottom + 1.0), None, "below the panel");
        assert!(d.hit_test(b, cx, cy), "a click in the list is a click on the control");
        d.toggle();
        assert!(!d.hit_test(b, cx, cy));
    }

    /// `maxHeight: 280` caps the list, and `MaxDropDownItems` — the replica's —
    /// caps it further.
    #[test]
    fn the_list_is_capped_by_the_web_height_and_by_the_replica() {
        let mut d = Dropdown::new();
        for i in 0..40 {
            d.add_option(format!("Option {i}"), None);
        }
        d.max_drop_down_items = 100;
        assert_eq!(d.visible_rows(), 9, "(280 - 10) / 30 whole rows");
        d.max_drop_down_items = 4;
        assert_eq!(d.visible_rows(), 4, "MaxDropDownItems still wins downwards");
    }

    // ── Editable: one model, two states, one geometry ─────────────────────

    /// The rest label's band and the edit field's content rectangle must agree,
    /// or clicking into the control would nudge its text.
    #[test]
    fn an_editable_paints_its_text_in_the_same_place_in_both_states() {
        let mut e = Editable::new();
        e.set_text("Rapport annuel");
        let b = bounds();

        let band = e.label().band(b);
        let content = text::content_rect(b, false, false);
        assert_eq!(band.left, content.left, "px-3 in both states");
        assert_eq!(band.right, content.right);
        // The label's band is the 20 DIP line box, centred on the same axis
        // `Canvas::text` and `edit_box::draw_text` centre theirs on.
        assert_eq!(band.bottom - band.top, Role::Body.line_height());
        assert_eq!(band.top + band.bottom, b.top + b.bottom, "same centre line");

        // …and at any height the caller assigns.
        let tall = Rect::new(0.0, 0.0, 240.0, 60.0);
        let band = e.label().band(tall);
        assert_eq!(band.left, text::content_rect(tall, false, false).left);
        assert_eq!(band.top + band.bottom, tall.top + tall.bottom);
    }

    /// The text lives in ONE place: the field, and the `TextBox` replica behind
    /// it. The label is built from it, never beside it.
    #[test]
    fn an_editable_has_a_single_text() {
        let mut e = Editable::new();
        e.max_length = 4; // TextBoxBase's own property, through two Derefs.
        e.set_text("abcdefgh");
        assert_eq!(e.text(), "abcd", "truncated by the replica");
        assert_eq!(e.label().text, "abcd", "the label is derived, not stored");
        e.set_text("");
        e.placeholder_text = "Sans titre".into();
        assert_eq!(e.label().text, "Sans titre", "the placeholder, from the replica");
        assert_eq!(e.model().type_name(), "TextBox");
        assert_eq!(e.type_name(), "Editable");
    }

    /// The two states are one control: it measures the same either way, so a
    /// layout does not reflow when the user starts typing.
    #[test]
    fn the_frame_follows_the_hover_and_the_measurement_never_moves() {
        let mut e = Editable::new();
        assert!(!e.shows_frame(WidgetState::REST));
        assert!(e.shows_frame(WidgetState::REST.hot(true)));
        e.always_framed = true;
        assert!(e.shows_frame(WidgetState::REST), "@ui/Editable's permanent box");
        // Editing implies a live caret, which is a focused field.
        assert!(e.edit_state(WidgetState::REST).focused);
    }

    // ── FontPicker ────────────────────────────────────────────────────────

    /// The classifier is `classifyFont`, order included: monospace, script and
    /// display are tested before the broad serif rule, and `\bsans\b` before it
    /// too — which is what keeps « PT Sans » out of the serif bucket.
    #[test]
    fn fonts_are_filed_the_way_the_web_files_them() {
        assert_eq!(classify_font("Cascadia Code"), FontCategory::Mono);
        assert_eq!(classify_font("Consolas"), FontCategory::Mono);
        assert_eq!(classify_font("Segoe Script"), FontCategory::Script);
        assert_eq!(classify_font("Impact"), FontCategory::Display);
        assert_eq!(classify_font("PT Sans"), FontCategory::Sans, "\\bsans\\b wins");
        assert_eq!(classify_font("Times New Roman"), FontCategory::Serif);
        assert_eq!(classify_font("Georgia"), FontCategory::Serif);
        assert_eq!(classify_font("Inconnue"), FontCategory::Sans, "the fallback");
        // `\bcode\b`: a word, not a substring — « Codex » is not monospace.
        assert!(contains_word("cascadia code", "code"));
        assert!(!contains_word("codex pro", "code"));
    }

    /// A list of families measures as headers + rows, in `CAT_ORDER`, with the
    /// recent ones pinned on top — and the popup's height follows from it.
    #[test]
    fn a_font_list_measures_as_its_rows() {
        let mut p = FontPicker::with_fonts(["Times New Roman", "Arial", "Consolas", "Verdana"]);
        p.recent = vec!["Verdana".into()];
        let rows = p.rows();
        assert_eq!(
            rows,
            vec![
                FontRow::Header("Récentes"),
                FontRow::Option { font: "Verdana".into(), index: 0 },
                FontRow::Header("Sans Serif"),
                FontRow::Option { font: "Arial".into(), index: 1 },
                FontRow::Header("Serif"),
                FontRow::Option { font: "Times New Roman".into(), index: 2 },
                FontRow::Header("Monospace"),
                FontRow::Option { font: "Consolas".into(), index: 3 },
            ],
        );
        assert_eq!(p.options().len(), 4);
        // 4 headers × 28 + 4 rows × 34 + the list's `padding: '4px 0'`.
        assert_eq!(p.list_height(), 4.0 * 28.0 + 4.0 * 34.0 + 8.0);
        assert_eq!(font_metrics::HEADER_H, 28.0);
        assert_eq!(font_metrics::ROW_H, 34.0);

        // The panel is the search row plus the list, at least 248 wide.
        let b = bounds();
        let panel = p.popup_rect(b);
        assert_eq!(panel.top, p.trigger_rect(b).bottom + 4.0);
        assert_eq!(panel.bottom - panel.top, font_metrics::SEARCH_H + p.list_height());
        assert_eq!(panel.right - panel.left, 248.0, "minWidth beats a 240 trigger");

        // A long list is capped at `maxHeight: 340`.
        let many = FontPicker::with_fonts(
            (0..60).map(|i| Box::leak(format!("Famille {i}").into_boxed_str()) as &str),
        );
        assert_eq!(many.list_height(), font_metrics::LIST_MAX);
    }

    /// Searching flattens the list, prefix matches first, and drops the
    /// headers.
    #[test]
    fn searching_flattens_and_ranks() {
        let mut p = FontPicker::with_fonts(["Courier New", "New Century", "Verdana"]);
        p.query = "new".into();
        assert_eq!(
            p.rows(),
            vec![
                FontRow::Option { font: "New Century".into(), index: 0 },
                FontRow::Option { font: "Courier New".into(), index: 1 },
            ],
            "prefix first, then the rest, and no headers",
        );
        p.query = "zzz".into();
        assert!(p.rows().is_empty());
    }

    /// Only options are targets, and the sample column is reserved whether or
    /// not this crate can paint it in its own face.
    #[test]
    fn only_the_option_rows_answer_a_hit_test() {
        let mut p = FontPicker::with_fonts(["Arial", "Consolas"]);
        let b = bounds();
        assert_eq!(p.item_at(b, 20.0, 100.0), None, "closed");
        p.open = true;
        let header = p.row_rect(b, 0).expect("the Sans Serif header");
        assert_eq!(p.item_at(b, 20.0, (header.top + header.bottom) / 2.0), None);
        let arial = p.row_rect(b, 1).expect("Arial");
        assert_eq!(p.item_at(b, 20.0, (arial.top + arial.bottom) / 2.0), Some(0));

        let sample = p.sample_rect(arial).expect("AaBbCc is on by default");
        assert_eq!(sample.right, arial.right - font_metrics::ROW_PAD_R);
        assert_eq!(sample.right - sample.left, font_metrics::SAMPLE_MAX);
        assert_eq!(p.name_rect(arial).right, sample.left - font_metrics::CELL_GAP);
        assert_eq!(
            p.name_rect(arial).left,
            arial.left + font_metrics::ROW_PAD_L + font_metrics::CHECK_CELL + font_metrics::CELL_GAP,
        );
        p.sample.clear();
        assert!(p.sample_rect(arial).is_none());
        assert_eq!(p.name_rect(arial).right, arial.right - font_metrics::ROW_PAD_R);
    }

    // ── FontSizeField ─────────────────────────────────────────────────────

    /// The ordering trap, and the clamp — both the replica's, neither rewritten
    /// here. A field built the naive way (`set_value` then `set_maximum`) would
    /// refuse every size above 100.
    #[test]
    fn a_font_size_clamps_through_the_replica() {
        let mut f = FontSizeField::with_presets(&FontSizeField::DEFAULT_PRESETS, 200.0)
            .expect("200 is inside 1…999 because the range is applied first");
        assert_eq!(f.value(), 200.0);
        assert_eq!(f.minimum(), 1.0);
        assert_eq!(f.maximum(), 999.0);

        assert_eq!(f.commit("5000"), 999.0, "clamped up to maxSize");
        assert_eq!(f.commit("0"), 1.0, "clamped down to minSize");
        assert_eq!(f.commit("14,6"), 15.0, "a comma is a decimal point, and it rounds");
        assert_eq!(f.commit("  24 "), 24.0);
        assert_eq!(f.commit("abc"), 24.0, "unparseable reverts, keeping a mixed state");
        assert_eq!(f.commit(""), 24.0);
        assert_eq!(f.step(1.0), 25.0);
        assert_eq!(f.display_text(), "25", "the replica's own formatting");

        // Out of range is refused ATOMICALLY: nothing is half-applied.
        assert!(FontSizeField::with_presets(&[], 1200.0).is_err());
        assert_eq!(f.model().type_name(), "NumericUpDown");
        assert_eq!(f.type_name(), "FontSizeField");
    }

    /// The preset list: anchored at `+4`, at least 56 wide, capped at 280, and
    /// hit-tested per row.
    #[test]
    fn the_preset_list_is_the_web_geometry() {
        let mut f = FontSizeField::with_presets(&FontSizeField::DEFAULT_PRESETS, 12.0)
            .expect("12 is in range");
        let b = Rect::new(0.0, 0.0, size_metrics::WIDTH, size_metrics::HEIGHT);
        assert_eq!(f.caret_rect(b).right, b.right);
        assert_eq!(f.caret_rect(b).right - f.caret_rect(b).left, 18.0);
        assert_eq!(f.text_rect(b).left, b.left + 8.0);
        assert_eq!(f.text_rect(b).right, f.caret_rect(b).left - 2.0);

        assert_eq!(f.item_at(b, 10.0, 50.0), None, "closed");
        f.open = true;
        let panel = f.drop_down_rect(b);
        assert_eq!(panel.top, b.bottom + 4.0);
        assert_eq!(panel.right - panel.left, size_metrics::MIN_WIDTH.max(size_metrics::WIDTH));
        // 16 presets × 30 + 8 > 280, so the list is capped and scrolls.
        assert_eq!(panel.bottom - panel.top, list_metrics::DROP_DOWN_MAX);
        assert_eq!(f.visible_rows(), 9, "(280 - 8) / 30 whole rows");
        let row = f.row_rect(b, 2).expect("visible");
        assert_eq!(f.item_at(b, 10.0, (row.top + row.bottom) / 2.0), Some(2));
        assert!(f.row_rect(b, 9).is_none(), "past the cap");
    }

    // ── RichTextToolbar ───────────────────────────────────────────────────

    /// Every button's active state is the replica's `Checked` — one storage
    /// location, written and read through it.
    #[test]
    fn each_command_carries_its_own_active_state() {
        use RichTextCommand::*;
        let mut bar = RichTextToolbar::standard();
        for cmd in [Bold, Italic, Underline, OrderedList, BulletList, Link, ClearFormat] {
            assert!(!bar.is_active(cmd), "{cmd:?} starts off");
        }
        assert!(bar.set_active(Bold, true));
        assert!(bar.set_active(BulletList, true));
        assert!(bar.is_active(Bold));
        assert!(bar.is_active(BulletList));
        assert!(!bar.is_active(Italic), "one button's state is not another's");
        // It really is the replica's field, not a copy beside it.
        assert!(matches!(&bar.items[0], StripItem::Button(b) if b.checked));
        assert!(matches!(&bar.items[1], StripItem::Button(b) if !b.checked));
        bar.set_active(Bold, false);
        assert!(!bar.is_active(Bold));
        // A command the bar does not carry is reported, not silently ignored.
        assert!(!bar.set_active(AlignCenter, true));
        assert!(RichTextToolbar::with_alignments().set_active(AlignCenter, true));
        assert_eq!(bar.model().type_name(), "ToolStrip");
        assert_eq!(bar.type_name(), "RichTextToolbar");
    }

    /// `item_at` at the borders: the first pixel of the first button answers,
    /// the last does not, a separator never does, and neither does the
    /// `gap-0.5` between two buttons.
    #[test]
    fn the_bar_hit_tests_its_cells_and_nothing_between_them() {
        let bar = RichTextToolbar::standard();
        let b = Rect::new(0.0, 0.0, 400.0, rich_metrics::HEIGHT);

        let first = bar.item_rect(b, 0).expect("Bold");
        assert_eq!(first.left, b.left + rich_metrics::PAD_X, "px-1.5");
        assert_eq!(first.right - first.left, rich_metrics::BUTTON);
        assert_eq!(first.top, b.top + rich_metrics::PAD_Y, "py-1");
        assert_eq!(bar.item_at(b, first.left, first.top), Some(0), "the first pixel");
        assert_eq!(bar.item_at(b, first.right - 0.5, first.bottom - 0.5), Some(0));
        assert_eq!(bar.item_at(b, first.right, first.top), None, "half-open on the right");
        assert_eq!(bar.item_at(b, first.left - 0.5, first.top), None, "the row's padding");

        // The gap between button 0 and button 1 belongs to neither.
        let second = bar.item_rect(b, 1).expect("Italic");
        assert_eq!(second.left - first.right, rich_metrics::GAP);
        assert_eq!(bar.item_at(b, first.right + rich_metrics::GAP / 2.0, first.top), None);

        // The separator is a cell, and never a target.
        let rule = bar.item_rect(b, 3).expect("the first rule");
        assert!(matches!(bar.items[3], StripItem::Separator(_)));
        assert_eq!(rule.right - rule.left, rich_metrics::SEPARATOR_CELL);
        assert_eq!(rule.bottom - rule.top, rich_metrics::RULE_H, "h-5, not h-8");
        assert_eq!(bar.item_at(b, (rule.left + rule.right) / 2.0, first.top + 1.0), None);
        // …and it is centred on the buttons' axis.
        assert_eq!(rule.top + rule.bottom, first.top + first.bottom);

        // Above and below the row, nothing.
        assert_eq!(bar.item_at(b, first.left + 1.0, b.top), None);
        assert_eq!(bar.item_at(b, first.left + 1.0, b.bottom - 1.0), None);
        assert_eq!(bar.item_at(b, 9_999.0, first.top + 1.0), None);

        // The whole bar: 7 buttons, 2 rules, 8 gaps, two paddings.
        assert_eq!(bar.items.len(), 9);
        assert_eq!(
            bar.content_width(),
            7.0 * 32.0 + 2.0 * rich_metrics::SEPARATOR_CELL + 8.0 * 2.0 + 2.0 * 6.0,
        );
        assert_eq!(rich_metrics::HEIGHT, 41.0, "py-1 twice around h-8, plus border-b");
    }

    /// Every command names a geometry the design system actually carries — a
    /// missing icon is a missing asset, and this is where it is caught rather
    /// than on screen.
    #[test]
    fn every_command_resolves_to_a_real_geometry() {
        use RichTextCommand::*;
        for cmd in [
            Bold, Italic, Underline, OrderedList, BulletList, Link, ClearFormat, AlignLeft,
            AlignCenter, AlignRight, AlignJustify,
        ] {
            assert!(icon_name(cmd.icon()).is_some(), "{cmd:?} → {}", cmd.icon());
            assert!(!cmd.label().is_empty());
        }
        // The caret every selector in this family wears is a geometry too.
        assert!(icon_name("CaretDown").is_some());
    }

    // ── Floating surfaces: placement, shadow, type-ahead ──────────────────

    fn screen() -> Rect {
        Rect::new(0.0, 0.0, 800.0, 600.0)
    }

    /// The web's `reposition`: under the anchor when it fits, flipped above
    /// (Dropdown, size list) or slid up (font picker) when it does not, and
    /// pulled back 8 DIP inside the viewport on the right.
    #[test]
    fn a_popup_flips_or_slides_and_stays_inside_the_monitor() {
        let a = Rect::new(100.0, 100.0, 300.0, 136.0);
        let below = place_floating(a, 200.0, 100.0, 2.0, screen(), Overflow::Flip);
        assert_eq!((below.left, below.top), (100.0, 138.0), "room below: r.bottom + 2");

        let low = Rect::new(100.0, 500.0, 300.0, 536.0);
        let flipped = place_floating(low, 200.0, 100.0, 2.0, screen(), Overflow::Flip);
        assert_eq!(flipped.bottom, low.top - 2.0, "no room below: tr.top - 2 - h");
        let slid = place_floating(low, 200.0, 100.0, 4.0, screen(), Overflow::Slide);
        assert_eq!(slid.bottom, 600.0 - float_metrics::VIEWPORT_MARGIN, "slid up, not flipped");

        let edge = Rect::new(700.0, 100.0, 780.0, 136.0);
        let pulled = place_floating(edge, 200.0, 100.0, 2.0, screen(), Overflow::Flip);
        assert_eq!(pulled.right, 800.0 - float_metrics::VIEWPORT_MARGIN);
        assert_eq!(pulled.right - pulled.left, 200.0, "moved, not squeezed");

        // A monitor that does not start at the origin (a second screen, or a
        // work area in client coordinates).
        let off = Rect::new(-1000.0, -50.0, -200.0, 550.0);
        let p = place_floating(Rect::new(-1100.0, 0.0, -1000.0, 30.0), 120.0, 60.0, 2.0, off, Overflow::Flip);
        assert_eq!(p.left, -1000.0 + float_metrics::VIEWPORT_MARGIN);
    }

    /// The popup window must hold the shadow: SHADOW_MENU reaches 2 + 6 + 2.
    #[test]
    fn the_paint_bounds_include_the_shadow() {
        assert_eq!(shadow_margin(), 10.0);
        let mut d = Dropdown::new();
        for s in ["a", "b"] {
            d.add_option(s, None);
        }
        d.open = true;
        let b = bounds();
        let panel = d.drop_down_rect(b);
        let pb = d.drop_down_paint_bounds(b);
        assert_eq!(pb.left, panel.left - 10.0);
        assert_eq!(pb.bottom, panel.bottom + 10.0);
        let local = rebase(panel, pb);
        assert_eq!((local.left, local.top), (10.0, 10.0));
    }

    /// `typeAhead`: a growing prefix stays on its row, a repeated letter
    /// cycles, a pause restarts, and nothing matching answers `None`.
    #[test]
    fn type_ahead_follows_the_web() {
        let labels = ["Belgique", "Bulgarie", "Canada", "Bosnie", "Brésil"];
        let mut ta = TypeAhead::default();
        assert_eq!(ta.find('b', 1_000, None, &labels), Some(0));
        assert_eq!(ta.find('u', 1_100, Some(0), &labels), Some(1), "« bu » from the b row");
        // A pause: a fresh search, from the row after the current one.
        assert_eq!(ta.find('b', 5_000, Some(1), &labels), Some(3));
        // « b » « b »: a repeated single letter walks the matches.
        assert_eq!(ta.find('b', 5_100, Some(3), &labels), Some(4));
        assert_eq!(ta.find('b', 5_200, Some(4), &labels), Some(0), "wraps around");
        assert_eq!(ta.text(), "bbb");
        assert_eq!(ta.find('z', 9_000, Some(0), &labels), None);
        // Case: lower-cased on both sides.
        assert_eq!(ta.find('É', 20_000, None, &["été", "Étage"]), Some(0));
    }

    // ── Dropdown: keyboard, scroll ────────────────────────────────────────

    fn countries() -> Dropdown {
        let mut d = Dropdown::new();
        for s in [
            "Allemagne", "Autriche", "Belgique", "Bulgarie", "Canada", "Croatie", "Danemark",
            "Espagne", "Estonie", "Finlande", "France", "Grèce",
        ] {
            d.add_option(s, None);
        }
        d
    }

    /// Closed: the opening keys open on the selected row; open: the arrows
    /// clamp, PageDown jumps 10, Enter chooses through the replica, Escape
    /// closes without choosing.
    #[test]
    fn the_dropdown_keyboard_is_a_native_select() {
        let mut d = countries();
        d.set_selected_index(3);
        assert_eq!(d.key_down(vk::LEFT, Modifiers::NONE), ListKey::Ignored);
        assert_eq!(d.key_down(vk::DOWN, Modifiers::NONE), ListKey::Opened);
        assert!(d.open);
        assert_eq!(d.hot_index, Some(3), "opens on the selected row");
        d.key_down(vk::DOWN, Modifiers::NONE);
        assert_eq!(d.hot_index, Some(4));
        d.key_down(vk::PAGE_DOWN, Modifiers::NONE);
        assert_eq!(d.hot_index, Some(11), "clamped to the last row");
        d.key_down(vk::HOME, Modifiers::NONE);
        assert_eq!(d.hot_index, Some(0));
        d.key_down(vk::UP, Modifiers::NONE);
        assert_eq!(d.hot_index, Some(0), "no wrap");
        assert_eq!(d.key_down(vk::ESCAPE, Modifiers::NONE), ListKey::Closed);
        assert!(!d.open);
        assert_eq!(d.selected(), Some(3), "Escape chooses nothing");

        assert_eq!(d.key_down(vk::END, Modifiers::NONE), ListKey::Opened);
        assert_eq!(d.hot_index, Some(11));
        assert_eq!(d.key_down(vk::ENTER, Modifiers::NONE), ListKey::Committed(11));
        assert_eq!(d.selected_item(), Some("Grèce"));
        assert!(!d.open);

        // Alt+↓ opens, Alt+↑ takes the highlight with it.
        assert_eq!(d.key_down(vk::DOWN, Modifiers::ALT), ListKey::Opened);
        d.key_down(vk::UP, Modifiers::NONE);
        assert_eq!(d.key_down(vk::UP, Modifiers::ALT), ListKey::Committed(10));

        // Tab away while open: the highlight is taken.
        d.open_with(Some(2));
        assert_eq!(d.commit_highlight(), ListKey::Committed(2));
        assert_eq!(d.commit_highlight(), ListKey::Ignored, "closed: nothing to take");
    }

    /// Letters open the list on the first match (closed) or move the
    /// highlight (open); a space is never part of a search.
    #[test]
    fn typing_on_a_dropdown_is_a_type_ahead() {
        let mut d = countries();
        assert_eq!(d.type_text("f", 1_000), ListKey::Opened);
        assert_eq!(d.hot_index, Some(9), "Finlande");
        assert_eq!(d.type_text("r", 1_100), ListKey::Moved);
        assert_eq!(d.hot_index, Some(10), "« fr » → France");
        assert_eq!(d.type_text(" ", 1_200), ListKey::Ignored);
        assert_eq!(d.type_text("x", 1_300), ListKey::Ignored, "« frx » matches nothing");
        assert_eq!(d.hot_index, Some(10));
    }

    /// Twelve rows in a nine-row window: the keyboard scrolls the window to
    /// keep the highlight in view, the rows answer by ITEM index, and a row
    /// scrolled out has no rectangle.
    #[test]
    fn a_long_dropdown_scrolls_to_its_highlight() {
        let mut d = countries();
        d.max_drop_down_items = 100;
        assert_eq!(d.visible_rows(), 9);
        assert!(d.scrolls());
        assert_eq!(d.max_scroll(), 3);
        d.open_with(Some(0));
        d.key_down(vk::END, Modifiers::NONE);
        assert_eq!(d.scroll, 3, "End brings the last row in");
        let b = bounds();
        assert!(d.row_rect(b, 2).is_none(), "scrolled out");
        let last = d.row_rect(b, 11).expect("in view");
        let (cx, cy) = ((last.left + last.right) / 2.0, (last.top + last.bottom) / 2.0);
        assert_eq!(d.item_at(b, cx, cy), Some(11));
        // The rows give up the bar's width.
        let panel = d.drop_down_rect(b);
        assert_eq!(last.right, panel.right - menu_metrics::PANEL_PAD - control::SCROLLBAR_THUMB);

        d.key_down(vk::HOME, Modifiers::NONE);
        assert_eq!(d.scroll, 0);
        d.scroll_by(99);
        assert_eq!(d.scroll, 3, "clamped");
        d.scroll_by_dip(-10.0);
        assert_eq!(d.scroll, 2, "a small wheel step still moves one row");
    }

    // ── Editable: typing ──────────────────────────────────────────────────

    /// The rename idiom: begin selects all, typing replaces, the arrows and
    /// Shift move and extend, Backspace and Ctrl+Backspace delete, Enter
    /// keeps and Escape restores.
    #[test]
    fn an_editable_edits_like_an_input() {
        let mut e = Editable::new();
        e.set_text("Rapport annuel");
        assert_eq!(e.key_down(vk::LEFT, Modifiers::NONE), EditKey::Ignored, "at rest");
        assert_eq!(e.key_down(vk::F2, Modifiers::NONE), EditKey::Started);
        assert_eq!(e.selected_text(), "Rapport annuel", "all selected");
        assert!(e.type_text("Bilan"));
        assert_eq!(e.text(), "Bilan");
        assert_eq!(e.caret(), 5);

        e.key_down(vk::HOME, Modifiers::NONE);
        e.key_down(vk::RIGHT, Modifiers::SHIFT);
        e.key_down(vk::RIGHT, Modifiers::SHIFT);
        assert_eq!(e.selected_text(), "Bi");
        // ← with a selection collapses to its start, as an input does.
        e.key_down(vk::LEFT, Modifiers::NONE);
        assert_eq!((e.caret(), e.selection_length()), (0, 0));
        e.key_down(vk::END, Modifiers::NONE);
        assert!(e.type_text(" 2025\r\n"), "a pasted line break becomes a space");
        assert_eq!(e.text(), "Bilan 2025 ");
        assert_eq!(e.key_down(vk::BACK, Modifiers::NONE), EditKey::Edited);
        assert_eq!(e.key_down(vk::BACK, Modifiers::CTRL), EditKey::Edited);
        assert_eq!(e.text(), "Bilan ", "Ctrl+Backspace ate the word");
        e.key_down(vk::LEFT, Modifiers::CTRL);
        assert_eq!(e.caret(), 0);
        assert_eq!(e.key_down(vk::DELETE, Modifiers::NONE), EditKey::Edited);
        assert_eq!(e.text(), "ilan ");
        e.key_down(vk::letter('a'), Modifiers::CTRL);
        assert_eq!(e.selected_text(), "ilan ");

        assert_eq!(e.key_down(vk::ESCAPE, Modifiers::NONE), EditKey::Cancelled);
        assert_eq!(e.text(), "Rapport annuel", "Escape restores");
        assert!(!e.editing);

        e.begin_edit();
        e.type_text("Nouveau");
        assert_eq!(e.key_down(vk::ENTER, Modifiers::NONE), EditKey::Committed);
        assert_eq!(e.text(), "Nouveau");
        assert!(!e.editing);

        // Read-only: the caret moves, the text does not change.
        e.read_only = true;
        e.begin_edit();
        assert!(!e.type_text("x"));
        assert_eq!(e.key_down(vk::BACK, Modifiers::NONE), EditKey::Moved);
        assert_eq!(e.text(), "Nouveau");
    }

    #[test]
    fn word_jumps_and_single_line_paste() {
        assert_eq!(word_start_before("un deux trois", 13), 8);
        assert_eq!(word_start_before("un deux trois", 8), 3);
        assert_eq!(word_start_before("un deux", 0), 0);
        assert_eq!(word_end_after("un deux trois", 0), 3);
        assert_eq!(word_end_after("un deux trois", 3), 8);
        assert_eq!(word_end_after("un", 2), 2);
        assert_eq!(single_line("a\r\nb\nc\td"), "a b cd");
    }

    // ── FontPicker: keyboard, search, scroll ──────────────────────────────

    fn fonts() -> FontPicker {
        let names: Vec<String> = (0..30).map(|i| format!("Famille {i:02}")).collect();
        let mut p = FontPicker::with_fonts(names.iter().map(String::as_str));
        p.inner.add_item("Segoe UI");
        p
    }

    /// Opening highlights the current font and centres it; typing filters
    /// and resets the highlight; Backspace edits; Enter chooses through the
    /// replica; Escape closes.
    #[test]
    fn the_font_picker_keyboard_searches_and_chooses() {
        let mut p = fonts();
        p.set_selected_index(20);
        assert_eq!(p.key_down(vk::DOWN, Modifiers::NONE), ListKey::Ignored, "a closed button");
        assert_eq!(p.key_down(vk::ENTER, Modifiers::NONE), ListKey::Opened);
        let current = p.options().iter().position(|o| o == "Famille 20");
        assert_eq!(p.hot_index, current);
        assert!(p.scroll_y > 0.0, "centred on the current font");

        p.key_down(vk::DOWN, Modifiers::NONE);
        assert_eq!(p.hot_index, current.map(|i| i + 1));

        assert_eq!(p.type_text("seg"), ListKey::Moved);
        assert_eq!(p.options(), vec!["Segoe UI".to_string()]);
        assert_eq!((p.hot_index, p.scroll_y), (Some(0), 0.0));
        p.key_down(vk::BACK, Modifiers::NONE);
        assert_eq!(p.query, "se");
        p.key_down(vk::BACK, Modifiers::CTRL);
        assert_eq!(p.query, "");
        p.type_text("famille 0");
        assert_eq!(p.option_count(), 10);
        p.key_down(vk::END, Modifiers::NONE);
        assert_eq!(p.key_down(vk::ENTER, Modifiers::NONE), ListKey::Committed(9));
        assert_eq!(p.display_text(), "Famille 09");
        assert!(!p.open);
        assert!(p.query.is_empty(), "the next open starts unfiltered");

        p.open_menu();
        assert_eq!(p.key_down(vk::ESCAPE, Modifiers::NONE), ListKey::Closed);
        assert_eq!(p.display_text(), "Famille 09");
    }

    /// The list scrolls inside its 340: a row above the window has no
    /// rectangle, the keyboard keeps the highlight in view, and a point on
    /// the search row is never an option.
    #[test]
    fn the_font_list_scrolls_inside_its_window() {
        let mut p = fonts();
        p.open_menu();
        assert!(p.scrolls());
        let b = bounds();
        p.key_down(vk::END, Modifiers::NONE);
        assert_eq!(p.scroll_y, p.max_scroll());
        assert!(p.row_rect(b, 1).is_none(), "scrolled out above");
        let panel = p.popup_rect(b);
        let search = p.search_rect(b);
        assert_eq!(p.item_at(b, panel.left + 20.0, (search.top + search.bottom) / 2.0), None);
        p.key_down(vk::HOME, Modifiers::NONE);
        assert_eq!(p.scroll_y, 0.0);
        p.scroll_by(-50.0);
        assert_eq!(p.scroll_y, 0.0, "clamped");
    }

    // ── FontSizeField: typing ─────────────────────────────────────────────

    /// Focus selects all; typing replaces; ↑ steps from the TYPED value;
    /// Enter commits clamped; Escape reverts; Alt+↓ drops the presets.
    #[test]
    fn the_size_field_is_typed_into() {
        let mut f = FontSizeField::with_presets(&FontSizeField::DEFAULT_PRESETS, 14.0).expect("in range");
        f.begin_edit();
        assert!(f.all_selected);
        assert_eq!(f.shown_text(), "14");
        f.type_text("2");
        assert_eq!(f.edit_text, "2", "the selection was replaced");
        f.type_text("0");
        assert_eq!(f.key_down(vk::UP, Modifiers::NONE), SizeKey::Stepped(21.0));
        assert_eq!(f.edit_text, "21");
        assert_eq!(f.key_down(vk::BACK, Modifiers::NONE), SizeKey::Edited);
        assert_eq!(f.edit_text, "", "all selected after a step, so Backspace clears");
        f.type_text("5000");
        assert_eq!(f.key_down(vk::ENTER, Modifiers::NONE), SizeKey::Committed(999.0));
        assert!(!f.editing);

        f.begin_edit();
        f.type_text("abc");
        assert_eq!(f.key_down(vk::ESCAPE, Modifiers::NONE), SizeKey::Cancelled);
        assert_eq!(f.value(), 999.0, "Escape reverts");
        assert_eq!(f.shown_text(), "999");

        assert_eq!(f.key_down(vk::DOWN, Modifiers::ALT), SizeKey::Opened);
        assert!(f.open);
        assert_eq!(f.key_down(vk::ESCAPE, Modifiers::NONE), SizeKey::Closed, "Escape closes the list first");
        f.open_list();
        assert_eq!(f.choose_preset(0), Some(8.0));
        assert!(!f.open);
        assert_eq!(f.value(), 8.0);
        f.begin_edit();
        f.type_text("x");
        assert_eq!(f.end_edit(), 8.0, "blur with garbage keeps the value");
    }

    /// The preset list: nine of sixteen rows, scrolled by the wheel, and the
    /// snapshot a popup paints answers the same geometry as the field.
    #[test]
    fn the_preset_list_scrolls_and_snapshots() {
        let mut f = FontSizeField::with_presets(&FontSizeField::DEFAULT_PRESETS, 12.0).expect("in range");
        f.open = true;
        let b = Rect::new(0.0, 0.0, size_metrics::WIDTH, size_metrics::HEIGHT);
        f.scroll_by_dip(100.0);
        assert_eq!(f.scroll, 3, "100 DIP of wheel = three 30 DIP rows");
        f.scroll_by(99);
        assert_eq!(f.scroll, 7, "16 - 9");
        assert!(f.row_rect(b, 0).is_none());
        let row = f.row_rect(b, 15).expect("the last preset is in view");
        let view = f.list_view();
        assert_eq!(view.item_at_in(f.drop_down_rect(b), 5.0, (row.top + row.bottom) / 2.0), Some(15));
        f.open_list();
        assert_eq!(f.scroll, 4, "opening keeps the current size in view");
        assert_eq!(f.hot_index, Some(4));
    }

    // ── RichTextToolbar: keyboard and command states ──────────────────────

    /// The WAI-ARIA toolbar: arrows walk the buttons, skip the rules and
    /// wrap; Home / End; Enter presses the focused one.
    #[test]
    fn the_toolbar_is_one_tab_stop_with_arrows_inside() {
        let mut bar = RichTextToolbar::standard();
        assert_eq!(bar.targets(), vec![0, 1, 2, 4, 5, 7, 8], "the rules are not targets");
        assert_eq!(bar.entry_index(), Some(0));
        assert_eq!(bar.key_down(vk::LEFT, Modifiers::NONE), ToolbarKey::Moved(8), "wraps");
        assert_eq!(bar.key_down(vk::RIGHT, Modifiers::NONE), ToolbarKey::Moved(0));
        bar.key_down(vk::RIGHT, Modifiers::NONE);
        bar.key_down(vk::RIGHT, Modifiers::NONE);
        assert_eq!(bar.key_down(vk::RIGHT, Modifiers::NONE), ToolbarKey::Moved(4), "over the rule");
        assert_eq!(bar.key_down(vk::END, Modifiers::NONE), ToolbarKey::Moved(8));
        assert_eq!(bar.key_down(vk::HOME, Modifiers::NONE), ToolbarKey::Moved(0));
        assert_eq!(bar.key_down(vk::ENTER, Modifiers::NONE), ToolbarKey::Activated(0));
        assert!(bar.is_active(RichTextCommand::Bold));
        assert_eq!(bar.key_down(vk::TAB, Modifiers::NONE), ToolbarKey::Ignored);
        assert_eq!(bar.tooltip_of(0), Some("Gras"));
        assert_eq!(bar.tooltip_of(3), None, "a rule has no title");
        assert_eq!(bar.command_at(5), Some(RichTextCommand::BulletList));
    }

    /// The states follow the editor: styles flip, the two lists exclude each
    /// other, an alignment is a radio, ClearFormat drops the inline styles
    /// and Link changes nothing.
    #[test]
    fn toolbar_states_follow_the_document() {
        use RichTextCommand::*;
        let mut bar = RichTextToolbar::with_alignments();
        assert!(bar.apply(Bold));
        assert!(bar.apply(Underline));
        assert!(bar.apply(OrderedList));
        assert!(bar.apply(BulletList));
        assert!(!bar.is_active(OrderedList), "one list at a time");
        assert!(bar.is_active(BulletList));
        assert!(bar.apply(BulletList));
        assert!(!bar.is_active(BulletList), "a list toggles off");

        bar.apply(AlignCenter);
        bar.apply(AlignRight);
        assert!(bar.is_active(AlignRight) && !bar.is_active(AlignCenter));
        assert!(!bar.apply(AlignRight), "pressing the active alignment keeps it");

        assert!(!bar.apply(Link), "an action, not a state");
        assert!(bar.apply(ClearFormat));
        assert!(!bar.is_active(Bold) && !bar.is_active(Underline));
        assert!(bar.is_active(AlignRight), "clear formatting keeps the paragraph's alignment");
        assert!(!Link.is_toggle() && Bold.is_toggle());
        for c in RichTextCommand::ALL {
            assert_eq!(RichTextCommand::from_icon(c.icon()), Some(c));
        }

        // A disabled button is neither a target nor pressable.
        if let StripItem::Button(b) = &mut bar.items[0] {
            b.item.enabled = false;
        }
        assert!(!bar.targets().contains(&0));
        assert_eq!(bar.activate(0), None);
    }
}
