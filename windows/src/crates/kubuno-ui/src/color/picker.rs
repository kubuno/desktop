//! [`ColorPicker`] — a port of `core/frontend/src/ui/ColorPicker.tsx`, the
//! Coolorus-inspired colour panel every Kubuno module uses.
//!
//! The geometry is the web's own, read off the live component's DOM (a
//! 312 DIP panel, 1 DIP border, `p-3`):
//!
//! ```text
//!  13  header — « Couleur » (10 px) · ✕                        16.5 high, mb-2
//!  37.5 [shape column 32] 6 [hue ring + SV area 210×212] 6 [harmony column 32]
//! 259.5 harmony swatches, h-6, gap-1                             mt-2.5
//! 291.5 preview 28×24 · « # » · hex input h-6                    mt-2
//! 325.5 RGB · HSV · HSL · CMYK · GRAY tabs, border-bottom        mt-2.5, mb-1.5
//! 353.5 channel rows (label w-3 · track h-3 · input w-11 h-5)    space-y-1.5
//!       12 fixed chips 16×16, gap-1                              mt-2.5
//!       « Récemment utilisées » + 10-column grid (if any)        mt-3 pt-2 border-t
//!       Annuler / Ajouter (if asked)                              mt-3 pt-2.5 border-t
//! ```
//!
//! Note the centre column: the row is 286 DIP wide and its two button
//! columns and gaps take 76, so the flex item the web sizes at 212 shrinks
//! to 210 — the ring is a 210 × 212 ELLIPSE, and the hue handle is still
//! placed from `SIZE / 2 = 106`. Both quirks are reproduced, since they are
//! what the web shows.
//!
//! The ring and the saturation/value area are per-pixel rasters
//! ([`super::raster`]), like the web's canvas; the channel tracks are
//! Direct2D linear gradients with the web's own stops. Without a Direct2D
//! device (a headless canvas) both fall back to one-DIP strips.

use std::ops::{Deref, DerefMut};

use drive_app_controls::{Canvas, Rect};
use kubuno_controls::containers::Panel as PanelModel;
use kubuno_controls::control::FontRole;
use kubuno_controls::enums::Size as ControlSize;
use kubuno_controls::host::{self, vk, Cursor, Modifiers};
use kubuno_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use super::raster::{self, Raster};
use super::{
    chip_focus, cmyk_to_rgb, fade, hsl_to_rgb, hsv_to_rgb, js_round, paint_chip, parse_web_hex, picker_swatches,
    ramp_x, rgb_to_cmyk, rgb_to_hsl, rgb_to_hsv, take_color_keys, Color, ColorKey, DraftKind, DraftOutcome,
    FieldDraft, Hsv, Rgb, SelectMark, SwatchGrid, DISABLED_ALPHA,
};
use crate::datetime::SHADOW_2XL;
use crate::graphics::{
    Brush, Font, Graphics, LinearGradientBrush, Pen, PointF, StringAlignment, StringFormat,
};
use drive_app_controls::themes::shape::SHADOW_BLACK;
use crate::widget::{Widget, WidgetState};

/// Every number the picker is laid out with — measured on the web component.
pub mod pm {
    /// `border: 1px` + `p-3`: where the content starts inside the bounds.
    pub const INSET: f32 = 13.0;
    /// `width: 312`, `borderRadius: 4`.
    pub const WIDTH: f32 = 312.0;
    pub const RADIUS: f32 = 4.0;
    /// The header row is the ✕ button's line box (11 px × 1.5), then `mb-2`.
    pub const HEADER_H: f32 = 16.5;
    pub const HEADER_GAP: f32 = 8.0;
    /// The ✕ button: `text-[11px] px-1` around its glyph.
    pub const CLOSE_W: f32 = 17.0;
    /// `const SIZE = 212, RING = 22`.
    pub const SIZE: f32 = 212.0;
    pub const RING: f32 = 22.0;
    /// The two button columns: `w-8 h-8` circles, `gap-1`; `gap-1.5` between
    /// the columns and the ring.
    pub const BTN: f32 = 32.0;
    pub const BTN_GAP: f32 = 4.0;
    pub const COL_GAP: f32 = 6.0;
    /// Glyphs: the shape icons `size={15}`, the pipette `size={14}`, the
    /// harmony glyphs `size={20}`.
    pub const SHAPE_ICON: f32 = 15.0;
    pub const PIPETTE_ICON: f32 = 14.0;
    pub const HARMONY_ICON: f32 = 20.0;
    /// The hue handle (`14 × 14`, `2px solid #fff`, `0 0 0 1px rgba(0,0,0,.6)`),
    /// the harmony markers (`10 × 10`, `2px solid rgba(255,255,255,.85)`) and
    /// the SV handle (`11 × 11`, `0 0 0 1px rgba(0,0,0,.5)`).
    pub const HUE_HANDLE: f32 = 14.0;
    pub const MARKER: f32 = 10.0;
    pub const SV_HANDLE: f32 = 11.0;
    /// `borderRadius: 2` on the square SV canvas.
    pub const SV_RADIUS: f32 = 2.0;
    /// The harmony swatches: `mt-2.5`, `h-6`, `gap-1`, `borderRadius: 3`.
    pub const HARM_TOP: f32 = 10.0;
    pub const HARM_H: f32 = 24.0;
    pub const HARM_RADIUS: f32 = 3.0;
    /// Preview + hex: `mt-2`, a 28 × 24 chip, `gap-2`, the `#` (its measured
    /// 10 px glyph width), `gap-2`, the `h-6` input with `px-2`.
    pub const HEX_TOP: f32 = 8.0;
    pub const PREVIEW_W: f32 = 28.0;
    pub const ROW_H: f32 = 24.0;
    pub const GAP: f32 = 8.0;
    pub const HASH_W: f32 = 8.33;
    pub const HEX_PAD: f32 = 8.0;
    pub const BOX_RADIUS: f32 = 2.0;
    /// The model tabs: `mt-2.5`, `px-1.5 py-0.5` buttons 21 high with a 2 DIP
    /// bottom border, the row's own 1 DIP rule, then `mb-1.5`.
    pub const TABS_TOP: f32 = 10.0;
    pub const TAB_H: f32 = 21.0;
    pub const TAB_UNDERLINE: f32 = 2.0;
    pub const TABS_BOTTOM: f32 = 6.0;
    /// The tabs' widths as the web lays them out (their 10 px label plus
    /// `px-1.5`). Fixed so hit-testing needs no text engine.
    pub const TAB_W: [f32; 5] = [33.58, 32.48, 31.17, 41.31, 39.03];
    /// `ColorChan`: rows `h-5` with `space-y-1.5`; label `w-3`; `gap-2`; track
    /// `h-3` (border 1, radius 2); thumb 3 wide overhanging 2; input `w-11 h-5`.
    pub const CHAN_H: f32 = 20.0;
    pub const CHAN_GAP_Y: f32 = 6.0;
    pub const CHAN_LABEL_W: f32 = 12.0;
    pub const TRACK_H: f32 = 12.0;
    pub const THUMB_W: f32 = 3.0;
    pub const THUMB_OVER: f32 = 2.0;
    pub const CHAN_BOX_W: f32 = 44.0;
    /// The fixed chips: `mt-2.5`, 16 × 16, `gap-1`, `borderRadius: 2`.
    pub const SWATCH_TOP: f32 = 10.0;
    pub const SWATCH: f32 = 16.0;
    pub const SWATCH_RADIUS: f32 = 2.0;
    pub const GRID_GAP: f32 = 4.0;
    /// Recent colours: `mt-3 pt-2 border-t`, the 10 px caption (15 high),
    /// `mb-1.5`, a 10-column grid of `aspect-square` cells, `borderRadius: 3`.
    pub const RECENT_TOP: f32 = 12.0;
    pub const RECENT_PAD: f32 = 8.0;
    pub const LABEL_H: f32 = 15.0;
    pub const LABEL_GAP: f32 = 6.0;
    pub const RECENT_COLS: usize = 10;
    pub const RECENT_RADIUS: f32 = 3.0;
    pub const RECENT_MAX: usize = 30;
    /// The footer: `mt-3 pt-2.5 border-t`, `gap-2`, buttons `px-3 h-7`.
    pub const FOOTER_TOP: f32 = 12.0;
    pub const FOOTER_PAD: f32 = 10.0;
    pub const FOOTER_BTN_H: f32 = 28.0;
    pub const FOOTER_BTN_PAD: f32 = 12.0;
    /// Type sizes: 10 px (title, labels, tabs, numeric inputs), 11 px (✕,
    /// hex input, footer buttons).
    pub const TEXT_SM: f32 = 10.0;
    pub const TEXT_MD: f32 = 11.0;
    /// `focus-visible:ring-2`.
    pub const FOCUS_RING: f32 = 2.0;
    /// `hover:scale-110` on a recent colour.
    pub const HOVER_GROW: f32 = 0.05;
    /// Keyboard steps: `SvArea` 0.02 / 0.1, `ColorChan` and the ring 1 / 10.
    pub const SV_STEP: f64 = 0.02;
    pub const SV_STEP_BIG: f64 = 0.1;
    pub const STEP: f64 = 1.0;
    pub const STEP_BIG: f64 = 10.0;
}

// ═════════════════════════════════════════════════════════════════════════════
// Model enums
// ═════════════════════════════════════════════════════════════════════════════

/// `SvShape`: the saturation/value area's outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SvShape {
    #[default]
    Square = 0,
    Triangle = 1,
    Circle = 2,
}

impl SvShape {
    pub const ALL: [SvShape; 3] = [SvShape::Square, SvShape::Triangle, SvShape::Circle];

    /// The lucide glyph of its button, and its `title`.
    fn icon(self) -> &'static str {
        match self {
            Self::Square => "Square",
            Self::Triangle => "Triangle",
            Self::Circle => "Circle",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Triangle => "triangle",
            Self::Circle => "circle",
        }
    }
}

/// `Scheme`: the harmony derived from the base colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Scheme {
    #[default]
    Comp,
    Analog,
    Triad,
    Tetrad,
    Split,
    Mono,
}

impl Scheme {
    pub const ALL: [Scheme; 6] = [Scheme::Comp, Scheme::Analog, Scheme::Triad, Scheme::Tetrad, Scheme::Split, Scheme::Mono];

    /// The French fallback label (`FALLBACK_LABELS`), shown as its tooltip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Comp => "Complémentaire",
            Self::Analog => "Analogues",
            Self::Triad => "Triade",
            Self::Tetrad => "Tétrade",
            Self::Split => "Complémentaires divisées",
            Self::Mono => "Monochrome",
        }
    }

    /// `HARMONY_ANGLES` — the relative hue angles that define the scheme.
    pub fn angles(self) -> &'static [f64] {
        match self {
            Self::Comp => &[0.0, 180.0],
            Self::Analog => &[-30.0, 0.0, 30.0],
            Self::Triad => &[0.0, 120.0, 240.0],
            Self::Tetrad => &[0.0, 90.0, 180.0, 270.0],
            Self::Split => &[0.0, 150.0, 210.0],
            Self::Mono => &[],
        }
    }
}

/// `harmonyColors(scheme, h, s, v)`, term for term.
pub fn harmony_colors(scheme: Scheme, h: f64, s: f64, v: f64) -> Vec<Hsv> {
    let at = |dh: f64| Hsv { h: (h + dh + 360.0) % 360.0, s, v };
    match scheme {
        Scheme::Mono => vec![
            Hsv { h, s, v: (v * 0.45).max(0.2) },
            Hsv { h, s, v },
            Hsv { h, s: (s * 0.45).max(0.12), v: (v + 0.15).min(1.0) },
        ],
        other => other.angles().iter().map(|&a| at(a)).collect(),
    }
}

/// The colour model the channel sliders show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ColorMode {
    #[default]
    Rgb,
    Hsv,
    Hsl,
    Cmyk,
    Gray,
}

impl ColorMode {
    pub const ALL: [ColorMode; 5] = [ColorMode::Rgb, ColorMode::Hsv, ColorMode::Hsl, ColorMode::Cmyk, ColorMode::Gray];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Hsv => "HSV",
            Self::Hsl => "HSL",
            Self::Cmyk => "CMYK",
            Self::Gray => "GRAY",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }
}

/// `PickerTool`: an extra circular button a module appends to the left
/// column (PaintSharp's Apex adds « no fill » this way).
#[derive(Debug, Clone, PartialEq)]
pub struct PickerTool {
    pub id: String,
    /// A lucide glyph name from the shared icon set.
    pub icon: &'static str,
    pub title: String,
    pub active: bool,
}

/// One channel slider of the active model — a `ColorChan`.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub label: &'static str,
    pub value: f64,
    pub max: f64,
    /// The CSS `linear-gradient(to right, …)` stops, evenly spaced.
    pub track: Vec<Rgb>,
}

/// The part of a picker a point, a Tab or a key lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PickerPart {
    Close,
    Shape(SvShape),
    Eyedropper,
    Tool(usize),
    /// The hue ring.
    Ring,
    /// The saturation/value area.
    Area,
    Scheme(Scheme),
    /// One of the harmony swatches under the ring.
    Harmony(usize),
    Hex,
    Mode(ColorMode),
    /// A channel slider's track, and its numeric box.
    Channel(usize),
    ChannelBox(usize),
    /// One of the twelve fixed chips.
    Swatch(usize),
    /// One of the recently used colours.
    Recent(usize),
    Cancel,
    Confirm,
}

impl PickerPart {
    /// A stable slot per TAB STOP, for a focus id. Every fixed chip shares
    /// one (the row is one stop with a roving cursor), and so does every
    /// recent colour.
    pub fn focus_slot(self) -> usize {
        match self {
            Self::Close => 0,
            Self::Shape(s) => 1 + s as usize,
            Self::Eyedropper => 4,
            Self::Tool(i) => 5 + i.min(7),
            Self::Ring => 13,
            Self::Area => 14,
            Self::Scheme(s) => 15 + Scheme::ALL.iter().position(|x| *x == s).unwrap_or(0),
            Self::Harmony(i) => 21 + i.min(3),
            Self::Hex => 25,
            Self::Mode(m) => 26 + m.index(),
            Self::Channel(i) => 31 + 2 * i.min(3),
            Self::ChannelBox(i) => 32 + 2 * i.min(3),
            Self::Swatch(_) => 39,
            Self::Recent(_) => 40,
            Self::Cancel => 41,
            Self::Confirm => 42,
        }
    }

    /// Whether the part is a text box a keyboard types into.
    pub fn is_text(self) -> bool {
        matches!(self, Self::Hex | Self::ChannelBox(_))
    }

    /// Whether it is a `<button>` Enter / Space activates.
    pub fn is_button(self) -> bool {
        matches!(
            self,
            Self::Close
                | Self::Shape(_)
                | Self::Eyedropper
                | Self::Tool(_)
                | Self::Scheme(_)
                | Self::Harmony(_)
                | Self::Mode(_)
                | Self::Swatch(_)
                | Self::Recent(_)
                | Self::Cancel
                | Self::Confirm
        )
    }
}

/// What a frame of interaction asks the picker's owner to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickerEvent {
    #[default]
    None,
    /// The ✕ (`onClose`).
    Close,
    /// The footer's two buttons (`onCancel` / `onConfirm`).
    Cancel,
    Confirm,
    /// A module tool button was clicked (`PickerTool.onClick`).
    Tool(usize),
    /// A recent colour was picked (`onPickHistory`).
    History(usize),
}

/// The pointer as a host hands it to [`ColorPicker::pointer`], in the same
/// space as the panel's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PickerPointer {
    pub x: f32,
    pub y: f32,
    /// The primary button is held.
    pub down: bool,
    /// It went down this frame.
    pub pressed: bool,
    /// The pointer left every window: a drag holds still.
    pub away: bool,
}

/// The HSV triangle inscribed in the inner circle: white at the top, black
/// bottom-left, pure hue bottom-right (`SvArea`'s `tri`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub white: (f64, f64),
    pub black: (f64, f64),
    pub hue: (f64, f64),
}

/// `tri` for an area `size` DIP wide.
pub fn triangle(size: f32) -> Triangle {
    let size = size as f64;
    let r = size / 2.0 - 1.0;
    let (cx, cy) = (size / 2.0, size / 2.0);
    const SIN60: f64 = 0.8660254;
    Triangle { white: (cx, cy - r), black: (cx - r * SIN60, cy + r * 0.5), hue: (cx + r * SIN60, cy + r * 0.5) }
}

/// `bary(px, py, a, b, c)` — barycentric weights of a point.
pub fn bary(px: f64, py: f64, a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> (f64, f64, f64) {
    let d = (b.1 - c.1) * (a.0 - c.0) + (c.0 - b.0) * (a.1 - c.1);
    if d == 0.0 {
        return (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0);
    }
    let wa = ((b.1 - c.1) * (px - c.0) + (c.0 - b.0) * (py - c.1)) / d;
    let wb = ((c.1 - a.1) * (px - c.0) + (a.0 - c.0) * (py - c.1)) / d;
    (wa, wb, 1.0 - wa - wb)
}

/// The `(s, v)` a point `(px, py)` — relative to the area's top-left — names
/// in an area of side `size` and `shape`: `SvArea`'s `upd`, term for term.
pub fn sv_from_point(shape: SvShape, size: f32, px: f64, py: f64) -> (f64, f64) {
    let size_f = size as f64;
    match shape {
        SvShape::Triangle => {
            let t = triangle(size);
            let (mut ww, mut wh, mut wb) = bary(px, py, t.white, t.hue, t.black);
            ww = ww.max(0.0);
            wh = wh.max(0.0);
            wb = wb.max(0.0);
            let sum = ww + wh + wb;
            let sum = if sum == 0.0 { 1.0 } else { sum };
            ww /= sum;
            wh /= sum;
            wb /= sum;
            let v = 1.0 - wb;
            let s = if ww + wh > 0.0 { wh / (ww + wh) } else { 0.0 };
            (s, v)
        }
        SvShape::Circle | SvShape::Square => {
            let (mut x, mut y) = (px, py);
            if shape == SvShape::Circle {
                let (c, r) = (size_f / 2.0, size_f / 2.0);
                let (dx, dy) = (x - c, y - c);
                let d = dx.hypot(dy);
                if d > r {
                    x = c + dx * r / d;
                    y = c + dy * r / d;
                }
            }
            let side = size_f.max(1.0);
            ((x / side).clamp(0.0, 1.0), (1.0 - y / side).clamp(0.0, 1.0))
        }
    }
}

/// `handlePos()`: where the SV handle sits for `(s, v)`, relative to the
/// area's top-left.
pub fn sv_handle_pos(shape: SvShape, size: f32, s: f64, v: f64) -> (f64, f64) {
    let size_f = size as f64;
    if shape == SvShape::Triangle {
        let t = triangle(size);
        let (wblk, whue, ww) = (1.0 - v, s * v, (1.0 - s) * v);
        return (
            ww * t.white.0 + whue * t.hue.0 + wblk * t.black.0,
            ww * t.white.1 + whue * t.hue.1 + wblk * t.black.1,
        );
    }
    let (mut x, mut y) = (s * size_f, (1.0 - v) * size_f);
    if shape == SvShape::Circle {
        let (c, r) = (size_f / 2.0, size_f / 2.0);
        let (mut dx, mut dy) = (x - c, y - c);
        let d = dx.hypot(dy);
        if d > r {
            dx *= r / d;
            dy *= r / d;
            x = c + dx;
            y = c + dy;
        }
    }
    (x, y)
}

/// A hue from a point on the ring: `atan2(dx, -dy)` around the box's centre,
/// in `0..360`.
pub fn hue_from_point(wheel: Rect, x: f32, y: f32) -> f64 {
    let cx = (wheel.left + wheel.right) / 2.0;
    let cy = (wheel.top + wheel.bottom) / 2.0;
    let (dx, dy) = ((x - cx) as f64, (y - cy) as f64);
    (dx.atan2(-dy).to_degrees() + 360.0) % 360.0
}

// ═════════════════════════════════════════════════════════════════════════════
// Layout
// ═════════════════════════════════════════════════════════════════════════════

/// Where every part of an open [`ColorPicker`] is — one pure function, so
/// painting, hit-testing and measuring cannot disagree.
#[derive(Clone)]
pub struct PickerLayout {
    pub panel: Rect,
    pub title: Rect,
    pub close: Rect,
    pub shapes: [Rect; 3],
    pub eyedropper: Option<Rect>,
    pub tools: Vec<Rect>,
    /// The ring's box (210 × 212 at the default width).
    pub wheel: Rect,
    /// The saturation/value area (a square box for every shape).
    pub sv: Rect,
    pub schemes: [Rect; 6],
    pub harmony: Vec<Rect>,
    pub preview: Rect,
    pub hash: Rect,
    pub hex: Rect,
    pub tabs: [Rect; 5],
    /// The 1 DIP rule under the tabs.
    pub tab_rule: Rect,
    /// One row per channel of the active model.
    pub channels: Vec<Rect>,
    pub swatches: Rect,
    pub swatch_grid: SwatchGrid,
    /// The recent block: its rule, caption and grid (zero-height when empty).
    pub recent_rule: Rect,
    pub recent_label: Rect,
    pub recent: Rect,
    pub recent_grid: SwatchGrid,
    pub footer_rule: Rect,
    pub cancel: Rect,
    pub confirm: Rect,
}

impl PickerLayout {
    /// The `h-3` track of a channel row.
    pub fn track(row: Rect) -> Rect {
        let top = row.top + (pm::CHAN_H - pm::TRACK_H) / 2.0;
        let left = row.left + pm::CHAN_LABEL_W + pm::GAP;
        let right = (row.right - pm::CHAN_BOX_W - pm::GAP).max(left);
        Rect::new(left, top, right, top + pm::TRACK_H)
    }

    /// The `w-11 h-5` numeric box of a channel row.
    pub fn channel_box(row: Rect) -> Rect {
        Rect::new(row.right - pm::CHAN_BOX_W, row.top, row.right, row.top + pm::CHAN_H)
    }

    /// The `w-3` caption column of a channel row.
    pub fn channel_label(row: Rect) -> Rect {
        Rect::new(row.left, row.top, row.left + pm::CHAN_LABEL_W, row.top + pm::CHAN_H)
    }

    /// The thumb of a channel row at `value / max`: `left: calc(p% - 1.5px)`
    /// inside the track's padding box, `top/bottom: -2px`.
    pub fn thumb(row: Rect, value: f64, max: f64) -> Rect {
        let t = Self::track(row);
        let inner_w = (t.right - t.left - 2.0).max(0.0);
        let f = if max > 0.0 { (value / max).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let x = t.left + 1.0 + f * inner_w - pm::THUMB_W / 2.0;
        Rect::new(x, t.top + 1.0 - pm::THUMB_OVER, x + pm::THUMB_W, t.bottom - 1.0 + pm::THUMB_OVER)
    }

    /// The value a pointer at `x` gives a channel: `(x - rc.left) / rc.width`
    /// over the track's border box, times `max`.
    pub fn channel_value_at(row: Rect, x: f32, max: f64) -> f64 {
        let t = Self::track(row);
        let w = (t.right - t.left).max(1.0);
        (((x - t.left) / w).clamp(0.0, 1.0) as f64) * max
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// The picker
// ═════════════════════════════════════════════════════════════════════════════

/// The full colour panel — `ColorPicker.tsx`.
///
/// Its **state is the web's**: `[h, s, v]` plus the SV shape, the harmony
/// scheme and the channel model, with the hex derived from the HSV
/// (`rgbToHex(...hsvToRgb(h, s, v))`), which is why a fully desaturated
/// colour keeps the hue it was left on. The web's colour is always
/// `#rrggbb`; the desktop's [`Color`] carries an opacity beside it, which the
/// picker keeps untouched ([`ColorPicker::opacity`]) and never edits.
#[derive(Clone)]
pub struct ColorPicker {
    inner: PanelModel,
    pub hsv: Hsv,
    /// Carried through from the colour the picker was opened on (0..=100).
    pub opacity: f64,
    pub mode: ColorMode,
    pub shape: SvShape,
    pub scheme: Scheme,
    /// `history.slice(0, 30)`.
    pub recent: Vec<Color>,
    /// The twelve fixed chips.
    pub swatches: Vec<Color>,
    /// The header caption (`tr('layer_color_picker')`, « Couleur »).
    pub title: String,
    /// The caption over the recent colours.
    pub recent_label: String,
    /// Whether the screen eyedropper button is offered (the web shows it
    /// wherever `window.EyeDropper` exists; the desktop always can).
    pub eyedropper: bool,
    /// `leftTools`.
    pub tools: Vec<PickerTool>,
    /// `(cancel, confirm)` labels of the optional footer.
    pub footer: Option<(String, String)>,
    /// The part under the pointer.
    pub hot: Option<PickerPart>,
    /// Kept for hosts that set it directly; mirrors `hot` on a recent chip.
    pub hot_recent: Option<usize>,
    /// The part holding the keyboard focus, and whether its ring shows.
    pub focus: Option<PickerPart>,
    pub focus_visible: bool,
    /// The box being typed into, and what has been typed.
    pub edit: Option<(PickerPart, FieldDraft)>,
    pub caret_on: bool,
    /// The roving cursors of the two chip rows.
    pub swatch_cursor: usize,
    pub recent_cursor: usize,
    /// The part a press grabbed (ring, area, channel), until release.
    pub drag: Option<PickerPart>,
    /// The screen eyedropper the pipette button arms.
    pub eye: ScreenEyedropper,
}

/// The screen eyedropper — the web's `new EyeDropper().open()`: once armed,
/// the next click ANYWHERE on screen samples the pixel under the cursor;
/// Escape cancels. The click that samples is swallowed: a host asks
/// [`ScreenEyedropper::swallows_pointer`] before routing a press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenEyedropper {
    /// Armed; `true` once the arming click's own release was seen.
    picking: Option<bool>,
    /// A press consumed by the eyedropper, until the button is up.
    swallow: bool,
}

impl ScreenEyedropper {
    pub fn arm(&mut self) {
        self.picking = Some(false);
    }

    pub fn cancel(&mut self) {
        self.picking = None;
    }

    pub fn is_picking(&self) -> bool {
        self.picking.is_some()
    }

    /// Whether a host must ignore the pointer's presses this frame.
    pub fn swallows_pointer(&self) -> bool {
        self.picking.is_some() || self.swallow
    }

    /// One frame: keeps frames coming while armed, samples on the next
    /// press, cancels on Escape. Returns the sampled colour.
    pub fn poll(&mut self) -> Option<Rgb> {
        let lbutton = eyedropper::button_down();
        if self.swallow && !lbutton {
            self.swallow = false;
        }
        let seen_up = self.picking?;
        host::request_repaint_after(16);
        if host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 || eyedropper::escape_down() {
            self.picking = None;
            return None;
        }
        if !lbutton {
            self.picking = Some(true);
            return None;
        }
        if !seen_up {
            return None;
        }
        self.picking = None;
        self.swallow = true;
        eyedropper::sample_under_cursor()
    }
}

impl Default for ColorPicker {
    fn default() -> Self {
        Self::new(Color::default())
    }
}

impl ColorPicker {
    pub fn new(colour: Color) -> Self {
        Self {
            inner: PanelModel::new(),
            hsv: colour.to_hsv(),
            opacity: colour.opacity,
            mode: ColorMode::Rgb,
            shape: SvShape::Square,
            scheme: Scheme::Comp,
            recent: Vec::new(),
            swatches: picker_swatches(),
            title: "Couleur".to_string(),
            recent_label: "Récemment utilisées".to_string(),
            eyedropper: true,
            tools: Vec::new(),
            footer: None,
            hot: None,
            hot_recent: None,
            focus: None,
            focus_visible: false,
            edit: None,
            caret_on: false,
            swatch_cursor: 0,
            recent_cursor: 0,
            drag: None,
            eye: ScreenEyedropper::default(),
        }
    }

    /// With the web's Cancel / Add footer (`onCancel` / `onConfirm`).
    pub fn with_footer(mut self, cancel: &str, confirm: &str) -> Self {
        self.footer = Some((cancel.to_string(), confirm.to_string()));
        self
    }

    // ── Value ────────────────────────────────────────────────────────────────

    /// The colour the panel names, with the carried opacity.
    pub fn color(&self) -> Color {
        Color::new(hsv_to_rgb(self.hsv.h, self.hsv.s, self.hsv.v), self.opacity)
    }

    /// Adopts `colour` from outside (the `color` prop). Like the web's effect,
    /// the HSV state is kept when it already names that hex, so a grey keeps
    /// its hue.
    pub fn set_color(&mut self, colour: Color) {
        if !self.color().same_swatch(colour) {
            self.hsv = colour.to_hsv();
        }
        self.opacity = colour.opacity;
    }

    /// `setRgb` — through `rgbToHsv`; the opacity is left alone.
    pub fn set_rgb(&mut self, rgb: Rgb) {
        self.hsv = rgb_to_hsv(rgb.r, rgb.g, rgb.b);
    }

    /// `setHSV`.
    pub fn set_hsv(&mut self, h: f64, s: f64, v: f64) {
        self.hsv = Hsv { h, s, v };
    }

    /// The rounded channels `[r, g, b]` the web derives every frame.
    fn rgb_rounded(&self) -> (f64, f64, f64) {
        let (r, g, b) = hsv_to_rgb(self.hsv.h, self.hsv.s, self.hsv.v).channels();
        (r as f64, g as f64, b as f64)
    }

    /// The hex the input shows: six upper-case digits, no `#`.
    pub fn hex_text(&self) -> String {
        self.color().rgb.to_hex()[1..].to_uppercase()
    }

    /// Applies what was typed into the hex field — three or six digits,
    /// nothing else. Returns whether it was taken.
    pub fn apply_hex(&mut self, text: &str) -> bool {
        match parse_web_hex(text) {
            Some(rgb) => {
                self.set_rgb(rgb);
                true
            }
            None => false,
        }
    }

    /// The harmony of the current colour.
    pub fn harmony(&self) -> Vec<Hsv> {
        harmony_colors(self.scheme, self.hsv.h, self.hsv.s, self.hsv.v)
    }

    /// The channel sliders of the active model, with their tracks — the
    /// `chans` array of the web, stop for stop.
    pub fn channels(&self) -> Vec<Channel> {
        let (r, g, b) = self.rgb_rounded();
        let Hsv { h, s, v } = self.hsv;
        let q = |c: Rgb| {
            let (r, g, b) = c.channels();
            Rgb::new(r as f64, g as f64, b as f64)
        };
        let hue_track = || {
            ["#f00", "#ff0", "#0f0", "#0ff", "#00f", "#f0f", "#f00"]
                .iter()
                .filter_map(|x| parse_web_hex(x))
                .collect::<Vec<_>>()
        };
        let black = Rgb::new(0.0, 0.0, 0.0);
        let white = Rgb::new(255.0, 255.0, 255.0);
        let ch = |label, value, max, track| Channel { label, value, max, track };
        match self.mode {
            ColorMode::Rgb => vec![
                ch("R", r, 255.0, vec![Rgb::new(0.0, g, b), Rgb::new(255.0, g, b)]),
                ch("G", g, 255.0, vec![Rgb::new(r, 0.0, b), Rgb::new(r, 255.0, b)]),
                ch("B", b, 255.0, vec![Rgb::new(r, g, 0.0), Rgb::new(r, g, 255.0)]),
            ],
            ColorMode::Hsv => vec![
                ch("H", h, 360.0, hue_track()),
                ch("S", s * 100.0, 100.0, vec![q(hsv_to_rgb(h, 0.0, v)), q(hsv_to_rgb(h, 1.0, v))]),
                ch("V", v * 100.0, 100.0, vec![black, q(hsv_to_rgb(h, s, 1.0))]),
            ],
            ColorMode::Hsl => {
                let hsl = rgb_to_hsl(r, g, b);
                vec![
                    ch("H", hsl.h, 360.0, hue_track()),
                    ch("S", hsl.s * 100.0, 100.0, vec![q(hsl_to_rgb(hsl.h, 0.0, hsl.l)), q(hsl_to_rgb(hsl.h, 1.0, hsl.l))]),
                    ch("L", hsl.l * 100.0, 100.0, vec![black, q(hsl_to_rgb(hsl.h, hsl.s, 0.5)), white]),
                ]
            }
            ColorMode::Cmyk => {
                let k4 = rgb_to_cmyk(r, g, b);
                let (c, m, y, k) = (k4.c, k4.m, k4.y, k4.k);
                vec![
                    ch("C", c, 100.0, vec![q(cmyk_to_rgb(0.0, m, y, k)), q(cmyk_to_rgb(100.0, m, y, k))]),
                    ch("M", m, 100.0, vec![q(cmyk_to_rgb(c, 0.0, y, k)), q(cmyk_to_rgb(c, 100.0, y, k))]),
                    ch("Y", y, 100.0, vec![q(cmyk_to_rgb(c, m, 0.0, k)), q(cmyk_to_rgb(c, m, 100.0, k))]),
                    ch("K", k, 100.0, vec![q(cmyk_to_rgb(c, m, y, 0.0)), black]),
                ]
            }
            ColorMode::Gray => {
                let gy = js_round((r + g + b) / 3.0);
                vec![ch("K", gy / 255.0 * 100.0, 100.0, vec![black, white])]
            }
        }
    }

    /// Writes channel `i` of the active model — each channel's `set` closure
    /// in the web, verbatim.
    pub fn set_channel(&mut self, i: usize, x: f64) {
        let (r, g, b) = self.rgb_rounded();
        let Hsv { h, s, v } = self.hsv;
        match (self.mode, i) {
            (ColorMode::Rgb, 0) => self.set_rgb(Rgb::new(x, g, b)),
            (ColorMode::Rgb, 1) => self.set_rgb(Rgb::new(r, x, b)),
            (ColorMode::Rgb, 2) => self.set_rgb(Rgb::new(r, g, x)),
            (ColorMode::Hsv, 0) => self.set_hsv(x, s, v),
            (ColorMode::Hsv, 1) => self.set_hsv(h, x / 100.0, v),
            (ColorMode::Hsv, 2) => self.set_hsv(h, s, x / 100.0),
            (ColorMode::Hsl, _) => {
                let hsl = rgb_to_hsl(r, g, b);
                let rgb = match i {
                    0 => hsl_to_rgb(x, hsl.s, hsl.l),
                    1 => hsl_to_rgb(hsl.h, x / 100.0, hsl.l),
                    2 => hsl_to_rgb(hsl.h, hsl.s, x / 100.0),
                    _ => return,
                };
                self.set_rgb(rgb);
            }
            (ColorMode::Cmyk, _) => {
                let k4 = rgb_to_cmyk(r, g, b);
                let rgb = match i {
                    0 => cmyk_to_rgb(x, k4.m, k4.y, k4.k),
                    1 => cmyk_to_rgb(k4.c, x, k4.y, k4.k),
                    2 => cmyk_to_rgb(k4.c, k4.m, x, k4.k),
                    3 => cmyk_to_rgb(k4.c, k4.m, k4.y, x),
                    _ => return,
                };
                self.set_rgb(rgb);
            }
            (ColorMode::Gray, 0) => {
                let gg = js_round(x / 100.0 * 255.0);
                self.set_rgb(Rgb::new(gg, gg, gg));
            }
            _ => {}
        }
    }

    // ── Layout ───────────────────────────────────────────────────────────────

    /// The side of the SV area: `sqSide = round((SIZE - 2·RING - 12) / √2)`
    /// for the square, `innerD = SIZE - 2·RING - 6` otherwise.
    pub fn sv_size(shape: SvShape) -> f32 {
        match shape {
            SvShape::Square => ((pm::SIZE - 2.0 * pm::RING - 12.0) / std::f32::consts::SQRT_2).round(),
            _ => pm::SIZE - 2.0 * pm::RING - 6.0,
        }
    }

    fn footer_btn_w(label: &str) -> f32 {
        let font = Font::role(FontRole::CaptionStrong).sized(pm::TEXT_MD);
        let (w, _) = crate::graphics::text::approximate_measure(label, &font, None);
        w + 2.0 * pm::FOOTER_BTN_PAD + 2.0
    }

    /// Everything's rectangle, for a panel occupying `bounds`.
    pub fn layout(&self, bounds: Rect) -> PickerLayout {
        let l = bounds.left + pm::INSET;
        let r = (bounds.right - pm::INSET).max(l);
        let cw = r - l;
        let mut y = bounds.top + pm::INSET;

        let close = Rect::new(r - pm::CLOSE_W, y, r, y + pm::HEADER_H);
        let title = Rect::new(l, y, close.left - 4.0, y + pm::HEADER_H);
        y += pm::HEADER_H + pm::HEADER_GAP;

        // The main row: `flex items-start gap-1.5 justify-center`, the centre
        // item shrinking when the row is narrower than 32 + 6 + 212 + 6 + 32.
        let wheel_w = pm::SIZE.min((cw - 2.0 * (pm::BTN + pm::COL_GAP)).max(0.0));
        let row_w = 2.0 * (pm::BTN + pm::COL_GAP) + wheel_w;
        let x0 = l + ((cw - row_w) / 2.0).max(0.0);
        let row_top = y;
        let btn = |x: f32, i: usize| {
            let t = row_top + i as f32 * (pm::BTN + pm::BTN_GAP);
            Rect::new(x, t, x + pm::BTN, t + pm::BTN)
        };
        let shapes = [btn(x0, 0), btn(x0, 1), btn(x0, 2)];
        let mut next = 3;
        let eyedropper = self.eyedropper.then(|| {
            next += 1;
            btn(x0, 3)
        });
        let tools = (0..self.tools.len()).map(|i| btn(x0, next + i)).collect();
        let wheel_left = x0 + pm::BTN + pm::COL_GAP;
        let wheel = Rect::new(wheel_left, row_top, wheel_left + wheel_w, row_top + pm::SIZE);
        let side = Self::sv_size(self.shape);
        // `left: (212 - size) / 2`, from the ring's own box.
        let sv_off = (pm::SIZE - side) / 2.0;
        let sv = Rect::new(wheel.left + sv_off, wheel.top + sv_off, wheel.left + sv_off + side, wheel.top + sv_off + side);
        let right_col = wheel.right + pm::COL_GAP;
        // `justify-between` over 212: six 32s and five equal gaps.
        let gap = (pm::SIZE - 6.0 * pm::BTN) / 5.0;
        let schemes = std::array::from_fn(|i| {
            let t = row_top + i as f32 * (pm::BTN + gap);
            Rect::new(right_col, t, right_col + pm::BTN, t + pm::BTN)
        });
        y = row_top + pm::SIZE + pm::HARM_TOP;

        let n = self.harmony().len().max(1);
        let hw = (cw - (n as f32 - 1.0) * pm::GRID_GAP) / n as f32;
        let harmony = (0..n)
            .map(|i| {
                let x = l + i as f32 * (hw + pm::GRID_GAP);
                Rect::new(x, y, x + hw, y + pm::HARM_H)
            })
            .collect();
        y += pm::HARM_H + pm::HEX_TOP;

        let preview = Rect::new(l, y, l + pm::PREVIEW_W, y + pm::ROW_H);
        let hash = Rect::new(preview.right + pm::GAP, y, preview.right + pm::GAP + pm::HASH_W, y + pm::ROW_H);
        let hex = Rect::new(hash.right + pm::GAP, y, r.max(hash.right + pm::GAP), y + pm::ROW_H);
        y += pm::ROW_H + pm::TABS_TOP;

        let mut tx = l;
        let tabs = std::array::from_fn(|i| {
            let rect = Rect::new(tx, y, tx + pm::TAB_W[i], y + pm::TAB_H);
            tx += pm::TAB_W[i];
            rect
        });
        let tab_rule = Rect::new(l, y + pm::TAB_H, r, y + pm::TAB_H + 1.0);
        y += pm::TAB_H + 1.0 + pm::TABS_BOTTOM;

        let count = self.channels().len();
        let mut channels = Vec::with_capacity(count);
        for i in 0..count {
            if i > 0 {
                y += pm::CHAN_GAP_Y;
            }
            channels.push(Rect::new(l, y, r, y + pm::CHAN_H));
            y += pm::CHAN_H;
        }
        y += pm::SWATCH_TOP;

        let swatch_grid = SwatchGrid::fixed(cw, pm::SWATCH, pm::GRID_GAP);
        let sh = swatch_grid.height(self.swatches.len());
        let swatches = Rect::new(l, y, r, y + sh);
        y += sh;

        let shown = self.recent.len().min(pm::RECENT_MAX);
        let recent_grid = SwatchGrid::fitting(cw, pm::RECENT_COLS, pm::GRID_GAP);
        let (recent_rule, recent_label, recent) = if shown == 0 {
            let e = Rect::new(l, y, r, y);
            (e, e, e)
        } else {
            let rule_y = y + pm::RECENT_TOP;
            let label_y = rule_y + 1.0 + pm::RECENT_PAD;
            let grid_y = label_y + pm::LABEL_H + pm::LABEL_GAP;
            let grid = Rect::new(l, grid_y, r, grid_y + recent_grid.height(shown));
            y = grid.bottom;
            (Rect::new(l, rule_y, r, rule_y + 1.0), Rect::new(l, label_y, r, label_y + pm::LABEL_H), grid)
        };

        let (footer_rule, cancel, confirm) = match &self.footer {
            Some((c_label, k_label)) => {
                let rule_y = y + pm::FOOTER_TOP;
                let top = rule_y + 1.0 + pm::FOOTER_PAD;
                let kw = Self::footer_btn_w(k_label);
                let cw2 = Self::footer_btn_w(c_label);
                let confirm = Rect::new(r - kw, top, r, top + pm::FOOTER_BTN_H);
                let cancel = Rect::new(confirm.left - pm::GAP - cw2, top, confirm.left - pm::GAP, top + pm::FOOTER_BTN_H);
                (Rect::new(l, rule_y, r, rule_y + 1.0), cancel, confirm)
            }
            None => {
                let e = Rect::new(r, y, r, y);
                (e, e, e)
            }
        };

        PickerLayout {
            panel: bounds,
            title,
            close,
            shapes,
            eyedropper,
            tools,
            wheel,
            sv,
            schemes,
            harmony,
            preview,
            hash,
            hex,
            tabs,
            tab_rule,
            channels,
            swatches,
            swatch_grid,
            recent_rule,
            recent_label,
            recent,
            recent_grid,
            footer_rule,
            cancel,
            confirm,
        }
    }

    /// The panel's height at `width`.
    pub fn height_for_width(&self, width: f32) -> f32 {
        let g = self.layout(Rect::new(0.0, 0.0, width, 0.0));
        let last = if self.footer.is_some() {
            g.confirm.bottom
        } else if g.recent.bottom > g.recent.top {
            g.recent.bottom
        } else {
            g.swatches.bottom
        };
        last + pm::INSET
    }

    // ── Geometry of the ring ─────────────────────────────────────────────────

    /// The hue handle's centre: `SIZE/2 + ringR·sin(h)`, `SIZE/2 − ringR·cos(h)`
    /// from the ring box's top-left, `ringR = SIZE/2 − RING/2`.
    pub fn ring_point(wheel: Rect, h: f64) -> (f32, f32) {
        let ring_r = (pm::SIZE / 2.0 - pm::RING / 2.0) as f64;
        let a = h.to_radians();
        let half = (pm::SIZE / 2.0) as f64;
        (wheel.left + (half + ring_r * a.sin()) as f32, wheel.top + (half - ring_r * a.cos()) as f32)
    }

    /// Whether `(x, y)` is on the ring itself — inside its ellipse and
    /// outside the inner disc (which, in the web, takes the pointer and does
    /// nothing with it).
    pub fn on_ring(wheel: Rect, x: f32, y: f32) -> bool {
        let (cx, cy) = ((wheel.left + wheel.right) / 2.0, (wheel.top + wheel.bottom) / 2.0);
        let (rx, ry) = ((wheel.right - wheel.left) / 2.0, (wheel.bottom - wheel.top) / 2.0);
        if rx <= 0.0 || ry <= 0.0 {
            return false;
        }
        let (dx, dy) = (x - cx, y - cy);
        let outer = (dx / rx).powi(2) + (dy / ry).powi(2);
        let (ix, iy) = (rx - pm::RING, ry - pm::RING);
        let inner = if ix > 0.0 && iy > 0.0 { (dx / ix).powi(2) + (dy / iy).powi(2) } else { 2.0 };
        outer <= 1.0 && inner > 1.0
    }

    /// The SV handle's centre, in the panel's space.
    pub fn sv_handle_center(&self, sv: Rect) -> (f32, f32) {
        let side = sv.right - sv.left;
        let (x, y) = sv_handle_pos(self.shape, side, self.hsv.s, self.hsv.v);
        (sv.left + x as f32, sv.top + y as f32)
    }

    /// The `(s, v)` a point designates on the area (a drag may leave it).
    pub fn sv_at(&self, sv: Rect, x: f32, y: f32) -> (f64, f64) {
        let side = sv.right - sv.left;
        sv_from_point(self.shape, side, (x - sv.left) as f64, (y - sv.top) as f64)
    }

    // ── Hit-testing and focus ────────────────────────────────────────────────

    /// Which part of the panel `(x, y)` lands on.
    pub fn part_at(&self, bounds: Rect, x: f32, y: f32) -> Option<PickerPart> {
        let g = self.layout(bounds);
        if !bounds.contains(x, y) {
            return None;
        }
        if g.close.contains(x, y) {
            return Some(PickerPart::Close);
        }
        let disc = |r: Rect| super::circular_hit(r, x, y);
        for (i, r) in g.shapes.iter().enumerate() {
            if disc(*r) {
                return Some(PickerPart::Shape(SvShape::ALL[i]));
            }
        }
        if g.eyedropper.is_some_and(disc) {
            return Some(PickerPart::Eyedropper);
        }
        if let Some(i) = g.tools.iter().position(|r| disc(*r)) {
            return Some(PickerPart::Tool(i));
        }
        if g.sv.contains(x, y) {
            return Some(PickerPart::Area);
        }
        if Self::on_ring(g.wheel, x, y) {
            return Some(PickerPart::Ring);
        }
        for (i, r) in g.schemes.iter().enumerate() {
            if disc(*r) {
                return Some(PickerPart::Scheme(Scheme::ALL[i]));
            }
        }
        if let Some(i) = g.harmony.iter().position(|r| r.contains(x, y)) {
            return Some(PickerPart::Harmony(i));
        }
        if g.hex.contains(x, y) {
            return Some(PickerPart::Hex);
        }
        for (i, r) in g.tabs.iter().enumerate() {
            if r.contains(x, y) {
                return Some(PickerPart::Mode(ColorMode::ALL[i]));
            }
        }
        for (i, row) in g.channels.iter().enumerate() {
            if PickerLayout::channel_box(*row).contains(x, y) {
                return Some(PickerPart::ChannelBox(i));
            }
            if PickerLayout::track(*row).inflate(0.0, pm::THUMB_OVER).contains(x, y) {
                return Some(PickerPart::Channel(i));
            }
        }
        if let Some(i) = g.swatch_grid.cell_at(g.swatches.left, g.swatches.top, self.swatches.len(), false, x, y) {
            return Some(PickerPart::Swatch(i));
        }
        let shown = self.recent.len().min(pm::RECENT_MAX);
        if let Some(i) = g.recent_grid.cell_at(g.recent.left, g.recent.top, shown, false, x, y) {
            return Some(PickerPart::Recent(i));
        }
        if self.footer.is_some() {
            if g.cancel.contains(x, y) {
                return Some(PickerPart::Cancel);
            }
            if g.confirm.contains(x, y) {
                return Some(PickerPart::Confirm);
            }
        }
        None
    }

    /// The parts a Tab walks through, in the web's DOM order, with the
    /// rectangle each registers for the focus manager. The chip rows appear
    /// once each, at their cursor.
    pub fn tab_stops(&self, bounds: Rect) -> Vec<(PickerPart, Rect)> {
        let g = self.layout(bounds);
        let mut out = vec![(PickerPart::Close, g.close)];
        for (i, r) in g.shapes.iter().enumerate() {
            out.push((PickerPart::Shape(SvShape::ALL[i]), *r));
        }
        if let Some(r) = g.eyedropper {
            out.push((PickerPart::Eyedropper, r));
        }
        for (i, r) in g.tools.iter().enumerate() {
            out.push((PickerPart::Tool(i), *r));
        }
        out.push((PickerPart::Ring, g.wheel));
        out.push((PickerPart::Area, g.sv));
        for (i, r) in g.schemes.iter().enumerate() {
            out.push((PickerPart::Scheme(Scheme::ALL[i]), *r));
        }
        for (i, r) in g.harmony.iter().enumerate() {
            out.push((PickerPart::Harmony(i), *r));
        }
        out.push((PickerPart::Hex, g.hex));
        for (i, r) in g.tabs.iter().enumerate() {
            out.push((PickerPart::Mode(ColorMode::ALL[i]), *r));
        }
        for (i, row) in g.channels.iter().enumerate() {
            out.push((PickerPart::Channel(i), PickerLayout::track(*row)));
            out.push((PickerPart::ChannelBox(i), PickerLayout::channel_box(*row)));
        }
        if !self.swatches.is_empty() {
            let i = self.swatch_cursor.min(self.swatches.len() - 1);
            out.push((PickerPart::Swatch(i), g.swatches));
        }
        let shown = self.recent.len().min(pm::RECENT_MAX);
        if shown > 0 {
            out.push((PickerPart::Recent(self.recent_cursor.min(shown - 1)), g.recent));
        }
        if self.footer.is_some() {
            out.push((PickerPart::Cancel, g.cancel));
            out.push((PickerPart::Confirm, g.confirm));
        }
        out
    }

    // ── Keyboard ─────────────────────────────────────────────────────────────

    /// Applies a navigation key to `part` as the web's handlers do. Returns
    /// whether anything changed.
    pub fn key(&mut self, bounds: Rect, part: PickerPart, key: ColorKey, shift: bool) -> bool {
        let before = (self.hsv, self.swatch_cursor, self.recent_cursor);
        let st = if shift { pm::STEP_BIG } else { pm::STEP };
        match part {
            PickerPart::Area => {
                let st = if shift { pm::SV_STEP_BIG } else { pm::SV_STEP };
                let cl = |x: f64| x.clamp(0.0, 1.0);
                let Hsv { h, s, v } = self.hsv;
                match key {
                    ColorKey::Left => self.set_hsv(h, cl(s - st), v),
                    ColorKey::Right => self.set_hsv(h, cl(s + st), v),
                    ColorKey::Up => self.set_hsv(h, s, cl(v + st)),
                    ColorKey::Down => self.set_hsv(h, s, cl(v - st)),
                    _ => return false,
                }
            }
            // The ring WRAPS: `(h - st + 360) % 360`.
            PickerPart::Ring => {
                let Hsv { h, s, v } = self.hsv;
                match key {
                    ColorKey::Left | ColorKey::Down => self.set_hsv((h - st + 360.0) % 360.0, s, v),
                    ColorKey::Right | ColorKey::Up => self.set_hsv((h + st) % 360.0, s, v),
                    _ => return false,
                }
            }
            PickerPart::Channel(i) => {
                let Some(ch) = self.channels().get(i).cloned() else { return false };
                let next = match key {
                    ColorKey::Left | ColorKey::Down => (ch.value - st).max(0.0),
                    ColorKey::Right | ColorKey::Up => (ch.value + st).min(ch.max),
                    ColorKey::Home => 0.0,
                    ColorKey::End => ch.max,
                    ColorKey::PageUp => (ch.value + pm::STEP_BIG).min(ch.max),
                    ColorKey::PageDown => (ch.value - pm::STEP_BIG).max(0.0),
                };
                self.set_channel(i, next);
            }
            PickerPart::ChannelBox(i) => {
                let delta = match key {
                    ColorKey::Up => 1.0,
                    ColorKey::Down => -1.0,
                    _ => return false,
                };
                let Some(ch) = self.channels().get(i).cloned() else { return false };
                self.set_channel(i, (js_round(ch.value) + delta).clamp(0.0, ch.max));
                self.begin_edit(part);
            }
            PickerPart::Swatch(_) => {
                let g = self.layout(bounds).swatch_grid;
                self.swatch_cursor = g.step(self.swatch_cursor, self.swatches.len(), key);
                self.focus = Some(PickerPart::Swatch(self.swatch_cursor));
            }
            PickerPart::Recent(_) => {
                let shown = self.recent.len().min(pm::RECENT_MAX);
                let g = self.layout(bounds).recent_grid;
                self.recent_cursor = g.step(self.recent_cursor, shown, key);
                self.focus = Some(PickerPart::Recent(self.recent_cursor));
            }
            _ => return false,
        }
        before != (self.hsv, self.swatch_cursor, self.recent_cursor)
    }

    /// Activates a button part (a click, or Enter / Space on it).
    pub fn activate(&mut self, part: PickerPart) -> PickerEvent {
        match part {
            PickerPart::Close => return PickerEvent::Close,
            PickerPart::Cancel => return PickerEvent::Cancel,
            PickerPart::Confirm => return PickerEvent::Confirm,
            PickerPart::Shape(s) => self.shape = s,
            PickerPart::Eyedropper => self.arm_eyedropper(),
            PickerPart::Tool(i) => return PickerEvent::Tool(i),
            PickerPart::Scheme(s) => self.scheme = s,
            PickerPart::Harmony(i) => {
                if let Some(c) = self.harmony().get(i).copied() {
                    self.set_hsv(c.h, c.s, c.v);
                }
            }
            PickerPart::Mode(m) => {
                self.mode = m;
                self.edit = None;
            }
            PickerPart::Swatch(i) => {
                if let Some(c) = self.swatches.get(i).copied() {
                    self.set_rgb(c.rgb);
                }
                self.swatch_cursor = i;
            }
            PickerPart::Recent(i) => {
                self.recent_cursor = i;
                if let Some(c) = self.recent.get(i).copied() {
                    self.set_rgb(c.rgb);
                    return PickerEvent::History(i);
                }
            }
            _ => {}
        }
        PickerEvent::None
    }

    // ── Text boxes ───────────────────────────────────────────────────────────

    /// The text a box shows when nobody is typing into it.
    pub fn box_text(&self, part: PickerPart) -> String {
        match part {
            PickerPart::Hex => self.hex_text(),
            PickerPart::ChannelBox(i) => {
                self.channels().get(i).map(|c| format!("{}", js_round(c.value))).unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    /// Starts typing into `part` (a box), with its whole value selected.
    pub fn begin_edit(&mut self, part: PickerPart) {
        if !part.is_text() {
            return;
        }
        let kind = if part == PickerPart::Hex { DraftKind::Hex } else { DraftKind::Digits };
        let mut d = FieldDraft::new(kind, &self.box_text(part));
        d.last_input_ms = host::now_ms();
        self.edit = Some((part, d));
    }

    /// Applies the draft — the web's `onChange`, on every keystroke: the hex
    /// field takes three or six digits and ignores the rest; a numeric box
    /// takes `Math.max(0, Math.min(max, +value))`, so an empty box is zero.
    pub fn apply_edit(&mut self) -> bool {
        let Some((part, d)) = self.edit.clone() else { return false };
        match part {
            PickerPart::Hex => self.apply_hex(d.digits()),
            PickerPart::ChannelBox(i) => {
                let Some(ch) = self.channels().get(i).cloned() else { return false };
                self.set_channel(i, d.number().unwrap_or(0.0).clamp(0.0, ch.max));
                true
            }
            _ => false,
        }
    }

    pub fn end_edit(&mut self) {
        self.edit = None;
    }

    /// The draft for `part`, if that box is being typed into.
    pub fn draft(&self, part: PickerPart) -> Option<&FieldDraft> {
        self.edit.as_ref().filter(|(p, _)| *p == part).map(|(_, d)| d)
    }

    /// The font and alignment a box's text uses.
    fn box_font(part: PickerPart) -> (Font, bool) {
        match part {
            PickerPart::Hex => (Font::role(FontRole::Caption).sized(pm::TEXT_MD), false),
            _ => (Font::role(FontRole::Caption).sized(pm::TEXT_SM), true),
        }
    }

    /// The rectangle a box's text lives in (inside its padding).
    pub fn text_rect(g: &PickerLayout, part: PickerPart) -> Rect {
        match part {
            PickerPart::Hex => Rect::new(g.hex.left + pm::HEX_PAD, g.hex.top, g.hex.right - pm::HEX_PAD, g.hex.bottom),
            PickerPart::ChannelBox(i) => {
                let b = g.channels.get(i).map(|r| PickerLayout::channel_box(*r)).unwrap_or(g.hex);
                Rect::new(b.left + 2.0, b.top, b.right - 2.0, b.bottom)
            }
            _ => g.hex,
        }
    }

    // ── The screen eyedropper ────────────────────────────────────────────────

    /// Arms the eyedropper (the pipette button).
    pub fn arm_eyedropper(&mut self) {
        self.eye.arm();
    }

    pub fn is_picking(&self) -> bool {
        self.eye.is_picking()
    }

    pub fn swallows_pointer(&self) -> bool {
        self.eye.swallows_pointer()
    }

    /// Runs the eyedropper for one frame; call it every frame the picker is
    /// shown. Returns whether the colour changed.
    pub fn poll_eyedropper(&mut self) -> bool {
        match self.eye.poll() {
            Some(rgb) => {
                self.set_rgb(rgb);
                true
            }
            None => false,
        }
    }

    // ── One frame of interaction ─────────────────────────────────────────────

    /// The pointer's frame: hover, the press, drags. `panel` is where the
    /// picker is this frame, in the pointer's space; `c` measures text for
    /// the caret a click places.
    pub fn pointer(&mut self, c: &dyn Canvas, panel: Rect, p: PickerPointer) -> PickerEvent {
        if self.swallows_pointer() {
            self.hot = None;
            self.hot_recent = None;
            if !p.down {
                self.drag = None;
            }
            return PickerEvent::None;
        }
        let g = self.layout(panel);
        let hovered = if self.drag.is_some() { None } else { self.part_at(panel, p.x, p.y) };
        self.hot = hovered;
        self.hot_recent = match hovered {
            Some(PickerPart::Recent(i)) => Some(i),
            _ => None,
        };
        match self.drag.or(hovered) {
            Some(PickerPart::Area) => host::set_cursor(Cursor::Crosshair),
            Some(part) if part.is_text() => host::set_cursor(Cursor::IBeam),
            Some(PickerPart::Ring | PickerPart::Channel(_)) => host::set_cursor(Cursor::Hand),
            _ => {}
        }
        let mut event = PickerEvent::None;
        if p.pressed {
            match hovered {
                Some(part @ (PickerPart::Area | PickerPart::Ring | PickerPart::Channel(_))) => self.drag = Some(part),
                Some(part) if part.is_text() => {
                    self.begin_edit(part);
                    let rect = Self::text_rect(&g, part);
                    let (font, centred) = Self::box_font(part);
                    if let Some((_, d)) = self.edit.as_mut() {
                        let shown = if part == PickerPart::Hex { d.text.to_uppercase() } else { d.text.clone() };
                        let gr = Graphics::new(c);
                        let fmt = StringFormat::generic_typographic();
                        let width = |s: &str| gr.measure_string(s, &font, None, &fmt).width;
                        let start = if centred {
                            (rect.left + rect.right) / 2.0 - width(&shown) / 2.0
                        } else {
                            rect.left
                        };
                        let at = d.index_at(p.x - start, |s| width(&shown[..s.len()]));
                        d.move_to(at, false);
                    }
                }
                Some(part) => event = self.activate(part),
                None => {}
            }
        }
        if !p.down {
            self.drag = None;
        }
        if !p.away {
            match self.drag {
                Some(PickerPart::Area) => {
                    let (s, v) = self.sv_at(g.sv, p.x, p.y);
                    self.set_hsv(self.hsv.h, s, v);
                }
                Some(PickerPart::Ring) => {
                    let h = hue_from_point(g.wheel, p.x, p.y);
                    self.set_hsv(h, self.hsv.s, self.hsv.v);
                }
                Some(PickerPart::Channel(i)) => {
                    if let (Some(row), Some(ch)) = (g.channels.get(i).copied(), self.channels().get(i).cloned()) {
                        self.set_channel(i, PickerLayout::channel_value_at(row, p.x, ch.max));
                    }
                }
                _ => {}
            }
        }
        event
    }

    /// The keyboard's frame, for the part in [`ColorPicker::focus`] (which the
    /// host sets from its focus manager, from [`ColorPicker::tab_stops`]):
    /// typing into a box, the arrows, Enter / Space on a button.
    pub fn keyboard(&mut self, panel: Rect, window_focused: bool) -> PickerEvent {
        let mut event = PickerEvent::None;
        match self.focus {
            Some(part) if part.is_text() => {
                if self.draft(part).is_none() {
                    self.begin_edit(part);
                }
                let outcome = self.edit.as_mut().map(|(_, d)| d.take_input()).unwrap_or(DraftOutcome::Idle);
                match outcome {
                    DraftOutcome::Edited => {
                        self.apply_edit();
                    }
                    DraftOutcome::Commit => {
                        self.apply_edit();
                        self.begin_edit(part);
                    }
                    DraftOutcome::Cancel => self.begin_edit(part),
                    _ => {}
                }
                for (k, shift) in take_color_keys() {
                    self.key(panel, part, k, shift);
                }
                let phase = self.draft(part).map(|d| d.last_input_ms).unwrap_or(0);
                self.caret_on = window_focused && crate::focus::caret_visible(phase);
            }
            Some(part) => {
                self.end_edit();
                for (k, shift) in take_color_keys() {
                    self.key(panel, part, k, shift);
                }
                let part = self.focus.unwrap_or(part);
                if part.is_button()
                    && (host::take_key(vk::ENTER, Modifiers::NONE) > 0 || host::take_key(vk::SPACE, Modifiers::NONE) > 0)
                {
                    event = self.activate(part);
                }
            }
            None => self.end_edit(),
        }
        event
    }

    // ── Painting ─────────────────────────────────────────────────────────────

    fn ring_on(&self, part: PickerPart) -> bool {
        self.focus_visible && self.focus.is_some_and(|f| f.focus_slot() == part.focus_slot())
    }

    fn has_focus(&self, part: PickerPart) -> bool {
        self.focus.is_some_and(|f| f.focus_slot() == part.focus_slot())
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Paint helpers
// ═════════════════════════════════════════════════════════════════════════════

/// Pure white — `#fff` in the web's literals (handles, active glyphs).
fn white(alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: alpha }
}

fn black(alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: alpha }
}

/// Text at an explicit web size (10 / 11 px) in the app's font.
#[allow(clippy::too_many_arguments)]
pub(super) fn text(c: &dyn Canvas, s: &str, rect: Rect, size: f32, strong: bool, colour: D2D1_COLOR_F, align: StringAlignment) {
    let role = if strong { FontRole::CaptionStrong } else { FontRole::Caption };
    let font = Font::role(role).sized(size);
    let fmt = StringFormat::single_line_ellipsis().with_alignment(align);
    Graphics::new(c).draw_string(s, &font, Brush::solid(colour), rect, &fmt);
}

/// A filled + outlined disc, anti-aliased.
fn disc(g: &Graphics<'_>, rect: Rect, fill: Option<D2D1_COLOR_F>, stroke: Option<(D2D1_COLOR_F, f32)>) {
    if let Some(f) = fill {
        g.fill_ellipse(Brush::solid(f), rect);
    }
    if let Some((s, w)) = stroke {
        let r = rect.inflate(-w / 2.0, -w / 2.0);
        g.draw_ellipse(&Pen::new(s, w), r);
    }
}

/// A web handle: a filled disc with a `width` white border and a 1 DIP
/// `box-shadow` ring OUTSIDE it.
#[allow(clippy::too_many_arguments)]
fn handle(g: &Graphics<'_>, cx: f32, cy: f32, d: f32, fill: Option<D2D1_COLOR_F>, border: D2D1_COLOR_F, width: f32, shadow: f32, alpha: f32) {
    let r = Rect::new(cx - d / 2.0, cy - d / 2.0, cx + d / 2.0, cy + d / 2.0);
    if shadow > 0.0 {
        disc(g, r.inflate(1.0, 1.0), None, Some((black(shadow * alpha), 1.0)));
    }
    disc(g, r, fill.map(|f| fade(f, alpha)), Some((fade(border, alpha), width)));
}

/// `HarmonyIcon` — a hue-ring outline with the scheme's marker dots at their
/// true angles, joined by the harmony's polygon; `mono` is graduated dots
/// along one radius.
fn harmony_icon(g: &Graphics<'_>, scheme: Scheme, rect: Rect, colour: D2D1_COLOR_F) {
    let size = pm::HARMONY_ICON;
    let ox = (rect.left + rect.right) / 2.0 - size / 2.0;
    let oy = (rect.top + rect.bottom) / 2.0 - size / 2.0;
    let c = size / 2.0;
    let r = size / 2.0 - 3.0;
    let dot = (size * 0.095).max(1.6);
    let with = |a: f32| fade(colour, a);
    let pt = |a: f64| {
        let a = a.to_radians() as f32;
        PointF::new(ox + c + r * a.sin(), oy + c - r * a.cos())
    };
    let circle = |x: f32, y: f32, rad: f32| Rect::new(x - rad, y - rad, x + rad, y + rad);
    g.draw_ellipse(&Pen::new(with(0.3), 1.0), circle(ox + c, oy + c, r));
    if scheme == Scheme::Mono {
        g.draw_line(&Pen::new(with(0.45), 1.2), PointF::new(ox + c, oy + c + r), PointF::new(ox + c, oy + c - r));
        for (i, f) in [-1.0f32, -0.33, 0.33, 1.0].iter().enumerate() {
            let rad = if i == 3 { dot * 1.25 } else { dot };
            g.fill_ellipse(Brush::solid(with(0.45 + 0.18 * (i as f32 + 1.0))), circle(ox + c, oy + c - r * f, rad));
        }
        return;
    }
    // Analogous hues are spread wider on the glyph so it reads at 20 px.
    let angles: Vec<f64> = if scheme == Scheme::Analog { vec![-48.0, 0.0, 48.0] } else { scheme.angles().to_vec() };
    let pts: Vec<PointF> = angles.iter().map(|a| pt(*a)).collect();
    if pts.len() == 2 {
        g.draw_line(&Pen::new(with(0.55), 1.2), pts[0], pts[1]);
    } else if scheme == Scheme::Analog {
        // An arc riding the hue ring between the outer two dots: from -48° to
        // +48° clockwise, i.e. GDI angles -138° over 96°.
        let ring = circle(ox + c, oy + c, r);
        g.draw_arc(&Pen::new(with(0.55), 1.2), ring, -90.0 - 48.0, 96.0);
    } else {
        g.fill_polygon(Brush::solid(with(0.14)), &pts);
        g.draw_polygon(&Pen::new(with(0.55), 1.2), &pts);
    }
    for (i, p) in pts.iter().enumerate() {
        let rad = if i == 0 { dot * 1.3 } else { dot };
        g.fill_ellipse(Brush::solid(colour), circle(p.x, p.y, rad));
    }
}

/// A channel track's CSS gradient, stops evenly spaced over the padding box.
fn paint_track(c: &dyn Canvas, track: Rect, stops: &[Rgb], alpha: f32) {
    let g = Graphics::new(c);
    let inner = track.inflate(-1.0, -1.0);
    c.push_clip_rounded(&track, pm::BOX_RADIUS);
    if g.has_device() && stops.len() >= 2 {
        let n = (stops.len() - 1) as f32;
        let ps: Vec<crate::graphics::GradientStop> = stops
            .iter()
            .enumerate()
            .map(|(i, s)| crate::graphics::GradientStop::new(i as f32 / n, s.to_d2d(alpha).into()))
            .collect();
        let brush = LinearGradientBrush::new(
            PointF::new(inner.left, inner.top),
            PointF::new(inner.right, inner.top),
            stops[0].to_d2d(alpha).into(),
            stops[stops.len() - 1].to_d2d(alpha).into(),
        )
        .with_stops(&ps);
        g.fill_rectangle(Brush::Linear(brush), track);
    } else if let Some(first) = stops.first() {
        // Headless: one-DIP strips, interpolated between the stops.
        let n = stops.len().max(2) - 1;
        ramp_x(c, track, |f| {
            let p = f * n as f64;
            let i = (p.floor() as usize).min(n.saturating_sub(1));
            let t = p - i as f64;
            let a = stops.get(i).copied().unwrap_or(*first);
            let b = stops.get(i + 1).copied().unwrap_or(a);
            Rgb::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t).to_d2d(alpha)
        });
    }
    c.pop_clip_rounded();
}

/// A web `<input>`: `background: surface`, `border: 1px solid border`,
/// `borderRadius: 2`, its text at `size`, centred or not; while typed into,
/// the draft with its selection and caret.
#[allow(clippy::too_many_arguments)]
fn input(c: &dyn Canvas, rect: Rect, text_rect: Rect, value: &str, (font_size, centred, upper): (f32, bool, bool), focused: bool, draft: Option<(&FieldDraft, bool)>, alpha: f32) {
    let t = c.theme();
    c.fill_rounded(&rect, pm::BOX_RADIUS, &fade(t.surface_2, alpha));
    c.stroke_rounded(&rect, pm::BOX_RADIUS, &fade(if focused { t.accent } else { t.card_stroke }, alpha));
    let align = if centred { StringAlignment::Center } else { StringAlignment::Near };
    match draft {
        Some((d, caret_on)) if focused => {
            let font = Font::role(FontRole::Caption).sized(font_size);
            let fmt = StringFormat::generic_typographic();
            let g = Graphics::new(c);
            let shown = if upper { d.text.to_uppercase() } else { d.text.clone() };
            let width = |i: usize| g.measure_string(shown.get(..i).unwrap_or(""), &font, None, &fmt).width;
            let total = width(shown.len());
            let x0 = if centred { (text_rect.left + text_rect.right) / 2.0 - total / 2.0 } else { text_rect.left };
            let line_h = (font_size * 1.5).round();
            let top = (text_rect.top + text_rect.bottom) / 2.0 - line_h / 2.0;
            c.push_clip(&rect);
            let (a, b) = d.selection();
            if a != b {
                let band = Rect::new(x0 + width(a), top, x0 + width(b), top + line_h);
                c.fill_rounded(&band, 0.0, &fade(t.accent, 0.3 * alpha));
            }
            text(c, &shown, Rect::new(x0, text_rect.top, x0 + total + 2.0, text_rect.bottom), font_size, false, fade(t.text_primary, alpha), StringAlignment::Near);
            if caret_on {
                let x = x0 + width(d.caret);
                c.fill_rounded(&Rect::new(x, top, x + 1.0, top + line_h), 0.0, &fade(t.text_primary, alpha));
            }
            c.pop_clip();
        }
        _ => text(c, value, text_rect, font_size, false, fade(t.text_primary, alpha), align),
    }
}

/// A `px-1.5 text-[11px]` numeric `<input>`, left-aligned — the
/// `GradientPicker`'s angle, position and opacity boxes.
pub(super) fn number_input(c: &dyn Canvas, rect: Rect, value: &str, focused: bool, draft: Option<(&FieldDraft, bool)>, alpha: f32) {
    let text_rect = Rect::new(rect.left + 6.0, rect.top, rect.right - 6.0, rect.bottom);
    input(c, rect, text_rect, value, (pm::TEXT_MD, false, false), focused, draft, alpha);
}

/// `focus-visible:ring-2`, drawn outside `rect`, in the accent (the web's
/// `ring-white/70` would vanish on a light panel).
fn ring(c: &dyn Canvas, rect: Rect, radius: f32) {
    let r = rect.inflate(pm::FOCUS_RING, pm::FOCUS_RING);
    c.stroke_rounded_w(&r, radius + pm::FOCUS_RING, &c.theme().accent, pm::FOCUS_RING);
}

impl Widget for ColorPicker {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> ControlSize {
        ControlSize::new(pm::WIDTH, self.height_for_width(pm::WIDTH))
    }

    fn paint(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = c.theme();
        let dead = state.disabled;
        let alpha = if dead { DISABLED_ALPHA } else { 1.0 };
        let g = self.layout(bounds);
        let colour = self.color();
        let gr = Graphics::new(c);

        // `shadow-2xl p-3`, `background: C.toolbar`, `border: 1px solid C.border`,
        // `borderRadius: 4`.
        c.draw_shadow(&bounds, pm::RADIUS, &SHADOW_2XL, SHADOW_BLACK);
        c.fill_rounded(&bounds, pm::RADIUS, &t.toolbar_background);
        c.stroke_rounded(&bounds, pm::RADIUS, &t.card_stroke);
        c.push_clip_rounded(&bounds, pm::RADIUS);
        c.push_bg(t.toolbar_background);

        // ── Header ─────────────────────────────────────────────────────────
        text(c, &self.title, g.title, pm::TEXT_SM, true, fade(t.text_secondary, alpha), StringAlignment::Near);
        if self.hot == Some(PickerPart::Close) && !dead {
            // `hover:bg-white/10`.
            c.fill_rounded(&g.close, crate::metrics::radius::SM, &white(0.1));
        }
        text(c, "✕", g.close, pm::TEXT_MD, false, fade(t.text_secondary, alpha), StringAlignment::Center);
        if self.ring_on(PickerPart::Close) {
            ring(c, g.close, crate::metrics::radius::SM);
        }

        // ── Left column: shapes, eyedropper, module tools ────────────────
        let round_button = |rect: Rect, active: bool, hot_accent: bool, part: PickerPart, glyph: &mut dyn FnMut(Rect, D2D1_COLOR_F)| {
            let (fill, edge, ink) = if active {
                (t.accent, t.accent, white(1.0))
            } else if hot_accent {
                (t.surface_2, t.accent, t.accent)
            } else {
                (t.surface_2, t.card_stroke, t.text_secondary)
            };
            disc(&gr, rect, Some(fade(fill, alpha)), Some((fade(edge, alpha), 1.0)));
            glyph(rect, fade(ink, alpha));
            if self.ring_on(part) {
                ring(c, rect, pm::BTN / 2.0);
            }
        };
        for (i, rect) in g.shapes.iter().enumerate() {
            let s = SvShape::ALL[i];
            round_button(*rect, self.shape == s, false, PickerPart::Shape(s), &mut |r, ink| {
                c.vector_icon(s.icon(), &r, pm::SHAPE_ICON, &ink)
            });
        }
        if let Some(rect) = g.eyedropper {
            // Hover (and the armed state) turn its glyph and border accent.
            let lit = (self.hot == Some(PickerPart::Eyedropper) || self.is_picking()) && !dead;
            round_button(rect, false, lit, PickerPart::Eyedropper, &mut |r, ink| {
                c.vector_icon("Pipette", &r, pm::PIPETTE_ICON, &ink)
            });
        }
        for (i, rect) in g.tools.iter().enumerate() {
            let Some(tool) = self.tools.get(i) else { continue };
            round_button(*rect, tool.active, false, PickerPart::Tool(i), &mut |r, ink| {
                c.vector_icon(tool.icon, &r, pm::SHAPE_ICON, &ink)
            });
        }

        // ── The ring ───────────────────────────────────────────────────────
        if !raster::draw(c, g.wheel, Raster::Ring { ring: pm::RING }) {
            // Headless: the ring as one-DIP arcs.
            let (cx, cy) = ((g.wheel.left + g.wheel.right) / 2.0, (g.wheel.top + g.wheel.bottom) / 2.0);
            let rad = (g.wheel.right - g.wheel.left).min(g.wheel.bottom - g.wheel.top) / 2.0 - pm::RING / 2.0;
            for d in 0..360 {
                let a0 = (d as f32 - 90.0).to_radians();
                c.stroke_arc((cx, cy), rad, a0, 1.2f32.to_radians(), pm::RING, &hsv_to_rgb(d as f64, 1.0, 1.0).to_d2d(alpha));
            }
        }
        if self.ring_on(PickerPart::Ring) {
            let r = g.wheel.inflate(pm::FOCUS_RING / 2.0, pm::FOCUS_RING / 2.0);
            gr.draw_ellipse(&Pen::new(t.accent, pm::FOCUS_RING), r);
        }

        // ── The SV area ────────────────────────────────────────────────────
        let side = g.sv.right - g.sv.left;
        let sv_raster = Raster::Sv { h: self.hsv.h, shape: self.shape, radius: pm::SV_RADIUS };
        if !raster::draw(c, g.sv, sv_raster) {
            let hue = hsv_to_rgb(self.hsv.h, 1.0, 1.0);
            c.push_clip_rounded(&g.sv, pm::SV_RADIUS);
            c.fill_rounded(&g.sv, 0.0, &hue.to_d2d(alpha));
            ramp_x(c, g.sv, |s| white((1.0 - s) as f32 * alpha));
            super::ramp_y(c, g.sv, |y| black(y as f32 * alpha));
            c.pop_clip_rounded();
        }
        if self.ring_on(PickerPart::Area) {
            let radius = if self.shape == SvShape::Circle { side / 2.0 } else { pm::SV_RADIUS };
            ring(c, g.sv, radius);
        }

        // ── Handles: hue, harmony markers, SV ─────────────────────────────
        let (hx, hy) = Self::ring_point(g.wheel, self.hsv.h);
        let hue_hex = hsv_to_rgb(self.hsv.h, 1.0, 1.0).to_d2d(1.0);
        handle(&gr, hx, hy, pm::HUE_HANDLE, Some(hue_hex), white(1.0), 2.0, 0.6, alpha);
        let harm = self.harmony();
        for hc in harm.iter().skip(1) {
            let (mx, my) = Self::ring_point(g.wheel, hc.h);
            let fill = hsv_to_rgb(hc.h, hc.s, hc.v).to_d2d(1.0);
            handle(&gr, mx, my, pm::MARKER, Some(fill), white(0.85), 2.0, 0.0, alpha);
        }
        let (sx, sy) = self.sv_handle_center(g.sv);
        handle(&gr, sx, sy, pm::SV_HANDLE, None, white(1.0), 2.0, 0.5, alpha);

        // ── Right column: harmony schemes ─────────────────────────────────
        for (i, rect) in g.schemes.iter().enumerate() {
            let s = Scheme::ALL[i];
            round_button(*rect, self.scheme == s, false, PickerPart::Scheme(s), &mut |r, ink| {
                harmony_icon(&gr, s, r, ink)
            });
        }

        // ── Harmony swatches ──────────────────────────────────────────────
        for (i, rect) in g.harmony.iter().enumerate() {
            let Some(hc) = harm.get(i) else { continue };
            let fill = hsv_to_rgb(hc.h, hc.s, hc.v).to_d2d(alpha);
            c.fill_rounded(rect, pm::HARM_RADIUS, &fill);
            c.stroke_rounded(rect, pm::HARM_RADIUS, &fade(t.card_stroke, alpha));
            if self.ring_on(PickerPart::Harmony(i)) {
                ring(c, *rect, pm::HARM_RADIUS);
            }
        }

        // ── Preview + hex ─────────────────────────────────────────────────
        c.fill_rounded(&g.preview, pm::BOX_RADIUS, &colour.rgb.to_d2d(alpha));
        c.stroke_rounded(&g.preview, pm::BOX_RADIUS, &fade(t.card_stroke, alpha));
        text(c, "#", g.hash, pm::TEXT_SM, false, fade(t.text_secondary, alpha), StringAlignment::Near);
        let hex_focus = self.has_focus(PickerPart::Hex) && !dead;
        let draft = |part: PickerPart| self.draft(part).map(|d| (d, self.caret_on));
        input(
            c,
            g.hex,
            Self::text_rect(&g, PickerPart::Hex),
            &self.hex_text(),
            (pm::TEXT_MD, false, true),
            hex_focus,
            draft(PickerPart::Hex),
            alpha,
        );

        // ── Model tabs ────────────────────────────────────────────────────
        c.fill_rounded(&g.tab_rule, 0.0, &fade(t.card_stroke, alpha));
        for (i, rect) in g.tabs.iter().enumerate() {
            let m = ColorMode::ALL[i];
            let on = self.mode == m;
            let ink = if on { t.accent } else { t.text_secondary };
            let label = Rect::new(rect.left, rect.top, rect.right, rect.bottom - pm::TAB_UNDERLINE);
            text(c, m.label(), label, pm::TEXT_SM, true, fade(ink, alpha), StringAlignment::Center);
            if on {
                let u = Rect::new(rect.left, rect.bottom - pm::TAB_UNDERLINE, rect.right, rect.bottom);
                c.fill_rounded(&u, 0.0, &fade(t.accent, alpha));
            }
            if self.ring_on(PickerPart::Mode(m)) {
                ring(c, *rect, crate::metrics::radius::SM);
            }
        }

        // ── Channel sliders ───────────────────────────────────────────────
        for (i, (row, ch)) in g.channels.iter().zip(self.channels()).enumerate() {
            text(c, ch.label, PickerLayout::channel_label(*row), pm::TEXT_SM, false, fade(t.text_secondary, alpha), StringAlignment::Center);
            let track = PickerLayout::track(*row);
            paint_track(c, track, &ch.track, alpha);
            c.stroke_rounded(&track, pm::BOX_RADIUS, &fade(t.card_stroke, alpha));
            let thumb = PickerLayout::thumb(*row, ch.value, ch.max);
            c.fill_rounded(&thumb, pm::BOX_RADIUS, &white(alpha));
            c.stroke_rounded(&thumb.inflate(1.0, 1.0), pm::BOX_RADIUS + 1.0, &black(0.6 * alpha));
            if self.ring_on(PickerPart::Channel(i)) {
                ring(c, track, pm::BOX_RADIUS);
            }
            let part = PickerPart::ChannelBox(i);
            let bx = PickerLayout::channel_box(*row);
            let value = format!("{}", js_round(ch.value));
            input(c, bx, Self::text_rect(&g, part), &value, (pm::TEXT_SM, true, false), self.has_focus(part) && !dead, draft(part), alpha);
        }

        // ── Fixed chips ───────────────────────────────────────────────────
        for (i, chip) in self.swatches.iter().enumerate() {
            let rect = g.swatch_grid.cell_rect(g.swatches.left, g.swatches.top, i);
            let mark = chip.same_swatch(colour).then_some(SelectMark::Border);
            paint_chip(c, rect, *chip, pm::SWATCH_RADIUS, mark, false, alpha);
        }
        if let Some(PickerPart::Swatch(i)) = self.focus.filter(|_| self.focus_visible) {
            if i < self.swatches.len() {
                chip_focus(c, g.swatch_grid.cell_rect(g.swatches.left, g.swatches.top, i), pm::SWATCH_RADIUS);
            }
        }

        // ── Recent colours ────────────────────────────────────────────────
        if g.recent.bottom > g.recent.top {
            c.fill_rounded(&g.recent_rule, 0.0, &fade(t.card_stroke, alpha));
            // `text-[10px] uppercase tracking-wide`.
            text(c, &self.recent_label.to_uppercase(), g.recent_label, pm::TEXT_SM, false, fade(t.text_secondary, alpha), StringAlignment::Near);
            for (i, chip) in self.recent.iter().take(pm::RECENT_MAX).enumerate() {
                let rect = g.recent_grid.cell_rect(g.recent.left, g.recent.top, i);
                let mark = chip.same_swatch(colour).then_some(SelectMark::BorderRing);
                let grow = self.hot_recent == Some(i) && !dead;
                paint_chip(c, rect, *chip, pm::RECENT_RADIUS, mark, grow, alpha);
            }
            if let Some(PickerPart::Recent(i)) = self.focus.filter(|_| self.focus_visible) {
                if i < self.recent.len().min(pm::RECENT_MAX) {
                    chip_focus(c, g.recent_grid.cell_rect(g.recent.left, g.recent.top, i), pm::RECENT_RADIUS);
                }
            }
        }

        // ── Footer ────────────────────────────────────────────────────────
        if let Some((cancel, confirm)) = &self.footer {
            c.fill_rounded(&g.footer_rule, 0.0, &fade(t.card_stroke, alpha));
            for (rect, label, part) in [(g.cancel, cancel, PickerPart::Cancel), (g.confirm, confirm, PickerPart::Confirm)] {
                let primary = part == PickerPart::Confirm;
                let hot = self.hot == Some(part) && !dead;
                let radius = crate::metrics::radius::SM;
                let fill = match (primary, hot) {
                    (true, true) => t.accent_hover,
                    (true, false) => t.accent,
                    (false, true) => t.control_fill_hover,
                    (false, false) => t.toolbar_background,
                };
                c.fill_rounded(&rect, radius, &fade(fill, alpha));
                c.stroke_rounded(&rect, radius, &fade(if primary { t.accent } else { t.card_stroke }, alpha));
                let ink = if primary { white(1.0) } else { t.text_primary };
                text(c, label, rect, pm::TEXT_MD, true, fade(ink, alpha), StringAlignment::Center);
                if self.ring_on(part) {
                    ring(c, rect, radius);
                }
            }
        }
        c.pop_bg();
        c.pop_clip_rounded();
    }

    fn type_name(&self) -> &'static str {
        "ColorPicker"
    }
}

impl Deref for ColorPicker {
    type Target = PanelModel;
    fn deref(&self) -> &PanelModel {
        &self.inner
    }
}
impl DerefMut for ColorPicker {
    fn deref_mut(&mut self) -> &mut PanelModel {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// The screen eyedropper (Win32)
// ═════════════════════════════════════════════════════════════════════════════

mod eyedropper {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC, CLR_INVALID};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    use super::Rgb;

    fn down(vk: i32) -> bool {
        // SAFETY: a plain state query.
        (unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000) != 0
    }

    pub fn button_down() -> bool {
        down(VK_LBUTTON.0 as i32)
    }

    pub fn escape_down() -> bool {
        down(VK_ESCAPE.0 as i32)
    }

    /// The screen pixel under the cursor, in physical pixels.
    pub fn sample_under_cursor() -> Option<Rgb> {
        let mut p = POINT::default();
        // SAFETY: Win32 calls on the screen DC, released before returning.
        unsafe {
            GetCursorPos(&mut p).ok()?;
            let dc = GetDC(None);
            if dc.is_invalid() {
                return None;
            }
            let c = GetPixel(dc, p.x, p.y);
            ReleaseDC(None, dc);
            if c.0 == CLR_INVALID {
                return None;
            }
            let v = c.0;
            Some(Rgb::new((v & 0xff) as f64, ((v >> 8) & 0xff) as f64, ((v >> 16) & 0xff) as f64))
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests — parity with the web. The golden values below were produced by the
// live web bundle itself (`@ui`'s `rgbToHsv`, `rgbToHsl`, `rgbToCmyk`,
// `harmonyColors` + `rgbToHex(hsvToRgb(...))`, evaluated in Chrome), so they
// pin the port to what the browser computes, not to a re-derivation.
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::parse;

    fn hex(c: Hsv) -> String {
        hsv_to_rgb(c.h, c.s, c.v).to_hex()
    }

    fn rgb(h: &str) -> Rgb {
        parse(h).expect("valid hex").rgb
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-5
    }

    /// `[hex, hsv, hsl, cmyk]` as the web computes them.
    /// `(hex, hsv, hsl, cmyk)`.
    type Conversion = (&'static str, [f64; 3], [f64; 3], [f64; 4]);
    const CONVERSIONS: [Conversion; 6] = [
        ("#4a90d9", [210.629371, 0.658986, 0.85098], [210.629371, 0.652968, 0.570588], [65.898618, 33.640553, 0.0, 14.901961]),
        ("#d93025", [3.666667, 0.829493, 0.85098], [3.666667, 0.708661, 0.498039], [0.0, 77.880184, 82.949309, 14.901961]),
        ("#000000", [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 100.0]),
        ("#ffffff", [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 0.0]),
        ("#16a085", [168.26087, 0.8625, 0.627451], [168.26087, 0.758242, 0.356863], [86.25, 0.0, 16.875, 37.254902]),
        ("#f9ab00", [41.204819, 1.0, 0.976471], [41.204819, 1.0, 0.488235], [0.0, 31.325301, 100.0, 2.352941]),
    ];

    #[test]
    fn conversions_match_the_web_bundle() {
        for (h, hsv, hsl, cmyk) in CONVERSIONS {
            let c = rgb(h);
            let a = rgb_to_hsv(c.r, c.g, c.b);
            assert!(near(a.h, hsv[0]) && near(a.s, hsv[1]) && near(a.v, hsv[2]), "{h} hsv {a:?}");
            let l = rgb_to_hsl(c.r, c.g, c.b);
            assert!(near(l.h, hsl[0]) && near(l.s, hsl[1]) && near(l.l, hsl[2]), "{h} hsl {l:?}");
            let k = rgb_to_cmyk(c.r, c.g, c.b);
            assert!(near(k.c, cmyk[0]) && near(k.m, cmyk[1]) && near(k.y, cmyk[2]) && near(k.k, cmyk[3]), "{h} cmyk {k:?}");
            // Every model round-trips to the same hex, as in the web.
            assert_eq!(hsv_to_rgb(a.h, a.s, a.v).to_hex(), h);
            assert_eq!(hsl_to_rgb(l.h, l.s, l.l).to_hex(), h);
            assert_eq!(cmyk_to_rgb(k.c, k.m, k.y, k.k).to_hex(), h);
        }
    }

    #[test]
    fn harmonies_match_the_web_bundle() {
        type Golden<'a> = (&'a str, [&'a [&'a str]; 6]);
        let golden: [Golden; 4] = [
            (
                "#4a90d9",
                [
                    &["#4a90d9", "#d9934a"],
                    &["#4ad7d9", "#4a90d9", "#4c4ad9"],
                    &["#4a90d9", "#d94a90", "#90d94a"],
                    &["#4a90d9", "#d94ad7", "#d9934a", "#4ad94c"],
                    &["#4a90d9", "#d94c4a", "#d7d94a"],
                    &["#214162", "#4a90d9", "#b3d8ff"],
                ],
            ),
            (
                "#d93025",
                [
                    &["#d93025", "#25ced9"],
                    &["#d92574", "#d93025", "#d98a25"],
                    &["#d93025", "#25d930", "#3025d9"],
                    &["#d93025", "#74d925", "#25ced9", "#8a25d9"],
                    &["#d93025", "#25d98a", "#2574d9"],
                    &["#621611", "#d93025", "#ffa6a0"],
                ],
            ),
            (
                "#ffffff",
                [&["#ffffff"; 2], &["#ffffff"; 3], &["#ffffff"; 3], &["#ffffff"; 4], &["#ffffff"; 3], &["#737373", "#ffffff", "#ffe0e0"]],
            ),
            (
                "#f9ab00",
                [
                    &["#f9ab00", "#004ef9"],
                    &["#f92e00", "#f9ab00", "#cbf900"],
                    &["#f9ab00", "#00f9ab", "#ab00f9"],
                    &["#f9ab00", "#00f92e", "#004ef9", "#f900cb"],
                    &["#f9ab00", "#00cbf9", "#2e00f9"],
                    &["#704d00", "#f9ab00", "#ffdb8c"],
                ],
            ),
        ];
        for (h, schemes) in golden {
            let c = rgb(h);
            let base = rgb_to_hsv(c.r, c.g, c.b);
            for (i, want) in schemes.iter().enumerate() {
                let got: Vec<String> = harmony_colors(Scheme::ALL[i], base.h, base.s, base.v).into_iter().map(hex).collect();
                assert_eq!(got, want.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "{h} {:?}", Scheme::ALL[i]);
            }
        }
    }

    #[test]
    fn the_channel_models_show_the_webs_values() {
        let mut p = ColorPicker::new(parse("#4a90d9").expect("hex"));
        let shown = |p: &ColorPicker| p.channels().iter().map(|c| js_round(c.value)).collect::<Vec<_>>();
        assert_eq!(shown(&p), [74.0, 144.0, 217.0]);
        p.mode = ColorMode::Hsv;
        assert_eq!(shown(&p), [211.0, 66.0, 85.0]);
        p.mode = ColorMode::Hsl;
        assert_eq!(shown(&p), [211.0, 65.0, 57.0]);
        p.mode = ColorMode::Cmyk;
        assert_eq!(shown(&p), [66.0, 34.0, 0.0, 15.0]);
        p.mode = ColorMode::Gray;
        // `gy = round((74 + 144 + 217) / 3) = 145`, shown as `145 / 255 * 100`.
        assert_eq!(shown(&p), [57.0]);
    }

    #[test]
    fn setting_a_channel_goes_through_the_webs_closure() {
        let mut p = ColorPicker::new(parse("#4a90d9").expect("hex"));
        p.set_channel(0, 255.0);
        assert_eq!(p.hex_text(), "FF90D9");
        p.mode = ColorMode::Gray;
        p.set_channel(0, 50.0);
        // `gg = round(0.5 * 255) = 128`.
        assert_eq!(p.hex_text(), "808080");
        p.mode = ColorMode::Cmyk;
        p.set_channel(3, 100.0);
        assert_eq!(p.hex_text(), "000000");
        p.mode = ColorMode::Hsv;
        p.set_hsv(0.0, 1.0, 1.0);
        p.set_channel(0, 120.0);
        assert_eq!(p.hex_text(), "00FF00");
    }

    #[test]
    fn the_layout_is_the_webs_to_the_half_dip() {
        let mut p = ColorPicker::new(parse("#4a90d9").expect("hex"));
        p.eyedropper = false;
        p.recent = (0..12).map(|_| Color::default()).collect();
        let g = p.layout(Rect::new(0.0, 0.0, pm::WIDTH, 1000.0));
        // Measured on the live component (`getBoundingClientRect`).
        assert_eq!((g.close.left, g.close.top, g.close.right), (282.0, 13.0, 299.0));
        assert_eq!((g.shapes[1].left, g.shapes[1].top), (13.0, 73.5));
        assert_eq!((g.wheel.left, g.wheel.top, g.wheel.right, g.wheel.bottom), (51.0, 37.5, 261.0, 249.5));
        assert_eq!((g.sv.left, g.sv.top, g.sv.right), (102.0, 88.5, 212.0));
        assert_eq!((g.schemes[5].left, g.schemes[5].top), (267.0, 217.5));
        assert_eq!((g.harmony[1].left, g.harmony[1].top, g.harmony[1].right), (158.0, 259.5, 299.0));
        assert_eq!((g.hex.top, g.preview.right), (291.5, 41.0));
        assert_eq!(g.tabs[0].top, 325.5);
        assert_eq!((g.channels[0].top, g.channels[2].top), (353.5, 405.5));
        let track = PickerLayout::track(g.channels[0]);
        assert_eq!((track.left, track.top, track.right), (33.0, 357.5, 247.0));
        assert_eq!(PickerLayout::channel_box(g.channels[0]).left, 255.0);
        assert_eq!(g.swatches.top, 435.5);
        assert_eq!((g.recent_label.top, g.recent.top), (472.5, 493.5));
        assert_eq!(g.recent_grid.cell, 25.0);
        assert_eq!(p.height_for_width(pm::WIDTH), 560.5);
        // The R thumb of `#4a90d9`: `calc(74/255 * 100% - 1.5px)` in the
        // track's padding box, 2 DIP taller than it at each end.
        let thumb = PickerLayout::thumb(g.channels[0], 74.0, 255.0);
        assert!((thumb.left - 94.02).abs() < 0.05 && thumb.top == 356.5 && thumb.bottom == 370.5, "{}", thumb.left);
    }

    #[test]
    fn the_sv_mapping_inverts_its_handle_in_every_shape() {
        for shape in SvShape::ALL {
            let size = ColorPicker::sv_size(shape);
            for (s, v) in [(0.5, 0.5), (0.2, 0.9), (0.9, 0.3), (1.0, 1.0)] {
                let (x, y) = sv_handle_pos(shape, size, s, v);
                let half = size as f64 / 2.0;
                if shape == SvShape::Circle && (x - half).hypot(y - half) >= half - 1e-9 {
                    continue; // clamped onto the rim: not invertible
                }
                let (bs, bv) = sv_from_point(shape, size, x, y);
                assert!(near(bs, s) && near(bv, v), "{shape:?} ({s}, {v}) -> ({bs}, {bv})");
            }
        }
        // Outside the triangle, a point clamps to its nearest edge.
        let size = ColorPicker::sv_size(SvShape::Triangle);
        let (s, v) = sv_from_point(SvShape::Triangle, size, -50.0, -50.0);
        assert!((0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&v));
    }

    #[test]
    fn the_ring_reads_its_hue_clockwise_from_the_top() {
        let wheel = Rect::new(0.0, 0.0, 212.0, 212.0);
        assert!(near(hue_from_point(wheel, 106.0, 0.0), 0.0));
        assert!(near(hue_from_point(wheel, 212.0, 106.0), 90.0));
        assert!(near(hue_from_point(wheel, 106.0, 212.0), 180.0));
        assert!(near(hue_from_point(wheel, 0.0, 106.0), 270.0));
        let (x, y) = ColorPicker::ring_point(wheel, 90.0);
        assert!((x - 201.0).abs() < 1e-3 && (y - 106.0).abs() < 1e-3);
        assert!(ColorPicker::on_ring(wheel, 106.0, 5.0));
        assert!(!ColorPicker::on_ring(wheel, 106.0, 106.0), "the inner disc is not the ring");
    }

    #[test]
    fn the_ring_keys_wrap_and_the_area_keys_clamp() {
        let mut p = ColorPicker::new(parse("#ff0000").expect("hex"));
        let b = Rect::new(0.0, 0.0, pm::WIDTH, 600.0);
        assert!(p.key(b, PickerPart::Ring, ColorKey::Left, false));
        assert!(near(p.hsv.h, 359.0));
        p.key(b, PickerPart::Ring, ColorKey::Up, true);
        assert!(near(p.hsv.h, 9.0));
        p.key(b, PickerPart::Area, ColorKey::Right, true);
        assert!(near(p.hsv.s, 1.0));
        p.key(b, PickerPart::Area, ColorKey::Down, false);
        assert!(near(p.hsv.v, 0.98));
        p.mode = ColorMode::Rgb;
        let g0 = js_round(p.channels()[1].value);
        p.key(b, PickerPart::Channel(1), ColorKey::Right, true);
        assert_eq!(js_round(p.channels()[1].value), g0 + 10.0);
    }

    #[test]
    fn a_click_routes_to_the_part_under_it() {
        let mut p = ColorPicker::new(parse("#4a90d9").expect("hex"));
        p.recent = vec![Color::default(); 3];
        let b = Rect::new(0.0, 0.0, pm::WIDTH, p.height_for_width(pm::WIDTH));
        assert_eq!(p.part_at(b, 29.0, 89.5), Some(PickerPart::Shape(SvShape::Triangle)));
        assert_eq!(p.part_at(b, 283.0, 53.5), Some(PickerPart::Scheme(Scheme::Comp)));
        assert_eq!(p.part_at(b, 156.0, 45.0), Some(PickerPart::Ring));
        assert_eq!(p.part_at(b, 156.0, 140.0), Some(PickerPart::Area));
        assert_eq!(p.part_at(b, 80.0, 143.0), None, "the inner disc takes no part");
        assert_eq!(p.part_at(b, 60.0, 336.0), Some(PickerPart::Mode(ColorMode::Hsv)));
        assert_eq!(p.part_at(b, 100.0, 363.5), Some(PickerPart::Channel(0)));
        assert_eq!(p.part_at(b, 270.0, 389.5), Some(PickerPart::ChannelBox(1)));
        assert_eq!(p.activate(PickerPart::Shape(SvShape::Circle)), PickerEvent::None);
        assert_eq!(p.shape, SvShape::Circle);
        assert_eq!(p.activate(PickerPart::Close), PickerEvent::Close);
        assert_eq!(p.activate(PickerPart::Recent(1)), PickerEvent::History(1));
    }

    #[test]
    fn a_typed_hex_or_channel_applies_like_the_web() {
        let mut p = ColorPicker::new(parse("#4a90d9").expect("hex"));
        assert!(p.apply_hex("abc"));
        assert_eq!(p.hex_text(), "AABBCC");
        assert!(!p.apply_hex("abcd"), "four digits are ignored");
        p.begin_edit(PickerPart::ChannelBox(2));
        if let Some((_, d)) = p.edit.as_mut() {
            d.select_all();
            d.insert("300");
        }
        p.apply_edit();
        // Clamped to the channel's max, 255.
        assert_eq!(p.hex_text(), "AABBFF");
    }

    #[test]
    fn a_grey_keeps_its_hue_when_the_prop_comes_back() {
        let mut p = ColorPicker::new(parse("#4a90d9").expect("hex"));
        p.set_hsv(200.0, 0.0, 0.5);
        let grey = p.color();
        p.set_color(grey);
        assert!(near(p.hsv.h, 200.0), "the web keeps its HSV when the hex already matches");
    }

    #[test]
    fn the_raster_samples_the_webs_formulas() {
        use super::super::raster::{sample, Raster};
        let sq = Raster::Sv { h: 210.0, shape: SvShape::Square, radius: 0.0 };
        assert_eq!(sample(sq, 0.0, 0.0, 110.0, 110.0).map(|c| c.to_hex()), Some("#ffffff".to_string()));
        assert_eq!(sample(sq, 109.99, 109.99, 110.0, 110.0).map(|c| c.to_hex()), Some("#000000".to_string()));
        let ring = Raster::Ring { ring: 22.0 };
        assert_eq!(sample(ring, 106.0, 2.0, 212.0, 212.0).map(|c| c.to_hex()), Some("#ff0000".to_string()));
        assert!(sample(ring, 106.0, 106.0, 212.0, 212.0).is_none());
        let tri = Raster::Sv { h: 0.0, shape: SvShape::Triangle, radius: 0.0 };
        assert!(sample(tri, 2.0, 2.0, 162.0, 162.0).is_none(), "outside the triangle");
    }

    #[test]
    fn a_gradient_reads_back_what_the_web_writes() {
        use crate::color::{Gradient, GradientKind};
        // `gradientToCss` of the web gallery's four stops, verbatim from the page.
        let css = "linear-gradient(120deg, rgba(74, 144, 217, 1) 0%, rgba(22, 160, 133, 1) 35%, rgba(249, 171, 0, 0.45) 70%, rgba(155, 89, 182, 1) 100%)";
        let g = Gradient::from_css(css).expect("parses");
        assert_eq!(g.kind, GradientKind::Linear);
        assert_eq!(g.stops.len(), 4);
        assert!(near(g.stops[2].opacity, 45.0));
        assert_eq!(g.to_css(), css);
        let radial = "radial-gradient(circle, rgba(0, 0, 0, 1) 0%, rgba(255, 255, 255, 0.5) 100%)";
        assert_eq!(Gradient::from_css(radial).map(|g| g.to_css()).as_deref(), Some(radial));
        assert!(Gradient::from_css("linear-gradient(90deg, #fff 0%)").is_none(), "one stop is not a gradient");
        assert!(Gradient::from_css("conic-gradient(red, blue)").is_none());
    }

    #[test]
    fn the_quick_picker_ends_with_plus_then_the_eyedropper() {
        let mut s = crate::color::SwatchPicker::new();
        s.custom = vec![Color::default(); 3];
        let n = s.colors.len();
        assert_eq!(s.add_index(), n + 3);
        assert_eq!(s.eyedropper_index(), Some(n + 4));
        assert_eq!(s.cell_count(), n + 5);
        assert!(s.colour_at(n + 3).is_none() && s.colour_at(n + 4).is_none());
        s.eyedropper = false;
        assert_eq!(s.eyedropper_index(), None);
        assert_eq!(s.cell_count(), n + 4);
    }
}
