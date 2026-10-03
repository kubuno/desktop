//! The Office ribbon — a port of the web's shared ribbon
//! (`office/frontend/src/ribbon/`: `Ribbon.tsx`, `types.ts`, `officeThemes.ts`,
//! `Backstage.tsx`), the command surface every Kubuno editor wears.
//!
//! # Data-driven, like the web
//!
//! An editor DECLARES its tabs ([`RibbonTab`] → [`RibbonGroup`] →
//! [`RibbonItem`]); [`Ribbon::frame`] does all the layout, painting, pointer and
//! keyboard handling, and reports what happened as [`RibbonEvent`]s. Live state
//! (is Bold on? which font?) is written into the items by the caller — rebuild
//! the tabs every frame, or patch them through [`Ribbon::item_mut`].
//!
//! # What is ported
//!
//! * **The tab strip** (`RibbonTabStrip`): an optional coloured band (the app's
//!   tone, [`tone`]), the « Fichier » tab as a solid pill in a lighter shade of
//!   the tone ([`file_accent_for`]), contextual tabs with their coloured top
//!   rule and dot, a block of quick actions right after « Fichier », and the
//!   collapse button at the end of the strip (Ctrl+F1).
//! * **The group row** (`RibbonGroupsRow` / `RibbonGroupView`): 84 DIP, one box
//!   per group with its label at the bottom and a 1 DIP rule between groups;
//!   one-slot items (small buttons, toggles, splits) are stacked in columns of
//!   **three at most**, a separator never interrupting a stack; large buttons,
//!   drop-downs, galleries and menus take a column of their own.
//! * **Every item kind** of `RibbonItemKind`: button, toggle, drop-down (the
//!   library's [`Dropdown`] list, opened in a floating surface), split (main
//!   action + chevron menu), menu (the whole surface opens the menu), gallery,
//!   separator, custom (a caller-painted slot of a given width).
//! * **Responsive collapse**: when the groups do not fit, the right-most ones
//!   fold one by one into a chip (icon + chevron + label) whose click opens the
//!   whole group in a popover — never a scroll bar, never a clipped control.
//! * **Ribbon collapse + peek**: collapsed, the group row is gone; clicking a
//!   tab shows its groups in a floating flyout that closes on a click outside
//!   the ribbon or Escape.
//! * **Backstage**: a tab declared with `backstage` hides the group row and
//!   reports its area; [`Backstage`] paints the web's accent rail and sections.
//!
//! The web's MOBILE ribbon (bottom bar + palette sheet) is not ported: the
//! desktop has no such viewport.

//!
//! # Beyond the web (`vskubuno/docs/RIBBON.md` §7.2)
//!
//! Owned icon names ([`Icon`]), the rectangle of every tab, group, control and menu entry
//! ([`RibbonRun::regions`], what the designer selects and the view runtime routes events to),
//! contextual tab groups with a coloured header, check boxes, editable combo boxes, numeric fields
//! and text boxes, joined control groups and boxes, a dialog launcher per group, a group icon for
//! the folded chip, cascaded sub-menus, multi-row galleries with categories (in the ribbon, as a
//! drop-down, in a menu), a colour picker over the Docs palette, the quick access toolbar below
//! the ribbon and its « add / remove » menu, KeyTips (Alt), size levels with `SizeDefinition`
//! templates and per-tab scaling policies ([`scaling`]), a simplified one-row display mode, and a
//! design mode (no input, every contextual tab shown, an inline drop-down, « + » glyphs). A ribbon
//! using none of them renders exactly as before.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

mod keytips;
mod panel;
pub mod scaling;
pub mod merge;

pub use keytips::{assign as assign_key_tips, conflicts as key_tip_conflicts};
pub use scaling::{ControlSize, GroupSize, ImageSize, LevelDefinition, ScaleStep, ScalingPolicy, SizeDefinition, Template};

use kubuno_drive_desktop_app_controls::{Canvas, Rect, ThemeMode};
use kubuno_desktop_controls::enums::Size;
use kubuno_desktop_controls::host::{self, vk, Modifiers};
use windows::core::HSTRING;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};

use crate::display::{place, Placement, Side, Tooltip, TooltipTrigger};
use crate::editors::{Dropdown, ListKey};
use crate::lists::{self, Menu, MenuEntry, MenuKey, MenuOutcome};
use crate::metrics::{SHADOW_GREY, SHADOW_MENU};
use crate::{Widget, WidgetState};

// ═════════════════════════════════════════════════════════════════════════════
// Metrics — the web's Tailwind classes, in DIP
// ═════════════════════════════════════════════════════════════════════════════

pub mod metrics {
    /// `TAB_H`: the tab strip.
    pub const TAB_H: f32 = 30.0;
    /// `h-[26px]`: a tab on the coloured strip, sitting on the strip's bottom.
    pub const COLORED_TAB_H: f32 = 26.0;
    /// `CONTENT_H`: the group row (items + group label), its bottom rule included.
    pub const CONTENT_H: f32 = 84.0;
    /// `px-2` of the strip and of the group row.
    pub const EDGE_PAD: f32 = 8.0;
    /// `px-3.5` of a tab.
    pub const TAB_PAD_X: f32 = 14.0;
    /// `gap-0.5` between tabs.
    pub const TAB_GAP: f32 = 2.0;
    /// `borderTopLeftRadius / borderTopRightRadius: 5`.
    pub const TAB_RADIUS: f32 = 5.0;
    /// `text-[14px]`.
    pub const TAB_FONT: f32 = 14.0;
    /// A contextual tab's `borderTop: 2px`.
    pub const CONTEXT_RULE: f32 = 2.0;
    /// A contextual tab's dot (`fontSize: 9`, `marginRight: 4`).
    pub const CONTEXT_DOT: f32 = 9.0;
    pub const CONTEXT_DOT_GAP: f32 = 4.0;
    /// The active tab's underline on a plain (uncoloured) strip: `h 2`, inset 6.
    pub const UNDERLINE_H: f32 = 2.0;
    pub const UNDERLINE_INSET: f32 = 6.0;
    /// The collapse button: `h-6 w-6`, `ml-1 mb-0.5`, chevron 16.
    pub const COLLAPSE_BTN: f32 = 24.0;
    pub const COLLAPSE_ICON: f32 = 16.0;
    /// A quick action of the strip (Save, Undo, Redo…): a 28 DIP square, icon 16.
    pub const ACTION_BTN: f32 = 28.0;
    /// `mx-1` around the quick-action block.
    pub const ACTION_MARGIN: f32 = 4.0;

    /// `px-2 py-0.5` of a group.
    pub const GROUP_PAD_X: f32 = 8.0;
    pub const GROUP_PAD_Y: f32 = 2.0;
    /// `gap-0.5` between a group's columns, `gap-[1px]` inside a column.
    pub const COLUMN_GAP: f32 = 2.0;
    pub const STACK_GAP: f32 = 1.0;
    /// The group label: `text-[10px]`, one line.
    pub const GROUP_LABEL_FONT: f32 = 10.0;
    pub const GROUP_LABEL_H: f32 = 14.0;
    /// Items: `text-[11px]`.
    pub const ITEM_FONT: f32 = 11.0;
    /// `MAX_STACK`: one-slot items per column.
    pub const MAX_STACK: usize = 3;
    /// A small button: `h-[20px] px-1.5 gap-1`, icon slot 16, chevron 11.
    pub const SMALL_H: f32 = 20.0;
    pub const SMALL_PAD_X: f32 = 6.0;
    pub const SMALL_GAP: f32 = 4.0;
    pub const SMALL_ICON: f32 = 16.0;
    pub const CHEVRON: f32 = 11.0;
    /// A small split's separate chevron button: `w-4 h-[20px]`.
    pub const SPLIT_CHEVRON_W: f32 = 16.0;
    /// A large button: `px-2`, `min-w-[3.5rem] max-w-[200px]`, icon 32, label
    /// block `max-width: 4.6rem`, two `leading-tight` lines always reserved.
    pub const LARGE_PAD_X: f32 = 8.0;
    pub const LARGE_MIN_W: f32 = 56.0;
    pub const LARGE_MAX_W: f32 = 200.0;
    pub const LARGE_ICON: f32 = 32.0;
    pub const LARGE_LABEL_W: f32 = 73.6;
    pub const LARGE_LABEL_H: f32 = 26.4;
    /// A drop-down: `height={24}`, default `width 120`.
    pub const DROPDOWN_H: f32 = 24.0;
    pub const DROPDOWN_W: f32 = 120.0;
    /// A gallery option: `px-2 h-7`, 1 DIP border.
    pub const GALLERY_H: f32 = 28.0;
    pub const GALLERY_PAD_X: f32 = 8.0;
    /// `rounded-xs`.
    pub const ITEM_RADIUS: f32 = 2.0;

    /// A folded group: `minWidth 48`, icon 22, chevron 12, `gap-1`.
    pub const CHIP_MIN_W: f32 = 48.0;
    pub const CHIP_ICON: f32 = 22.0;
    pub const CHIP_CHEVRON: f32 = 12.0;
    pub const CHIP_GAP: f32 = 4.0;
    /// The folded group's popover: `padding 4`, `borderRadius 8`, 2 DIP under
    /// the chip, kept 8 DIP inside the screen.
    pub const POPOVER_PAD: f32 = 4.0;
    pub const POPOVER_RADIUS: f32 = 8.0;
    pub const POPOVER_GAP: f32 = 2.0;
    pub const SCREEN_MARGIN: f32 = 8.0;

    /// The Backstage rail: `w-60`, rows `h-9 px-5 gap-3`, icon slot 20, the
    /// spacer the web keeps where the back arrow was (`h-10 mb-2`).
    pub const BACKSTAGE_RAIL_W: f32 = 240.0;
    pub const BACKSTAGE_ROW_H: f32 = 36.0;
    pub const BACKSTAGE_PAD_X: f32 = 20.0;
    pub const BACKSTAGE_GAP: f32 = 12.0;
    pub const BACKSTAGE_ICON: f32 = 17.0;
    pub const BACKSTAGE_ICON_SLOT: f32 = 20.0;
    pub const BACKSTAGE_TOP: f32 = 48.0;
    /// The rows' `text-xs` (`Backstage.tsx`): the meta role.
    pub const BACKSTAGE_FONT: f32 = crate::metrics::text::META;

    // ── Beyond the web port (RIBBON.md §7.2) ─────────────────────────────
    /// A check box's square, and its corner radius.
    pub const CHECK_BOX: f32 = 14.0;
    /// A numeric field's default width, and the column of its up/down arrows.
    pub const NUMERIC_W: f32 = 64.0;
    pub const NUMERIC_ARROWS_W: f32 = 14.0;
    /// A text box's default width.
    pub const TEXT_BOX_W: f32 = 120.0;
    /// An editable combo box's chevron column.
    pub const COMBO_CHEVRON_W: f32 = 20.0;
    /// The dialog launcher at the bottom right of a group.
    pub const LAUNCHER: f32 = 14.0;
    /// A gallery cell's default size, and the scroll column of an in-ribbon gallery.
    pub const GALLERY_ITEM_W: f32 = 72.0;
    pub const GALLERY_ITEM_H: f32 = 56.0;
    pub const GALLERY_SCROLL_W: f32 = 16.0;
    /// A colour picker's colour bar under its icon.
    pub const COLOR_BAR_H: f32 = 3.0;
    /// The swatches of a colour picker's panel.
    pub const SWATCH: f32 = 16.0;
    /// A contextual tab group's header chip: height, padding, font.
    pub const CTX_HEADER_H: f32 = 18.0;
    pub const CTX_HEADER_PAD_X: f32 = 8.0;
    pub const CTX_HEADER_FONT: f32 = 10.0;
    /// The quick access toolbar's row when it sits below the ribbon.
    pub const QAT_ROW_H: f32 = 28.0;
    /// The one-row group row of the simplified display mode.
    pub const SIMPLIFIED_H: f32 = 40.0;
    /// The designer's « + » glyph.
    pub const ADD_GLYPH: f32 = 16.0;
    /// A KeyTip badge's font.
    pub const KEYTIP_FONT: f32 = 10.0;
}

use metrics as m;

// ═════════════════════════════════════════════════════════════════════════════
// Colours — `officeThemes.ts`
// ═════════════════════════════════════════════════════════════════════════════

/// `0xRRGGBB` as an opaque colour.
pub const fn hex(rgb: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

/// `OFFICE_TONE`: one tab-strip colour per editor, in the spirit of MS Office.
/// The colour is ALSO the accent (active tab text, active item tint).
pub mod tone {
    use super::hex;
    use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
    pub const DOCUMENTS: D2D1_COLOR_F = hex(0x1557b0);
    pub const SPREADSHEET: D2D1_COLOR_F = hex(0x0f7b3f);
    pub const PRESENTATION: D2D1_COLOR_F = hex(0xb7472a);
    pub const PROJECTS: D2D1_COLOR_F = hex(0x1e7a6f);
    pub const DIAGRAMS: D2D1_COLOR_F = hex(0x3b53b5);
    pub const DATA: D2D1_COLOR_F = hex(0x0e7490);
    pub const MATHS: D2D1_COLOR_F = hex(0x6a3fa0);
    pub const WHITEBOARD: D2D1_COLOR_F = hex(0x5b4bd0);
}

/// WCAG 2 relative luminance of an opaque colour.
pub fn relative_luminance(c: D2D1_COLOR_F) -> f32 {
    let lin = |v: f32| if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG 2 contrast ratio of two colours (1..21).
pub fn contrast_ratio(a: D2D1_COLOR_F, b: D2D1_COLOR_F) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// A contextual tab group's header chip on the tab strip: its accent with white text, or — when the
/// accent is too close to the strip (a green group on the green Spreadsheet tone) — a pale tint of
/// the accent with dark ink, like Office, so the chip stays distinct and its text readable.
pub fn header_colors(accent: D2D1_COLOR_F, strip: D2D1_COLOR_F) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
    // Distinct from the strip: a different hue or lightness (RGB distance), not only luminance — a
    // green chip on a blue strip has close luminances but reads fine.
    let distance = ((accent.r - strip.r).powi(2) + (accent.g - strip.g).powi(2) + (accent.b - strip.b).powi(2)).sqrt();
    if distance >= 0.25 && contrast_ratio(WHITE, accent) >= 3.0 {
        return (accent, WHITE);
    }
    let fill = lighten(accent, 0.82);
    let ink = [accent, hex(0x1f1f1f)]
        .into_iter()
        .find(|ink| contrast_ratio(*ink, fill) >= 4.5)
        .unwrap_or(hex(0x1f1f1f));
    (fill, ink)
}

/// lighten: mixes `c` towards white by `f` (0..1), on 8-bit channels like
/// the web.
pub fn lighten(c: D2D1_COLOR_F, f: f32) -> D2D1_COLOR_F {
    let mix = |v: f32| {
        let b = (v * 255.0).round();
        (b + (255.0 - b) * f).round() / 255.0
    };
    D2D1_COLOR_F { r: mix(c.r), g: mix(c.g), b: mix(c.b), a: c.a }
}

/// `fileAccentFor`: the « Fichier » tab and Backstage rail colour for an app
/// accent — a lighter shade that stands out on the dark strip; Documents keeps
/// the shade the web chose by hand.
pub fn file_accent_for(accent: D2D1_COLOR_F) -> D2D1_COLOR_F {
    let same = |a: D2D1_COLOR_F, b: D2D1_COLOR_F| {
        (a.r - b.r).abs() < 0.002 && (a.g - b.g).abs() < 0.002 && (a.b - b.b).abs() < 0.002
    };
    if same(accent, tone::DOCUMENTS) {
        hex(0x3f7dd0)
    } else {
        lighten(accent, 0.3)
    }
}

/// Which ribbon look: `officeTheme(color)` (a coloured strip in the app's
/// tone, the tone as accent) or the plain workspace chrome (a strip on the
/// page ground, an accent underline under the active tab).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RibbonTheme {
    /// The strip colour; `None` = a plain strip.
    pub tone: Option<D2D1_COLOR_F>,
    /// Active tab text, active item tint.
    pub accent: D2D1_COLOR_F,
}

impl RibbonTheme {
    /// `officeTheme(color)`: the coloured strip of the Office editors.
    pub fn office(tone: D2D1_COLOR_F) -> Self {
        Self { tone: Some(tone), accent: tone }
    }

    /// The plain workspace chrome (`WORKSPACE_LIGHT`), accent `#1a73e8`.
    pub fn plain() -> Self {
        Self { tone: None, accent: hex(0x1a73e8) }
    }

    /// The « Fichier » tab and Backstage rail colour.
    pub fn file_accent(&self) -> D2D1_COLOR_F {
        file_accent_for(self.accent)
    }
}

impl Default for RibbonTheme {
    fn default() -> Self {
        Self::office(tone::DOCUMENTS)
    }
}

/// The resolved palette of one frame: the ribbon theme over the canvas theme
/// (light or dark).
#[derive(Debug, Clone, Copy)]
struct Colors {
    colored:     bool,
    bg:          D2D1_COLOR_F,
    border:      D2D1_COLOR_F,
    text:        D2D1_COLOR_F,
    text_dim:    D2D1_COLOR_F,
    hover:       D2D1_COLOR_F,
    active_bg:   D2D1_COLOR_F,
    active_fg:   D2D1_COLOR_F,
    strip_bg:    D2D1_COLOR_F,
    strip_text:  D2D1_COLOR_F,
    strip_hover: D2D1_COLOR_F,
    file_bg:     D2D1_COLOR_F,
    accent:      D2D1_COLOR_F,
    field_bg:    D2D1_COLOR_F,
}

const WHITE: D2D1_COLOR_F = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

fn alpha(c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * a, ..c }
}

impl Colors {
    fn resolve(c: &dyn Canvas, rt: &RibbonTheme) -> Self {
        let t = c.theme();
        let dark = t.mode == ThemeMode::Dark;
        let colored = rt.tone.is_some();
        // `var(--kbn-ws-hover, #f1f3f4)` / dark `rgba(255,255,255,0.08)`.
        let hover = if dark { alpha(WHITE, 0.08) } else { t.row_hover };
        // The web's « Kubuno Dark » theme (the source of truth, its `--kbn-office-*` variables): the
        // tab strip takes the window's ground (`--kbn-office-tabstrip: #17181b`) instead of the app's
        // tone, its text the primary ink, an active item and an active tab the theme's accent
        // (`--kbn-office-item-active-bg: rgba(138,180,248,0.16)`, `--kbn-office-tab-active-text`).
        // Light: `${accent}22` (≈13 %) and the tone.
        let active_bg = if dark { alpha(t.accent, 0.16) } else { alpha(rt.accent, 0.13) };
        let active_fg = if dark { t.accent } else { rt.accent };
        let (strip_bg, strip_text, strip_hover) = match (rt.tone, dark) {
            (Some(_), true) => (t.window_background, t.text_primary, alpha(WHITE, 0.08)),
            (Some(tone), false) => (tone, WHITE, alpha(WHITE, 0.16)),
            (None, _) => (t.layer_background, t.text_secondary, hover),
        };
        // « Fichier » and the Backstage rail: in the dark theme the web lightens the workspace
        // accent (`fileAccentFor(#1a73e8)` = #5f9def), which stands out of the dark strip.
        let file_bg = if dark && colored { file_accent_for(RibbonTheme::plain().accent) } else { rt.file_accent() };
        Self {
            colored,
            bg: t.layer_background,
            border: t.card_stroke,
            text: t.text_primary,
            text_dim: t.text_secondary,
            hover,
            active_bg,
            active_fg,
            strip_bg,
            strip_text,
            strip_hover,
            file_bg,
            accent: active_fg,
            field_bg: t.layer_background,
        }
    }

    /// `disabled:opacity-40`.
    fn faded(c: D2D1_COLOR_F, disabled: bool) -> D2D1_COLOR_F {
        if disabled {
            alpha(c, 0.4)
        } else {
            c
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Text formats at the ribbon's sizes (11 / 10 / 14 DIP)
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Clone)]
struct Fonts {
    item:      IDWriteTextFormat,
    item_wrap: IDWriteTextFormat,
    label:     IDWriteTextFormat,
    tab:       IDWriteTextFormat,
    tab_file:  IDWriteTextFormat,
    dot:       IDWriteTextFormat,
    section:   IDWriteTextFormat,
    section_b: IDWriteTextFormat,
    /// A contextual group header and a KeyTip badge (10 DIP, semibold).
    small_b:   IDWriteTextFormat,
}

/// The format cache: the shared factory, the UI family the entries were made
/// for, and `((size in 1/100 DIP, weight, wraps), format)`.
type FormatCache = (Option<IDWriteFactory>, String, Vec<((u32, u32, bool), IDWriteTextFormat)>);

thread_local! {
    static FORMATS: RefCell<FormatCache> =
        const { RefCell::new((None, String::new(), Vec::new())) };
}

fn body_family(c: &dyn Canvas) -> String {
    let f = &c.formats().body;
    // SAFETY: plain COM getters on a live text format.
    unsafe {
        let n = f.GetFontFamilyNameLength() as usize;
        let mut buf = vec![0u16; n + 1];
        if f.GetFontFamilyName(&mut buf).is_err() {
            return String::from("Segoe UI");
        }
        String::from_utf16_lossy(&buf[..n])
    }
}

/// A text format of the UI family at `size` DIP and `weight`, cached. Falls
/// back to the closest shared format when DirectWrite refuses.
fn format(c: &dyn Canvas, size: f32, weight: u32, wrap: bool) -> IDWriteTextFormat {
    let key = ((size * 100.0).round() as u32, weight, wrap);
    let family = body_family(c);
    let made = FORMATS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.1 != family {
            cache.1 = family.clone();
            cache.2.clear();
        }
        if let Some((_, f)) = cache.2.iter().find(|(k, _)| *k == key) {
            return Some(f.clone());
        }
        if cache.0.is_none() {
            // SAFETY: creating the shared DirectWrite factory has no preconditions.
            cache.0 = unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }.ok();
        }
        let factory = cache.0.clone()?;
        // SAFETY: plain COM calls on a live factory.
        let f = unsafe {
            factory
                .CreateTextFormat(
                    &HSTRING::from(family.as_str()),
                    None,
                    DWRITE_FONT_WEIGHT(weight as i32),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    &HSTRING::from("fr-FR"),
                )
                .and_then(|f| {
                    f.SetWordWrapping(if wrap { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP })
                        .map(|_| f)
                })
        }
        .ok()?;
        cache.2.push((key, f.clone()));
        Some(f)
    });
    made.unwrap_or_else(|| if size >= 14.0 { c.formats().body.clone() } else { c.formats().caption.clone() })
}

impl Fonts {
    fn of(c: &dyn Canvas) -> Self {
        Self {
            item: format(c, m::ITEM_FONT, 400, false),
            item_wrap: format(c, m::ITEM_FONT, 400, true),
            label: format(c, m::GROUP_LABEL_FONT, 400, false),
            tab: format(c, m::TAB_FONT, 500, false),
            tab_file: format(c, m::TAB_FONT, 600, false),
            dot: format(c, m::CONTEXT_DOT, 400, false),
            section: format(c, m::BACKSTAGE_FONT, 400, false),
            section_b: format(c, m::BACKSTAGE_FONT, 600, false),
            small_b: format(c, m::CTX_HEADER_FONT, 600, false),
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// The data model — `types.ts`
// ═════════════════════════════════════════════════════════════════════════════

/// `RibbonItemKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// An action button (large = icon over label; small = compact).
    Button,
    /// A toggle (its active state is highlighted).
    Toggle,
    /// A drop-down list (font, size…).
    Dropdown,
    /// A main action plus a small chevron opening a menu of options.
    Split,
    /// A button WITHOUT an action of its own: its whole surface opens the menu.
    Menu,
    /// A row of options.
    Gallery,
    /// A vertical separator (ignored when stacking — see [`MAX_STACK`]).
    ///
    /// [`MAX_STACK`]: metrics::MAX_STACK
    Separator,
    /// A slot of a given width the caller paints itself.
    Custom,
    /// A check box and its label (`active` is the check).
    CheckBox,
    /// A combo box: a drop-down list whose text can be typed when `editable`.
    ComboBox,
    /// A number with up/down arrows (`value`, `range`).
    NumericField,
    /// A one-line text field (`value`).
    TextBox,
    /// A split button whose menu is a colour palette; `color` is shown under its icon.
    ColorPicker,
    /// A static text (an optional icon before it).
    Label,
    /// Buttons joined in one row (`children`: B I U…); one stack slot.
    ControlGroup,
    /// A row (one stack slot) or, `vertical`, a column of its `children`.
    Box,
}

/// Where a gallery shows its items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GalleryDisplay {
    /// A grid inside the group, with a scroll column and a « more » button opening all the items.
    #[default]
    InRibbon,
    /// A button opening the grid in a drop-down panel.
    DropDown,
    /// Inside a menu (an entry of a split / menu button).
    InMenu,
}

/// The grid of a gallery (`RibbonGallery`): absent, a gallery is the web's one row of options.
#[derive(Debug, Clone, PartialEq)]
pub struct GallerySpec {
    pub display: GalleryDisplay,
    /// Rows shown in the ribbon, columns shown in the ribbon (and in the drop-down panel).
    pub rows: usize,
    pub columns: usize,
    pub item_w: f32,
    pub item_h: f32,
    /// The first row shown in the ribbon (the scroll position, kept by the ribbon).
    pub first_row: usize,
}

impl Default for GallerySpec {
    fn default() -> Self {
        Self { display: GalleryDisplay::InRibbon, rows: 1, columns: 4, item_w: m::GALLERY_ITEM_W, item_h: m::GALLERY_ITEM_H, first_row: 0 }
    }
}

/// An owned or static icon name of the shared icon set (a `.kbview` attribute is not `'static`).
pub type Icon = Cow<'static, str>;

thread_local! {
    static INTERNED: RefCell<std::collections::HashSet<&'static str>> = RefCell::new(std::collections::HashSet::new());
}

/// The `'static` name the canvas needs for `icon`. An owned name is interned once: icon names
/// are a small closed set, so the few bytes leaked per distinct name are bounded.
pub fn icon_str(icon: &Icon) -> &'static str {
    match icon {
        Cow::Borrowed(s) => s,
        Cow::Owned(s) => INTERNED.with(|set| {
            if let Some(found) = set.borrow().get(s.as_str()) {
                return *found;
            }
            let leaked: &'static str = Box::leak(s.clone().into_boxed_str());
            set.borrow_mut().insert(leaked);
            leaked
        }),
    }
}

/// `size?: 'large' | 'small'` (default small).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemSize {
    #[default]
    Small,
    Large,
}

/// `RibbonOption`: one choice of a drop-down or a gallery.
#[derive(Debug, Clone, PartialEq)]
pub struct RibbonOption {
    pub value:    String,
    pub label:    String,
    /// A Lucide icon name from the shared icon set.
    pub icon:     Option<Icon>,
    /// A gallery cell previewing a colour.
    pub color:    Option<D2D1_COLOR_F>,
    /// The gallery category it is listed under (its header in the drop-down panel).
    pub category: Option<String>,
}

impl RibbonOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self { value: value.into(), label: label.into(), icon: None, color: None, category: None }
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn color(mut self, color: D2D1_COLOR_F) -> Self {
        self.color = Some(color);
        self
    }

    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }
}

/// What a custom slot paints: the canvas, its rectangle, and its state.
pub type CustomPaint = Rc<dyn Fn(&dyn Canvas, Rect, WidgetState)>;

/// `RibbonItem`.
#[derive(Clone)]
pub struct RibbonItem {
    pub id:          String,
    pub kind:        ItemKind,
    /// A large button shows it; otherwise it is the tooltip's fallback.
    pub label:       Option<String>,
    /// A Lucide icon name from the shared icon set.
    pub icon:        Option<Icon>,
    pub size:        ItemSize,
    pub active:      bool,
    pub disabled:    bool,
    pub tooltip:     Option<String>,
    /// Shown in the tooltip (« Copier · Ctrl+C »).
    pub shortcut:    Option<String>,
    /// Drop-down / gallery choices.
    pub options:     Vec<RibbonOption>,
    /// The selected option's value.
    pub value:       Option<String>,
    /// A drop-down's width (default 120) or a custom slot's width.
    pub width:       Option<f32>,
    /// A split or menu button's entries.
    pub split_items: Vec<RibbonItem>,
    /// Whether the item has its own action (`onClick`): a split or menu
    /// WITHOUT one opens its menu from the whole surface.
    pub has_action:  bool,
    /// A custom slot's painter.
    pub render:      Option<CustomPaint>,
    /// Shown (`Visible`); a hidden item takes no room.
    pub visible:     bool,
    /// The members of a control group or a box.
    pub children:    Vec<RibbonItem>,
    /// A box stacks its children vertically (it then takes a column of its own).
    pub vertical:    bool,
    /// A combo box whose text can be typed.
    pub editable:    bool,
    /// A numeric field's `(minimum, maximum, increment)`.
    pub range:       Option<(f32, f32, f32)>,
    /// A gallery's grid (see [`GallerySpec`]); `None` is the web's single row.
    pub gallery:     Option<GallerySpec>,
    /// A colour picker's current colour (its bar under the icon).
    pub color:       Option<D2D1_COLOR_F>,
    /// The KeyTip (assigned automatically when `None`).
    pub key_tip:     Option<String>,
    /// Starts a new column (a size definition's `ColumnBreak`).
    pub column_break: bool,
    /// Offers « Add to the quick access toolbar » on a right click.
    pub qat:         bool,
}

impl std::fmt::Debug for RibbonItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RibbonItem").field("id", &self.id).field("kind", &self.kind).finish()
    }
}

impl RibbonItem {
    fn new(id: impl Into<String>, kind: ItemKind) -> Self {
        Self {
            id: id.into(),
            kind,
            label: None,
            icon: None,
            size: ItemSize::Small,
            active: false,
            disabled: false,
            tooltip: None,
            shortcut: None,
            options: Vec::new(),
            value: None,
            width: None,
            split_items: Vec::new(),
            has_action: true,
            render: None,
            visible: true,
            children: Vec::new(),
            vertical: false,
            editable: false,
            range: None,
            gallery: None,
            color: None,
            key_tip: None,
            column_break: false,
            qat: true,
        }
    }

    /// An empty item of `kind` (the `kubuno-desktop-views` ribbon fills it field by field).
    pub fn of_kind(id: impl Into<String>, kind: ItemKind) -> Self {
        Self::new(id, kind)
    }

    /// An action button.
    pub fn button(id: impl Into<String>, label: impl Into<String>, icon: impl Into<Icon>) -> Self {
        Self::new(id, ItemKind::Button).label(label).icon(icon)
    }

    /// A toggle.
    pub fn toggle(id: impl Into<String>, label: impl Into<String>, icon: impl Into<Icon>, active: bool) -> Self {
        Self::new(id, ItemKind::Toggle).label(label).icon(icon).active(active)
    }

    /// A check box.
    pub fn check_box(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        Self::new(id, ItemKind::CheckBox).label(label).active(checked)
    }

    /// A combo box (`editable`: its text can be typed).
    pub fn combo_box(id: impl Into<String>, options: Vec<RibbonOption>, value: impl Into<String>, width: f32, editable: bool) -> Self {
        let mut it = Self::new(id, ItemKind::ComboBox);
        it.options = options;
        it.value = Some(value.into());
        it.width = Some(width);
        it.editable = editable;
        it
    }

    /// A numeric field.
    pub fn numeric(id: impl Into<String>, value: f32, min: f32, max: f32, step: f32, width: f32) -> Self {
        let mut it = Self::new(id, ItemKind::NumericField);
        it.value = Some(format_number(value));
        it.range = Some((min, max, step));
        it.width = Some(width);
        it
    }

    /// A text box.
    pub fn text_box(id: impl Into<String>, value: impl Into<String>, width: f32) -> Self {
        let mut it = Self::new(id, ItemKind::TextBox);
        it.value = Some(value.into());
        it.width = Some(width);
        it
    }

    /// A colour picker: the main part applies `color`, the chevron opens the palette.
    pub fn color_picker(id: impl Into<String>, label: impl Into<String>, icon: impl Into<Icon>, color: Option<D2D1_COLOR_F>) -> Self {
        let mut it = Self::new(id, ItemKind::ColorPicker).label(label).icon(icon);
        it.color = color;
        it
    }

    /// A static text.
    pub fn text_label(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(id, ItemKind::Label).label(text)
    }

    /// Buttons joined in one row.
    pub fn control_group(id: impl Into<String>, children: Vec<RibbonItem>) -> Self {
        let mut it = Self::new(id, ItemKind::ControlGroup);
        it.children = children;
        it
    }

    /// A row (or, `vertical`, a column) of controls.
    pub fn row_box(id: impl Into<String>, vertical: bool, children: Vec<RibbonItem>) -> Self {
        let mut it = Self::new(id, ItemKind::Box);
        it.children = children;
        it.vertical = vertical;
        it
    }

    /// A gallery laid out as a grid (see [`GallerySpec`]).
    pub fn gallery_grid(id: impl Into<String>, options: Vec<RibbonOption>, spec: GallerySpec) -> Self {
        let mut it = Self::gallery(id, options);
        it.gallery = Some(spec);
        it
    }

    /// A split button: its surface runs the action, its chevron opens `items`.
    pub fn split(id: impl Into<String>, label: impl Into<String>, icon: impl Into<Icon>, items: Vec<RibbonItem>) -> Self {
        let mut it = Self::new(id, ItemKind::Split).label(label).icon(icon);
        it.split_items = items;
        it
    }

    /// A menu button: its whole surface opens `items`.
    pub fn menu(id: impl Into<String>, label: impl Into<String>, icon: impl Into<Icon>, items: Vec<RibbonItem>) -> Self {
        let mut it = Self::new(id, ItemKind::Menu).label(label).icon(icon);
        it.split_items = items;
        it.has_action = false;
        it
    }

    /// A drop-down list.
    pub fn dropdown(id: impl Into<String>, options: Vec<RibbonOption>, value: impl Into<String>, width: f32) -> Self {
        let mut it = Self::new(id, ItemKind::Dropdown);
        it.options = options;
        it.value = Some(value.into());
        it.width = Some(width);
        it
    }

    /// A gallery of options.
    pub fn gallery(id: impl Into<String>, options: Vec<RibbonOption>) -> Self {
        let mut it = Self::new(id, ItemKind::Gallery);
        it.options = options;
        it
    }

    /// A separator.
    pub fn separator(id: impl Into<String>) -> Self {
        Self::new(id, ItemKind::Separator)
    }

    /// A caller-painted slot `width` DIP wide.
    pub fn custom(id: impl Into<String>, width: f32, render: CustomPaint) -> Self {
        let mut it = Self::new(id, ItemKind::Custom);
        it.width = Some(width);
        it.render = Some(render);
        it
    }

    /// A menu entry (for [`RibbonItem::split`] / [`RibbonItem::menu`]).
    pub fn entry(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, ItemKind::Button).label(label)
    }

    /// An empty label is no label: the item is an icon, the tooltip names it.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.label = (!label.is_empty()).then_some(label);
        self
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        let icon = icon.into();
        self.icon = (!icon.is_empty()).then_some(icon);
        self
    }

    pub fn key_tip(mut self, tip: impl Into<String>) -> Self {
        self.key_tip = Some(tip.into());
        self
    }

    pub fn large(mut self) -> Self {
        self.size = ItemSize::Large;
        self
    }

    pub fn active(mut self, on: bool) -> Self {
        self.active = on;
        self
    }

    pub fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }

    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn shortcut(mut self, keys: impl Into<String>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    /// The split/menu surface has no action of its own: it opens the menu.
    pub fn without_action(mut self) -> Self {
        self.has_action = false;
        self
    }

    /// Whether this item takes one stack slot (`toColumns`'s `stackable`).
    fn stackable(&self) -> bool {
        match self.kind {
            ItemKind::Button | ItemKind::Toggle | ItemKind::Split | ItemKind::ColorPicker => self.size == ItemSize::Small,
            ItemKind::CheckBox | ItemKind::Label | ItemKind::ControlGroup => true,
            ItemKind::Box => !self.vertical,
            _ => false,
        }
    }

    fn opens_menu(&self) -> bool {
        matches!(self.kind, ItemKind::Split | ItemKind::Menu | ItemKind::ColorPicker) || self.is_drop_down_gallery()
    }

    /// A gallery shown as a button opening its grid.
    fn is_drop_down_gallery(&self) -> bool {
        self.kind == ItemKind::Gallery && self.gallery.as_ref().is_some_and(|g| g.display == GalleryDisplay::DropDown)
    }

    /// A gallery laid out as a grid inside the group.
    fn is_grid_gallery(&self) -> bool {
        self.kind == ItemKind::Gallery && self.gallery.as_ref().is_some_and(|g| g.display == GalleryDisplay::InRibbon)
    }

    /// Its menu holds a gallery (or it is a colour picker): the menu opens as a panel.
    fn menu_is_panel(&self) -> bool {
        self.kind == ItemKind::ColorPicker || self.is_drop_down_gallery() || self.split_items.iter().any(|e| e.kind == ItemKind::Gallery)
    }

    /// `[tooltip ?? label, shortcut].join(' · ')`.
    fn tip_text(&self) -> Option<String> {
        let head = self.tooltip.clone().or_else(|| self.label.clone());
        let parts: Vec<String> = head.into_iter().chain(self.shortcut.clone()).collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

/// `RibbonGroup`.
#[derive(Debug, Clone)]
pub struct RibbonGroup {
    pub id:       String,
    pub label:    String,
    pub items:    Vec<RibbonItem>,
    /// The icon of its folded chip (else its first item's).
    pub icon:     Option<Icon>,
    /// Shows the dialog launcher (reported as [`RibbonEvent::Launcher`]).
    pub launcher: bool,
    /// How its controls look at each size level.
    pub size:     SizeDefinition,
    pub key_tip:  Option<String>,
    pub visible:  bool,
}

impl RibbonGroup {
    pub fn new(id: impl Into<String>, label: impl Into<String>, items: Vec<RibbonItem>) -> Self {
        Self { id: id.into(), label: label.into(), items, icon: None, launcher: false, size: SizeDefinition::Auto, key_tip: None, visible: true }
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn launcher(mut self, on: bool) -> Self {
        self.launcher = on;
        self
    }

    pub fn size_definition(mut self, size: SizeDefinition) -> Self {
        self.size = size;
        self
    }
}

/// A contextual tab's accent (`contextual: { accent }`), and the contextual tab group it belongs
/// to: consecutive tabs of one group share a coloured header chip naming it (« Outils de tableau »).
#[derive(Debug, Clone, PartialEq)]
pub struct Contextual {
    pub accent: D2D1_COLOR_F,
    /// The contextual tab group's id and header.
    pub group:  Option<String>,
    pub header: Option<String>,
}

/// `RibbonTab`.
#[derive(Debug, Clone)]
pub struct RibbonTab {
    pub id:         String,
    pub label:      String,
    pub groups:     Vec<RibbonGroup>,
    /// A contextual tab: shown on the right side with a coloured rule, and
    /// only while `visible`.
    pub contextual: Option<Contextual>,
    pub visible:    bool,
    /// The « Fichier » tab: while active, the group row is replaced by the
    /// Backstage the caller paints.
    pub backstage:  bool,
    /// The order in which its groups shrink (empty: the automatic folding).
    pub scaling:    ScalingPolicy,
    pub key_tip:    Option<String>,
}

impl RibbonTab {
    pub fn new(id: impl Into<String>, label: impl Into<String>, groups: Vec<RibbonGroup>) -> Self {
        Self { id: id.into(), label: label.into(), groups, contextual: None, visible: true, backstage: false, scaling: ScalingPolicy::default(), key_tip: None }
    }

    /// A tab of the contextual tab group `group`, whose header chip reads `header`.
    pub fn contextual_group(mut self, accent: D2D1_COLOR_F, visible: bool, group: impl Into<String>, header: impl Into<String>) -> Self {
        self.contextual = Some(Contextual { accent, group: Some(group.into()), header: Some(header.into()) });
        self.visible = visible;
        self
    }

    /// The « Fichier » tab.
    pub fn file(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { backstage: true, ..Self::new(id, label, Vec::new()) }
    }

    pub fn contextual(mut self, accent: D2D1_COLOR_F, visible: bool) -> Self {
        self.contextual = Some(Contextual { accent, group: None, header: None });
        self.visible = visible;
        self
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Layout
// ═════════════════════════════════════════════════════════════════════════════

/// An item, by group and item index in the active tab; `sub` is a member of a control group or
/// a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ItemKey {
    group: usize,
    item:  usize,
    sub:   Option<usize>,
}

/// What the pointer can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Tab(usize),
    Action(usize),
    Collapse,
    Item(ItemKey),
    /// A small split's separate chevron, or a large split's chevron.
    Chevron(ItemKey),
    Option(ItemKey, usize),
    Chip(usize),
    /// A part of an item: a numeric field's up (0) / down (1) arrow; a gallery's scroll up (0),
    /// scroll down (1) and « more » (2) buttons.
    Part(ItemKey, u8),
    /// A group's dialog launcher.
    Launcher(usize),
    /// The simplified row's overflow button.
    Overflow,
}

#[derive(Clone)]
struct PlacedItem {
    key:       ItemKey,
    rect:      Rect,
    /// A split's chevron (separate button when small, inside when large).
    chevron:   Option<Rect>,
    /// A gallery's option buttons (an empty rectangle for an option scrolled out of view).
    options:   Vec<Rect>,
    /// See [`Target::Part`].
    parts:     Vec<Rect>,
    /// A control group or a box: its members are placed items of their own.
    container: bool,
}

impl PlacedItem {
    fn new(key: ItemKey, rect: Rect) -> Self {
        Self { key, rect, chevron: None, options: Vec::new(), parts: Vec::new(), container: false }
    }
}

#[derive(Clone)]
struct GroupBox {
    index:    usize,
    label:    Rect,
    items:    Vec<PlacedItem>,
    /// The 1 DIP rule on the right, when not the last group.
    rule:     Option<Rect>,
    /// The dialog launcher.
    launcher: Option<Rect>,
    /// The whole group (its rule excluded).
    bounds:   Rect,
}

#[derive(Clone)]
struct ChipBox {
    index:  usize,
    button: Rect,
    label:  Rect,
    rule:   Option<Rect>,
}

#[derive(Clone, Default)]
struct RowLayout {
    rect:     Rect,
    groups:   Vec<GroupBox>,
    chips:    Vec<ChipBox>,
    /// The simplified row's overflow button and the items it holds.
    overflow: Option<(Rect, Vec<ItemKey>)>,
}

/// One column of a group: its items, and its width.
struct Column {
    items: Vec<usize>,
    width: f32,
}

/// The item `k` names in `groups` (a member of a control group or box when `k.sub` is set).
fn item_of(groups: &[RibbonGroup], k: ItemKey) -> Option<&RibbonItem> {
    let it = groups.get(k.group)?.items.get(k.item)?;
    match k.sub {
        Some(s) => it.children.get(s),
        None => Some(it),
    }
}

/// `toColumns`: one-slot items stacked three to a column (a separator is
/// skipped, never breaking a stack); anything wider takes its own column. A hidden item takes no
/// room; a `column_break` starts a new column.
fn to_columns(items: &[RibbonItem]) -> Vec<Vec<usize>> {
    let mut cols = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        if it.kind == ItemKind::Separator || !it.visible {
            continue;
        }
        if it.column_break && !run.is_empty() {
            cols.push(std::mem::take(&mut run));
        }
        if it.stackable() {
            run.push(i);
            if run.len() == m::MAX_STACK {
                cols.push(std::mem::take(&mut run));
            }
        } else {
            if !run.is_empty() {
                cols.push(std::mem::take(&mut run));
            }
            cols.push(vec![i]);
        }
    }
    if !run.is_empty() {
        cols.push(run);
    }
    cols
}

/// Whether a large label fits one line (`twoLines` is its negation).
fn large_label_width(c: &dyn Canvas, fonts: &Fonts, label: &str) -> f32 {
    c.measure(label, &fonts.item)
}

/// The width of an in-ribbon gallery: its visible columns and its scroll column.
fn grid_gallery_width(spec: &GallerySpec) -> f32 {
    let cols = spec.columns.max(1) as f32;
    cols * spec.item_w + (cols - 1.0) * m::COLUMN_GAP + m::COLUMN_GAP + m::GALLERY_SCROLL_W
}

/// The natural width of an item.
fn item_width(c: &dyn Canvas, fonts: &Fonts, it: &RibbonItem) -> f32 {
    match it.kind {
        ItemKind::Separator => 0.0,
        ItemKind::Custom => it.width.unwrap_or(0.0),
        ItemKind::Dropdown => it.width.unwrap_or(m::DROPDOWN_W),
        ItemKind::ComboBox => it.width.unwrap_or(m::DROPDOWN_W),
        ItemKind::NumericField => it.width.unwrap_or(m::NUMERIC_W),
        ItemKind::TextBox => it.width.unwrap_or(m::TEXT_BOX_W),
        ItemKind::Gallery if it.is_grid_gallery() => it.gallery.as_ref().map_or(0.0, grid_gallery_width),
        ItemKind::Gallery if !it.is_drop_down_gallery() => {
            let n = it.options.len() as f32;
            it.options.iter().map(|o| gallery_option_width(c, fonts, o)).sum::<f32>() + m::COLUMN_GAP * (n - 1.0).max(0.0)
        }
        ItemKind::CheckBox => {
            let mut w = m::SMALL_PAD_X * 2.0 + m::CHECK_BOX;
            if let Some(l) = it.label.as_deref() {
                w += m::SMALL_GAP + c.measure(l, &fonts.item);
            }
            w
        }
        ItemKind::Label => {
            let mut w = m::SMALL_PAD_X * 2.0 + it.label.as_deref().map_or(0.0, |l| c.measure(l, &fonts.item));
            if it.icon.is_some() {
                w += m::SMALL_ICON + m::SMALL_GAP;
            }
            w
        }
        ItemKind::ControlGroup => it.children.iter().filter(|ch| ch.visible).map(|ch| item_width(c, fonts, ch)).sum(),
        ItemKind::Box => {
            let widths: Vec<f32> = it.children.iter().filter(|ch| ch.visible).map(|ch| item_width(c, fonts, ch)).collect();
            if it.vertical {
                widths.iter().copied().fold(0.0, f32::max)
            } else {
                widths.iter().sum::<f32>() + m::COLUMN_GAP * (widths.len() as f32 - 1.0).max(0.0)
            }
        }
        _ if it.size == ItemSize::Large => {
            let label = it.label.as_deref().map_or(0.0, |l| large_label_width(c, fonts, l).min(m::LARGE_LABEL_W));
            (m::LARGE_PAD_X * 2.0 + label.max(m::LARGE_ICON)).clamp(m::LARGE_MIN_W, m::LARGE_MAX_W)
        }
        _ => {
            let mut w = m::SMALL_PAD_X * 2.0 + m::SMALL_ICON;
            if let Some(l) = it.label.as_deref() {
                w += m::SMALL_GAP + c.measure(l, &fonts.item);
            }
            if it.kind == ItemKind::Menu || it.is_drop_down_gallery() {
                w += m::SMALL_GAP + m::CHEVRON;
            }
            if matches!(it.kind, ItemKind::Split | ItemKind::ColorPicker) {
                w += m::SPLIT_CHEVRON_W;
            }
            w
        }
    }
}

fn gallery_option_width(c: &dyn Canvas, fonts: &Fonts, o: &RibbonOption) -> f32 {
    let content = if o.icon.is_some() { m::SMALL_ICON } else { c.measure(&o.label, &fonts.item) };
    m::GALLERY_PAD_X * 2.0 + content + 2.0
}

fn item_height(it: &RibbonItem, full: f32) -> f32 {
    match it.kind {
        ItemKind::Dropdown | ItemKind::ComboBox | ItemKind::NumericField | ItemKind::TextBox => m::DROPDOWN_H,
        ItemKind::Gallery if it.is_grid_gallery() => full,
        ItemKind::Gallery if !it.is_drop_down_gallery() => m::GALLERY_H,
        ItemKind::ControlGroup => it.children.iter().filter(|c| c.visible).map(|c| item_height(c, full)).fold(m::SMALL_H, f32::max),
        ItemKind::Box if it.vertical => {
            let hs: Vec<f32> = it.children.iter().filter(|c| c.visible).map(|c| item_height(c, full)).collect();
            (hs.iter().sum::<f32>() + m::STACK_GAP * (hs.len() as f32 - 1.0).max(0.0)).min(full)
        }
        ItemKind::Box => it.children.iter().filter(|c| c.visible).map(|c| item_height(c, full)).fold(m::SMALL_H, f32::max),
        ItemKind::Custom => full,
        _ if it.size == ItemSize::Large => full,
        _ => m::SMALL_H,
    }
}

/// A group's columns with their widths, and its natural width (padding and
/// the right rule included when `rule`).
fn group_columns(c: &dyn Canvas, fonts: &Fonts, g: &RibbonGroup) -> (Vec<Column>, f32) {
    let cols: Vec<Column> = to_columns(&g.items)
        .into_iter()
        .map(|items| {
            let width = items.iter().map(|&i| item_width(c, fonts, &g.items[i])).fold(0.0, f32::max);
            Column { items, width }
        })
        .collect();
    let n = cols.len() as f32;
    let span = cols.iter().map(|c| c.width).sum::<f32>() + m::COLUMN_GAP * (n - 1.0).max(0.0);
    (cols, span)
}

/// The width the group label row needs: the label, and the launcher after it.
fn label_width(c: &dyn Canvas, fonts: &Fonts, g: &RibbonGroup) -> f32 {
    c.measure(&g.label, &fonts.label) + if g.launcher { (m::LAUNCHER + 4.0) * 2.0 } else { 0.0 }
}

fn group_width(c: &dyn Canvas, fonts: &Fonts, g: &RibbonGroup, rule: bool) -> f32 {
    let (_, span) = group_columns(c, fonts, g);
    let label = label_width(c, fonts, g);
    m::GROUP_PAD_X * 2.0 + span.max(label) + if rule { 1.0 } else { 0.0 }
}

fn chip_width(c: &dyn Canvas, fonts: &Fonts, g: &RibbonGroup, rule: bool) -> f32 {
    let label = c.measure(&g.label, &fonts.label);
    m::GROUP_PAD_X * 2.0 + m::CHIP_MIN_W.max(label) + if rule { 1.0 } else { 0.0 }
}

/// The geometry of one control placed at `rect`: its chevron, its gallery cells, its parts.
fn place_leaf(c: &dyn Canvas, fonts: &Fonts, it: &RibbonItem, key: ItemKey, rect: Rect) -> PlacedItem {
    let mut placed = PlacedItem::new(key, rect);
    match it.kind {
        ItemKind::Split | ItemKind::ColorPicker if it.size == ItemSize::Small => {
            let core = Rect::new(rect.left, rect.top, rect.right - m::SPLIT_CHEVRON_W, rect.bottom);
            placed.chevron = Some(Rect::new(core.right, rect.top, rect.right, rect.bottom));
            placed.rect = core;
        }
        ItemKind::Split | ItemKind::ColorPicker => {
            // The large split's chevron sits on the label's second line.
            let cy = rect.bottom - m::GROUP_PAD_Y - m::LARGE_LABEL_H / 4.0;
            let mid = (rect.left + rect.right) / 2.0;
            let r = m::CHEVRON;
            placed.chevron = Some(Rect::new(mid - r, cy - r, mid + r, cy + r));
        }
        ItemKind::Gallery if it.is_grid_gallery() => {
            if let Some(spec) = &it.gallery {
                let cols = spec.columns.max(1);
                let rows = spec.rows.max(1);
                let h = rect.bottom - rect.top;
                let cell_h = spec.item_h.min((h - m::STACK_GAP * (rows as f32 - 1.0)) / rows as f32);
                let first = spec.first_row.min(it.options.len().div_ceil(cols).saturating_sub(rows)) * cols;
                for i in 0..it.options.len() {
                    if i < first || i >= first + rows * cols {
                        placed.options.push(Rect::default());
                        continue;
                    }
                    let (col, row) = (((i - first) % cols) as f32, ((i - first) / cols) as f32);
                    let x = rect.left + col * (spec.item_w + m::COLUMN_GAP);
                    let y = rect.top + row * (cell_h + m::STACK_GAP);
                    placed.options.push(Rect::new(x, y, x + spec.item_w, y + cell_h));
                }
                let sx = rect.right - m::GALLERY_SCROLL_W;
                let third = h / 3.0;
                for p in 0..3 {
                    let t = rect.top + third * p as f32;
                    placed.parts.push(Rect::new(sx, t, rect.right, t + third));
                }
            }
        }
        ItemKind::Gallery if !it.is_drop_down_gallery() => {
            let mut ox = rect.left;
            for o in &it.options {
                let w = gallery_option_width(c, fonts, o);
                placed.options.push(Rect::new(ox, rect.top, ox + w, rect.bottom));
                ox += w + m::COLUMN_GAP;
            }
        }
        ItemKind::ComboBox => {
            placed.chevron = Some(Rect::new(rect.right - m::COMBO_CHEVRON_W, rect.top, rect.right, rect.bottom));
        }
        ItemKind::NumericField => {
            let x = rect.right - m::NUMERIC_ARROWS_W - 2.0;
            let mid = (rect.top + rect.bottom) / 2.0;
            placed.parts.push(Rect::new(x, rect.top + 1.0, rect.right - 1.0, mid));
            placed.parts.push(Rect::new(x, mid, rect.right - 1.0, rect.bottom - 1.0));
        }
        _ => {}
    }
    placed
}

/// Places a control group's or a box's members inside `rect`, then recurses into none: a member
/// is a plain control.
fn place_children(c: &dyn Canvas, fonts: &Fonts, it: &RibbonItem, key: ItemKey, rect: Rect, full: f32, out: &mut Vec<PlacedItem>) {
    let (mut x, mut y) = (rect.left, rect.top);
    for (j, ch) in it.children.iter().enumerate() {
        if !ch.visible || ch.kind == ItemKind::Separator {
            continue;
        }
        let w = item_width(c, fonts, ch);
        let h = item_height(ch, full);
        let r = if it.vertical {
            Rect::new(rect.left, y, rect.left + w, y + h)
        } else {
            let top = (rect.top + rect.bottom - h) / 2.0;
            Rect::new(x, top, x + w, top + h)
        };
        out.push(place_leaf(c, fonts, ch, ItemKey { sub: Some(j), ..key }, r));
        if it.vertical {
            y += h + m::STACK_GAP;
        } else {
            x += w + if it.kind == ItemKind::Box { m::COLUMN_GAP } else { 0.0 };
        }
    }
}

/// Places a group at `x` in the band `top..bottom` (the row inside its
/// bottom rule). Returns the box and its right edge.
#[allow(clippy::too_many_arguments)]
fn place_group(c: &dyn Canvas, fonts: &Fonts, g: &RibbonGroup, index: usize, x: f32, top: f32, bottom: f32, rule: bool) -> (GroupBox, f32) {
    let (cols, span) = group_columns(c, fonts, g);
    let label_w = label_width(c, fonts, g);
    let inner = span.max(label_w);
    let right = x + m::GROUP_PAD_X * 2.0 + inner + if rule { 1.0 } else { 0.0 };
    let items_top = top + m::GROUP_PAD_Y;
    let label_bottom = bottom - m::GROUP_PAD_Y;
    let label_top = label_bottom - m::GROUP_LABEL_H;
    let full = label_top - items_top;
    // Rows of joined controls (the Police group): the rows of each column are spread evenly, as
    // Office lays them out; otherwise stacked from the top, one DIP apart (the web).
    let even = g.items.iter().any(|it| it.visible && matches!(it.kind, ItemKind::ControlGroup | ItemKind::Box));
    // `items-center`: the items row is centred when the label is wider.
    let mut cx = x + m::GROUP_PAD_X + (inner - span) / 2.0;
    let mut items = Vec::new();
    for col in &cols {
        let heights: Vec<f32> = col.items.iter().map(|&i| item_height(&g.items[i], full)).collect();
        let gap = if even { ((full - heights.iter().sum::<f32>()) / (heights.len() as f32 + 1.0)).max(m::STACK_GAP) } else { m::STACK_GAP };
        let mut y = if even && heights.iter().all(|h| *h < full) { items_top + gap } else { items_top };
        for (&i, &h) in col.items.iter().zip(&heights) {
            let it = &g.items[i];
            let w = match it.kind {
                ItemKind::Dropdown
                | ItemKind::Custom
                | ItemKind::Gallery
                | ItemKind::ComboBox
                | ItemKind::NumericField
                | ItemKind::TextBox
                | ItemKind::ControlGroup
                | ItemKind::Box => item_width(c, fonts, it),
                // `align-items: stretch`: stacked buttons share the column width.
                _ => col.width,
            };
            let rect = Rect::new(cx, y, cx + w, y + h);
            let key = ItemKey { group: index, item: i, sub: None };
            let mut placed = place_leaf(c, fonts, it, key, rect);
            if matches!(it.kind, ItemKind::ControlGroup | ItemKind::Box) {
                placed.container = true;
                items.push(placed);
                place_children(c, fonts, it, key, rect, full, &mut items);
            } else {
                items.push(placed);
            }
            y += h + gap;
        }
        cx += col.width + m::COLUMN_GAP;
    }
    let label = Rect::new(x, label_top, right - if rule { 1.0 } else { 0.0 }, label_bottom);
    let launcher = g.launcher.then(|| {
        let cy = (label.top + label.bottom) / 2.0;
        Rect::new(label.right - m::GROUP_PAD_X / 2.0 - m::LAUNCHER, cy - m::LAUNCHER / 2.0, label.right - m::GROUP_PAD_X / 2.0, cy + m::LAUNCHER / 2.0)
    });
    let bounds = Rect::new(x, top, label.right, bottom);
    let rule = rule.then(|| Rect::new(right - 1.0, top + m::GROUP_PAD_Y * 3.0, right, bottom - m::GROUP_PAD_Y * 3.0));
    (GroupBox { index, label, items, rule, launcher, bounds }, right)
}

/// Places a folded group's chip at `x`. Returns the chip and its right edge.
#[allow(clippy::too_many_arguments)]
fn place_chip(w: f32, index: usize, x: f32, top: f32, bottom: f32, rule: bool) -> (ChipBox, f32) {
    let right = x + w;
    let label_bottom = bottom - m::GROUP_PAD_Y;
    let label_top = label_bottom - m::GROUP_LABEL_H;
    let inner_l = x + m::GROUP_PAD_X;
    let inner_r = right - m::GROUP_PAD_X - if rule { 1.0 } else { 0.0 };
    let bw = m::CHIP_MIN_W;
    let bx = (inner_l + inner_r) / 2.0 - bw / 2.0;
    let chip = ChipBox {
        index,
        button: Rect::new(bx, top + m::GROUP_PAD_Y, bx + bw, label_top),
        label: Rect::new(x, label_top, right - if rule { 1.0 } else { 0.0 }, label_bottom),
        rule: rule.then(|| Rect::new(right - 1.0, top + m::GROUP_PAD_Y * 3.0, right, bottom - m::GROUP_PAD_Y * 3.0)),
    };
    (chip, right)
}

/// Lays the groups of `groups` out in the row `rect`, folding the right-most
/// ones into chips until the rest fits (`RibbonGroupsRow`).
fn layout_row(c: &dyn Canvas, fonts: &Fonts, groups: &[RibbonGroup], rect: Rect) -> RowLayout {
    let n = groups.len();
    let avail = (rect.right - rect.left) - m::EDGE_PAD * 2.0;
    let natural: Vec<f32> = groups.iter().enumerate().map(|(i, g)| group_width(c, fonts, g, i + 1 < n)).collect();
    let folded: Vec<f32> = groups.iter().enumerate().map(|(i, g)| chip_width(c, fonts, g, i + 1 < n)).collect();
    let total_for = |k: usize| (0..n).map(|i| if i < n - k { natural[i] } else { folded[i] }).sum::<f32>();
    let mut k = 0;
    while k < n && total_for(k) > avail - 1.0 {
        k += 1;
    }
    let sizes: Vec<GroupSize> = (0..n).map(|i| if i < n - k { GroupSize::Large } else { GroupSize::Collapsed }).collect();
    place_row(c, fonts, groups, &sizes, &folded, rect)
}

/// Places the groups at the sizes of `sizes` (`Collapsed` ones as chips of width `folded`). The
/// groups carry the items of their size already (see [`scaling::items_at`]).
fn place_row(c: &dyn Canvas, fonts: &Fonts, groups: &[RibbonGroup], sizes: &[GroupSize], folded: &[f32], rect: Rect) -> RowLayout {
    let n = groups.len();
    let top = rect.top;
    let bottom = rect.bottom - 1.0;
    let mut x = rect.left + m::EDGE_PAD;
    let mut out = RowLayout { rect, ..Default::default() };
    for (i, g) in groups.iter().enumerate() {
        let rule = i + 1 < n;
        if sizes[i] != GroupSize::Collapsed {
            let (gb, right) = place_group(c, fonts, g, i, x, top, bottom, rule);
            out.groups.push(gb);
            x = right;
        } else {
            let (chip, right) = place_chip(folded[i], i, x, top, bottom, rule);
            out.chips.push(chip);
            x = right;
        }
    }
    out
}

/// A tab laid out with its scaling policy and its groups' size definitions: returns the layout
/// and the groups as shown (each with the items of its size level).
fn layout_row_scaled(c: &dyn Canvas, fonts: &Fonts, groups: &[RibbonGroup], policy: &ScalingPolicy, rect: Rect) -> (RowLayout, Vec<RibbonGroup>) {
    let n = groups.len();
    let avail = (rect.right - rect.left) - m::EDGE_PAD * 2.0;
    let folded: Vec<f32> = groups.iter().enumerate().map(|(i, g)| chip_width(c, fonts, g, i + 1 < n)).collect();
    // Each group's width at each level, measured once.
    let widths: Vec<[f32; 4]> = groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let mut w = [0.0; 4];
            for (k, s) in GroupSize::ALL.iter().enumerate() {
                w[k] = if *s == GroupSize::Collapsed {
                    folded[i]
                } else {
                    let shown = RibbonGroup { items: scaling::items_at(g, *s).into_owned(), ..g.clone() };
                    group_width(c, fonts, &shown, i + 1 < n)
                };
            }
            w
        })
        .collect();
    let level = |s: GroupSize| GroupSize::ALL.iter().position(|x| *x == s).unwrap_or(0);
    let sizes = scaling::plan(groups, policy, |sizes| sizes.iter().enumerate().map(|(i, s)| widths[i][level(*s)]).sum::<f32>() <= avail - 1.0);
    let shown: Vec<RibbonGroup> = groups.iter().zip(&sizes).map(|(g, s)| RibbonGroup { items: scaling::items_at(g, *s).into_owned(), ..g.clone() }).collect();
    (place_row(c, fonts, &shown, &sizes, &folded, rect), shown)
}

/// The simplified row: every group's controls on one line (large ones made small, labels kept
/// for them only), the controls that do not fit in the overflow menu. Returns the layout and the
/// groups as shown.
fn layout_simplified(c: &dyn Canvas, fonts: &Fonts, groups: &[RibbonGroup], rect: Rect) -> (RowLayout, Vec<RibbonGroup>) {
    let shown: Vec<RibbonGroup> = groups
        .iter()
        .map(|g| {
            let items = g
                .items
                .iter()
                .map(|it| {
                    let mut it = it.clone();
                    if it.size == ItemSize::Large {
                        it.size = ItemSize::Small;
                    } else if it.icon.is_some() && matches!(it.kind, ItemKind::Button | ItemKind::Toggle | ItemKind::Split | ItemKind::Menu | ItemKind::ColorPicker) {
                        if let Some(l) = it.label.take() {
                            it.tooltip.get_or_insert(l);
                        }
                    }
                    if let Some(g) = it.gallery.as_mut() {
                        g.display = GalleryDisplay::DropDown;
                    } else if it.kind == ItemKind::Gallery {
                        it.gallery = Some(GallerySpec { display: GalleryDisplay::DropDown, ..GallerySpec::default() });
                    }
                    it
                })
                .collect();
            RibbonGroup { items, ..g.clone() }
        })
        .collect();
    let mut out = RowLayout { rect, ..Default::default() };
    let more_w = m::SMALL_H + m::SMALL_PAD_X;
    let limit = rect.right - m::EDGE_PAD - more_w;
    let mut x = rect.left + m::EDGE_PAD;
    let mut overflow: Vec<ItemKey> = Vec::new();
    let cy = (rect.top + rect.bottom - 1.0) / 2.0;
    for (gi, g) in shown.iter().enumerate() {
        let start = x;
        let mut placed = Vec::new();
        for (i, it) in g.items.iter().enumerate() {
            if !it.visible || it.kind == ItemKind::Separator {
                continue;
            }
            let w = item_width(c, fonts, it);
            let h = item_height(it, m::DROPDOWN_H).min(m::DROPDOWN_H);
            let key = ItemKey { group: gi, item: i, sub: None };
            if x + w > limit || !overflow.is_empty() {
                overflow.push(key);
                continue;
            }
            let r = Rect::new(x, cy - h / 2.0, x + w, cy + h / 2.0);
            let mut p = place_leaf(c, fonts, it, key, r);
            if matches!(it.kind, ItemKind::ControlGroup | ItemKind::Box) {
                p.container = true;
                placed.push(p);
                place_children(c, fonts, it, key, r, h, &mut placed);
            } else {
                placed.push(p);
            }
            x += w + m::COLUMN_GAP;
        }
        if placed.is_empty() {
            continue;
        }
        let right = x + m::GROUP_PAD_X;
        let rule = Rect::new(right - 1.0, rect.top + m::GROUP_PAD_Y * 4.0, right, rect.bottom - m::GROUP_PAD_Y * 4.0);
        out.groups.push(GroupBox {
            index: gi,
            label: Rect::new(start, cy, start, cy),
            items: placed,
            rule: Some(rule),
            launcher: None,
            bounds: Rect::new(start - m::GROUP_PAD_X / 2.0, rect.top, right, rect.bottom - 1.0),
        });
        x = right + m::GROUP_PAD_X;
    }
    if let Some(last) = out.groups.last_mut() {
        last.rule = None;
    }
    if !overflow.is_empty() {
        let r = Rect::new(rect.right - m::EDGE_PAD - m::SMALL_H - 4.0, cy - m::SMALL_H / 2.0, rect.right - m::EDGE_PAD, cy + m::SMALL_H / 2.0);
        out.overflow = Some((r, overflow));
    }
    (out, shown)
}

/// The popover of a folded group: the group laid out whole under its chip,
/// kept inside `area` (flipped above when there is no room below). Returns
/// the popover and the group inside it.
fn layout_popover(c: &dyn Canvas, fonts: &Fonts, g: &RibbonGroup, index: usize, chip: Rect, area: Rect) -> (Rect, GroupBox) {
    let w = group_width(c, fonts, g, false) + (m::POPOVER_PAD + 1.0) * 2.0;
    let h = (m::CONTENT_H - 1.0) + (m::POPOVER_PAD + 1.0) * 2.0;
    let mm = m::SCREEN_MARGIN;
    let left = chip.left.min(area.right - mm - w).max(area.left + mm);
    let mut top = chip.bottom + m::POPOVER_GAP;
    if top + h > area.bottom - mm {
        let above = chip.top - m::POPOVER_GAP - h;
        top = if above >= area.top + mm { above } else { (area.bottom - mm - h).max(area.top + mm) };
    }
    let pop = Rect::new(left, top, left + w, top + h);
    let inset = m::POPOVER_PAD + 1.0;
    let (gb, _) = place_group(c, fonts, g, index, pop.left + inset, pop.top + inset, pop.bottom - inset, false);
    (pop, gb)
}

// ═════════════════════════════════════════════════════════════════════════════
// Painting
// ═════════════════════════════════════════════════════════════════════════════

/// What the painters need to know about the pointer and the open floats.
#[derive(Debug, Clone, Copy, Default)]
struct Look {
    hot:  Option<Target>,
    open: Option<Target>,
    /// The field being typed in (its value is already the typed text).
    edit: Option<ItemKey>,
}

fn is(look_t: Option<Target>, t: Target) -> bool {
    look_t == Some(t)
}

fn paint_item(c: &dyn Canvas, col: &Colors, fonts: &Fonts, it: &RibbonItem, p: &PlacedItem, look: Look) {
    let dead = it.disabled;
    let hot_main = !dead && is(look.hot, Target::Item(p.key));
    let hot_chev = !dead && is(look.hot, Target::Chevron(p.key));
    let open = is(look.open, Target::Item(p.key)) || is(look.open, Target::Chevron(p.key));
    let fg = Colors::faded(if it.active { col.active_fg } else { col.text }, dead);
    let r = p.rect;
    match it.kind {
        ItemKind::Separator | ItemKind::ControlGroup | ItemKind::Box => {}
        ItemKind::Custom => {
            if let Some(render) = &it.render {
                render(c, r, WidgetState::REST.hot(hot_main).disabled(dead));
            }
        }
        ItemKind::Dropdown => paint_dropdown_trigger(c, col, fonts, it, r, hot_main, open),
        ItemKind::ComboBox if !it.editable => paint_dropdown_trigger(c, col, fonts, it, r, hot_main || hot_chev, open),
        ItemKind::ComboBox | ItemKind::NumericField | ItemKind::TextBox => paint_field(c, col, fonts, it, p, look),
        ItemKind::CheckBox => {
            if hot_main {
                c.fill_rounded(&r, m::ITEM_RADIUS, &col.hover);
            }
            let cy = (r.top + r.bottom) / 2.0;
            let b = Rect::new(r.left + m::SMALL_PAD_X, cy - m::CHECK_BOX / 2.0, r.left + m::SMALL_PAD_X + m::CHECK_BOX, cy + m::CHECK_BOX / 2.0);
            if it.active {
                c.fill_rounded(&b, m::ITEM_RADIUS + 1.0, &Colors::faded(col.accent, dead));
                c.vector_icon("Check", &b, m::CHECK_BOX - 3.0, &Colors::faded(if col.accent.r + col.accent.g + col.accent.b > 2.0 { hex(0x1f1f1f) } else { WHITE }, dead));
            } else {
                c.fill_rounded(&b, m::ITEM_RADIUS + 1.0, &col.field_bg);
                c.stroke_rounded(&b, m::ITEM_RADIUS + 1.0, &Colors::faded(col.text_dim, dead));
            }
            if let Some(l) = it.label.as_deref() {
                let x = b.right + m::SMALL_GAP;
                c.text(l, &Rect::new(x, r.top, r.right, r.bottom), &fonts.item, &Colors::faded(col.text, dead), false);
            }
        }
        ItemKind::Label => {
            let mut x = r.left + m::SMALL_PAD_X;
            if let Some(icon) = &it.icon {
                let ir = Rect::new(x, r.top, x + m::SMALL_ICON, r.bottom);
                c.vector_icon(icon_str(icon), &ir, m::SMALL_ICON - 2.0, &Colors::faded(col.text, dead));
                x += m::SMALL_ICON + m::SMALL_GAP;
            }
            if let Some(l) = it.label.as_deref() {
                c.text(l, &Rect::new(x, r.top, r.right, r.bottom), &fonts.item, &Colors::faded(col.text, dead), false);
            }
        }
        ItemKind::Gallery if it.is_grid_gallery() => paint_grid_gallery(c, col, fonts, it, p, look),
        ItemKind::Gallery if !it.is_drop_down_gallery() => {
            for (i, (o, or)) in it.options.iter().zip(&p.options).enumerate() {
                let hot = !dead && is(look.hot, Target::Option(p.key, i));
                if hot {
                    c.fill_rounded(or, m::ITEM_RADIUS, &col.hover);
                }
                c.stroke_rounded(or, m::ITEM_RADIUS, &col.border);
                let ink = Colors::faded(col.text, dead);
                match &o.icon {
                    Some(icon) => c.vector_icon(icon_str(icon), or, m::SMALL_ICON, &ink),
                    None => c.text(&o.label, or, &fonts.item, &ink, true),
                }
            }
        }
        _ => {
            // The surface: active tint, else hover (or open menu).
            let whole_opens = it.kind == ItemKind::Menu || it.is_drop_down_gallery();
            let bg = if it.active {
                Some(col.active_bg)
            } else if hot_main || (open && whole_opens) {
                Some(col.hover)
            } else {
                None
            };
            if let Some(bg) = bg {
                c.fill_rounded(&r, m::ITEM_RADIUS, &Colors::faded(bg, dead));
            }
            if it.size == ItemSize::Large {
                paint_large(c, col, fonts, it, r, fg, dead);
            } else {
                let icon_r = Rect::new(r.left + m::SMALL_PAD_X, r.top + 2.0, r.left + m::SMALL_PAD_X + m::SMALL_ICON, r.bottom - 2.0);
                if let Some(icon) = &it.icon {
                    c.vector_icon(icon_str(icon), &icon_r, m::SMALL_ICON - 2.0, &fg);
                }
                if it.kind == ItemKind::ColorPicker {
                    let bar = Rect::new(icon_r.left + 1.0, icon_r.bottom - m::COLOR_BAR_H + 1.0, icon_r.right - 1.0, icon_r.bottom + 1.0);
                    c.fill_rounded(&bar, 0.0, &Colors::faded(it.color.unwrap_or(col.text), dead));
                }
                let mut x = icon_r.right + m::SMALL_GAP;
                if let Some(l) = it.label.as_deref() {
                    let w = c.measure(l, &fonts.item);
                    c.text(l, &Rect::new(x, r.top, x + w + 1.0, r.bottom), &fonts.item, &fg, false);
                    x += w + m::SMALL_GAP;
                }
                if whole_opens {
                    let cr = Rect::new(x, r.top, x + m::CHEVRON, r.bottom);
                    c.vector_icon("ChevronDown", &cr, m::CHEVRON, &Colors::faded(col.text_dim, dead));
                }
            }
            // A small split's separate chevron button.
            if let (ItemKind::Split | ItemKind::ColorPicker, ItemSize::Small, Some(ch)) = (it.kind, it.size, p.chevron) {
                if hot_chev || open {
                    c.fill_rounded(&ch, m::ITEM_RADIUS, &col.hover);
                }
                c.vector_icon("ChevronDown", &ch, m::CHEVRON, &Colors::faded(col.text_dim, dead));
            }
        }
    }
}

/// `LargeButtonContent`: icon on top (32), a label block of two lines always
/// reserved below it; a menu/split chevron on the second line when the label
/// fits one, at the end of the second line otherwise.
fn paint_large(c: &dyn Canvas, col: &Colors, fonts: &Fonts, it: &RibbonItem, r: Rect, fg: D2D1_COLOR_F, dead: bool) {
    let label_bottom = r.bottom - m::GROUP_PAD_Y;
    let label_top = label_bottom - m::LARGE_LABEL_H;
    let icon_area = Rect::new(r.left, r.top + m::GROUP_PAD_Y, r.right, label_top);
    if let Some(icon) = &it.icon {
        c.vector_icon(icon_str(icon), &icon_area, m::LARGE_ICON, &fg);
    }
    if it.kind == ItemKind::ColorPicker {
        let cx = (r.left + r.right) / 2.0;
        let cy = (icon_area.top + icon_area.bottom) / 2.0 + m::LARGE_ICON / 2.0;
        let bar = Rect::new(cx - m::LARGE_ICON / 2.0 + 2.0, cy - 1.0, cx + m::LARGE_ICON / 2.0 - 2.0, cy - 1.0 + m::COLOR_BAR_H + 1.0);
        c.fill_rounded(&bar, 0.0, &Colors::faded(it.color.unwrap_or(col.text), dead));
    }
    let cx = (r.left + r.right) / 2.0;
    let block_w = m::LARGE_LABEL_W.min(r.right - r.left - m::LARGE_PAD_X * 2.0);
    let block = Rect::new(cx - block_w / 2.0, label_top, cx + block_w / 2.0, label_bottom);
    let chevron = it.opens_menu();
    let line_h = m::LARGE_LABEL_H / 2.0;
    let label = it.label.as_deref().unwrap_or("");
    let one_line = large_label_width(c, fonts, label) <= block_w;
    let chev_ink = Colors::faded(if it.active { fg } else { col.text }, dead);
    c.push_clip(&block);
    if one_line {
        c.text(label, &Rect::new(block.left, block.top, block.right, block.top + line_h), &fonts.item, &fg, true);
        if chevron {
            let row = Rect::new(block.left, block.top + line_h, block.right, block.bottom);
            c.vector_icon("ChevronDown", &row, m::CHEVRON, &chev_ink);
        }
    } else {
        // `line-clamp-2`, the chevron after the text on the second line.
        let text_block = if chevron { Rect::new(block.left, block.top, block.right - m::CHEVRON, block.bottom) } else { block };
        c.text(label, &text_block, &fonts.item_wrap, &fg, true);
        if chevron {
            let row = Rect::new(block.right - m::CHEVRON - 2.0, block.top + line_h, block.right, block.bottom);
            c.vector_icon("ChevronDown", &row, m::CHEVRON, &chev_ink);
        }
    }
    c.pop_clip();
}

/// The compact `Dropdown` trigger of the ribbon (`fontSize 11`, `height 24`).
fn paint_dropdown_trigger(c: &dyn Canvas, col: &Colors, fonts: &Fonts, it: &RibbonItem, r: Rect, hot: bool, open: bool) {
    let dead = it.disabled;
    let t = c.theme();
    c.fill_rounded(&r, 4.0, &Colors::faded(col.field_bg, dead));
    let line = if open { col.accent } else if hot { t.border_strong } else { col.border };
    c.stroke_rounded(&r, 4.0, &Colors::faded(line, dead));
    let chev = Rect::new(r.right - 8.0 - 12.0, r.top, r.right - 8.0, r.bottom);
    let text = it
        .value
        .as_deref()
        .and_then(|v| it.options.iter().find(|o| o.value == v))
        .map(|o| o.label.as_str())
        // A value outside the options (a font the machine lacks) still shows,
        // as the web's font field shows what the document says.
        .or(it.value.as_deref())
        .unwrap_or("");
    let tr = Rect::new(r.left + 8.0, r.top, chev.left - 4.0, r.bottom);
    c.push_clip(&tr);
    c.text_ellipsis(text, &tr, &fonts.item, &Colors::faded(col.text, dead));
    c.pop_clip();
    c.vector_icon("ChevronDown", &chev, 12.0, &Colors::faded(col.text_dim, dead));
}

/// A typed field: an editable combo box (text and chevron), a numeric field (text and arrows), a
/// text box. While typed in, its border is the accent and a caret follows the text.
fn paint_field(c: &dyn Canvas, col: &Colors, fonts: &Fonts, it: &RibbonItem, p: &PlacedItem, look: Look) {
    let dead = it.disabled;
    let r = p.rect;
    let t = c.theme();
    let editing = look.edit == Some(p.key);
    let hot = !dead && (is(look.hot, Target::Item(p.key)) || is(look.hot, Target::Chevron(p.key)) || matches!(look.hot, Some(Target::Part(k, _)) if k == p.key));
    let open = is(look.open, Target::Item(p.key)) || is(look.open, Target::Chevron(p.key));
    c.fill_rounded(&r, 4.0, &Colors::faded(col.field_bg, dead));
    let line = if editing || open { col.accent } else if hot { t.border_strong } else { col.border };
    c.stroke_rounded_w(&r, 4.0, &Colors::faded(line, dead), if editing { 1.5 } else { 1.0 });
    let right = p.chevron.map(|ch| ch.left).or_else(|| p.parts.first().map(|a| a.left)).unwrap_or(r.right - 4.0);
    let tr = Rect::new(r.left + 8.0, r.top, right - 2.0, r.bottom);
    let text = it.value.as_deref().unwrap_or("");
    c.push_clip(&tr);
    c.text_ellipsis(text, &tr, &fonts.item, &Colors::faded(col.text, dead));
    if editing {
        let x = (tr.left + c.measure(text, &fonts.item) + 1.0).min(tr.right - 1.0);
        c.fill_rounded(&Rect::new(x, r.top + 5.0, x + 1.0, r.bottom - 5.0), 0.0, &col.text);
    }
    c.pop_clip();
    if let Some(ch) = p.chevron {
        if is(look.hot, Target::Chevron(p.key)) || open {
            c.fill_rounded(&Rect::new(ch.left + 2.0, ch.top + 2.0, ch.right - 2.0, ch.bottom - 2.0), m::ITEM_RADIUS, &col.hover);
        }
        c.vector_icon("ChevronDown", &ch, 12.0, &Colors::faded(col.text_dim, dead));
    }
    for (i, part) in p.parts.iter().enumerate() {
        if is(look.hot, Target::Part(p.key, i as u8)) {
            c.fill_rounded(part, m::ITEM_RADIUS, &col.hover);
        }
        c.vector_icon(if i == 0 { "ChevronUp" } else { "ChevronDown" }, part, 10.0, &Colors::faded(col.text_dim, dead));
    }
}

/// An in-ribbon gallery: its visible cells, then the scroll column (up, down, « more »).
fn paint_grid_gallery(c: &dyn Canvas, col: &Colors, fonts: &Fonts, it: &RibbonItem, p: &PlacedItem, look: Look) {
    let dead = it.disabled;
    for (i, (o, or)) in it.options.iter().zip(&p.options).enumerate() {
        if or.right <= or.left {
            continue;
        }
        let hot = !dead && is(look.hot, Target::Option(p.key, i));
        let cell = panel::Cell { value: o.value.clone(), label: o.label.clone(), icon: o.icon.clone(), color: o.color, selected: it.value.as_deref() == Some(o.value.as_str()) };
        panel::paint_cell(c, &cell, *or, hot, &fonts.item, col.accent);
    }
    if let (Some(first), Some(last)) = (p.parts.first(), p.parts.last()) {
        let column = Rect::new(first.left, first.top, last.right, last.bottom);
        c.stroke_rounded(&column, m::ITEM_RADIUS, &col.border);
    }
    for (i, part) in p.parts.iter().enumerate() {
        if !dead && is(look.hot, Target::Part(p.key, i as u8)) {
            c.fill_rounded(part, m::ITEM_RADIUS, &col.hover);
        }
        let icon = ["ChevronUp", "ChevronDown", "ChevronsUpDown"][i.min(2)];
        c.vector_icon(icon, part, 11.0, &Colors::faded(col.text_dim, dead));
    }
}

/// The dialog launcher: a small corner with an arrow into it.
fn paint_launcher(c: &dyn Canvas, col: &Colors, r: Rect, hot: bool) {
    if hot {
        c.fill_rounded(&r, m::ITEM_RADIUS, &col.hover);
    }
    let ink = if hot { col.text } else { col.text_dim };
    let (l, t, rr, b) = (r.left + 3.0, r.top + 3.0, r.right - 3.0, r.bottom - 3.0);
    // The corner (bottom and right edges), then the diagonal arrow.
    c.fill_rounded(&Rect::new(l, b - 1.0, rr, b), 0.0, &ink);
    c.fill_rounded(&Rect::new(rr - 1.0, t, rr, b), 0.0, &ink);
    c.fill_triangle((rr - 2.0, b - 2.0), (rr - 2.0, b - 6.0), (rr - 6.0, b - 2.0), &ink);
    let steps = 4;
    for i in 0..steps {
        let x = l + 1.0 + i as f32 * 1.2;
        let y = t + 1.0 + i as f32 * 1.2;
        c.fill_rounded(&Rect::new(x, y, x + 1.3, y + 1.3), 0.0, &ink);
    }
}

fn paint_group(c: &dyn Canvas, col: &Colors, fonts: &Fonts, g: &RibbonGroup, gb: &GroupBox, look: Look) {
    let one = std::slice::from_ref(g);
    for p in &gb.items {
        let key = ItemKey { group: 0, ..p.key };
        if let Some(it) = item_of(one, key) {
            paint_item(c, col, fonts, it, p, look);
        }
    }
    if gb.label.bottom > gb.label.top {
        c.text(&g.label, &gb.label, &fonts.label, &col.text_dim, true);
    }
    if let Some(l) = gb.launcher {
        paint_launcher(c, col, l, is(look.hot, Target::Launcher(gb.index)));
    }
    if let Some(rule) = gb.rule {
        c.fill_rounded(&rule, 0.0, &col.border);
    }
}

fn paint_chip(c: &dyn Canvas, col: &Colors, fonts: &Fonts, g: &RibbonGroup, chip: &ChipBox, look: Look) {
    let open = is(look.open, Target::Chip(chip.index));
    if open || is(look.hot, Target::Chip(chip.index)) {
        c.fill_rounded(&chip.button, m::ITEM_RADIUS, &col.hover);
    }
    let b = chip.button;
    let stack = m::CHIP_ICON + m::CHIP_GAP + m::CHIP_CHEVRON;
    let top = (b.top + b.bottom) / 2.0 - stack / 2.0;
    // The group's own icon, else its first item's, stands for it.
    if let Some(icon) = g.icon.as_ref().or_else(|| g.items.iter().find_map(|it| it.icon.as_ref())) {
        let ir = Rect::new(b.left, top, b.right, top + m::CHIP_ICON);
        c.vector_icon(icon_str(icon), &ir, m::CHIP_ICON, &col.text);
    }
    let cr = Rect::new(b.left, top + m::CHIP_ICON + m::CHIP_GAP, b.right, top + stack);
    c.vector_icon("ChevronDown", &cr, m::CHIP_CHEVRON, &col.text_dim);
    c.text(&g.label, &chip.label, &fonts.label, &col.text_dim, true);
    if let Some(rule) = chip.rule {
        c.fill_rounded(&rule, 0.0, &col.border);
    }
}

fn paint_row(c: &dyn Canvas, col: &Colors, fonts: &Fonts, groups: &[RibbonGroup], row: &RowLayout, look: Look) {
    let r = row.rect;
    c.fill_rounded(&r, 0.0, &col.bg);
    c.fill_rounded(&Rect::new(r.left, r.bottom - 1.0, r.right, r.bottom), 0.0, &col.border);
    c.push_clip(&r);
    c.push_bg(col.bg);
    for gb in &row.groups {
        if let Some(g) = groups.get(gb.index) {
            paint_group(c, col, fonts, g, gb, look);
        }
    }
    for chip in &row.chips {
        if let Some(g) = groups.get(chip.index) {
            paint_chip(c, col, fonts, g, chip, look);
        }
    }
    if let Some((more, _)) = &row.overflow {
        if is(look.hot, Target::Overflow) || is(look.open, Target::Overflow) {
            c.fill_rounded(more, m::ITEM_RADIUS, &col.hover);
        }
        c.vector_icon("MoreHorizontal", more, m::SMALL_ICON, &col.text);
    }
    c.pop_bg();
    c.pop_clip();
}

// ═════════════════════════════════════════════════════════════════════════════
// Events
// ═════════════════════════════════════════════════════════════════════════════

/// What happened during one [`Ribbon::frame`].
#[derive(Debug, Clone, PartialEq)]
pub enum RibbonEvent {
    /// The active tab changed (click, or a contextual tab that appeared).
    TabChanged(String),
    /// A button, toggle, check box, split main action or strip action was clicked.
    Clicked(String),
    /// … and was double-clicked (`onDoubleClick`; `Clicked` fired too).
    DoubleClicked(String),
    /// An entry of a split or menu button (or of a panel) was chosen.
    Chosen { item: String, entry: String },
    /// A drop-down, combo box, numeric field, text box, gallery or colour value was picked or
    /// typed (a colour as `#rrggbb`, empty for « Automatique »).
    Changed { item: String, value: String },
    /// The ribbon was collapsed (`true`) or expanded (`false`).
    Collapsed(bool),
    /// A group's dialog launcher was clicked (the group's id).
    Launcher(String),
    /// The items the user added to the quick access toolbar changed (their ids, in order): what
    /// an application persists per user.
    QatChanged(Vec<String>),
}

/// What a [`RibbonRegion`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    /// The « Fichier » tab (the Backstage).
    FileTab,
    Tab,
    /// A contextual tab group's header chip (its id is the group's).
    ContextualHeader,
    /// A quick action of the strip (its id is the item's).
    QuickAction,
    Collapse,
    Group,
    /// A folded group's chip (its id is the group's).
    Chip,
    /// A control (or a member of a control group / box).
    Item,
    /// A group's dialog launcher (its id is the group's).
    Launcher,
    /// An entry of a drop-down shown in design mode (its id is the entry's; `parent` the item).
    MenuEntry,
    /// Cell `n` of a gallery (its id is the gallery's).
    GalleryItem(usize),
    /// The designer's « + » glyphs: a new tab, a new group of the tab `id`, a new control of the
    /// group `id`.
    AddTab,
    AddGroup,
    AddItem,
}

/// Where one element of the ribbon was drawn this frame — what the view runtime's designer
/// selects and its input router routes events to (`RIBBON.md` §7.1, virtual regions).
#[derive(Debug, Clone, PartialEq)]
pub struct RibbonRegion {
    pub id:     String,
    pub kind:   RegionKind,
    pub rect:   Rect,
    /// The item a menu entry belongs to.
    pub parent: Option<String>,
}

impl RibbonRegion {
    fn new(id: impl Into<String>, kind: RegionKind, rect: Rect) -> Self {
        Self { id: id.into(), kind, rect, parent: None }
    }
}

/// What [`Ribbon::frame`] reports.
#[derive(Clone, Default)]
pub struct RibbonRun {
    pub events:    Vec<RibbonEvent>,
    /// The ribbon's own height (strip + row, 0 row when collapsed).
    pub height:    f32,
    /// Where the page goes: `bounds` below the ribbon.
    pub content:   Rect,
    /// The « Fichier » tab is active: paint the Backstage in `content`.
    pub backstage: bool,
    /// Where every tab, group, control… was drawn (see [`RibbonRegion`]).
    pub regions:   Vec<RibbonRegion>,
}

/// Where the quick access toolbar sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QatPosition {
    /// Right after « Fichier », in the tab strip (the web).
    #[default]
    InTabStrip,
    /// On a row of its own below the ribbon.
    BelowRibbon,
}

/// `DisplayMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    /// Tabs over a row of groups (the web).
    #[default]
    Classic,
    /// One row of commands (large ones made small), the rest in an overflow menu.
    Simplified,
}

/// The ribbon in a visual designer: no input, every contextual tab shown (dashed), the tab the
/// designer selected active, the Backstage or a control's drop-down shown on request, and « + »
/// glyphs where something can be added.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RibbonDesign {
    pub active_tab: Option<String>,
    pub backstage:  bool,
    /// The control whose menu / panel shows inline (see [`Ribbon::paint_design_overlay`]).
    pub open_menu:  Option<String>,
    pub add_glyphs: bool,
}

/// An open floating surface.
#[derive(Clone)]
enum Float {
    /// A split/menu button's menu: the item it belongs to, each row's entry id and its
    /// sub-menu's ids. `overflow`: the simplified row's overflow menu (a row is a control).
    Menu { owner: String, key: Option<Target>, anchor: Rect, menu: Box<Menu>, ids: Vec<(String, Vec<String>)>, overflow: bool },
    /// A drop-down's list and the values of its rows.
    List { owner: String, key: ItemKey, anchor: Rect, list: Box<Dropdown>, values: Vec<String> },
    /// A panel (galleries, colours, rows) and the item each of its rows reports for.
    Panel { owner: String, key: Option<Target>, anchor: Rect, panel: Box<panel::Panel>, owners: Vec<Option<String>> },
    /// The « add to / remove from the quick access toolbar » menu of a control.
    Qat { id: String, add: bool, anchor: Rect, menu: Box<Menu> },
}

/// A field being typed in.
#[derive(Clone)]
struct EditState {
    id:    String,
    key:   ItemKey,
    text:  String,
    /// The text is selected: the first character typed replaces it.
    fresh: bool,
}

/// The KeyTips' level.
#[derive(Clone, Debug, PartialEq)]
enum KeyTipMode {
    Off,
    Tabs(String),
    Items(String),
}

/// The inline drop-down of design mode.
#[derive(Clone)]
enum DesignDrop {
    Menu(Rect, Box<Menu>),
    Panel(Rect, Box<panel::Panel>),
}

// ═════════════════════════════════════════════════════════════════════════════
// The ribbon
// ═════════════════════════════════════════════════════════════════════════════

/// The live ribbon: its declaration, and the state the web keeps in hooks
/// (active tab, collapse, peek, open menus, hover, tooltip).
pub struct Ribbon {
    pub tabs:          Vec<RibbonTab>,
    pub theme:         RibbonTheme,
    /// The strip's quick actions (Save, Undo, Redo…): icon buttons right after
    /// « Fichier » (`tabStripActions`).
    pub strip_actions: Vec<RibbonItem>,
    /// Tooltips of the collapse button.
    pub collapse_label: String,
    pub expand_label:   String,
    /// The window caption takes the tab strip's colours, so the two read as
    /// one band, as the web's topbar does over its ribbon (`topbarBg`). On by
    /// default — every app ribbon owns the top of its window. Turn it off for
    /// a ribbon shown INSIDE a page (a gallery demo).
    pub owns_caption:   bool,
    /// Where the quick actions sit.
    pub qat_position:   QatPosition,
    pub display_mode:   DisplayMode,
    /// The controls the user added to the quick access toolbar (ids), after `strip_actions`.
    pub qat_custom:     Vec<String>,
    /// A right click on a control offers to add it to (or remove it from) the toolbar.
    pub qat_customizable: bool,
    /// Alt shows the KeyTips.
    pub key_tips:       bool,
    /// Design mode (see [`RibbonDesign`]); `None` at run time.
    pub design:         Option<RibbonDesign>,
    /// The labels of the menus and panels the ribbon builds itself.
    pub texts:          RibbonTexts,
    active:        String,
    collapsed:     bool,
    peeking:       bool,
    prev_ctx:      Vec<String>,
    prev_down:     bool,
    prev_right:    bool,
    pressed:       Option<Target>,
    pressed_float: bool,
    chip:          Option<usize>,
    float:         Option<Float>,
    tip:           TooltipTrigger,
    tip_target:    Option<Target>,
    edit:          Option<EditState>,
    keytip:        KeyTipMode,
    alt_armed:     bool,
    gallery_rows:  std::collections::HashMap<String, usize>,
    regions:       Vec<RibbonRegion>,
    design_drop:   Option<DesignDrop>,
    /// A control a KeyTip picked, activated once the frame has its layout.
    pending_key_tip: Option<Target>,
}

/// The texts of what the ribbon builds itself (French by default, like the web).
#[derive(Debug, Clone)]
pub struct RibbonTexts {
    pub automatic:   String,
    pub theme_colors: String,
    pub qat_add:     String,
    pub qat_remove:  String,
    pub more:        String,
    pub scroll_up:   String,
    pub scroll_down: String,
    pub launcher:    String,
}

impl Default for RibbonTexts {
    fn default() -> Self {
        Self {
            automatic: "Automatique".into(),
            theme_colors: "Couleurs du thème".into(),
            qat_add: "Ajouter à la barre d'outils Accès rapide".into(),
            qat_remove: "Supprimer de la barre d'outils Accès rapide".into(),
            more: "Plus".into(),
            scroll_up: "Ligne précédente".into(),
            scroll_down: "Ligne suivante".into(),
            launcher: "Plus d'options".into(),
        }
    }
}

/// A number as a field shows it (`11`, `10.5`).
pub fn format_number(v: f32) -> String {
    if v.fract() == 0.0 && v.abs() < 1.0e9 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// `#rrggbb` of a colour.
fn color_hex(c: D2D1_COLOR_F) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

fn same_color(a: D2D1_COLOR_F, b: D2D1_COLOR_F) -> bool {
    (a.r - b.r).abs() < 0.003 && (a.g - b.g).abs() < 0.003 && (a.b - b.b).abs() < 0.003
}

impl Ribbon {
    pub fn new(tabs: Vec<RibbonTab>, theme: RibbonTheme) -> Self {
        let active = tabs.iter().find(|t| t.visible && !t.backstage).map(|t| t.id.clone()).unwrap_or_default();
        let prev_ctx = tabs.iter().filter(|t| t.visible && t.contextual.is_some()).map(|t| t.id.clone()).collect();
        Self {
            tabs,
            theme,
            strip_actions: Vec::new(),
            collapse_label: "Réduire le ruban (Ctrl+F1)".to_string(),
            expand_label: "Développer le ruban (Ctrl+F1)".to_string(),
            owns_caption: true,
            qat_position: QatPosition::InTabStrip,
            display_mode: DisplayMode::Classic,
            qat_custom: Vec::new(),
            qat_customizable: true,
            key_tips: true,
            design: None,
            texts: RibbonTexts::default(),
            active,
            collapsed: false,
            peeking: false,
            prev_ctx,
            prev_down: false,
            prev_right: false,
            pressed: None,
            pressed_float: false,
            chip: None,
            float: None,
            tip: TooltipTrigger::new(),
            tip_target: None,
            edit: None,
            keytip: KeyTipMode::Off,
            alt_armed: false,
            gallery_rows: std::collections::HashMap::new(),
            regions: Vec::new(),
            design_drop: None,
            pending_key_tip: None,
        }
    }

    /// The active tab's id.
    pub fn active(&self) -> &str {
        &self.active
    }

    pub fn select(&mut self, id: &str) {
        self.active = id.to_string();
        self.close_floats();
    }

    /// Leaves the Backstage for the first normal tab.
    pub fn close_backstage(&mut self) {
        if let Some(t) = self.tabs.iter().find(|t| t.visible && !t.backstage) {
            self.active = t.id.clone();
        }
    }

    pub fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    pub fn set_collapsed(&mut self, on: bool) {
        self.collapsed = on;
        self.peeking = false;
        self.close_floats();
    }

    /// The colours a window caption takes to read as one band with the tab strip: the strip's
    /// colour, and of white, the theme's text and near-black, the ink that contrasts most with it
    /// (WCAG relative luminance).
    pub fn caption_colors(&self, c: &dyn Canvas) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
        let col = Colors::resolve(c, &self.theme);
        let band = col.strip_bg;
        let ink = [WHITE, c.theme().text_primary, hex(0x1f1f1f)]
            .into_iter()
            .max_by(|a, b| contrast_ratio(*a, band).total_cmp(&contrast_ratio(*b, band)))
            .unwrap_or(WHITE);
        (band, ink)
    }

    /// Where every element was drawn at the last frame.
    pub fn regions(&self) -> &[RibbonRegion] {
        &self.regions
    }

    /// Whether the KeyTips show.
    pub fn key_tips_shown(&self) -> bool {
        self.keytip != KeyTipMode::Off
    }

    /// The item `id`, anywhere in the tabs (a member of a control group or box included), to
    /// update its live state.
    pub fn item_mut(&mut self, id: &str) -> Option<&mut RibbonItem> {
        fn find<'a>(items: &'a mut [RibbonItem], id: &str) -> Option<&'a mut RibbonItem> {
            for it in items.iter_mut() {
                if it.id == id {
                    return Some(it);
                }
                if let Some(found) = find(&mut it.children, id) {
                    return Some(found);
                }
            }
            None
        }
        for t in self.tabs.iter_mut() {
            for g in t.groups.iter_mut() {
                if let Some(found) = find(&mut g.items, id) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn find_item(&self, id: &str) -> Option<&RibbonItem> {
        fn find<'a>(items: &'a [RibbonItem], id: &str) -> Option<&'a RibbonItem> {
            items.iter().find_map(|it| if it.id == id { Some(it) } else { find(&it.children, id) })
        }
        self.tabs.iter().flat_map(|t| t.groups.iter()).find_map(|g| find(&g.items, id)).or_else(|| self.strip_actions.iter().find(|a| a.id == id))
    }

    /// The strip's quick actions, then the controls the user added.
    fn quick_actions(&self) -> Vec<RibbonItem> {
        let mut out = self.strip_actions.clone();
        for id in &self.qat_custom {
            if out.iter().any(|a| a.id == *id) {
                continue;
            }
            if let Some(it) = self.find_item(id) {
                let mut it = it.clone();
                if it.tooltip.is_none() {
                    it.tooltip = it.label.clone();
                }
                it.size = ItemSize::Small;
                out.push(it);
            }
        }
        out
    }

    fn close_floats(&mut self) {
        self.float = None;
        self.chip = None;
    }

    fn designing(&self) -> bool {
        self.design.is_some()
    }

    fn visible(&self) -> Vec<usize> {
        let design = self.designing();
        (0..self.tabs.len()).filter(|&i| self.tabs[i].visible || (design && self.tabs[i].contextual.is_some())).collect()
    }

    fn current(&self) -> Option<usize> {
        let vis = self.visible();
        if let Some(d) = &self.design {
            if d.backstage {
                if let Some(i) = vis.iter().copied().find(|&i| self.tabs[i].backstage) {
                    return Some(i);
                }
            }
            if let Some(i) = d.active_tab.as_ref().and_then(|a| vis.iter().copied().find(|&i| self.tabs[i].id == *a && !self.tabs[i].backstage)) {
                return Some(i);
            }
            if self.tabs.iter().any(|t| t.id == self.active && t.backstage) {
                return vis.iter().copied().find(|&i| !self.tabs[i].backstage).or_else(|| vis.first().copied());
            }
        }
        vis.iter()
            .copied()
            .find(|&i| self.tabs[i].id == self.active)
            // Never the Backstage by default (a ribbon whose tabs arrived after it was made).
            .or_else(|| vis.iter().copied().find(|&i| !self.tabs[i].backstage))
            .or_else(|| vis.first().copied())
    }

    /// The contextual-tab rules: switch to one that just appeared, fall back
    /// to the first normal tab when the active one disappeared.
    fn follow_contextual(&mut self, events: &mut Vec<RibbonEvent>) {
        let now: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| t.visible && t.contextual.is_some())
            .map(|t| t.id.clone())
            .collect();
        let fresh = now.iter().find(|id| !self.prev_ctx.contains(id)).cloned();
        self.prev_ctx = now;
        if let Some(id) = fresh {
            self.active = id.clone();
            events.push(RibbonEvent::TabChanged(id));
            return;
        }
        if !self.tabs.iter().any(|t| t.visible && t.id == self.active) {
            if let Some(t) = self.tabs.iter().find(|t| t.visible && !t.backstage).or_else(|| self.tabs.iter().find(|t| t.visible)) {
                self.active = t.id.clone();
                events.push(RibbonEvent::TabChanged(self.active.clone()));
            }
        }
    }

    /// The groups of tab `i` as they are laid out: the visible ones, the gallery scroll
    /// positions and the text being typed applied.
    fn live_groups(&self, i: Option<usize>) -> Vec<RibbonGroup> {
        let Some(i) = i else { return Vec::new() };
        let mut groups: Vec<RibbonGroup> = self.tabs[i].groups.iter().filter(|g| g.visible).cloned().collect();
        let edit = self.edit.as_ref().map(|e| (e.id.as_str(), e.text.as_str()));
        fn patch(items: &mut [RibbonItem], rows: &std::collections::HashMap<String, usize>, edit: Option<(&str, &str)>) {
            for it in items.iter_mut() {
                if let (Some(g), Some(r)) = (it.gallery.as_mut(), rows.get(&it.id)) {
                    g.first_row = *r;
                }
                if let Some((id, text)) = edit {
                    if it.id == id {
                        it.value = Some(text.to_string());
                    }
                }
                patch(&mut it.children, rows, edit);
            }
        }
        if !self.gallery_rows.is_empty() || edit.is_some() {
            for g in &mut groups {
                patch(&mut g.items, &self.gallery_rows, edit);
            }
        }
        groups
    }

    /// Runs one frame: the ribbon sits at the top of `bounds`; the rest of
    /// `bounds` is reported as [`RibbonRun::content`].
    pub fn frame(&mut self, c: &dyn Canvas, bounds: Rect, f: &host::Frame) -> RibbonRun {
        let mut events = Vec::new();
        let live = !self.designing();
        if live {
            self.follow_contextual(&mut events);
        } else {
            self.close_floats();
            self.peeking = false;
            self.edit = None;
            self.keytip = KeyTipMode::Off;
        }
        let fonts = Fonts::of(c);
        let col = Colors::resolve(c, &self.theme);
        if self.owns_caption && live {
            // The caption continues the tab strip: its colour, and the ink that contrasts most
            // with it (WCAG), as the web's topbar over its ribbon.
            let (band, ink) = self.caption_colors(c);
            host::set_caption_colors(band, ink);
        }
        let area = f.screen_area();
        let (mx, my) = if live { f.mouse } else { (host::POINTER_AWAY, host::POINTER_AWAY) };
        let mouse_down = live && f.mouse_down;
        let right_down = live && f.right_down;
        let pressed_edge = mouse_down && !self.prev_down;
        let released = !mouse_down && self.prev_down;
        let right_edge = right_down && !self.prev_right;
        self.prev_down = mouse_down;
        self.prev_right = right_down;

        // Losing the window closes every float, as the web closes on blur.
        if f.dismiss && live {
            self.close_floats();
            self.peeking = false;
            self.keytip = KeyTipMode::Off;
        }
        // ── Keyboard: Ctrl+F1, Escape, typing, KeyTips ─────────────────────
        if live && host::take_key(vk::F1, Modifiers::CTRL) > 0 {
            self.collapsed = !self.collapsed;
            self.peeking = false;
            self.close_floats();
            events.push(RibbonEvent::Collapsed(self.collapsed));
        }
        if live && self.edit.is_some() {
            self.handle_edit_keys(&mut events);
        }
        let cur = self.current();
        let backstage = cur.is_some_and(|i| self.tabs[i].backstage);
        if live && (self.float.is_some() || self.chip.is_some() || self.peeking) {
            self.handle_float_keys(c, area, &mut events);
        }

        // ── Layout ─────────────────────────────────────────────────────────
        let quick = self.quick_actions();
        let strip_actions = if self.qat_position == QatPosition::InTabStrip { quick.len() } else { 0 };
        let strip = Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + m::TAB_H);
        let lay = self.layout_strip(c, &fonts, strip, strip_actions);
        let content_h = if self.display_mode == DisplayMode::Simplified { m::SIMPLIFIED_H } else { m::CONTENT_H };
        let row_rect = Rect::new(bounds.left, strip.bottom, bounds.right, strip.bottom + content_h);
        let show_row = !backstage && (!self.collapsed || self.peeking);
        let groups = self.live_groups(cur);
        let (row, groups) = if !show_row {
            (RowLayout::default(), groups)
        } else if self.display_mode == DisplayMode::Simplified {
            layout_simplified(c, &fonts, &groups, row_rect)
        } else {
            let policy = cur.map(|i| self.tabs[i].scaling.clone()).unwrap_or_default();
            if policy.is_auto() && groups.iter().all(|g| g.size == SizeDefinition::Auto) {
                (layout_row(c, &fonts, &groups, row_rect), groups)
            } else {
                layout_row_scaled(c, &fonts, &groups, &policy, row_rect)
            }
        };
        // The quick access toolbar below the ribbon.
        let qat_row = (self.qat_position == QatPosition::BelowRibbon && !quick.is_empty()).then(|| {
            let top = if show_row && !self.collapsed { row_rect.bottom } else { strip.bottom };
            Rect::new(bounds.left, top, bounds.right, top + m::QAT_ROW_H)
        });
        let action_rects: Vec<Rect> = match qat_row {
            Some(q) => {
                let cy = (q.top + q.bottom) / 2.0;
                (0..quick.len())
                    .map(|i| {
                        let x = q.left + m::EDGE_PAD + i as f32 * (m::ACTION_BTN - 4.0 + m::TAB_GAP);
                        Rect::new(x, cy - (m::ACTION_BTN - 4.0) / 2.0, x + m::ACTION_BTN - 4.0, cy + (m::ACTION_BTN - 4.0) / 2.0)
                    })
                    .collect()
            }
            None => lay.actions.clone(),
        };
        let popover = self.chip.and_then(|gi| {
            let chip = row.chips.iter().find(|ch| ch.index == gi)?;
            let g = groups.get(gi)?;
            // The chip opens the group at its ideal (Large) look.
            let g = RibbonGroup { items: scaling::items_at(g, GroupSize::Large).into_owned(), ..g.clone() };
            Some(layout_popover(c, &fonts, &g, gi, chip.button, area))
        });
        if self.chip.is_some() && popover.is_none() {
            self.chip = None;
        }

        // ── Hit-testing, floats first ───────────────────────────────────────
        let float_panel = self.float_hit_rect(c, area);
        let on_float = float_panel.is_some_and(|p| p.contains(mx, my));
        let in_popover = popover.as_ref().is_some_and(|(p, _)| p.contains(mx, my));
        let hot: Option<Target> = if !live || on_float {
            None
        } else if let Some((_, gb)) = popover.as_ref().filter(|_| in_popover) {
            hit_group(gb, &groups, mx, my)
        } else {
            lay.tabs
                .iter()
                .position(|r| r.1.contains(mx, my))
                .map(|p| Target::Tab(lay.tabs[p].0))
                .or_else(|| action_rects.iter().position(|r| r.contains(mx, my)).map(Target::Action))
                .or_else(|| lay.collapse.filter(|r| r.contains(mx, my)).map(|_| Target::Collapse))
                .or_else(|| if show_row && row.rect.contains(mx, my) { hit_row(&row, &groups, mx, my) } else { None })
        };
        // A disabled item takes neither hover nor clicks.
        let hot = hot.filter(|t| !self.target_disabled(&groups, &quick, t));

        // ── Presses and clicks ──────────────────────────────────────────────
        if pressed_edge {
            self.keytip = KeyTipMode::Off;
            self.pressed = hot;
            self.pressed_float = on_float;
            let inside_ribbon = strip.contains(mx, my) || (show_row && row.rect.contains(mx, my)) || qat_row.is_some_and(|q| q.contains(mx, my));
            // A press outside the field being typed in commits it.
            if let Some(e) = &self.edit {
                let on_field = matches!(hot, Some(Target::Item(k) | Target::Part(k, _)) if k == e.key);
                if !on_field && !on_float {
                    self.commit_edit(&groups, &mut events);
                }
            }
            // A press outside every open float closes them (the web's
            // `mousedown` capture outside the popover / menu).
            if !on_float && !in_popover {
                let on_own_trigger = matches!((self.float.as_ref(), hot), (Some(_), Some(Target::Item(_) | Target::Chevron(_) | Target::Part(..) | Target::Overflow)));
                if !on_own_trigger {
                    self.float = None;
                }
                if !matches!(hot, Some(Target::Chip(_))) {
                    self.chip = None;
                }
                if self.peeking && !inside_ribbon {
                    self.peeking = false;
                }
            }
        }
        if right_edge && self.qat_customizable && !on_float {
            self.open_qat_menu(hot, &groups, &row, popover.as_ref(), &quick, &action_rects, mx, my);
        }
        if released {
            if self.pressed_float && on_float {
                self.click_float(c, area, mx, my, &mut events);
            } else if let (Some(p), Some(h)) = (self.pressed, hot) {
                if p == h {
                    self.activate(c, h, &groups, &quick, &row, popover.as_ref(), f.click_count, &mut events);
                }
            }
            self.pressed = None;
            self.pressed_float = false;
        }
        // Hover over an open menu / list moves its highlight.
        if live {
            self.hover_float(c, area, mx, my, f.wheel_dip().1);
            // The wheel over an in-ribbon gallery scrolls its rows.
            if let Some(Target::Option(k, _) | Target::Part(k, _) | Target::Item(k)) = hot {
                if let Some(it) = item_of(&groups, k).filter(|it| it.is_grid_gallery()) {
                    let (_, wy) = f.wheel_dip();
                    if wy != 0.0 && !host::wheel_claimed() {
                        host::claim_wheel();
                        self.scroll_gallery(it, if wy > 0.0 { 1 } else { -1 });
                    }
                }
            }
            self.run_key_tips(&lay, &row, &groups, &quick, &mut events);
            if let Some(t) = self.pending_key_tip.take() {
                self.activate(c, t, &groups, &quick, &row, popover.as_ref(), 1, &mut events);
            }
        }

        // ── Paint ──────────────────────────────────────────────────────────
        let edit_key = self.edit.as_ref().map(|e| e.key);
        let look = Look { hot, open: self.open_target(), edit: edit_key };
        let cur = self.current();
        let backstage = cur.is_some_and(|i| self.tabs[i].backstage);
        let strip_active = if self.collapsed && !self.peeking && !backstage { None } else { cur };
        self.paint_strip(c, &col, &fonts, strip, &lay, &action_rects[..strip_actions.min(action_rects.len())], &quick, strip_active, look);
        let mut height = m::TAB_H;
        let row_shown = show_row && !self.collapsed;
        if row_shown {
            paint_row(c, &col, &fonts, &groups, &row, look);
            height += content_h;
        } else if show_row && self.peeking {
            // The peek flyout floats over the page, with a shadow.
            let pb = inflate(row.rect, lists::FLOAT_SHADOW_MARGIN);
            let (g2, r2, f2, c2) = (groups.clone(), row.clone(), fonts.clone(), col);
            host::popup(pb, move |cv| {
                cv.push_offset(-pb.left, -pb.top);
                cv.draw_shadow(&r2.rect, 0.0, &SHADOW_MENU, SHADOW_GREY);
                paint_row(cv, &c2, &f2, &g2, &r2, look);
                cv.pop_offset();
            });
        }
        if let Some(q) = qat_row {
            c.fill_rounded(&q, 0.0, &col.bg);
            c.fill_rounded(&Rect::new(q.left, q.bottom - 1.0, q.right, q.bottom), 0.0, &col.border);
            for (i, r) in action_rects.iter().enumerate() {
                let Some(it) = quick.get(i) else { continue };
                if !it.disabled && is(look.hot, Target::Action(i)) {
                    c.fill_rounded(r, 4.0, &col.hover);
                }
                if let Some(icon) = &it.icon {
                    c.vector_icon(icon_str(icon), r, m::SMALL_ICON, &Colors::faded(col.text, it.disabled));
                }
            }
            height = q.bottom - bounds.top;
        }
        if let (Some((pop, gb)), Some(gi)) = (popover.clone(), self.chip) {
            if let Some(g) = groups.get(gi) {
                let g = RibbonGroup { items: scaling::items_at(g, GroupSize::Large).into_owned(), ..g.clone() };
                let pb = inflate(pop, lists::FLOAT_SHADOW_MARGIN);
                let (f2, c2) = (fonts.clone(), col);
                host::popup(pb, move |cv| {
                    cv.push_offset(-pb.left, -pb.top);
                    cv.draw_shadow(&pop, m::POPOVER_RADIUS, &SHADOW_MENU, SHADOW_GREY);
                    cv.fill_rounded(&pop, m::POPOVER_RADIUS, &c2.bg);
                    cv.stroke_rounded(&pop, m::POPOVER_RADIUS, &c2.border);
                    cv.push_bg(c2.bg);
                    paint_group(cv, &c2, &f2, &g, &gb, look);
                    cv.pop_bg();
                    cv.pop_offset();
                });
            }
        }
        self.show_float(c, area, &col, &fonts);
        if live {
            self.show_tooltip(c, f, &groups, &quick, hot, &action_rects, lay.collapse, &row, popover.as_ref());
            self.paint_key_tips(c, &fonts, &lay, &row, &groups, &quick, &action_rects);
        }

        // ── Design mode: dashed contextual tabs, « + » glyphs, the inline drop-down ──
        let mut regions = Vec::new();
        self.collect_regions(&lay, &row, &groups, &quick, &action_rects, strip_actions, &mut regions);
        self.design_drop = None;
        if let Some(d) = self.design.clone() {
            self.paint_design(c, &col, &fonts, &d, &lay, &row, &groups, row_shown, bounds, &mut regions);
        }
        self.regions = regions.clone();

        let content = Rect::new(bounds.left, (bounds.top + height).min(bounds.bottom), bounds.right, bounds.bottom);
        RibbonRun { events, height, content, backstage, regions }
    }

    /// The strip: the tabs, the quick actions, the collapse button, the contextual group headers
    /// and (design) the « + » of a new tab.
    fn layout_strip(&self, c: &dyn Canvas, fonts: &Fonts, strip: Rect, actions_n: usize) -> StripLayout {
        let colored = self.theme.tone.is_some();
        let tab_top = if colored { strip.bottom - m::COLORED_TAB_H } else { strip.top };
        let mut x = strip.left + m::EDGE_PAD;
        let mut tabs = Vec::new();
        let mut actions = Vec::new();
        let mut headers = Vec::new();
        let place_actions = |x: &mut f32, actions: &mut Vec<Rect>, before: f32, after: f32| {
            if actions_n == 0 {
                return;
            }
            *x += before;
            let cy = (strip.top + strip.bottom) / 2.0;
            for _ in 0..actions_n {
                actions.push(Rect::new(*x, cy - m::ACTION_BTN / 2.0, *x + m::ACTION_BTN, cy + m::ACTION_BTN / 2.0));
                *x += m::ACTION_BTN + m::TAB_GAP;
            }
            *x += after - m::TAB_GAP;
        };
        let vis = self.visible();
        let has_file = vis.iter().any(|&i| self.tabs[i].backstage);
        if !has_file {
            place_actions(&mut x, &mut actions, 0.0, m::ACTION_MARGIN);
        }
        let mut prev_group: Option<String> = None;
        for i in vis {
            let t = &self.tabs[i];
            let group = t.contextual.as_ref().and_then(|ctx| ctx.group.clone());
            if let (Some(ctx), Some(g)) = (&t.contextual, &group) {
                if prev_group.as_ref() != Some(g) {
                    if let Some(h) = ctx.header.as_deref().filter(|h| !h.is_empty()) {
                        let w = c.measure(h, &fonts.small_b) + m::CTX_HEADER_PAD_X * 2.0;
                        let cy = (strip.top + strip.bottom) / 2.0;
                        let r = Rect::new(x, cy - m::CTX_HEADER_H / 2.0, x + w, cy + m::CTX_HEADER_H / 2.0);
                        headers.push((g.clone(), h.to_string(), r, ctx.accent));
                        x += w + m::TAB_GAP * 2.0;
                    }
                }
            }
            prev_group = group;
            let font = if t.backstage { &fonts.tab_file } else { &fonts.tab };
            let mut w = c.measure(&t.label, font) + m::TAB_PAD_X * 2.0;
            if t.contextual.is_some() {
                w += c.measure("●", &fonts.dot) + m::CONTEXT_DOT_GAP;
            }
            tabs.push((i, Rect::new(x, tab_top, x + w, strip.bottom)));
            x += w + m::TAB_GAP;
            if t.backstage {
                place_actions(&mut x, &mut actions, m::ACTION_MARGIN - m::TAB_GAP, m::ACTION_MARGIN);
            }
        }
        let add_tab = self.design.as_ref().filter(|d| d.add_glyphs).map(|_| {
            let cy = (strip.top + strip.bottom) / 2.0;
            let r = Rect::new(x + 2.0, cy - m::ADD_GLYPH / 2.0, x + 2.0 + m::ADD_GLYPH, cy + m::ADD_GLYPH / 2.0);
            x += m::ADD_GLYPH + 6.0;
            r
        });
        let bx = x - m::TAB_GAP + m::ACTION_MARGIN;
        let collapse = Rect::new(bx, strip.bottom - 2.0 - m::COLLAPSE_BTN, bx + m::COLLAPSE_BTN, strip.bottom - 2.0);
        StripLayout { tabs, actions, collapse: Some(collapse), headers, add_tab }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_strip(&self, c: &dyn Canvas, col: &Colors, fonts: &Fonts, strip: Rect, lay: &StripLayout, actions: &[Rect], quick: &[RibbonItem], active: Option<usize>, look: Look) {
        c.fill_rounded(&strip, 0.0, &col.strip_bg);
        for (_, header, r, accent) in &lay.headers {
            let (fill, ink) = header_colors(*accent, col.strip_bg);
            c.fill_rounded(r, 4.0, &fill);
            c.text(header, r, &fonts.small_b, &ink, true);
        }
        for &(i, r) in &lay.tabs {
            let t = &self.tabs[i];
            let hot = is(look.hot, Target::Tab(i));
            let on = active == Some(i);
            if t.backstage {
                // The « Fichier » pill: solid, lighter shade of the tone;
                // `brightness(1.1)` on hover.
                let bg = if hot { lighten(col.file_bg, 0.1) } else { col.file_bg };
                c.fill_top_rounded(&r, m::TAB_RADIUS, &bg);
                c.text(&t.label, &r, &fonts.tab_file, &WHITE, true);
                continue;
            }
            if on {
                c.fill_top_rounded(&r, m::TAB_RADIUS, &col.bg);
            } else if hot {
                c.fill_top_rounded(&r, m::TAB_RADIUS, &col.strip_hover);
            }
            let ink = if on { col.accent } else { col.strip_text };
            let mut text_r = r;
            if let Some(ctx) = &t.contextual {
                // `borderTop: 2px solid accent`, then the dot before the label.
                c.fill_rounded(&Rect::new(r.left + 1.0, r.top, r.right - 1.0, r.top + m::CONTEXT_RULE), 0.0, &ctx.accent);
                let dot_w = c.measure("●", &fonts.dot);
                let label_w = c.measure(&t.label, &fonts.tab);
                let start = (r.left + r.right) / 2.0 - (dot_w + m::CONTEXT_DOT_GAP + label_w) / 2.0;
                let dot_ink = if on { ctx.accent } else { col.strip_text };
                c.text("●", &Rect::new(start, r.top, start + dot_w, r.bottom), &fonts.dot, &dot_ink, false);
                text_r = Rect::new(start + dot_w + m::CONTEXT_DOT_GAP, r.top, start + dot_w + m::CONTEXT_DOT_GAP + label_w + 1.0, r.bottom);
                c.text(&t.label, &text_r, &fonts.tab, &ink, false);
            } else {
                c.text(&t.label, &text_r, &fonts.tab, &ink, true);
            }
            if on && !col.colored {
                let u = Rect::new(r.left + m::UNDERLINE_INSET, r.bottom - m::UNDERLINE_H, r.right - m::UNDERLINE_INSET, r.bottom);
                c.fill_rounded(&u, m::UNDERLINE_H, &col.accent);
            }
        }
        for (i, r) in actions.iter().enumerate() {
            let Some(it) = quick.get(i) else { continue };
            let hot = !it.disabled && is(look.hot, Target::Action(i));
            if hot {
                c.fill_rounded(r, 4.0, &col.strip_hover);
            }
            if let Some(icon) = &it.icon {
                c.vector_icon(icon_str(icon), r, m::SMALL_ICON, &Colors::faded(col.strip_text, it.disabled));
            }
        }
        if let Some(r) = lay.collapse {
            let hot = is(look.hot, Target::Collapse);
            // `text-white/70 hover:bg-white/15 hover:text-white` on the band,
            // `text-text-tertiary hover:bg-black/5` on a plain strip.
            if hot {
                c.fill_rounded(&r, 4.0, &if col.colored { alpha(WHITE, 0.15) } else { col.hover });
            }
            let ink = if col.colored {
                alpha(WHITE, if hot { 1.0 } else { 0.7 })
            } else if hot {
                col.text
            } else {
                c.theme().text_tertiary
            };
            let icon = if self.collapsed { "ChevronDown" } else { "ChevronUp" };
            c.vector_icon(icon, &r, m::COLLAPSE_ICON, &ink);
        }
    }

    fn target_disabled(&self, groups: &[RibbonGroup], quick: &[RibbonItem], t: &Target) -> bool {
        match *t {
            Target::Item(k) | Target::Chevron(k) | Target::Option(k, _) | Target::Part(k, _) => item_of(groups, k).is_none_or(|it| it.disabled),
            Target::Action(i) => quick.get(i).is_none_or(|it| it.disabled),
            _ => false,
        }
    }

    /// Where item `k` was placed (the popover's copy first, when it is open).
    fn placed<'a>(row: &'a RowLayout, popover: Option<&'a (Rect, GroupBox)>, k: ItemKey) -> Option<&'a PlacedItem> {
        popover
            .and_then(|(_, gb)| gb.items.iter().find(|p| p.key == k))
            .or_else(|| row.groups.iter().flat_map(|g| g.items.iter()).find(|p| p.key == k))
    }

    fn scroll_gallery(&mut self, it: &RibbonItem, delta: i32) {
        let Some(spec) = &it.gallery else { return };
        let cols = spec.columns.max(1);
        let max = it.options.len().div_ceil(cols).saturating_sub(spec.rows.max(1));
        let row = self.gallery_rows.entry(it.id.clone()).or_insert(0);
        *row = (*row as i32 + delta).clamp(0, max as i32) as usize;
    }

    /// Runs a click on `t`.
    #[allow(clippy::too_many_arguments)]
    fn activate(
        &mut self,
        c: &dyn Canvas,
        t: Target,
        groups: &[RibbonGroup],
        quick: &[RibbonItem],
        row: &RowLayout,
        popover: Option<&(Rect, GroupBox)>,
        clicks: u8,
        events: &mut Vec<RibbonEvent>,
    ) {
        let _ = c;
        match t {
            Target::Tab(i) => {
                let id = self.tabs[i].id.clone();
                let file = self.tabs[i].backstage;
                if self.collapsed && !file {
                    // Collapsed: a click peeks; the same tab again closes it.
                    let closing = self.peeking && self.active == id;
                    self.peeking = !closing;
                } else {
                    self.peeking = false;
                }
                self.close_floats();
                if self.active != id {
                    self.active = id.clone();
                    events.push(RibbonEvent::TabChanged(id));
                }
            }
            Target::Action(i) => {
                if let Some(it) = quick.get(i) {
                    events.push(RibbonEvent::Clicked(it.id.clone()));
                }
            }
            Target::Collapse => {
                self.collapsed = !self.collapsed;
                self.peeking = false;
                self.close_floats();
                events.push(RibbonEvent::Collapsed(self.collapsed));
            }
            Target::Chip(gi) => {
                self.float = None;
                self.chip = if self.chip == Some(gi) { None } else { Some(gi) };
            }
            Target::Launcher(gi) => {
                if let Some(g) = groups.get(gi) {
                    events.push(RibbonEvent::Launcher(g.id.clone()));
                }
            }
            Target::Overflow => {
                let Some((anchor, keys)) = row.overflow.clone() else { return };
                if matches!(&self.float, Some(Float::Menu { overflow: true, .. })) {
                    self.float = None;
                    return;
                }
                let mut rows = Vec::new();
                let mut ids = Vec::new();
                for k in keys {
                    let Some(it) = item_of(groups, k) else { continue };
                    let label = it.label.clone().or_else(|| it.tooltip.clone()).unwrap_or_else(|| it.id.clone());
                    let mut e = MenuEntry::new(label).enabled(!it.disabled).checked(it.active && it.kind == ItemKind::Toggle);
                    if let Some(icon) = &it.icon {
                        e = e.icon(icon.to_string());
                    }
                    rows.push(e.build());
                    ids.push((it.id.clone(), Vec::new()));
                }
                self.float = Some(Float::Menu { owner: String::new(), key: Some(Target::Overflow), anchor, menu: Box::new(Menu::with_items(rows)), ids, overflow: true });
            }
            Target::Option(k, oi) => {
                if let Some(it) = item_of(groups, k) {
                    if let Some(o) = it.options.get(oi) {
                        events.push(RibbonEvent::Changed { item: it.id.clone(), value: o.value.clone() });
                    }
                }
            }
            Target::Part(k, part) => {
                let Some(it) = item_of(groups, k) else { return };
                match it.kind {
                    ItemKind::NumericField => {
                        let (min, max, step) = it.range.unwrap_or((f32::MIN, f32::MAX, 1.0));
                        let v = it.value.as_deref().and_then(|v| v.replace(',', ".").trim().parse::<f32>().ok()).unwrap_or(min.max(0.0));
                        let nv = (v + if part == 0 { step } else { -step }).clamp(min, max);
                        self.edit = None;
                        events.push(RibbonEvent::Changed { item: it.id.clone(), value: format_number(nv) });
                    }
                    ItemKind::Gallery => match part {
                        0 => self.scroll_gallery(it, -1),
                        1 => self.scroll_gallery(it, 1),
                        _ => {
                            let anchor = Self::placed(row, popover, k).map(|p| p.rect).unwrap_or_default();
                            self.float = Some(gallery_panel(it, Target::Part(k, 2), anchor, &self.texts));
                        }
                    },
                    _ => {}
                }
            }
            Target::Item(k) | Target::Chevron(k) => {
                let Some(it) = item_of(groups, k) else { return };
                let Some(p) = Self::placed(row, popover, k) else { return };
                let anchor = match (t, p.chevron) {
                    (Target::Chevron(_), Some(ch)) if it.size == ItemSize::Small => Rect::new(p.rect.left, p.rect.top, ch.right, ch.bottom),
                    _ => p.rect,
                };
                let chevron_click = matches!(t, Target::Chevron(_));
                match it.kind {
                    ItemKind::Dropdown => {
                        let already = matches!(&self.float, Some(Float::List { key, .. }) if *key == k);
                        self.float = if already { None } else { Some(open_list(it, k, anchor)) };
                    }
                    ItemKind::ComboBox if chevron_click || !it.editable => {
                        let full = Rect::new(p.rect.left, p.rect.top, p.chevron.map_or(p.rect.right, |ch| ch.right.max(p.rect.right)), p.rect.bottom);
                        let already = matches!(&self.float, Some(Float::List { key, .. }) if *key == k);
                        self.edit = None;
                        self.float = if already { None } else { Some(open_list(it, k, full)) };
                    }
                    ItemKind::ComboBox | ItemKind::NumericField | ItemKind::TextBox => {
                        if self.edit.as_ref().is_none_or(|e| e.key != k) {
                            self.float = None;
                            self.edit = Some(EditState { id: it.id.clone(), key: k, text: it.value.clone().unwrap_or_default(), fresh: true });
                        }
                    }
                    ItemKind::Label | ItemKind::ControlGroup | ItemKind::Box => {}
                    ItemKind::Menu | ItemKind::Split | ItemKind::ColorPicker | ItemKind::Gallery if chevron_click || !it.has_action || it.kind == ItemKind::Gallery => {
                        let already = self.open_target() == Some(Target::Item(k));
                        self.float = if already {
                            None
                        } else if it.kind == ItemKind::ColorPicker {
                            Some(color_panel(it, Target::Item(k), anchor, &self.texts))
                        } else if it.kind == ItemKind::Gallery {
                            Some(gallery_panel(it, Target::Item(k), anchor, &self.texts))
                        } else if it.menu_is_panel() {
                            Some(menu_panel(it, Target::Item(k), anchor))
                        } else {
                            Some(open_menu(it, k, anchor))
                        };
                    }
                    _ => {
                        events.push(RibbonEvent::Clicked(it.id.clone()));
                        if clicks >= 2 {
                            events.push(RibbonEvent::DoubleClicked(it.id.clone()));
                        }
                    }
                }
            }
        }
    }

    fn open_target(&self) -> Option<Target> {
        match &self.float {
            Some(Float::List { key, .. }) => Some(Target::Item(*key)),
            Some(Float::Menu { key, .. }) | Some(Float::Panel { key, .. }) => *key,
            Some(Float::Qat { .. }) => None,
            None => self.chip.map(Target::Chip),
        }
    }

    /// Places the open menu / list / panel against `area`; its panel in client DIP.
    fn float_panel(&mut self, c: &dyn Canvas, area: Rect) -> Option<Rect> {
        let e = lists::VIEWPORT_EDGE;
        let place = |anchor: Rect, w: f32, h: f32| {
            let x = anchor.left.min(area.right - e - w).max(area.left + e);
            let mut y = anchor.bottom + m::POPOVER_GAP;
            if y + h > area.bottom - e {
                y = (anchor.top - m::POPOVER_GAP - h).max(area.top + e);
            }
            Rect::new(x, y, x + w, y + h)
        };
        match self.float.as_mut()? {
            Float::Menu { anchor, menu, .. } | Float::Qat { anchor, menu, .. } => {
                menu.viewport = Some(area);
                let want = menu.measure(c);
                Some(place(*anchor, want.width, want.height))
            }
            Float::List { anchor, list, .. } => {
                list.place_drop_down(c, *anchor, area);
                Some(list.drop_down_rect(*anchor))
            }
            Float::Panel { anchor, panel, .. } => {
                let (w, h) = panel.measure(c);
                Some(place(*anchor, w, h))
            }
        }
    }

    /// The open float's panel, and its cascaded sub-menu (both take the pointer).
    fn float_hit_rect(&mut self, c: &dyn Canvas, area: Rect) -> Option<Rect> {
        let panel = self.float_panel(c, area)?;
        if let Some(Float::Menu { menu, .. }) = &self.float {
            if let Some(sub) = menu.open_submenu.and_then(|i| menu.submenu_rect_in(c, panel, i)) {
                return Some(Rect::new(panel.left.min(sub.left), panel.top.min(sub.top), panel.right.max(sub.right), panel.bottom.max(sub.bottom)));
            }
        }
        Some(panel)
    }

    fn hover_float(&mut self, c: &dyn Canvas, area: Rect, x: f32, y: f32, wheel: f32) {
        let Some(panel) = self.float_panel(c, area) else { return };
        match self.float.as_mut() {
            Some(Float::Menu { menu, .. }) | Some(Float::Qat { menu, .. }) => {
                let sub_rect = menu.open_submenu.and_then(|i| menu.submenu_rect_in(c, panel, i));
                if let (Some(sr), Some(si)) = (sub_rect, menu.open_submenu) {
                    if sr.contains(x, y) {
                        if let Some(sub) = menu.submenu(si) {
                            menu.submenu_hot = sub.item_at(sr, x, y).filter(|&j| sub.is_actionable(j));
                        }
                        return;
                    }
                }
                if panel.contains(x, y) {
                    if let Some(i) = menu.item_at(panel, x, y).filter(|&i| menu.is_actionable(i)) {
                        menu.hot_index = Some(i);
                        let has_sub = menu.submenu(i).is_some();
                        if has_sub && menu.open_submenu != Some(i) {
                            menu.open_submenu = Some(i);
                            menu.submenu_hot = None;
                        } else if !has_sub {
                            menu.open_submenu = None;
                            menu.submenu_hot = None;
                        }
                    }
                }
            }
            Some(Float::List { list, .. }) if panel.contains(x, y) => {
                if wheel != 0.0 {
                    list.scroll_by_dip(wheel);
                    host::claim_wheel();
                }
                if let Some(i) = list.item_at_in(panel, x, y) {
                    list.hot_index = Some(i);
                }
            }
            Some(Float::Panel { panel: p, .. }) => {
                p.hot = p.hit(panel, x, y);
            }
            _ => {}
        }
    }

    fn click_float(&mut self, c: &dyn Canvas, area: Rect, x: f32, y: f32, events: &mut Vec<RibbonEvent>) {
        let Some(panel) = self.float_panel(c, area) else { return };
        let mut close = false;
        let mut qat_change: Option<(String, bool)> = None;
        match self.float.as_mut() {
            Some(Float::Menu { owner, menu, ids, overflow, .. }) => {
                let sub_rect = menu.open_submenu.and_then(|i| menu.submenu_rect_in(c, panel, i));
                let pick = match (sub_rect, menu.open_submenu) {
                    (Some(sr), Some(si)) if sr.contains(x, y) => menu.submenu(si).and_then(|sub| sub.item_at(sr, x, y).filter(|&j| sub.is_actionable(j))).map(|j| (si, Some(j))),
                    _ => menu.item_at(panel, x, y).filter(|&i| menu.is_actionable(i) && menu.submenu(i).is_none()).map(|i| (i, None)),
                };
                if let Some((i, sub)) = pick {
                    if let Some((id, subs)) = ids.get(i) {
                        let entry = match sub {
                            Some(j) => subs.get(j).cloned().unwrap_or_default(),
                            None => id.clone(),
                        };
                        if *overflow {
                            events.push(RibbonEvent::Clicked(entry));
                        } else {
                            events.push(RibbonEvent::Chosen { item: owner.clone(), entry });
                        }
                    }
                    close = true;
                }
            }
            Some(Float::Qat { id, add, menu, .. }) => {
                if menu.item_at(panel, x, y).is_some() {
                    qat_change = Some((id.clone(), *add));
                    close = true;
                }
            }
            Some(Float::List { owner, list, values, .. }) => {
                if let Some(i) = list.item_at_in(panel, x, y) {
                    if let Some(v) = values.get(i) {
                        events.push(RibbonEvent::Changed { item: owner.clone(), value: v.clone() });
                    }
                    close = true;
                }
            }
            Some(Float::Panel { owner, panel: p, owners, .. }) => {
                if let Some(at) = p.hit(panel, x, y) {
                    let item = owners.get(at.0).cloned().flatten().unwrap_or_else(|| owner.clone());
                    match p.pick(at) {
                        Some(panel::Pick::Value(v)) => events.push(RibbonEvent::Changed { item, value: v }),
                        Some(panel::Pick::Entry(e)) => events.push(RibbonEvent::Chosen { item, entry: e }),
                        None => {}
                    }
                    close = true;
                }
            }
            None => {}
        }
        if close {
            self.float = None;
        }
        if let Some((id, add)) = qat_change {
            if add {
                if !self.qat_custom.contains(&id) {
                    self.qat_custom.push(id);
                }
            } else {
                self.qat_custom.retain(|i| *i != id);
            }
            events.push(RibbonEvent::QatChanged(self.qat_custom.clone()));
        }
    }

    /// A right click on a control, or on a quick action the user added: the « add / remove »
    /// menu at the pointer.
    #[allow(clippy::too_many_arguments)]
    fn open_qat_menu(&mut self, hot: Option<Target>, groups: &[RibbonGroup], _row: &RowLayout, _popover: Option<&(Rect, GroupBox)>, quick: &[RibbonItem], _actions: &[Rect], x: f32, y: f32) {
        let (id, add) = match hot {
            Some(Target::Item(k) | Target::Chevron(k)) => match item_of(groups, k) {
                Some(it) if it.qat && !matches!(it.kind, ItemKind::Label | ItemKind::ControlGroup | ItemKind::Box) => (it.id.clone(), !self.qat_custom.contains(&it.id)),
                _ => return,
            },
            Some(Target::Action(i)) if i >= self.strip_actions.len() => match quick.get(i) {
                Some(it) => (it.id.clone(), false),
                None => return,
            },
            _ => return,
        };
        let label = if add { self.texts.qat_add.clone() } else { self.texts.qat_remove.clone() };
        let menu = Menu::with_items(vec![MenuEntry::new(label).icon(if add { "Plus" } else { "Minus" }).build()]);
        self.float = Some(Float::Qat { id, add, anchor: Rect::new(x, y, x + 1.0, y), menu: Box::new(menu) });
        self.chip = None;
    }

    /// Arrow keys, Enter and Escape while a menu, a list, a popover or the
    /// peek flyout is open.
    fn handle_float_keys(&mut self, _c: &dyn Canvas, _area: Rect, events: &mut Vec<RibbonEvent>) {
        let keys: Vec<u16> = host::consume(|e| {
            matches!(e, host::InputEvent::Key { vk: code, down: true, mods, .. }
                if mods.is_none() && [vk::UP, vk::DOWN, vk::HOME, vk::END, vk::ENTER, vk::SPACE, vk::ESCAPE, vk::LEFT, vk::RIGHT].contains(code))
        })
        .into_iter()
        .filter_map(|e| match e {
            host::InputEvent::Key { vk: code, .. } => Some(code),
            _ => None,
        })
        .collect();
        for code in keys {
            let mut close = false;
            match self.float.as_mut() {
                Some(Float::Menu { owner, menu, ids, overflow, .. }) => {
                    let mk = match code {
                        c if c == vk::UP => MenuKey::Up,
                        c if c == vk::DOWN => MenuKey::Down,
                        c if c == vk::HOME => MenuKey::Home,
                        c if c == vk::END => MenuKey::End,
                        c if c == vk::ENTER => MenuKey::Enter,
                        c if c == vk::SPACE => MenuKey::Space,
                        c if c == vk::LEFT => MenuKey::Left,
                        c if c == vk::RIGHT => MenuKey::Right,
                        _ => MenuKey::Escape,
                    };
                    match menu.navigate(mk) {
                        MenuOutcome::Chosen { index, sub } => {
                            if let Some((id, subs)) = ids.get(index) {
                                let entry = match sub {
                                    Some(j) => subs.get(j).cloned().unwrap_or_default(),
                                    None => id.clone(),
                                };
                                events.push(if *overflow { RibbonEvent::Clicked(entry) } else { RibbonEvent::Chosen { item: owner.clone(), entry } });
                            }
                            close = true;
                        }
                        MenuOutcome::Close => close = true,
                        _ => {}
                    }
                }
                Some(Float::Qat { .. }) | Some(Float::Panel { .. }) => {
                    if code == vk::ESCAPE {
                        close = true;
                    }
                }
                Some(Float::List { owner, list, values, .. }) => match list.key_down(code, Modifiers::NONE) {
                    ListKey::Committed(i) => {
                        if let Some(v) = values.get(i) {
                            events.push(RibbonEvent::Changed { item: owner.clone(), value: v.clone() });
                        }
                        close = true;
                    }
                    ListKey::Closed => close = true,
                    _ => {}
                },
                None => {
                    if code == vk::ESCAPE {
                        if self.chip.is_some() {
                            self.chip = None;
                        } else {
                            self.peeking = false;
                        }
                    }
                }
            }
            if close {
                self.float = None;
            }
        }
    }

    /// Typing in a field: characters, Backspace, Enter (commit) and Escape (cancel).
    fn handle_edit_keys(&mut self, events: &mut Vec<RibbonEvent>) {
        let taken = host::consume(|e| match e {
            host::InputEvent::Text(_) => true,
            host::InputEvent::Key { vk: code, down: true, .. } => [vk::BACK, vk::ENTER, vk::ESCAPE, vk::TAB].contains(code),
            _ => false,
        });
        for e in taken {
            let Some(state) = self.edit.as_mut() else { return };
            match e {
                host::InputEvent::Text(t) => {
                    if state.fresh {
                        state.text.clear();
                        state.fresh = false;
                    }
                    state.text.push_str(&t);
                }
                host::InputEvent::Key { vk: code, .. } if code == vk::BACK => {
                    if state.fresh {
                        state.text.clear();
                        state.fresh = false;
                    } else {
                        state.text.pop();
                    }
                }
                host::InputEvent::Key { vk: code, .. } if code == vk::ESCAPE => self.edit = None,
                host::InputEvent::Key { .. } => {
                    let groups = self.live_groups(self.current());
                    self.commit_edit(&groups, events);
                }
                _ => {}
            }
        }
    }

    /// Ends the typing: a numeric field's text is read as a number and kept in its range.
    fn commit_edit(&mut self, groups: &[RibbonGroup], events: &mut Vec<RibbonEvent>) {
        let Some(state) = self.edit.take() else { return };
        let value = match item_of(groups, state.key).filter(|it| it.id == state.id) {
            Some(it) if it.kind == ItemKind::NumericField => {
                let (min, max, _) = it.range.unwrap_or((f32::MIN, f32::MAX, 1.0));
                match state.text.replace(',', ".").trim().parse::<f32>() {
                    Ok(v) => format_number(v.clamp(min, max)),
                    Err(_) => return,
                }
            }
            _ => state.text,
        };
        events.push(RibbonEvent::Changed { item: state.id, value });
    }

    /// Paints the open menu / list / panel in its floating surface.
    fn show_float(&mut self, c: &dyn Canvas, area: Rect, col: &Colors, fonts: &Fonts) {
        let Some(panel) = self.float_panel(c, area) else { return };
        match self.float.as_ref() {
            Some(Float::Menu { menu, .. }) | Some(Float::Qat { menu, .. }) => {
                let pb = menu.paint_bounds(c, panel);
                let mut snap = menu.clone();
                snap.viewport = Some(Rect::new(area.left - pb.left, area.top - pb.top, area.right - pb.left, area.bottom - pb.top));
                let local = Rect::new(panel.left - pb.left, panel.top - pb.top, panel.right - pb.left, panel.bottom - pb.top);
                host::popup(pb, move |cv| snap.paint(cv, local, WidgetState::REST));
            }
            Some(Float::List { anchor, list, .. }) => {
                let pb = list.drop_down_paint_bounds(*anchor);
                let snap = list.clone();
                let local = Rect::new(panel.left - pb.left, panel.top - pb.top, panel.right - pb.left, panel.bottom - pb.top);
                host::popup(pb, move |cv| snap.paint_drop_down_at(cv, local));
            }
            Some(Float::Panel { panel: p, .. }) => {
                let pb = inflate(panel, lists::FLOAT_SHADOW_MARGIN);
                let snap = p.clone();
                let (font, accent) = (fonts.item.clone(), col.accent);
                host::popup(pb, move |cv| {
                    cv.push_offset(-pb.left, -pb.top);
                    snap.paint(cv, panel, &font, accent);
                    cv.pop_offset();
                });
            }
            None => {}
        }
    }

    /// The web's `title`: « label · shortcut » under a still pointer.
    #[allow(clippy::too_many_arguments)]
    fn show_tooltip(
        &mut self,
        c: &dyn Canvas,
        f: &host::Frame,
        groups: &[RibbonGroup],
        quick: &[RibbonItem],
        hot: Option<Target>,
        actions: &[Rect],
        collapse: Option<Rect>,
        row: &RowLayout,
        popover: Option<&(Rect, GroupBox)>,
    ) {
        let found: Option<(String, Rect)> = match hot {
            Some(Target::Item(k) | Target::Chevron(k)) => {
                let it = item_of(groups, k);
                match (it.and_then(|it| it.tip_text()), Self::placed(row, popover, k)) {
                    (Some(t), Some(p)) => Some((t, p.rect)),
                    _ => None,
                }
            }
            Some(Target::Option(k, oi)) => {
                let it = item_of(groups, k);
                match (it.and_then(|it| it.options.get(oi)), Self::placed(row, popover, k).and_then(|p| p.options.get(oi))) {
                    (Some(o), Some(r)) => Some((o.label.clone(), *r)),
                    _ => None,
                }
            }
            Some(Target::Part(k, part)) => {
                let is_gallery = item_of(groups, k).is_some_and(|it| it.kind == ItemKind::Gallery);
                let text = match (is_gallery, part) {
                    (true, 0) => Some(self.texts.scroll_up.clone()),
                    (true, 1) => Some(self.texts.scroll_down.clone()),
                    (true, _) => Some(self.texts.more.clone()),
                    _ => None,
                };
                text.zip(Self::placed(row, popover, k).and_then(|p| p.parts.get(part as usize).copied()))
            }
            Some(Target::Launcher(gi)) => {
                let r = row.groups.iter().find(|g| g.index == gi).and_then(|g| g.launcher);
                groups.get(gi).map(|g| format!("{} · {}", g.label, self.texts.launcher)).zip(r)
            }
            Some(Target::Action(i)) => quick.get(i).and_then(|it| it.tip_text()).zip(actions.get(i).copied()),
            Some(Target::Collapse) => collapse.map(|r| {
                (if self.collapsed { self.expand_label.clone() } else { self.collapse_label.clone() }, r)
            }),
            Some(Target::Chip(gi)) => {
                let r = row.chips.iter().find(|ch| ch.index == gi).map(|ch| ch.button);
                groups.get(gi).map(|g| g.label.clone()).zip(r)
            }
            _ => None,
        };
        if self.tip_target != hot {
            self.tip = TooltipTrigger::new();
            self.tip_target = hot;
        }
        let hovering = found.is_some() && self.float.is_none() && self.keytip == KeyTipMode::Off;
        let tick = self.tip.update(hovering, f.mouse, f.mouse_down, host::now_ms(), Tooltip::DELAY_MS);
        if let Some(ms) = tick.repaint_in_ms {
            host::request_repaint_after(ms);
        }
        let (Some(_), Some((text, anchor))) = (tick.show_at, found) else { return };
        let tip = Tooltip::new(text).side(Side::Bottom);
        let size = tip.measure(c);
        let area = f.screen_area();
        let rel = Rect::new(anchor.left - area.left, anchor.top - area.top, anchor.right - area.left, anchor.bottom - area.top);
        let placed = place(rel, size, Side::Bottom, Size::new(area.right - area.left, area.bottom - area.top));
        let rect = Rect::new(placed.rect.left + area.left, placed.rect.top + area.top, placed.rect.right + area.left, placed.rect.bottom + area.top);
        let (tx, ty) = (placed.tip.0 + area.left, placed.tip.1 + area.top);
        let mm = crate::editors::shadow_margin();
        let pb = Rect::new(rect.left.min(tx) - mm, rect.top.min(ty) - mm, rect.right.max(tx) + mm, rect.bottom.max(ty) + mm);
        let local = Placement {
            rect: Rect::new(rect.left - pb.left, rect.top - pb.top, rect.right - pb.left, rect.bottom - pb.top),
            side: placed.side,
            tip: (tx - pb.left, ty - pb.top),
        };
        host::overlay(pb, move |cv| tip.paint_placed(cv, &local, WidgetState::REST));
    }

    // ── KeyTips ────────────────────────────────────────────────────────────

    /// The first level: the tabs (« Fichier » is F) and the quick actions (digits).
    fn tab_tips(&self, lay: &StripLayout, quick: &[RibbonItem]) -> Vec<(Target, String)> {
        let entries: Vec<(Option<&str>, &str)> = lay
            .tabs
            .iter()
            .map(|(i, _)| {
                let t = &self.tabs[*i];
                (t.key_tip.as_deref().or(if t.backstage { Some("F") } else { None }), t.label.as_str())
            })
            .collect();
        let tips = keytips::assign(&entries, false);
        let mut out: Vec<(Target, String)> = lay.tabs.iter().zip(tips).map(|((i, _), t)| (Target::Tab(*i), t)).collect();
        for (i, _) in quick.iter().enumerate().take(9) {
            out.push((Target::Action(i), (i + 1).to_string()));
        }
        out
    }

    /// The second level: the controls of the active tab, its folded chips and its launchers.
    fn item_tips(&self, row: &RowLayout, groups: &[RibbonGroup]) -> Vec<(Target, String)> {
        let mut targets = Vec::new();
        let mut entries: Vec<(Option<String>, String)> = Vec::new();
        for gb in &row.groups {
            for p in &gb.items {
                let Some(it) = item_of(groups, p.key) else { continue };
                if p.container || matches!(it.kind, ItemKind::Label | ItemKind::Separator) {
                    continue;
                }
                targets.push(Target::Item(p.key));
                entries.push((it.key_tip.clone(), it.label.clone().or_else(|| it.tooltip.clone()).unwrap_or_else(|| it.id.clone())));
            }
            if let (Some(_), Some(g)) = (gb.launcher, groups.get(gb.index)) {
                targets.push(Target::Launcher(gb.index));
                entries.push((None, format!("Z{}", g.label)));
            }
        }
        for ch in &row.chips {
            if let Some(g) = groups.get(ch.index) {
                targets.push(Target::Chip(ch.index));
                entries.push((g.key_tip.clone(), format!("Z{}", g.label)));
            }
        }
        let refs: Vec<(Option<&str>, &str)> = entries.iter().map(|(k, l)| (k.as_deref(), l.as_str())).collect();
        targets.into_iter().zip(keytips::assign(&refs, true)).collect()
    }

    /// Alt (pressed and released alone) shows the KeyTips; letters then pick a tab, then a control.
    fn run_key_tips(&mut self, lay: &StripLayout, row: &RowLayout, groups: &[RibbonGroup], quick: &[RibbonItem], events: &mut Vec<RibbonEvent>) {
        if !self.key_tips {
            return;
        }
        for e in host::events() {
            match e {
                host::InputEvent::Key { vk: code, down: true, repeat: false, .. } if code == vk::MENU => self.alt_armed = true,
                host::InputEvent::Key { vk: code, down: true, .. } if code != vk::MENU => self.alt_armed = false,
                host::InputEvent::Key { vk: code, down: false, .. } if code == vk::MENU && self.alt_armed => {
                    self.alt_armed = false;
                    self.keytip = if self.keytip == KeyTipMode::Off { KeyTipMode::Tabs(String::new()) } else { KeyTipMode::Off };
                    self.close_floats();
                    host::request_repaint_after(0);
                }
                _ => {}
            }
        }
        if self.keytip == KeyTipMode::Off {
            return;
        }
        let typed_keys = host::consume(|e| {
            matches!(e, host::InputEvent::Key { vk: code, down: true, .. } if *code == vk::ESCAPE || (0x30..=0x39).contains(code) || (0x41..=0x5A).contains(code))
        });
        host::consume(|e| matches!(e, host::InputEvent::Text(_)));
        for e in typed_keys {
            let host::InputEvent::Key { vk: code, .. } = e else { continue };
            if code == vk::ESCAPE {
                self.keytip = match &self.keytip {
                    KeyTipMode::Items(_) => KeyTipMode::Tabs(String::new()),
                    _ => KeyTipMode::Off,
                };
                continue;
            }
            let ch = char::from_u32(code as u32).unwrap_or(' ');
            let (typed, tips) = match &self.keytip {
                KeyTipMode::Tabs(t) => (format!("{t}{ch}"), self.tab_tips(lay, quick)),
                KeyTipMode::Items(t) => (format!("{t}{ch}"), self.item_tips(row, groups)),
                KeyTipMode::Off => return,
            };
            let items_level = matches!(self.keytip, KeyTipMode::Items(_));
            if let Some((target, _)) = tips.iter().find(|(_, tip)| *tip == typed) {
                match *target {
                    Target::Tab(i) => {
                        let id = self.tabs[i].id.clone();
                        if self.active != id {
                            self.active = id.clone();
                            events.push(RibbonEvent::TabChanged(id));
                        }
                        self.keytip = if self.tabs[i].backstage { KeyTipMode::Off } else { KeyTipMode::Items(String::new()) };
                    }
                    Target::Action(i) => {
                        if let Some(it) = quick.get(i) {
                            events.push(RibbonEvent::Clicked(it.id.clone()));
                        }
                        self.keytip = KeyTipMode::Off;
                    }
                    t => {
                        self.keytip = KeyTipMode::Off;
                        self.pending_key_tip = Some(t);
                    }
                }
            } else if tips.iter().any(|(_, tip)| tip.starts_with(&typed)) {
                self.keytip = if items_level { KeyTipMode::Items(typed) } else { KeyTipMode::Tabs(typed) };
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_key_tips(&mut self, c: &dyn Canvas, fonts: &Fonts, lay: &StripLayout, row: &RowLayout, groups: &[RibbonGroup], quick: &[RibbonItem], actions: &[Rect]) {
        let (typed, tips, rect_of): (String, Vec<(Target, String)>, TipAnchor) = match &self.keytip {
            KeyTipMode::Off => return,
            KeyTipMode::Tabs(t) => {
                let tabs = lay.tabs.clone();
                let actions = actions.to_vec();
                (
                    t.clone(),
                    self.tab_tips(lay, quick),
                    Box::new(move |target| match target {
                        Target::Tab(i) => tabs.iter().find(|(j, _)| *j == i).map(|(_, r)| ((r.left + r.right) / 2.0, r.bottom)),
                        Target::Action(i) => actions.get(i).map(|r| ((r.left + r.right) / 2.0, r.bottom)),
                        _ => None,
                    }),
                )
            }
            KeyTipMode::Items(t) => {
                let row2 = row.clone();
                (
                    t.clone(),
                    self.item_tips(row, groups),
                    Box::new(move |target| match target {
                        Target::Item(k) => row2.groups.iter().flat_map(|g| g.items.iter()).find(|p| p.key == k).map(|p| {
                            let r = p.rect;
                            if r.bottom - r.top > 40.0 { ((r.left + r.right) / 2.0, r.bottom - 4.0) } else { (r.left + 10.0, r.bottom - 2.0) }
                        }),
                        Target::Launcher(g) => row2.groups.iter().find(|b| b.index == g).and_then(|b| b.launcher).map(|r| ((r.left + r.right) / 2.0, r.bottom)),
                        Target::Chip(g) => row2.chips.iter().find(|ch| ch.index == g).map(|ch| ((ch.button.left + ch.button.right) / 2.0, ch.button.bottom)),
                        _ => None,
                    }),
                )
            }
        };
        let font = fonts.small_b.clone();
        let mut badges: Vec<(String, f32, f32, bool)> = Vec::new();
        for (target, tip) in tips {
            if let Some((x, y)) = rect_of(target) {
                badges.push((tip.clone(), x, y, !typed.is_empty() && !tip.starts_with(&typed)));
            }
        }
        for (tip, x, y, dim) in badges {
            keytips::paint_badge(c, &tip, x, y, &font, dim);
        }
        host::request_repaint_after(250);
    }

    // ── Regions and design mode ────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn collect_regions(&self, lay: &StripLayout, row: &RowLayout, groups: &[RibbonGroup], quick: &[RibbonItem], actions: &[Rect], strip_actions: usize, out: &mut Vec<RibbonRegion>) {
        for (g, _, r, _) in &lay.headers {
            out.push(RibbonRegion::new(g.clone(), RegionKind::ContextualHeader, *r));
        }
        for (i, r) in &lay.tabs {
            let t = &self.tabs[*i];
            out.push(RibbonRegion::new(t.id.clone(), if t.backstage { RegionKind::FileTab } else { RegionKind::Tab }, *r));
        }
        for (i, r) in actions.iter().enumerate() {
            if let Some(it) = quick.get(i) {
                let _ = strip_actions;
                out.push(RibbonRegion::new(it.id.clone(), RegionKind::QuickAction, *r));
            }
        }
        if let Some(r) = lay.collapse {
            out.push(RibbonRegion::new("collapse", RegionKind::Collapse, r));
        }
        for gb in &row.groups {
            let Some(g) = groups.get(gb.index) else { continue };
            out.push(RibbonRegion::new(g.id.clone(), RegionKind::Group, gb.bounds));
            for p in &gb.items {
                let Some(it) = item_of(groups, p.key) else { continue };
                let rect = match p.chevron {
                    Some(ch) if it.size == ItemSize::Small && matches!(it.kind, ItemKind::Split | ItemKind::ColorPicker) => Rect::new(p.rect.left, p.rect.top, ch.right, p.rect.bottom),
                    _ => p.rect,
                };
                out.push(RibbonRegion::new(it.id.clone(), RegionKind::Item, rect));
                for (n, r) in p.options.iter().enumerate() {
                    if r.right > r.left {
                        out.push(RibbonRegion { parent: Some(it.id.clone()), ..RibbonRegion::new(it.id.clone(), RegionKind::GalleryItem(n), *r) });
                    }
                }
            }
            if let Some(l) = gb.launcher {
                out.push(RibbonRegion::new(g.id.clone(), RegionKind::Launcher, l));
            }
        }
        for ch in &row.chips {
            if let Some(g) = groups.get(ch.index) {
                let r = Rect::new(ch.label.left, ch.button.top, ch.label.right, ch.label.bottom);
                out.push(RibbonRegion::new(g.id.clone(), RegionKind::Chip, r));
            }
        }
    }

    /// Design mode: dashed outlines around the contextual tabs, « + » glyphs, and the inline
    /// drop-down of the control the designer opened (painted by [`Self::paint_design_overlay`]).
    #[allow(clippy::too_many_arguments)]
    fn paint_design(&mut self, c: &dyn Canvas, col: &Colors, fonts: &Fonts, d: &RibbonDesign, lay: &StripLayout, row: &RowLayout, groups: &[RibbonGroup], row_shown: bool, bounds: Rect, regions: &mut Vec<RibbonRegion>) {
        let _ = fonts;
        let dash = if col.colored { alpha(WHITE, 0.85) } else { col.text_dim };
        for (i, r) in &lay.tabs {
            let t = &self.tabs[*i];
            if t.contextual.is_some() && !t.visible {
                dashed_rect(c, *r, &dash);
            }
        }
        for (g, _, r, _) in &lay.headers {
            let hidden = self.tabs.iter().filter(|t| t.contextual.as_ref().and_then(|x| x.group.as_ref()) == Some(g)).all(|t| !t.visible);
            if hidden {
                dashed_rect(c, Rect::new(r.left - 2.0, r.top - 2.0, r.right + 2.0, r.bottom + 2.0), &dash);
            }
        }
        if d.add_glyphs {
            if let Some(r) = lay.add_tab {
                paint_add_glyph(c, col, r, true);
                regions.push(RibbonRegion::new("", RegionKind::AddTab, r));
            }
            if row_shown {
                let tab_id = self.current().map(|i| self.tabs[i].id.clone()).unwrap_or_default();
                let last_right = row.groups.iter().map(|g| g.bounds.right).chain(row.chips.iter().map(|ch| ch.label.right)).fold(row.rect.left + m::EDGE_PAD, f32::max);
                let cy = (row.rect.top + row.rect.bottom) / 2.0;
                let r = Rect::new(last_right + 6.0, cy - m::ADD_GLYPH / 2.0, last_right + 6.0 + m::ADD_GLYPH, cy + m::ADD_GLYPH / 2.0);
                paint_add_glyph(c, col, r, false);
                regions.push(RibbonRegion::new(tab_id, RegionKind::AddGroup, r));
                for gb in &row.groups {
                    let Some(g) = groups.get(gb.index) else { continue };
                    let r = Rect::new(gb.bounds.left + 2.0, gb.label.top + (gb.label.bottom - gb.label.top - m::ADD_GLYPH) / 2.0, gb.bounds.left + 2.0 + m::ADD_GLYPH, gb.label.top + (gb.label.bottom - gb.label.top + m::ADD_GLYPH) / 2.0);
                    paint_add_glyph(c, col, r, false);
                    regions.push(RibbonRegion::new(g.id.clone(), RegionKind::AddItem, r));
                }
            }
        }
        // The inline drop-down of the control the designer opened.
        let Some(open) = d.open_menu.as_deref() else { return };
        let found = row.groups.iter().flat_map(|g| g.items.iter()).find_map(|p| item_of(groups, p.key).filter(|it| it.id == open).map(|it| (it.clone(), p.rect, p.chevron)));
        let Some((it, rect, chevron)) = found else { return };
        if it.split_items.is_empty() && !matches!(it.kind, ItemKind::ColorPicker | ItemKind::Gallery) {
            return;
        }
        let anchor = Rect::new(rect.left, rect.top, chevron.map_or(rect.right, |ch| ch.right.max(rect.right)), rect.bottom);
        let key = Target::Item(ItemKey { group: 0, item: 0, sub: None });
        let float = if it.kind == ItemKind::ColorPicker {
            color_panel(&it, key, anchor, &self.texts)
        } else if it.kind == ItemKind::Gallery {
            gallery_panel(&it, key, anchor, &self.texts)
        } else if it.menu_is_panel() {
            menu_panel(&it, key, anchor)
        } else {
            open_menu(&it, ItemKey { group: 0, item: 0, sub: None }, anchor)
        };
        let place = |w: f32, h: f32| {
            let x = anchor.left.min(bounds.right - w - 4.0).max(bounds.left);
            Rect::new(x, anchor.bottom + m::POPOVER_GAP, x + w, anchor.bottom + m::POPOVER_GAP + h)
        };
        match float {
            Float::Menu { menu, ids, .. } => {
                let want = menu.measure(c);
                let r = place(want.width, want.height);
                for (i, (id, _)) in ids.iter().enumerate() {
                    if let (Some(ir), true) = (menu.item_rect(r, i), menu.is_actionable(i) || !id.is_empty()) {
                        if matches!(menu.items().get(i), Some(kubuno_desktop_controls::toolstrip::StripItem::MenuItem(_))) {
                            regions.push(RibbonRegion { parent: Some(it.id.clone()), ..RibbonRegion::new(id.clone(), RegionKind::MenuEntry, ir) });
                        }
                    }
                }
                self.design_drop = Some(DesignDrop::Menu(r, menu));
            }
            Float::Panel { panel: p, owners, .. } => {
                let (w, h) = p.measure(c);
                let r = place(w, h);
                for ((row_i, cell), tr) in p.targets(r) {
                    match &p.rows[row_i] {
                        panel::Row::Entry { entry, .. } if !entry.starts_with("value:") => {
                            regions.push(RibbonRegion { parent: Some(it.id.clone()), ..RibbonRegion::new(entry.clone(), RegionKind::MenuEntry, tr) });
                        }
                        panel::Row::Grid { .. } => {
                            let owner = owners.get(row_i).cloned().flatten().unwrap_or_else(|| it.id.clone());
                            regions.push(RibbonRegion { parent: Some(it.id.clone()), ..RibbonRegion::new(owner, RegionKind::GalleryItem(cell), tr) });
                        }
                        _ => {}
                    }
                }
                self.design_drop = Some(DesignDrop::Panel(r, p));
            }
            _ => {}
        }
    }

    /// Paints the inline drop-down of design mode (after everything else of the view, so the
    /// page below the ribbon does not cover it).
    pub fn paint_design_overlay(&self, c: &dyn Canvas) {
        let col = Colors::resolve(c, &self.theme);
        let fonts = Fonts::of(c);
        match &self.design_drop {
            Some(DesignDrop::Menu(r, menu)) => {
                c.draw_shadow(r, 8.0, &SHADOW_MENU, SHADOW_GREY);
                menu.paint(c, *r, WidgetState::REST);
            }
            Some(DesignDrop::Panel(r, p)) => p.paint(c, *r, &fonts.item, col.accent),
            None => {}
        }
    }

    /// Whether design mode has an inline drop-down to paint, and where.
    pub fn design_overlay_rect(&self) -> Option<Rect> {
        match &self.design_drop {
            Some(DesignDrop::Menu(r, _)) | Some(DesignDrop::Panel(r, _)) => Some(*r),
            None => None,
        }
    }
}

/// Where a KeyTip of a target goes.
type TipAnchor = Box<dyn Fn(Target) -> Option<(f32, f32)>>;

/// The strip's layout.
#[derive(Clone, Default)]
struct StripLayout {
    /// `(tab index, rect)` per visible tab.
    tabs:     Vec<(usize, Rect)>,
    actions:  Vec<Rect>,
    collapse: Option<Rect>,
    /// `(group id, header, rect, accent)` per contextual tab group with a header.
    headers:  Vec<(String, String, Rect, D2D1_COLOR_F)>,
    add_tab:  Option<Rect>,
}

fn dashed_rect(c: &dyn Canvas, r: Rect, color: &D2D1_COLOR_F) {
    let (dash, gap) = (4.0, 3.0);
    let mut x = r.left;
    while x < r.right {
        let e = (x + dash).min(r.right);
        c.fill_rounded(&Rect::new(x, r.top, e, r.top + 1.0), 0.0, color);
        c.fill_rounded(&Rect::new(x, r.bottom - 1.0, e, r.bottom), 0.0, color);
        x += dash + gap;
    }
    let mut y = r.top;
    while y < r.bottom {
        let e = (y + dash).min(r.bottom);
        c.fill_rounded(&Rect::new(r.left, y, r.left + 1.0, e), 0.0, color);
        c.fill_rounded(&Rect::new(r.right - 1.0, y, r.right, e), 0.0, color);
        y += dash + gap;
    }
}

fn paint_add_glyph(c: &dyn Canvas, col: &Colors, r: Rect, on_strip: bool) {
    let (bg, ink) = if on_strip && col.colored { (alpha(WHITE, 0.22), WHITE) } else { (alpha(col.accent, 0.14), col.accent) };
    c.fill_rounded(&r, 3.0, &bg);
    c.vector_icon("Plus", &r, m::ADD_GLYPH - 4.0, &ink);
}

fn inflate(r: Rect, by: f32) -> Rect {
    Rect::new(r.left - by, r.top - by, r.right + by, r.bottom + by)
}

fn hit_group(gb: &GroupBox, groups: &[RibbonGroup], x: f32, y: f32) -> Option<Target> {
    if gb.launcher.is_some_and(|l| l.contains(x, y)) {
        return Some(Target::Launcher(gb.index));
    }
    for p in &gb.items {
        if p.container {
            continue;
        }
        if let Some(i) = p.parts.iter().position(|r| r.contains(x, y)) {
            return Some(Target::Part(p.key, i as u8));
        }
        if let Some(i) = p.options.iter().position(|r| r.contains(x, y)) {
            return Some(Target::Option(p.key, i));
        }
        if p.chevron.is_some_and(|r| r.contains(x, y)) {
            return Some(Target::Chevron(p.key));
        }
        if p.rect.contains(x, y) && p.options.is_empty() && !item_of(groups, p.key).is_some_and(|it| it.is_grid_gallery()) {
            return Some(Target::Item(p.key));
        }
    }
    None
}

fn hit_row(row: &RowLayout, groups: &[RibbonGroup], x: f32, y: f32) -> Option<Target> {
    if row.overflow.as_ref().is_some_and(|(r, _)| r.contains(x, y)) {
        return Some(Target::Overflow);
    }
    row.groups
        .iter()
        .find_map(|g| hit_group(g, groups, x, y))
        .or_else(|| row.chips.iter().find(|ch| ch.button.contains(x, y)).map(|ch| Target::Chip(ch.index)))
}

/// The menu rows of `items`: separators, section labels, check boxes, entries with their
/// sub-menus. Returns the rows and each row's entry id with its sub-menu's ids.
fn menu_rows(items: &[RibbonItem]) -> (Vec<kubuno_desktop_controls::toolstrip::StripItem>, Vec<(String, Vec<String>)>) {
    let mut rows = Vec::new();
    let mut ids = Vec::new();
    for e in items.iter().filter(|e| e.visible) {
        if e.kind == ItemKind::Separator {
            rows.push(lists::separator());
            ids.push((String::new(), Vec::new()));
            continue;
        }
        if e.kind == ItemKind::Label {
            rows.push(lists::section(e.label.clone().unwrap_or_default()));
            ids.push((String::new(), Vec::new()));
            continue;
        }
        let mut entry = MenuEntry::new(e.label.clone().unwrap_or_else(|| e.id.clone())).checked(e.active).enabled(!e.disabled);
        if let Some(icon) = &e.icon {
            entry = entry.icon(icon.to_string());
        }
        let mut sub_ids = Vec::new();
        if !e.split_items.is_empty() {
            let (sub_rows, subs) = menu_rows(&e.split_items);
            entry = entry.submenu(sub_rows);
            sub_ids = subs.into_iter().map(|(id, _)| id).collect();
        }
        rows.push(entry.build());
        ids.push((e.id.clone(), sub_ids));
    }
    (rows, ids)
}

fn open_menu(it: &RibbonItem, key: ItemKey, anchor: Rect) -> Float {
    let (rows, ids) = menu_rows(&it.split_items);
    Float::Menu { owner: it.id.clone(), key: Some(Target::Item(key)), anchor, menu: Box::new(Menu::with_items(rows)), ids, overflow: false }
}

fn open_list(it: &RibbonItem, key: ItemKey, anchor: Rect) -> Float {
    let mut d = Dropdown::new();
    for o in &it.options {
        d.add_option(o.label.clone(), o.icon.as_deref());
    }
    if let Some(i) = it.value.as_deref().and_then(|v| it.options.iter().position(|o| o.value == v)) {
        d.commit(i);
    }
    d.open_with(d.selected());
    let values = it.options.iter().map(|o| o.value.clone()).collect();
    Float::List { owner: it.id.clone(), key, anchor, list: Box::new(d), values }
}

/// The panel rows of menu entries (`split_items`).
fn entry_rows(items: &[RibbonItem], rows: &mut Vec<panel::Row>, owners: &mut Vec<Option<String>>) {
    for e in items.iter().filter(|e| e.visible) {
        match e.kind {
            ItemKind::Separator => {
                rows.push(panel::Row::Separator);
                owners.push(None);
            }
            ItemKind::Label => {
                rows.push(panel::Row::Header(e.label.clone().unwrap_or_default()));
                owners.push(None);
            }
            ItemKind::Gallery => {
                let spec = e.gallery.clone().unwrap_or_default();
                if let Some(l) = e.label.as_deref().filter(|l| !l.is_empty()) {
                    rows.push(panel::Row::Header(l.to_string()));
                    owners.push(None);
                }
                for (header, cells) in gallery_sections(e) {
                    if let Some(h) = header {
                        rows.push(panel::Row::Header(h));
                        owners.push(None);
                    }
                    rows.push(panel::Row::Grid { cells, columns: spec.columns.max(1), cell_w: spec.item_w, cell_h: spec.item_h, swatch: false });
                    owners.push(Some(e.id.clone()));
                }
            }
            _ => {
                rows.push(panel::Row::Entry { entry: e.id.clone(), label: e.label.clone().unwrap_or_else(|| e.id.clone()), icon: e.icon.clone(), checked: e.active, enabled: !e.disabled });
                owners.push(None);
            }
        }
    }
}

/// A gallery's cells, by category (the uncategorised first, without a header).
fn gallery_sections(it: &RibbonItem) -> Vec<(Option<String>, Vec<panel::Cell>)> {
    let mut out: Vec<(Option<String>, Vec<panel::Cell>)> = Vec::new();
    for o in &it.options {
        let cell = panel::Cell { value: o.value.clone(), label: o.label.clone(), icon: o.icon.clone(), color: o.color, selected: it.value.as_deref() == Some(o.value.as_str()) };
        match out.iter_mut().find(|(c, _)| *c == o.category) {
            Some((_, cells)) => cells.push(cell),
            None => out.push((o.category.clone(), vec![cell])),
        }
    }
    out.sort_by_key(|(c, _)| c.is_some());
    out
}

fn gallery_panel(it: &RibbonItem, key: Target, anchor: Rect, _texts: &RibbonTexts) -> Float {
    let spec = it.gallery.clone().unwrap_or_default();
    let mut rows = Vec::new();
    let mut owners = Vec::new();
    for (header, cells) in gallery_sections(it) {
        if let Some(h) = header {
            rows.push(panel::Row::Header(h));
            owners.push(None);
        }
        rows.push(panel::Row::Grid { cells, columns: spec.columns.max(1), cell_w: spec.item_w, cell_h: spec.item_h, swatch: false });
        owners.push(None);
    }
    if !it.split_items.is_empty() {
        rows.push(panel::Row::Separator);
        owners.push(None);
        entry_rows(&it.split_items, &mut rows, &mut owners);
    }
    Float::Panel { owner: it.id.clone(), key: Some(key), anchor, panel: Box::new(panel::Panel::new(rows)), owners }
}

fn color_panel(it: &RibbonItem, key: Target, anchor: Rect, texts: &RibbonTexts) -> Float {
    let palette: Vec<(String, D2D1_COLOR_F)> = if it.options.is_empty() {
        crate::color::docs_swatches().into_iter().map(|c| {
            let d = c.to_d2d();
            (color_hex(d), d)
        }).collect()
    } else {
        it.options.iter().map(|o| (o.value.clone(), o.color.or_else(|| crate::color::parse(&o.value).map(|c| c.to_d2d())).unwrap_or(WHITE))).collect()
    };
    let cells = palette
        .into_iter()
        .map(|(v, d)| panel::Cell { label: v.clone(), selected: it.color.is_some_and(|c| same_color(c, d)), value: v, icon: None, color: Some(d) })
        .collect();
    let mut rows = vec![
        panel::Row::Entry { entry: "value:".into(), label: texts.automatic.clone(), icon: None, checked: it.color.is_none(), enabled: true },
        panel::Row::Header(texts.theme_colors.clone()),
        panel::Row::Grid { cells, columns: 10, cell_w: m::SWATCH, cell_h: m::SWATCH, swatch: true },
    ];
    let mut owners = vec![None, None, None];
    if !it.split_items.is_empty() {
        rows.push(panel::Row::Separator);
        owners.push(None);
        entry_rows(&it.split_items, &mut rows, &mut owners);
    }
    Float::Panel { owner: it.id.clone(), key: Some(key), anchor, panel: Box::new(panel::Panel::new(rows)), owners }
}

fn menu_panel(it: &RibbonItem, key: Target, anchor: Rect) -> Float {
    let mut rows = Vec::new();
    let mut owners = Vec::new();
    entry_rows(&it.split_items, &mut rows, &mut owners);
    Float::Panel { owner: it.id.clone(), key: Some(key), anchor, panel: Box::new(panel::Panel::new(rows)), owners }
}

// ═════════════════════════════════════════════════════════════════════════════
// Backstage — `Backstage.tsx`
// ═════════════════════════════════════════════════════════════════════════════

/// `BackstageSection`.
#[derive(Debug, Clone)]
pub struct BackstageSection {
    pub id:        String,
    pub label:     String,
    pub icon:      Icon,
    /// A « view » section shows a panel on the right; otherwise the section is
    /// an immediate action (Imprimer, Fermer…).
    pub view:      bool,
    pub disabled:  bool,
    /// A rule above this section in the rail.
    pub separated: bool,
}

impl BackstageSection {
    pub fn view(id: impl Into<String>, label: impl Into<String>, icon: impl Into<Icon>) -> Self {
        Self { id: id.into(), label: label.into(), icon: icon.into(), view: true, disabled: false, separated: false }
    }

    pub fn action(id: impl Into<String>, label: impl Into<String>, icon: impl Into<Icon>) -> Self {
        Self { view: false, ..Self::view(id, label, icon) }
    }

    pub fn separated(mut self) -> Self {
        self.separated = true;
        self
    }
}

/// What one [`Backstage::frame`] did.
#[derive(Clone, Default)]
pub struct BackstageRun {
    /// An action section that was clicked.
    pub action:  Option<String>,
    /// Escape was pressed (and the Backstage is not locked): go back.
    pub back:    bool,
    /// The panel on the right, for the active view section.
    pub content: Rect,
    /// The active view section.
    pub active:  String,
    /// Every section's row: (id, rect) (the designer's regions).
    pub rows:    Vec<(String, Rect)>,
}

/// The « Fichier » view: an accent rail of sections on the left, the active
/// section's panel on the right (painted by the caller).
pub struct Backstage {
    pub sections: Vec<BackstageSection>,
    /// No file open: Escape does not go back.
    pub locked:   bool,
    /// Design mode: no input (the designer selects the sections).
    pub design:   bool,
    active:       String,
    prev_down:    bool,
    pressed:      Option<usize>,
}

impl Backstage {
    pub fn new(sections: Vec<BackstageSection>) -> Self {
        let active = sections.iter().find(|s| s.view).map(|s| s.id.clone()).unwrap_or_default();
        Self { sections, locked: false, design: false, active, prev_down: false, pressed: None }
    }

    /// The active view section.
    pub fn active(&self) -> &str {
        &self.active
    }

    /// Shows the view section `id` (the designer selecting a `BackstageTab`).
    pub fn set_active(&mut self, id: &str) {
        if self.sections.iter().any(|s| s.id == id && s.view) {
            self.active = id.to_string();
        }
    }

    /// The rows of the rail, with the rule above a `separated` one.
    fn rows(&self, rail: Rect) -> Vec<(Rect, Option<Rect>)> {
        let mut y = rail.top + m::BACKSTAGE_TOP;
        self.sections
            .iter()
            .map(|s| {
                let rule = s.separated.then(|| {
                    let r = Rect::new(rail.left + m::BACKSTAGE_PAD_X, y + 6.0, rail.right - m::BACKSTAGE_PAD_X, y + 7.0);
                    y += 13.0;
                    r
                });
                let row = Rect::new(rail.left, y, rail.right, y + m::BACKSTAGE_ROW_H);
                y += m::BACKSTAGE_ROW_H;
                (row, rule)
            })
            .collect()
    }

    pub fn frame(&mut self, c: &dyn Canvas, rect: Rect, f: &host::Frame, theme: &RibbonTheme) -> BackstageRun {
        let mut run = BackstageRun::default();
        if !self.locked && !self.design && host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 {
            run.back = true;
        }
        let fonts = Fonts::of(c);
        let rail = Rect::new(rect.left, rect.top, (rect.left + m::BACKSTAGE_RAIL_W).min(rect.right), rect.bottom);
        let rows = self.rows(rail);
        let (mx, my) = if self.design { (host::POINTER_AWAY, host::POINTER_AWAY) } else { f.mouse };
        let mouse_down = f.mouse_down && !self.design;
        let hot = rows.iter().position(|(r, _)| r.contains(mx, my)).filter(|&i| !self.sections[i].disabled);
        let pressed = mouse_down && !self.prev_down;
        let released = !mouse_down && self.prev_down;
        self.prev_down = mouse_down;
        if pressed {
            self.pressed = hot;
        }
        if released {
            if let (Some(p), Some(h)) = (self.pressed, hot) {
                if p == h {
                    let s = &self.sections[h];
                    if s.view {
                        self.active = s.id.clone();
                    } else {
                        run.action = Some(s.id.clone());
                    }
                }
            }
            self.pressed = None;
        }

        let t = c.theme();
        c.fill_rounded(&rect, 0.0, &t.layer_background);
        // The rail takes the « Fichier » tab's colour (its dark-theme shade included).
        c.fill_rounded(&rail, 0.0, &Colors::resolve(c, theme).file_bg);
        for (i, (row, rule)) in rows.iter().enumerate() {
            let s = &self.sections[i];
            if let Some(rule) = rule {
                c.fill_rounded(rule, 0.0, &alpha(WHITE, 0.2));
            }
            let on = s.view && s.id == self.active;
            if on {
                c.fill_rounded(row, 0.0, &alpha(WHITE, 0.18));
            } else if hot == Some(i) {
                c.fill_rounded(row, 0.0, &alpha(WHITE, 0.1));
            }
            let ink = if s.disabled { alpha(WHITE, 0.4) } else { WHITE };
            let icon = Rect::new(row.left + m::BACKSTAGE_PAD_X, row.top, row.left + m::BACKSTAGE_PAD_X + m::BACKSTAGE_ICON_SLOT, row.bottom);
            c.vector_icon(icon_str(&s.icon), &icon, m::BACKSTAGE_ICON, &ink);
            let text = Rect::new(icon.right + m::BACKSTAGE_GAP, row.top, row.right - m::BACKSTAGE_PAD_X, row.bottom);
            c.text_ellipsis(&s.label, &text, if on { &fonts.section_b } else { &fonts.section }, &ink);
        }
        run.content = Rect::new(rail.right, rect.top, rect.right, rect.bottom);
        run.active = self.active.clone();
        run.rows = rows.iter().zip(&self.sections).map(|((r, _), s)| (s.id.clone(), *r)).collect();
        run
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contextual_header_stays_readable_on_a_like_strip() {
        let green = hex(0x107c41);
        // On the blue Documents strip: the accent itself, white text.
        let (fill, ink) = header_colors(green, tone::DOCUMENTS);
        assert_eq!((fill.g, ink.r), (green.g, 1.0));
        // On the green Spreadsheet strip: a pale tint, readable ink, distinct from the strip.
        let (fill, ink) = header_colors(green, tone::SPREADSHEET);
        assert!(contrast_ratio(fill, tone::SPREADSHEET) >= 3.0);
        assert!(contrast_ratio(ink, fill) >= 4.5);
    }

    fn small(id: &str) -> RibbonItem {
        RibbonItem::button(id, id, "Copy")
    }

    #[test]
    fn one_slot_items_stack_three_to_a_column() {
        let items = vec![small("a"), small("b"), small("c"), small("d"), small("e")];
        assert_eq!(to_columns(&items), vec![vec![0, 1, 2], vec![3, 4]]);
    }

    #[test]
    fn a_separator_never_breaks_a_stack() {
        let items = vec![small("a"), RibbonItem::separator("s"), small("b"), small("c")];
        assert_eq!(to_columns(&items), vec![vec![0, 2, 3]]);
    }

    #[test]
    fn wide_items_take_a_column_of_their_own() {
        let items = vec![
            RibbonItem::button("paste", "Coller", "ClipboardPaste").large(),
            small("cut"),
            small("copy"),
            RibbonItem::dropdown("font", vec![RibbonOption::new("a", "A")], "a", 120.0),
            small("bold"),
        ];
        assert_eq!(to_columns(&items), vec![vec![0], vec![1, 2], vec![3], vec![4]]);
    }

    #[test]
    fn the_tooltip_joins_label_and_shortcut() {
        let it = RibbonItem::button("copy", "Copier", "Copy").shortcut("Ctrl+C");
        assert_eq!(it.tip_text().as_deref(), Some("Copier · Ctrl+C"));
        let it = RibbonItem::button("copy", "Copier", "Copy").tooltip("Copier la sélection");
        assert_eq!(it.tip_text().as_deref(), Some("Copier la sélection"));
    }

    #[test]
    fn the_file_accent_is_the_lighter_tone() {
        let doc = file_accent_for(tone::DOCUMENTS);
        let want = hex(0x3f7dd0);
        assert!((doc.r - want.r).abs() < 1e-6 && (doc.b - want.b).abs() < 1e-6, "Documents keeps its own shade");
        // `lighten('#0f7b3f', 0.3)` = #57a379.
        let sheet = file_accent_for(tone::SPREADSHEET);
        let want = hex(0x57a379);
        assert!((sheet.r - want.r).abs() < 1e-6 && (sheet.g - want.g).abs() < 1e-6 && (sheet.b - want.b).abs() < 1e-6);
    }

    /// The designer's surface paints only when something happens: an idle design frame asks for no
    /// other frame and draws exactly what the previous one drew (a flicker regression).
    #[test]
    fn an_idle_design_frame_is_stable_and_asks_for_no_repaint() {
        use crate::graphics::testing::RecordingCanvas;
        let tabs = vec![
            RibbonTab::file("file", "Fichier"),
            RibbonTab::new("home", "Accueil", vec![RibbonGroup::new("clip", "Presse-papiers", vec![RibbonItem::button("paste", "Coller", "ClipboardPaste").large(), RibbonItem::button("cut", "Couper", "Scissors")])]),
            RibbonTab::new("pic", "Image", vec![]).contextual(hex(0xb7472a), false),
        ];
        let mut r = Ribbon::new(tabs, RibbonTheme::default());
        r.design = Some(RibbonDesign { active_tab: Some("home".into()), add_glyphs: true, ..RibbonDesign::default() });
        let frame = host::Frame {
            size: (900.0, 400.0),
            mouse: (300.0, 60.0),
            mouse_down: false,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 900.0, 400.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: 0,
            window_focused: true,
        };
        let bounds = Rect::new(0.0, 0.0, 900.0, 200.0);
        let canvas = RecordingCanvas::new();
        let first = r.frame(&canvas, bounds, &frame);
        let drawn = canvas.calls();
        canvas.clear();
        let second = r.frame(&canvas, bounds, &frame);
        assert_eq!(host::repaint_requested(), None, "an idle design frame asks for no repaint");
        assert_eq!(first.height, second.height);
        assert_eq!(first.regions, second.regions);
        assert_eq!(drawn, canvas.calls(), "the same picture twice");
    }

    #[test]
    fn a_new_contextual_tab_takes_the_focus_and_its_loss_falls_back() {
        let tabs = vec![
            RibbonTab::file("file", "Fichier"),
            RibbonTab::new("home", "Accueil", vec![]),
            RibbonTab::new("pic", "Image", vec![]).contextual(hex(0xb7472a), false),
        ];
        let mut r = Ribbon::new(tabs, RibbonTheme::default());
        assert_eq!(r.active(), "home", "starts on the first normal tab, never on Fichier");
        let mut ev = Vec::new();
        r.tabs[2].visible = true;
        r.follow_contextual(&mut ev);
        assert_eq!(r.active(), "pic");
        assert_eq!(ev, vec![RibbonEvent::TabChanged("pic".into())]);
        ev.clear();
        r.tabs[2].visible = false;
        r.follow_contextual(&mut ev);
        assert_eq!(r.active(), "home", "falls back to a normal tab, not the Backstage");
    }
}
