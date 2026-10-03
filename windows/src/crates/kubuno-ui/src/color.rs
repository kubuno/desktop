//! Kubuno primitives — the colour family.
//!
//! [`ColorField`], [`ColorPicker`], [`SwatchPicker`] and [`GradientPicker`]:
//! everything that names a colour.
//!
//! ## Why this family has no WinForms replica to own
//!
//! The rest of this crate is built on `kubuno_controls`, which reproduces the
//! .NET control surface. WinForms has **no colour control**: it has
//! `ColorDialog`, a modal window put up by the common-dialog API, whose only
//! property is the chosen `Color`. There is nothing to inherit a model from.
//!
//! So the reference here is the **web only** —
//! `core/frontend/src/ui/ColorPicker.tsx`, `ColorField.tsx`,
//! `ColorSwatchPicker.tsx`, `GradientPicker.tsx`, and the two pure modules
//! `color.ts` and `gradient.ts` — and the rule the brief actually cares about
//! (never restate a model that already exists) is honoured by **wrapping what
//! can be wrapped**:
//!
//! | part | what carries it |
//! |---|---|
//! | the swatch button | [`kubuno_controls::buttons::Button`] |
//! | every panel surface | [`kubuno_controls::containers::Panel`] |
//! | the gradient's angle and opacity sliders | [`crate::range::Slider`], itself a `TrackBar` |
//! | a circular swatch's hit target | [`crate::buttons::circular_hit`] |
//!
//! What is left had to be **written new**, and is called out as such where it
//! appears:
//!
//! * the **saturation/value area** — a two-dimensional range. No toolkit in
//!   this codebase has one, so its mapping, its handle and its hit-testing are
//!   new code, ported term for term from `SvArea` in `ColorPicker.tsx`;
//! * the **colour-space arithmetic** — [`rgb_to_hsv`], [`hsv_to_rgb`],
//!   [`rgb_to_hsl`], [`hsl_to_rgb`], [`rgb_to_cmyk`], [`cmyk_to_rgb`], the
//!   notation parser and the hex/`rgba()` serialisers. These are ports of
//!   `core/frontend/src/ui/color.ts` and `gradient.ts`, **rounding included**:
//!   the arithmetic runs in `f64` (JavaScript has no other number type) and
//!   rounds through [`js_round`], because `Math.round` breaks ties toward `+∞`
//!   while Rust's `f64::round` breaks them away from zero. A conversion that
//!   differs by one on a channel is visible as a seam in a ramp.
//!
//! ## How the ramps are painted
//!
//! The picker (`picker.rs`) is the web's panel part for part — hue ring,
//! square / triangle / circle saturation-value area, harmony schemes, five
//! channel models, eyedropper. Its ring and SV area are per-pixel rasters
//! (`raster.rs`, the web's own `<canvas>` recipe, supersampled ×3), its
//! channel tracks and the gradient preview are Direct2D gradient brushes.
//! Without a Direct2D device (a headless test canvas) each falls back to
//! one-DIP strips ([`m::RAMP_STEP`](m)) or [`m::GRAD_CELL`](m) cells.
//!
//! ## What the web does NOT do, and is therefore absent here
//!
//! * **No contrast check.** Nothing in `core/frontend/src/ui` computes a
//!   relative luminance or a contrast ratio — the two files that mention
//!   contrast (`Callout.tsx`, `ProvenanceLine.tsx`) discuss it in prose. None
//!   is invented here.
//! * **No alpha in the picker.** The web's colour is always
//!   `#rrggbb`; opacity exists in the *gradient* model as
//!   `GradientStop.opacity`, 0..=100. This family therefore carries alpha as
//!   [`Color::opacity`], on that scale and with that name.

use std::ops::{Deref, DerefMut};

use drive_app_controls::{Canvas, Rect};
use kubuno_controls::buttons as replica_buttons;
use kubuno_controls::containers::Panel as PanelModel;
use kubuno_controls::enums::Size as ControlSize;
use kubuno_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use kubuno_controls::host::{self, vk};

use crate::buttons::circular_hit;
use crate::metrics::space;
use drive_app_controls::themes::shape::SHADOW_BLACK;
use crate::range::Slider;
use crate::widget::{Widget, WidgetState};

mod picker;
pub mod raster;

pub use picker::{
    ScreenEyedropper, bary, harmony_colors, hue_from_point, pm, sv_from_point, sv_handle_pos, triangle, Channel, ColorMode, ColorPicker,
    PickerEvent, PickerLayout, PickerPart, PickerPointer, PickerTool, Scheme, SvShape, Triangle,
};

// ═════════════════════════════════════════════════════════════════════════════
// The family's metrics.
//
// `crate::metrics` re-exports the shape tokens; what is here is what the web
// describes and the token table does not carry, each with the file and the
// utility it was read from. Nothing below is multiplied by `Canvas::scale`.
// ═════════════════════════════════════════════════════════════════════════════

/// Every number this family paints with, and where it came from.
pub mod m {
    /// A hairline: `border: 1px solid …`, on every surface in all four files.
    pub const BORDER: f32 = 1.0;

    // ── ColorField (`ColorField.tsx`) ────────────────────────────────────────

    /// `width = 32`, `height = 24` — the swatch button's defaults.
    pub const FIELD_W: f32 = 32.0;
    pub const FIELD_H: f32 = 24.0;
    /// `borderRadius: 4`.
    pub const FIELD_RADIUS: f32 = 4.0;

    // ── shared panel chrome ──────────────────────────────────────────────────

    /// `p-3` on `ColorPicker`, `ColorSwatchPicker` and `GradientPicker`.
    pub const PANEL_PAD: f32 = 12.0;
    /// `borderRadius: 4` (`ColorPicker`, `GradientPicker`).
    pub const PANEL_RADIUS: f32 = 4.0;

    /// `mt-2` / `mb-2` — the gap between two blocks of a panel.
    pub const ROW_GAP: f32 = 8.0;
    /// `mt-2.5` — the wider gap before a new group of controls.
    pub const SECTION_GAP: f32 = 10.0;
    /// `mb-1.5` — a group label to its content.
    pub const LABEL_GAP: f32 = 6.0;
    /// `mb-3` — under the gradient's preview bar, which has to clear the stop
    /// markers hanging below it.
    pub const BAR_GAP: f32 = 12.0;

    /// The body line box: 14/20, the design system's only body size.
    pub const LINE: f32 = 20.0;
    /// A micro label's line box — the 12 DIP caption face at the same 1.43
    /// ratio the 14/20 body uses, rounded to a whole DIP. The web writes these
    /// at 10 px, a size the shared `TextFormats` table does not carry, so they
    /// are drawn with `caption` (12) and given its line.
    pub const MICRO_LINE: f32 = 16.0;

    // ── ColorPicker (`ColorPicker.tsx`) ──────────────────────────────────────

    /// `width:312` on the picker's outer div.
    pub const PICKER_W: f32 = 312.0;

    /// The saturation/value square: `sqSide = Math.round((SIZE - 2*RING -
    /// 12)/Math.SQRT2)` with `SIZE = 212`, `RING = 22` — the square inscribed
    /// in the hue ring's inner circle, which comes to **110**.
    ///
    /// The web's `SvArea` is `size × size`. The desktop panel stacks
    /// vertically instead of nesting the area inside a ring, so 110 is used as
    /// the area's HEIGHT and its width is the panel's content width.
    pub const SV_SIDE: f32 = 110.0;
    /// `borderRadius: shape === 'circle' ? '50%' : 2` — the square shape's 2.
    pub const SV_RADIUS: f32 = 2.0;
    /// The SV handle: `width:11, height:11`, `border: '2px solid #fff'`,
    /// `boxShadow: '0 0 0 1px rgba(0,0,0,.5)'`.
    pub const SV_HANDLE: f32 = 11.0;
    pub const HANDLE_RING: f32 = 2.0;
    pub const HANDLE_SHADOW: f32 = 1.0;

    // `ColorChan` — one labelled channel slider.

    /// `h-3` on the track.
    pub const CHAN_TRACK_H: f32 = 12.0;
    /// `borderRadius: 2` on the track and on the thumb.
    pub const CHAN_RADIUS: f32 = 2.0;
    /// `w-3` on the label span, `gap-2` between the three children,
    /// `w-11 h-5` on the numeric box.
    pub const CHAN_LABEL_W: f32 = 12.0;
    pub const CHAN_GAP: f32 = 8.0;
    pub const CHAN_BOX_W: f32 = 44.0;
    pub const CHAN_BOX_H: f32 = 20.0;
    /// The thumb: `width:3`, `top:-2 bottom:-2`, i.e. 2 DIP taller than the
    /// track at each end.
    pub const CHAN_THUMB_W: f32 = 3.0;
    pub const CHAN_THUMB_OVER: f32 = 2.0;
    /// The row's height is its tallest child — the `h-5` numeric box.
    pub const CHAN_ROW_H: f32 = CHAN_BOX_H;
    /// `space-y-1.5` between two channel rows.
    pub const CHAN_GAP_Y: f32 = 6.0;

    // The preview + hex row.

    /// `width:28, height:24, borderRadius:2` on the preview chip.
    pub const PREVIEW_W: f32 = 28.0;
    pub const PREVIEW_H: f32 = 24.0;
    pub const PREVIEW_RADIUS: f32 = 2.0;
    /// `h-6` on the hex input, `borderRadius: 2`.
    pub const HEX_H: f32 = 24.0;
    /// The `#` label's column.
    ///
    /// **A decision, not a measurement**: the web sizes that span by its own
    /// glyph, which a pure layout function — one that takes a rectangle and no
    /// canvas, so hit-testing and painting cannot disagree — has no way to
    /// measure. One step of the spacing scale is used instead, and it is
    /// written here so the decision is visible rather than hidden in a paint
    /// body.
    pub const HASH_W: f32 = super::space::SM;

    /// The fixed swatch row: `width:16, height:16, borderRadius:2`, `gap-1`.
    pub const SWATCH: f32 = 16.0;
    pub const SWATCH_RADIUS: f32 = 2.0;
    pub const GRID_GAP: f32 = 4.0;

    /// Recently used colours: `gridTemplateColumns:'repeat(10, 1fr)'`,
    /// `aspect-square`, `borderRadius: 3`, `history.slice(0, 30)`.
    pub const RECENT_COLS: usize = 10;
    pub const RECENT_RADIUS: f32 = 3.0;
    pub const RECENT_MAX: usize = 30;

    // ── SwatchPicker (`ColorSwatchPicker.tsx`) ───────────────────────────────

    /// `width: 232`, `rounded-lg` (8), 10 columns of `aspect-square`
    /// `rounded-full` buttons with `gap-1`.
    pub const SWATCHES_W: f32 = 232.0;
    pub const SWATCHES_RADIUS: f32 = 6.0;
    /// `mt-3 mb-1` around the « Personnalisé » caption, whose line box is
    /// 11 px × 1.5.
    pub const CUSTOM_GAP_TOP: f32 = 12.0;
    pub const CUSTOM_LABEL_H: f32 = 16.5;
    pub const CUSTOM_GAP_BOTTOM: f32 = 4.0;
    /// `Plus size={12}`, `Pipette size={11}` in the custom row.
    pub const PLUS_ICON: f32 = 12.0;
    pub const SWATCHES_COLS: usize = 10;
    /// `boxShadow: '0 0 0 2px #1a73e8'` on the selected swatch — a 2 DIP ring
    /// in the accent, drawn OUTSIDE the circle as a box-shadow spread is.
    pub const SELECT_RING: f32 = 2.0;

    // ── GradientPicker (`GradientPicker.tsx`) ────────────────────────────────

    /// `width: 260`.
    pub const GRAD_W: f32 = 260.0;
    /// The preview bar: `height: 22`, `borderRadius: 3`.
    pub const BAR_H: f32 = 22.0;
    pub const BAR_RADIUS: f32 = 3.0;
    /// The transparency chequer: `repeating-conic-gradient(#bbb 0% 25%, #fff
    /// 0% 50%)` at `background-size: 10px 10px` — a 2×2 board per 10 px tile,
    /// so each square is 5.
    pub const CHEQUER: f32 = 5.0;
    /// A stop marker: `width:12, height:12`, `borderRadius:2`, `border: 2px`,
    /// `left: calc(pos% - 6px)`, `-bottom-1` (4 DIP below the bar).
    pub const STOP: f32 = 12.0;
    pub const STOP_RADIUS: f32 = 2.0;
    pub const STOP_RING: f32 = 2.0;
    pub const STOP_DROP: f32 = 4.0;
    /// The `linear` / `radial` buttons: `px-2 py-0.5 text-[10px]`,
    /// `borderRadius: 3` — 10 over `py-0.5` twice is a 20 DIP box.
    pub const TYPE_BTN_H: f32 = 21.0;
    /// The two type buttons' widths as the web lays them out (their 10 px
    /// label, `px-2`, the border).
    pub const TYPE_W: [f32; 2] = [55.36, 47.42];
    pub const TYPE_BTN_PAD: f32 = 8.0;
    pub const TYPE_RADIUS: f32 = 3.0;
    pub const TYPE_GAP: f32 = 4.0;
    /// `width: 48` on the `Angle` / `Opacité` labels, `gap-2` beside them.
    pub const GRAD_LABEL_W: f32 = 48.0;
    /// `w-14` (56) on the angle and opacity boxes, `w-12` (48) on the position
    /// box, all `py-0.5` over a 10 px line.
    pub const NUM_W: f32 = 56.0;
    pub const NUM_SMALL_W: f32 = 48.0;
    pub const NUM_H: f32 = 22.5;
    /// The « Position » caption's width before its box (`gap-1`).
    pub const POS_LABEL_W: f32 = 48.0;
    /// `px-1.5 py-1 text-[10px] rounded` — the « add a stop » button.
    pub const ADD_H: f32 = 23.0;
    /// The « add a stop » button's width. **A decision**: the web sizes it to
    /// its content (px-1.5, a Plus size={11}, gap-1, the label); a
    /// canvas-free layout cannot measure the label, so this holds « Ajouter un
    /// arrêt » at the caption size with that padding. A longer label ellipsizes.
    pub const ADD_W: f32 = 99.8;
    /// `Trash2 size={13}` / `Plus size={11}` / `size={13}` — the glyphs.
    pub const ICON_SM: f32 = 11.0;
    pub const ICON_MD: f32 = 13.0;
    /// `pt-2` above the selected-stop editor, under its rule.
    pub const RULE_GAP: f32 = 8.0;

    // ── Floating surfaces and interaction ────────────────────────────────────

    /// `const M = 8` in `ColorField.tsx`'s `reposition()` — the gap between
    /// the swatch and its popover, and the margin the popover keeps from every
    /// viewport edge.
    pub const POPOVER_MARGIN: f32 = 8.0;
    /// How far `SHADOW_MENU` reaches past a panel's edge: its widest layer is
    /// `dy 2 + blur 6 + spread 2`. A host popup that carries a panel must be
    /// this much larger on every side, or the shadow is cut.
    pub const SHADOW_MARGIN: f32 = 10.0;
    /// `focus-visible:ring-2` on the SV area, the channel tracks and the hue
    /// ring — a 2 DIP ring drawn OUTSIDE the element, as a box-shadow is.
    pub const FOCUS_RING: f32 = 2.0;
    /// `hover:scale-110` on a round swatch: the chip grows by 10 %, i.e. by
    /// 5 % of its side on every edge.
    pub const HOVER_GROW: f32 = 0.05;
    /// `SvArea`'s arrow keys: `st = e.shiftKey ? 0.1 : 0.02`.
    pub const SV_STEP: f64 = 0.02;
    pub const SV_STEP_BIG: f64 = 0.1;
    /// `ColorChan`'s arrow keys: `st = e.shiftKey ? 10 : 1`.
    pub const CHAN_STEP: f64 = 1.0;
    pub const CHAN_STEP_BIG: f64 = 10.0;
    /// A gradient stop nudged from the keyboard: one percent, ten with Shift —
    /// the same 1 / 10 pair `ColorChan` uses, on the 0..=100 scale the
    /// position box shows. A desktop addition: the web's markers are `<div>`s
    /// with no `tabIndex`.
    pub const STOP_STEP: f64 = 0.01;
    pub const STOP_STEP_BIG: f64 = 0.1;
    /// The optional Cancel / Add footer (`ColorPicker.tsx`, `onConfirm` /
    /// `onCancel`): `mt-3 pt-2.5` above it, `gap-2` between the buttons,
    /// `px-3 h-7` on each.
    pub const FOOTER_GAP_TOP: f32 = 12.0;
    pub const FOOTER_PAD_TOP: f32 = 10.0;
    pub const FOOTER_BTN_H: f32 = 28.0;
    pub const FOOTER_BTN_PAD: f32 = 12.0;
    pub const FOOTER_BTN_GAP: f32 = 8.0;
    /// The caret of an edited box: one DIP wide, the text line tall.
    pub const CARET_W: f32 = 1.0;
    /// The horizontal inset of the text inside the hex box (`px-2`) and inside
    /// a numeric box (`px-1.5`).
    pub const HEX_PAD_X: f32 = 8.0;
    pub const NUM_PAD_X: f32 = 6.0;

    // ── Rendering steps forced by the Canvas (see the module docs) ───────────

    /// One DIP per strip in a one-dimensional ramp.
    pub const RAMP_STEP: f32 = 1.0;
    /// The cell of the two-dimensional gradient preview. Two DIP keeps a 260 ×
    /// 22 bar at ~1400 fills; one DIP would be ~5700 for a difference no
    /// larger than the chequer squares behind it.
    pub const GRAD_CELL: f32 = 2.0;
}

/// The same colour at a fraction of its alpha — how this design system dims.
fn fade(c: D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * alpha, ..c }
}

/// Tailwind's `shadow-lg`: `0 10px 15px -3px rgb(0 0 0 / .1), 0 4px 6px -4px
/// rgb(0 0 0 / .1)` — `ColorSwatchPicker`'s panel.
pub const SHADOW_LG: [drive_app_controls::themes::shape::ShadowLayer; 2] = [
    drive_app_controls::themes::shape::ShadowLayer { dy: 10.0, blur: 15.0, spread: -3.0, opacity: 0.1 },
    drive_app_controls::themes::shape::ShadowLayer { dy: 4.0, blur: 6.0, spread: -4.0, opacity: 0.1 },
];

/// `disabled:opacity-50`, the alpha the rest of the crate dims with.
const DISABLED_ALPHA: f32 = 0.5;

// ═════════════════════════════════════════════════════════════════════════════
// Colour arithmetic — a port of `core/frontend/src/ui/color.ts`
//
// Channels, in the source's own words: « RGB 0..255, H 0..360, S/V/L 0..1,
// CMYK 0..100 ». Everything is `f64` because every JavaScript number is, and
// the rounding the browser performs is reproduced rather than approximated.
// ═════════════════════════════════════════════════════════════════════════════

/// `Math.round`, which is **not** [`f64::round`].
///
/// JavaScript rounds a tie toward `+∞` (`Math.round(-0.5)` is `-0`); Rust
/// rounds a tie away from zero (`(-0.5f64).round()` is `-1`). Every rounding
/// in `color.ts` and `gradient.ts` goes through this, so a channel can never
/// come out one unit from what the browser shows.
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// A colour in sRGB, channels 0..=255 and **not rounded** — exactly what the
/// web's converters hand back, so a value survives a round trip through HSV
/// without being quantised twice.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

/// HSV: `h` 0..360, `s`/`v` 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hsv {
    pub h: f64,
    pub s: f64,
    pub v: f64,
}

/// HSL: `h` 0..360, `s`/`l` 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hsl {
    pub h: f64,
    pub s: f64,
    pub l: f64,
}

/// CMYK, all four 0..100.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cmyk {
    pub c: f64,
    pub m: f64,
    pub y: f64,
    pub k: f64,
}

impl Rgb {
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    /// The three channels as the web writes them into a hex string:
    /// `Math.max(0, Math.min(255, Math.round(v)))`, in that order — rounded
    /// first, clamped after.
    pub fn channels(self) -> (u8, u8, u8) {
        let ch = |v: f64| js_round(v).clamp(0.0, 255.0) as u8;
        (ch(self.r), ch(self.g), ch(self.b))
    }

    /// `rgbToHex` — lowercase, always six digits, always a leading `#`.
    pub fn to_hex(self) -> String {
        let (r, g, b) = self.channels();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// The colour a [`Canvas`] paints with, at `alpha`.
    ///
    /// Built from the ROUNDED channels, because the web only ever hands CSS a
    /// hex string: what the browser shows is the quantised colour, and this
    /// must show the same one.
    pub fn to_d2d(self, alpha: f32) -> D2D1_COLOR_F {
        let (r, g, b) = self.channels();
        D2D1_COLOR_F {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: alpha,
        }
    }
}

/// `rgbToHsl`, term for term.
pub fn rgb_to_hsl(r: f64, g: f64, b: f64) -> Hsl {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if max != min {
        let d = max - min;
        s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h *= 60.0;
    }
    Hsl { h, s, l }
}

/// `hue2rgb`, the private helper `hslToRgb` leans on.
fn hue2rgb(p: f64, q: f64, mut tt: f64) -> f64 {
    if tt < 0.0 {
        tt += 1.0;
    }
    if tt > 1.0 {
        tt -= 1.0;
    }
    if tt < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * tt;
    }
    if tt < 1.0 / 2.0 {
        return q;
    }
    if tt < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - tt) * 6.0;
    }
    p
}

/// `hslToRgb`, term for term — including the `s === 0` short circuit, which
/// returns a grey of `l * 255` without going through `hue2rgb` at all.
pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Rgb {
    let h = ((h % 360.0) + 360.0) % 360.0 / 360.0;
    if s == 0.0 {
        let v = l * 255.0;
        return Rgb::new(v, v, v);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    Rgb::new(
        hue2rgb(p, q, h + 1.0 / 3.0) * 255.0,
        hue2rgb(p, q, h) * 255.0,
        hue2rgb(p, q, h - 1.0 / 3.0) * 255.0,
    )
}

/// `rgbToHsv`, term for term.
///
/// The `% 6` on the red branch is JavaScript's remainder, which keeps the sign
/// of the dividend — the same operator Rust's `%` is on floats — and the
/// `if (h < 0) h += 360` that follows is what makes magenta come out at 300
/// rather than at −60.
pub fn rgb_to_hsv(r: f64, g: f64, b: f64) -> Hsv {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let mut h = 0.0;
    if d != 0.0 {
        h = if max == r {
            ((g - b) / d) % 6.0
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }
    }
    let s = if max == 0.0 { 0.0 } else { d / max };
    Hsv { h, s, v: max }
}

/// `hsvToRgb`, term for term.
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> Rgb {
    let h = ((h % 360.0) + 360.0) % 360.0;
    let c = v * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Rgb::new((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0)
}

/// `rgbToCmyk` — including the `k >= 1` short circuit, which returns pure
/// black rather than dividing by zero.
pub fn rgb_to_cmyk(r: f64, g: f64, b: f64) -> Cmyk {
    let (rr, gg, bb) = (r / 255.0, g / 255.0, b / 255.0);
    let k = 1.0 - rr.max(gg).max(bb);
    if k >= 1.0 {
        return Cmyk { c: 0.0, m: 0.0, y: 0.0, k: 100.0 };
    }
    Cmyk {
        c: (1.0 - rr - k) / (1.0 - k) * 100.0,
        m: (1.0 - gg - k) / (1.0 - k) * 100.0,
        y: (1.0 - bb - k) / (1.0 - k) * 100.0,
        k: k * 100.0,
    }
}

/// `cmykToRgb`.
pub fn cmyk_to_rgb(c: f64, m: f64, y: f64, k: f64) -> Rgb {
    let (c, m, y, k) = (c / 100.0, m / 100.0, y / 100.0, k / 100.0);
    Rgb::new(
        255.0 * (1.0 - c) * (1.0 - k),
        255.0 * (1.0 - m) * (1.0 - k),
        255.0 * (1.0 - y) * (1.0 - k),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Color — an RGB triple with the gradient model's opacity
// ─────────────────────────────────────────────────────────────────────────────

/// A colour and its opacity.
///
/// `opacity` is on the **0..=100** scale and carries that name because that is
/// what the web's gradient model uses (`GradientStop.opacity`, serialised by
/// `rgbaFromHex` as `opacity/100`). The picker's own colour is always fully
/// opaque on the web; the desktop adds an alpha slider, and this is the field
/// it moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub rgb: Rgb,
    /// 0..=100.
    pub opacity: f64,
}

impl Default for Color {
    /// Opaque black — the first swatch of both palettes.
    fn default() -> Self {
        Self { rgb: Rgb::new(0.0, 0.0, 0.0), opacity: 100.0 }
    }
}

impl Color {
    pub const fn new(rgb: Rgb, opacity: f64) -> Self {
        Self { rgb, opacity }
    }

    /// An opaque colour.
    pub const fn opaque(rgb: Rgb) -> Self {
        Self { rgb, opacity: 100.0 }
    }

    /// Parses a `#rrggbb` literal, panicking on a malformed one — for the
    /// palette constants below, which are checked by a test rather than at
    /// run time.
    fn lit(hex: &str) -> Self {
        // The fallback is only reachable from the palette constants below,
        // whose spelling a unit test pins; user input never comes through
        // here — it goes through `parse` and gets a `None` it can act on.
        parse(hex).unwrap_or_default()
    }

    /// `rgbaFromHex(hex, opacity)`: `rgba(r, g, b, a)` with `a =
    /// clamp(opacity, 0, 100) / 100`.
    pub fn to_css(self) -> String {
        let (r, g, b) = self.rgb.channels();
        let a = self.opacity.clamp(0.0, 100.0) / 100.0;
        format!("rgba({r}, {g}, {b}, {a})")
    }

    /// The hex the web compares swatches by. Opacity does not take part —
    /// the web's own test is `c.toLowerCase() === hex.toLowerCase()` on a
    /// six-digit string.
    pub fn same_swatch(self, other: Color) -> bool {
        self.rgb.channels() == other.rgb.channels()
    }

    /// The colour a [`Canvas`] paints with, opacity included.
    pub fn to_d2d(self) -> D2D1_COLOR_F {
        self.rgb.to_d2d((self.opacity.clamp(0.0, 100.0) / 100.0) as f32)
    }

    pub fn to_hsv(self) -> Hsv {
        rgb_to_hsv(self.rgb.r, self.rgb.g, self.rgb.b)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Parsing
// ─────────────────────────────────────────────────────────────────────────────

/// The hex field of `ColorPicker.tsx`, reproduced exactly.
///
/// ```text
/// let v = e.target.value.trim().replace(/^#/,'')
/// if (/^[0-9a-fA-F]{3}$/.test(v)) v = v.split('').map(c=>c+c).join('')
/// if (/^[0-9a-fA-F]{6}$/.test(v)) { …accept… }
/// ```
///
/// So: trimmed, **one** optional leading `#`, then three digits (each doubled)
/// or six. Anything else — four digits, eight digits, a `rgb(…)` string — is
/// silently ignored by the web, and is `None` here. Alpha cannot be expressed:
/// the result is always opaque.
pub fn parse_web_hex(text: &str) -> Option<Rgb> {
    let v = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if !v.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let six = match v.len() {
        3 => v.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => v.to_string(),
        _ => return None,
    };
    hex_triple(&six)
}

/// Reads six hex digits.
fn hex_triple(six: &str) -> Option<Rgb> {
    let byte = |i: usize| u8::from_str_radix(six.get(i..i + 2)?, 16).ok();
    Some(Rgb::new(byte(0)? as f64, byte(2)? as f64, byte(4)? as f64))
}

/// Every notation this family reads.
///
/// | notation | where it comes from |
/// |---|---|
/// | `#rgb`, `#rrggbb` (with or without the `#`) | the web's hex field — see [`parse_web_hex`] |
/// | `rgb(r, g, b)`, `rgba(r, g, b, a)` | the web's own OUTPUT: `rgbaFromHex` serialises every gradient stop this way, so a serialised gradient reads back |
/// | `#rrggbbaa` | an **addition**. The web has no notation for a colour with alpha — its picker is `#rrggbb` and its opacity lives beside the colour, in the model. The desktop's alpha slider needs one notation that carries both, and the eight-digit hex is the one CSS already defines |
/// | `hsl(h, s%, l%)`, `hsla(h, s%, l%, a)` | an **addition**, for symmetry with `rgb()`: the web parses no `hsl()` anywhere |
///
/// `#rgba`, the four-digit shorthand, is deliberately **refused**: it would be
/// a fourth hex length nothing in this codebase has ever written, and a
/// four-digit string is far more often a typo of a six-digit one.
///
/// Out-of-range numbers inside `rgb()` / `hsl()` are clamped, as CSS clamps
/// them. Anything that is not one of the notations above is `None`.
pub fn parse(text: &str) -> Option<Color> {
    let t = text.trim();
    if let Some(args) = css_call(t, "rgba").or_else(|| css_call(t, "rgb")) {
        return parse_rgb_call(&args);
    }
    if let Some(args) = css_call(t, "hsla").or_else(|| css_call(t, "hsl")) {
        return parse_hsl_call(&args);
    }
    let v = t.strip_prefix('#').unwrap_or(t);
    if !v.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match v.len() {
        3 => parse_web_hex(v).map(Color::opaque),
        6 => hex_triple(v).map(Color::opaque),
        8 => {
            let rgb = hex_triple(&v[..6])?;
            let a = u8::from_str_radix(&v[6..8], 16).ok()?;
            Some(Color::new(rgb, a as f64 / 255.0 * 100.0))
        }
        _ => None,
    }
}

/// Splits `name( a , b , c )` into its trimmed arguments, case-insensitively.
/// `None` when `text` is not a call to `name`.
fn css_call(text: &str, name: &str) -> Option<Vec<String>> {
    let lower = text.to_ascii_lowercase();
    let head = format!("{name}(");
    if !lower.starts_with(&head) || !lower.ends_with(')') {
        return None;
    }
    let inner = &text[head.len()..text.len() - 1];
    Some(inner.split(',').map(|a| a.trim().to_string()).collect())
}

/// `rgb(r, g, b)` / `rgba(r, g, b, a)`, the format `rgbaFromHex` emits.
fn parse_rgb_call(args: &[String]) -> Option<Color> {
    if args.len() != 3 && args.len() != 4 {
        return None;
    }
    let n = |i: usize| args[i].parse::<f64>().ok().filter(|v| v.is_finite());
    let rgb = Rgb::new(n(0)?.clamp(0.0, 255.0), n(1)?.clamp(0.0, 255.0), n(2)?.clamp(0.0, 255.0));
    let opacity = match args.len() {
        4 => n(3)?.clamp(0.0, 1.0) * 100.0,
        _ => 100.0,
    };
    Some(Color::new(rgb, opacity))
}

/// `hsl(h, s%, l%)` / `hsla(h, s%, l%, a)`. The percent signs are **required**
/// on `s` and `l`, as CSS's legacy comma syntax requires them.
fn parse_hsl_call(args: &[String]) -> Option<Color> {
    if args.len() != 3 && args.len() != 4 {
        return None;
    }
    let h = args[0].parse::<f64>().ok().filter(|v| v.is_finite())?;
    let pct = |i: usize| -> Option<f64> {
        let raw = args[i].strip_suffix('%')?;
        raw.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 100.0) / 100.0)
    };
    let rgb = hsl_to_rgb(h, pct(1)?, pct(2)?);
    let opacity = match args.len() {
        4 => args[3].parse::<f64>().ok().filter(|v| v.is_finite())?.clamp(0.0, 1.0) * 100.0,
        _ => 100.0,
    };
    Some(Color::new(rgb, opacity))
}

// ─────────────────────────────────────────────────────────────────────────────
// The two palettes the web ships
// ─────────────────────────────────────────────────────────────────────────────

/// `ColorPicker.tsx`'s `SWATCHES` — the twelve chips under the channel
/// sliders.
pub const PICKER_SWATCHES: [&str; 12] = [
    "#000000", "#ffffff", "#e84a4a", "#f9ab00", "#f4d03f", "#1e8e3e", "#16a085", "#4a90e8",
    "#2c3e50", "#9b51e0", "#ff7eb6", "#7f8c8d",
];

/// `ColorSwatchPicker.tsx`'s `SWATCHES` — « palette de pastilles style Google
/// Docs (10 colonnes) : gris, teintes pures, puis nuances claires → foncées »,
/// eight rows of ten.
pub const DOCS_SWATCHES: [&str; 80] = [
    "#000000", "#434343", "#666666", "#999999", "#b7b7b7", "#cccccc", "#d9d9d9", "#efefef",
    "#f3f3f3", "#ffffff", //
    "#980000", "#ff0000", "#ff9900", "#ffff00", "#00ff00", "#00ffff", "#4a86e8", "#0000ff",
    "#9900ff", "#ff00ff", //
    "#e6b8af", "#f4cccc", "#fce5cd", "#fff2cc", "#d9ead3", "#d0e0e3", "#c9daf8", "#cfe2f3",
    "#d9d2e9", "#ead1dc", //
    "#dd7e6b", "#ea9999", "#f9cb9c", "#ffe599", "#b6d7a8", "#a2c4c9", "#a4c2f4", "#9fc5e8",
    "#b4a7d6", "#d5a6bd", //
    "#cc4125", "#e06666", "#f6b26b", "#ffd966", "#93c47d", "#76a5af", "#6d9eeb", "#6fa8dc",
    "#8e7cc3", "#c27ba0", //
    "#a61c00", "#cc0000", "#e69138", "#f1c232", "#6aa84f", "#45818e", "#3c78d8", "#3d85c6",
    "#674ea7", "#a64d79", //
    "#85200c", "#990000", "#b45f06", "#bf9000", "#38761d", "#134f5c", "#1155cc", "#0b5394",
    "#351c75", "#741b47", //
    "#5b0f00", "#660000", "#783f04", "#7f6000", "#274e13", "#0c343d", "#1c4587", "#073763",
    "#20124d", "#4c1130",
];

/// The twelve chips, parsed.
pub fn picker_swatches() -> Vec<Color> {
    PICKER_SWATCHES.iter().map(|h| Color::lit(h)).collect()
}

/// The eighty chips of the quick picker, parsed.
pub fn docs_swatches() -> Vec<Color> {
    DOCS_SWATCHES.iter().map(|h| Color::lit(h)).collect()
}

// ═════════════════════════════════════════════════════════════════════════════
// Gradient — a port of `core/frontend/src/ui/gradient.ts`
// ═════════════════════════════════════════════════════════════════════════════

/// `Gradient['type']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
}

/// `GradientStop`: a colour, a position in 0..=1, an opacity in 0..=100.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub color: Rgb,
    pub position: f64,
    pub opacity: f64,
}

impl GradientStop {
    pub const fn new(color: Rgb, position: f64, opacity: f64) -> Self {
        Self { color, position, opacity }
    }

    fn as_color(self) -> Color {
        Color::new(self.color, self.opacity)
    }
}

/// A gradient: « at least 2 » stops, says the model.
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Degrees. Linear only.
    pub angle: f64,
    pub stops: Vec<GradientStop>,
}

impl Default for Gradient {
    /// `DEFAULT_GRADIENT`: linear, 90°, `#4a90d9` → `#9b59b6`, both opaque.
    fn default() -> Self {
        Self {
            kind: GradientKind::Linear,
            angle: 90.0,
            stops: vec![
                GradientStop::new(Color::lit("#4a90d9").rgb, 0.0, 100.0),
                GradientStop::new(Color::lit("#9b59b6").rgb, 1.0, 100.0),
            ],
        }
    }
}

impl Gradient {
    /// The stops in drawing order — `[...stops].sort((a,b) => a.position -
    /// b.position)`, which every part of the web's gradient code starts with.
    /// The stored order is left alone: it is what the stop indices refer to.
    pub fn sorted(&self) -> Vec<GradientStop> {
        let mut s = self.stops.clone();
        s.sort_by(|a, b| a.position.partial_cmp(&b.position).unwrap_or(std::cmp::Ordering::Equal));
        s
    }

    /// `gradientToCss` — the exact string the web hands to `background-image`.
    pub fn to_css(&self) -> String {
        let stops = self
            .sorted()
            .iter()
            .map(|s| format!("{} {}%", s.as_color().to_css(), js_round(s.position * 100.0)))
            .collect::<Vec<_>>()
            .join(", ");
        match self.kind {
            GradientKind::Radial => format!("radial-gradient(circle, {stops})"),
            GradientKind::Linear => {
                format!("linear-gradient({}deg, {stops})", js_round(self.angle))
            }
        }
    }

    /// Reads back what [`Gradient::to_css`] writes: `linear-gradient(<a>deg,
    /// <colour> <p>%, …)` or `radial-gradient(circle, <colour> <p>%, …)`,
    /// each colour in any notation [`parse`] reads (`rgba(…)` included).
    /// `None` for anything else, or for fewer than two stops.
    pub fn from_css(text: &str) -> Option<Gradient> {
        let t = text.trim();
        let lower = t.to_ascii_lowercase();
        let (kind, inner) = if lower.starts_with("linear-gradient(") && t.ends_with(')') {
            (GradientKind::Linear, &t["linear-gradient(".len()..t.len() - 1])
        } else if lower.starts_with("radial-gradient(") && t.ends_with(')') {
            (GradientKind::Radial, &t["radial-gradient(".len()..t.len() - 1])
        } else {
            return None;
        };
        // Split on the commas that are not inside a colour's parentheses.
        let mut parts = Vec::new();
        let (mut depth, mut start) = (0i32, 0usize);
        for (i, ch) in inner.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(inner[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(inner[start..].trim());
        let mut angle = 180.0;
        let mut first = 0;
        if let Some(head) = parts.first() {
            if kind == GradientKind::Linear {
                if let Some(deg) = head.strip_suffix("deg") {
                    angle = deg.trim().parse::<f64>().ok().filter(|v| v.is_finite())?;
                    first = 1;
                }
            } else if head.eq_ignore_ascii_case("circle") {
                first = 1;
            }
        }
        let mut stops = Vec::new();
        for p in &parts[first..] {
            let (colour, pos) = match p.rfind(' ') {
                Some(i) if p[i + 1..].ends_with('%') => (&p[..i], p[i + 1..].trim_end_matches('%').parse::<f64>().ok()?),
                _ => return None,
            };
            let c = parse(colour.trim())?;
            stops.push(GradientStop::new(c.rgb, (pos / 100.0).clamp(0.0, 1.0), c.opacity));
        }
        if stops.len() < 2 {
            return None;
        }
        Some(Gradient { kind, angle, stops })
    }

    /// `sampleStop` — the colour the gradient already has at `p`, so an
    /// inserted stop blends in instead of jumping.
    ///
    /// Ported with its edges intact: before the first stop and after the last
    /// one it COPIES that stop (colour and opacity) and only moves it, and the
    /// interpolated opacity is rounded while the colour is quantised by the
    /// round trip through `rgbToHex` the web performs on it.
    pub fn sample_stop(&self, p: f64) -> GradientStop {
        let s = self.sorted();
        // The model guarantees at least two stops; an empty one would be a
        // caller bug, and the web would throw on `s[0]`. Answer the requested
        // position in black rather than panicking.
        let Some(first) = s.first().copied() else {
            return GradientStop::new(Rgb::default(), p, 100.0);
        };
        if p <= first.position {
            return GradientStop { position: p, ..first };
        }
        let last = s[s.len() - 1];
        if p >= last.position {
            return GradientStop { position: p, ..last };
        }
        let mut i = 0;
        while i < s.len() - 1 && s[i + 1].position < p {
            i += 1;
        }
        let (a, b) = (s[i], s[i + 1]);
        let span = b.position - a.position;
        let t = (p - a.position) / if span == 0.0 { 1.0 } else { span };
        let mix = |x: f64, y: f64| x + (y - x) * t;
        // `rgbToHex(...)` in the web: the interpolated colour is quantised to
        // eight bits per channel on the way into the stop.
        let raw = Rgb::new(
            mix(a.color.r, b.color.r),
            mix(a.color.g, b.color.g),
            mix(a.color.b, b.color.b),
        );
        let (r, g, bl) = raw.channels();
        GradientStop {
            color: Rgb::new(r as f64, g as f64, bl as f64),
            position: p,
            opacity: js_round(mix(a.opacity, b.opacity)),
        }
    }

    /// `addStop` — appends the sampled stop and returns its index, which the
    /// web then selects (`setSel(stops.length - 1)`).
    pub fn add_stop(&mut self, p: f64) -> usize {
        self.stops.push(self.sample_stop(p));
        self.stops.len() - 1
    }

    /// `removeStop` — **refused** below three stops (`if (grad.stops.length <=
    /// 2) return`), because a gradient needs two ends. Returns whether it
    /// happened.
    pub fn remove_stop(&mut self, idx: usize) -> bool {
        if self.stops.len() <= 2 || idx >= self.stops.len() {
            return false;
        }
        self.stops.remove(idx);
        true
    }

    /// The colour at `(u, v)`, both 0..=1 inside the painted box, for a box
    /// `w × h` DIP.
    ///
    /// This is the browser's job in the web, so the CSS definitions are what is
    /// implemented: for a linear gradient the gradient LINE runs through the
    /// centre in the direction `angle` (0° points up, 90° points right) and is
    /// `|w·sin θ| + |h·cos θ|` long; for a radial one the default is a circle
    /// centred in the box reaching its `farthest-corner`.
    pub fn sample_at(&self, u: f64, v: f64, w: f64, h: f64) -> Color {
        let t = match self.kind {
            GradientKind::Linear => {
                let a = self.angle.to_radians();
                let (sin, cos) = (a.sin(), a.cos());
                let len = (w * sin).abs() + (h * cos).abs();
                if len == 0.0 {
                    0.5
                } else {
                    // Offset from the centre along the gradient line; y grows
                    // downward on a canvas, hence the minus on `cos`.
                    let (dx, dy) = ((u - 0.5) * w, (v - 0.5) * h);
                    0.5 + (dx * sin - dy * cos) / len
                }
            }
            GradientKind::Radial => {
                let (dx, dy) = ((u - 0.5) * w, (v - 0.5) * h);
                let far = ((w / 2.0).powi(2) + (h / 2.0).powi(2)).sqrt();
                if far == 0.0 {
                    0.0
                } else {
                    (dx * dx + dy * dy).sqrt() / far
                }
            }
        };
        self.sample_stop(t.clamp(0.0, 1.0)).as_color()
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Painting helpers forced by the Canvas' lack of a gradient brush
// ═════════════════════════════════════════════════════════════════════════════

/// Fills `area` with a horizontal ramp, one DIP per strip.
///
/// `colour_at` receives the strip's own fraction as the web's canvas loop
/// computes it — `x / n`, the strip's LEFT edge, not its centre.
fn ramp_x(c: &dyn Canvas, area: Rect, mut colour_at: impl FnMut(f64) -> D2D1_COLOR_F) {
    let w = area.right - area.left;
    if w <= 0.0 {
        return;
    }
    let n = (w / m::RAMP_STEP).ceil().max(1.0) as usize;
    for i in 0..n {
        let x0 = area.left + i as f32 * m::RAMP_STEP;
        let x1 = (x0 + m::RAMP_STEP).min(area.right);
        if x1 <= x0 {
            continue;
        }
        let colour = colour_at(i as f64 / n as f64);
        c.fill_rounded(&Rect::new(x0, area.top, x1, area.bottom), 0.0, &colour);
    }
}

/// Fills `area` with a vertical ramp, one DIP per strip.
fn ramp_y(c: &dyn Canvas, area: Rect, mut colour_at: impl FnMut(f64) -> D2D1_COLOR_F) {
    let h = area.bottom - area.top;
    if h <= 0.0 {
        return;
    }
    let n = (h / m::RAMP_STEP).ceil().max(1.0) as usize;
    for i in 0..n {
        let y0 = area.top + i as f32 * m::RAMP_STEP;
        let y1 = (y0 + m::RAMP_STEP).min(area.bottom);
        if y1 <= y0 {
            continue;
        }
        let colour = colour_at(i as f64 / n as f64);
        c.fill_rounded(&Rect::new(area.left, y0, area.right, y1), 0.0, &colour);
    }
}

/// The transparency chequer every surface that can show through is drawn on.
fn chequer(c: &dyn Canvas, area: Rect) {
    let t = c.theme();
    c.fill_rounded(&area, 0.0, &t.picker_chequer_a);
    let cols = ((area.right - area.left) / m::CHEQUER).ceil().max(0.0) as usize;
    let rows = ((area.bottom - area.top) / m::CHEQUER).ceil().max(0.0) as usize;
    for row in 0..rows {
        for col in 0..cols {
            if (row + col) % 2 == 0 {
                continue;
            }
            let x0 = area.left + col as f32 * m::CHEQUER;
            let y0 = area.top + row as f32 * m::CHEQUER;
            let cell = Rect::new(
                x0,
                y0,
                (x0 + m::CHEQUER).min(area.right),
                (y0 + m::CHEQUER).min(area.bottom),
            );
            c.fill_rounded(&cell, 0.0, &t.picker_chequer_b);
        }
    }
}

/// Paints a colour over the chequer, so its opacity reads.
fn swatch_fill(c: &dyn Canvas, area: Rect, radius: f32, colour: Color, alpha: f32) {
    if colour.opacity < 100.0 {
        c.push_clip_rounded(&area, radius);
        chequer(c, area);
        c.pop_clip_rounded();
    }
    c.fill_rounded(&area, radius, &fade(colour.to_d2d(), alpha));
}

/// The keyboard focus ring: `focus-visible:ring-2`, 2 DIP OUTSIDE `rect` (the
/// stroke is drawn inward, so the rectangle is grown by the ring first).
///
/// The web writes `ring-white/70` on the picker's own sliders — a ring that is
/// invisible on the light panel it sits on. The accent is used instead, the
/// colour every other focus ring of this design system wears.
fn focus_ring(c: &dyn Canvas, rect: Rect, radius: f32) {
    let ring = rect.inflate(m::FOCUS_RING, m::FOCUS_RING);
    c.stroke_rounded_w(&ring, radius + m::FOCUS_RING, &c.theme().accent, m::FOCUS_RING);
}

// ═════════════════════════════════════════════════════════════════════════════
// Keyboard — the keys the web's `onKeyDown` handlers read
// ═════════════════════════════════════════════════════════════════════════════

/// A navigation key, as the family's keyboard handlers see it.
///
/// `ColorChan`, `SvArea` and the hue ring read `ArrowLeft/Right/Up/Down`;
/// `Home` / `End` / `PageUp` / `PageDown` are the rest of the ARIA slider
/// pattern, which a native `<input type="range">` (the gradient's
/// `RangeSlider`) answers too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

impl ColorKey {
    /// The key a virtual-key code names, if it is one of these.
    pub fn from_vk(code: u16) -> Option<Self> {
        Some(match code {
            vk::LEFT => Self::Left,
            vk::RIGHT => Self::Right,
            vk::UP => Self::Up,
            vk::DOWN => Self::Down,
            vk::HOME => Self::Home,
            vk::END => Self::End,
            vk::PAGE_UP => Self::PageUp,
            vk::PAGE_DOWN => Self::PageDown,
            _ => return None,
        })
    }

    /// `ArrowRight` / `ArrowUp` grow a value, `ArrowLeft` / `ArrowDown`
    /// shrink it — `ColorChan`'s pairing.
    pub fn sign(self) -> f64 {
        match self {
            Self::Right | Self::Up | Self::PageUp | Self::End => 1.0,
            Self::Left | Self::Down | Self::PageDown | Self::Home => -1.0,
        }
    }
}

/// Consumes this frame's navigation keys — pressed alone or with Shift, the
/// only modifier the web's handlers read — and returns them with their Shift
/// flag, in the order they were pressed. Keys held with Ctrl or Alt are left
/// in the queue for whoever owns those chords.
pub fn take_color_keys() -> Vec<(ColorKey, bool)> {
    let mut out = Vec::new();
    host::consume(|e| match e {
        host::InputEvent::Key { vk: code, down: true, mods, .. } => {
            let plain = !mods.ctrl && !mods.alt && !mods.meta;
            match (plain, ColorKey::from_vk(*code)) {
                (true, Some(k)) => {
                    out.push((k, mods.shift));
                    true
                }
                _ => false,
            }
        }
        _ => false,
    });
    out
}

/// One channel's value after `key` — `ColorChan`'s `onKeyDown`, verbatim:
/// `st = shift ? 10 : 1`, Left/Down subtract and Right/Up add, clamped to
/// `0..=max`. Home / End jump to the ends and PageUp / PageDown move by ten,
/// the rest of the ARIA slider pattern.
pub fn channel_step(value: f64, max: f64, key: ColorKey, shift: bool) -> f64 {
    let st = if shift { m::CHAN_STEP_BIG } else { m::CHAN_STEP };
    let next = match key {
        ColorKey::Home => 0.0,
        ColorKey::End => max,
        ColorKey::PageUp => value + m::CHAN_STEP_BIG,
        ColorKey::PageDown => value - m::CHAN_STEP_BIG,
        k => value + k.sign() * st,
    };
    next.clamp(0.0, max)
}

// ═════════════════════════════════════════════════════════════════════════════
// FieldDraft — the text a user is typing into a hex or numeric box
// ═════════════════════════════════════════════════════════════════════════════

/// What a [`FieldDraft`] accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftKind {
    /// The hex field: hex digits, and one `#` in front.
    Hex,
    /// A numeric box (`<input type="number">` with `min={0}`): digits only.
    Digits,
}

/// The outcome of one frame of typing into a [`FieldDraft`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftOutcome {
    /// Nothing reached the box.
    Idle,
    /// The caret or the selection moved; the text did not change.
    Moved,
    /// The text changed — apply it now, the way the web's `onChange` does on
    /// every keystroke.
    Edited,
    /// Enter: the value is final.
    Commit,
    /// Escape: throw the draft away and show the model's value again.
    Cancel,
}

/// A single-line edit buffer: the text, the caret and the selection anchor.
///
/// The web's hex box is a CONTROLLED input whose `value` is the current hex:
/// a keystroke that leaves it invalid is thrown away on the next render, so a
/// user can only ever get from one valid colour to another. That makes the
/// field nearly impossible to type into (erase one digit and it springs back).
/// The desktop keeps the user's draft while the box has the focus instead,
/// applies it the moment it parses — the same live update the web's
/// `onChange` performs — and shows the model's value again on blur or Escape.
///
/// All positions are byte offsets, and every character the draft admits is
/// ASCII, so they are character offsets too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDraft {
    pub kind: DraftKind,
    pub text: String,
    pub caret: usize,
    /// The other end of the selection; equal to `caret` when there is none.
    pub anchor: usize,
    /// `host::now_ms()` at the last edit, for the caret's blink phase.
    pub last_input_ms: u64,
}

impl FieldDraft {
    /// `#` and six digits.
    pub const HEX_MAX: usize = 7;
    /// `0..=360`, the widest range a numeric box in this family carries.
    pub const DIGITS_MAX: usize = 3;

    /// A draft of `text` with ALL of it selected — what a browser does when a
    /// box receives the focus from the keyboard, so typing replaces it.
    pub fn new(kind: DraftKind, text: &str) -> Self {
        let mut d = Self { kind, text: String::new(), caret: 0, anchor: 0, last_input_ms: 0 };
        d.insert(text);
        d.anchor = 0;
        d.caret = d.text.len();
        d
    }

    fn max_len(&self) -> usize {
        match self.kind {
            DraftKind::Hex => Self::HEX_MAX,
            DraftKind::Digits => Self::DIGITS_MAX,
        }
    }

    /// The selection as `(start, end)`, `start <= end`.
    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    pub fn selected_text(&self) -> &str {
        let (a, b) = self.selection();
        self.text.get(a..b).unwrap_or("")
    }

    /// Removes the selection, if any; returns whether it did.
    fn delete_selection(&mut self) -> bool {
        let (a, b) = self.selection();
        if a == b {
            return false;
        }
        self.text.replace_range(a..b, "");
        self.caret = a;
        self.anchor = a;
        true
    }

    /// Types `s` over the selection. Characters the box does not accept are
    /// dropped (a `<input type="number">` refuses letters the same way), and
    /// the text is capped at [`FieldDraft::HEX_MAX`] / [`FieldDraft::DIGITS_MAX`].
    /// Returns whether the text changed.
    pub fn insert(&mut self, s: &str) -> bool {
        let mut changed = self.delete_selection();
        for ch in s.chars() {
            if self.text.len() >= self.max_len() {
                break;
            }
            let ok = match self.kind {
                DraftKind::Digits => ch.is_ascii_digit(),
                // One `#`, and only in front — the web strips exactly one.
                DraftKind::Hex => {
                    ch.is_ascii_hexdigit() || (ch == '#' && self.caret == 0 && !self.text.starts_with('#'))
                }
            };
            if !ok {
                continue;
            }
            self.text.insert(self.caret, ch);
            self.caret += 1;
            self.anchor = self.caret;
            changed = true;
        }
        changed
    }

    /// Backspace: the selection, or the character before the caret.
    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret == 0 {
            return false;
        }
        self.caret -= 1;
        self.text.remove(self.caret);
        self.anchor = self.caret;
        true
    }

    /// Delete: the selection, or the character after the caret.
    pub fn delete(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret >= self.text.len() {
            return false;
        }
        self.text.remove(self.caret);
        true
    }

    /// Moves the caret to `to`, extending the selection when `extend`.
    pub fn move_to(&mut self, to: usize, extend: bool) {
        self.caret = to.min(self.text.len());
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// Left arrow: without Shift a selection collapses to its START, as in
    /// every text box.
    pub fn left(&mut self, extend: bool) {
        if !extend && self.has_selection() {
            let (a, _) = self.selection();
            self.move_to(a, false);
        } else {
            self.move_to(self.caret.saturating_sub(1), extend);
        }
    }

    /// Right arrow: without Shift a selection collapses to its END.
    pub fn right(&mut self, extend: bool) {
        if !extend && self.has_selection() {
            let (_, b) = self.selection();
            self.move_to(b, false);
        } else {
            self.move_to(self.caret + 1, extend);
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    /// The text with its `#` removed — what the hex field's parser reads.
    pub fn digits(&self) -> &str {
        self.text.strip_prefix('#').unwrap_or(&self.text)
    }

    /// A numeric draft's value, `None` while it is empty.
    pub fn number(&self) -> Option<f64> {
        self.text.parse::<f64>().ok()
    }

    /// The caret index nearest to `x`, given a function measuring the width of
    /// a prefix — a click places the caret between the two characters it
    /// falls between.
    pub fn index_at(&self, x: f32, width_of: impl Fn(&str) -> f32) -> usize {
        let mut best = 0;
        let mut best_d = f32::MAX;
        for i in 0..=self.text.len() {
            let w = width_of(self.text.get(..i).unwrap_or(""));
            let d = (w - x).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    }

    /// Reads this frame's typing from the host's queue: text, Backspace,
    /// Delete, the arrows (Shift extends), Home / End, Ctrl+A / C / X / V,
    /// Enter and Escape. Call it only while the box has the focus; what it
    /// consumes is gone for everyone else.
    pub fn take_input(&mut self) -> DraftOutcome {
        let mut out = DraftOutcome::Idle;
        let mut edited = false;
        let bump = |out: &mut DraftOutcome, to: DraftOutcome| {
            if *out == DraftOutcome::Idle {
                *out = to;
            }
        };
        for e in host::consume(|e| match e {
            host::InputEvent::Text(_) => true,
            host::InputEvent::Key { down: true, vk: code, mods, .. } => {
                let plain = !mods.ctrl && !mods.alt;
                let ctrl = mods.ctrl && !mods.alt && !mods.shift;
                matches!(
                    (*code, plain, ctrl),
                    (vk::BACK | vk::DELETE | vk::LEFT | vk::RIGHT | vk::HOME | vk::END | vk::ENTER | vk::ESCAPE, true, _)
                ) || (ctrl
                    && [vk::letter('a'), vk::letter('c'), vk::letter('x'), vk::letter('v')].contains(code))
            }
            _ => false,
        }) {
            match e {
                host::InputEvent::Text(s) => edited |= self.insert(&s),
                host::InputEvent::Key { vk: code, mods, .. } => {
                    let shift = mods.shift;
                    if mods.ctrl {
                        if code == vk::letter('a') {
                            self.select_all();
                            bump(&mut out, DraftOutcome::Moved);
                        } else if code == vk::letter('c') {
                            host::set_clipboard_text(self.selected_text());
                        } else if code == vk::letter('x') {
                            host::set_clipboard_text(self.selected_text());
                            edited |= self.delete_selection();
                        } else if code == vk::letter('v') {
                            if let Some(t) = host::clipboard_text() {
                                edited |= self.insert(t.trim());
                            }
                        }
                        continue;
                    }
                    match code {
                        vk::BACK => edited |= self.backspace(),
                        vk::DELETE => edited |= self.delete(),
                        vk::LEFT => self.left(shift),
                        vk::RIGHT => self.right(shift),
                        vk::HOME => self.move_to(0, shift),
                        vk::END => self.move_to(self.text.len(), shift),
                        vk::ENTER => out = DraftOutcome::Commit,
                        vk::ESCAPE => out = DraftOutcome::Cancel,
                        _ => {}
                    }
                    bump(&mut out, DraftOutcome::Moved);
                }
                _ => {}
            }
        }
        if out != DraftOutcome::Idle || edited {
            self.last_input_ms = host::now_ms();
        }
        match out {
            DraftOutcome::Commit | DraftOutcome::Cancel => out,
            _ if edited => DraftOutcome::Edited,
            other => other,
        }
    }

}

// ═════════════════════════════════════════════════════════════════════════════
// ColorField
// ═════════════════════════════════════════════════════════════════════════════

/// The swatch button that opens a picker — `ColorField.tsx`.
///
/// Its model is [`kubuno_controls::buttons::Button`], because that is what it
/// is: a `<button type="button">` whose whole face is the colour. `text`,
/// `enabled`, `padding`, `dock`, `anchor` and `perform_click()` are the
/// replica's, reached through [`Deref`]; the fields declared here are the
/// colour it shows and whether its popover is open, neither of which .NET has a
/// concept for.
///
/// The web's popover — a portal to `<body>`, clamped to the viewport on every
/// edge — is a host popup on the desktop (`kubuno_controls::host::popup`),
/// placed by [`ColorField::popover_rect`], which is `reposition()` ported term
/// for term. What `open` changes on the field itself is what it changes in the
/// web: the border turns accent.
///
/// Keyboard: it is a `<button>`, so Enter and Space toggle it, and it shows
/// the focus ring only when the focus came from the keyboard
/// ([`WidgetState::show_focus_ring`]).
#[derive(Clone)]
pub struct ColorField {
    inner: replica_buttons::Button,
    pub color: Color,
    /// Whether the picker attached to it is showing.
    pub open: bool,
    /// `width = 32`, `height = 24` — overridable, as the web's props are.
    pub width: f32,
    pub height: f32,
}

impl ColorField {
    pub fn new(color: Color) -> Self {
        Self {
            inner: replica_buttons::Button::new(),
            color,
            open: false,
            width: m::FIELD_W,
            height: m::FIELD_H,
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Where the popover opens — `reposition()` in `ColorField.tsx`:
    ///
    /// ```text
    /// let left = r.left - PW - M            // prefer LEFT of the swatch
    /// if (left < M) left = r.right + M      // else to its right
    /// if (left + PW > vw - M) left = vw - PW - M
    /// if (left < M) left = M
    /// let top = r.top                        // top-aligned with the swatch
    /// if (top + PH > vh - M) top = vh - PH - M
    /// if (top < M) top = M
    /// ```
    ///
    /// `anchor` is the swatch, `size` the popover's measured size, `viewport`
    /// the area it must stay inside — on the desktop the monitor's work area
    /// (`Frame::screen_area`), since a popup may leave the window. All three
    /// in the same space; the result is in that space too, WITHOUT the shadow
    /// margin (grow it by [`m::SHADOW_MARGIN`](m) for the popup's bounds).
    pub fn popover_rect(anchor: Rect, size: (f32, f32), viewport: Rect) -> Rect {
        let (pw, ph) = size;
        let mg = m::POPOVER_MARGIN;
        let mut left = anchor.left - pw - mg;
        if left < viewport.left + mg {
            left = anchor.right + mg;
        }
        if left + pw > viewport.right - mg {
            left = viewport.right - pw - mg;
        }
        if left < viewport.left + mg {
            left = viewport.left + mg;
        }
        let mut top = anchor.top;
        if top + ph > viewport.bottom - mg {
            top = viewport.bottom - ph - mg;
        }
        if top < viewport.top + mg {
            top = viewport.top + mg;
        }
        Rect::new(left, top, left + pw, top + ph)
    }
}

impl Widget for ColorField {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> ControlSize {
        ControlSize::new(self.width, self.height)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let t = canvas.theme();
        let dead = !self.inner.enabled || state.disabled;
        let alpha = if dead { DISABLED_ALPHA } else { 1.0 };

        swatch_fill(canvas, bounds, m::FIELD_RADIUS, self.color, alpha);
        // `border: 1px solid ${open ? theme.accent : theme.border}`. Hover is
        // not a state the web gives this button — it states no hover rule.
        let border = if self.open && !dead { t.accent } else { t.card_stroke };
        canvas.stroke_rounded(&bounds, m::FIELD_RADIUS, &fade(border, alpha));
        // The browser's own focus outline on a `<button>`, which is two-tone
        // (dark then light) precisely so it reads on ANY colour — and this
        // button's face can be any colour, the accent included. Drawn INSIDE
        // the bounds so the field never paints over a neighbour, and only for
        // a keyboard focus.
        if state.show_focus_ring() && !dead {
            canvas.stroke_rounded_w(&bounds, m::FIELD_RADIUS, &t.text_primary, m::FOCUS_RING);
            let inner = bounds.inflate(-m::FOCUS_RING, -m::FOCUS_RING);
            canvas.stroke_rounded(&inner, (m::FIELD_RADIUS - m::FOCUS_RING).max(0.0), &t.picker_handle);
        }
    }

    fn type_name(&self) -> &'static str {
        "ColorField"
    }
}

impl Deref for ColorField {
    type Target = replica_buttons::Button;
    fn deref(&self) -> &replica_buttons::Button {
        &self.inner
    }
}
impl DerefMut for ColorField {
    fn deref_mut(&mut self) -> &mut replica_buttons::Button {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// The swatch grid, shared by SwatchPicker and by the picker's two chip rows
// ═════════════════════════════════════════════════════════════════════════════

/// A grid of `columns` equal cells with `gap` between them, laid out inside a
/// content rectangle — `gridTemplateColumns: 'repeat(10, 1fr)'` with
/// `aspect-square` and `gap-1`, which makes the cell size a consequence of the
/// available width and not a number to choose.
///
/// Pure, so the paint, the measurement and the hit-testing all read the same
/// geometry and cannot drift.
#[derive(Debug, Clone, Copy)]
pub struct SwatchGrid {
    pub columns: usize,
    pub gap: f32,
    pub cell: f32,
}

impl SwatchGrid {
    /// The grid `columns` wide that fills `width`.
    pub fn fitting(width: f32, columns: usize, gap: f32) -> Self {
        let columns = columns.max(1);
        let cell = ((width - (columns as f32 - 1.0) * gap) / columns as f32).max(0.0);
        Self { columns, gap, cell }
    }

    /// A grid of cells of a FIXED size — the picker's `w-4 h-4` chip row, which
    /// wraps rather than stretching (`flex flex-wrap gap-1`).
    pub fn fixed(width: f32, cell: f32, gap: f32) -> Self {
        let columns = (((width + gap) / (cell + gap)).floor() as usize).max(1);
        Self { columns, gap, cell }
    }

    pub fn rows(&self, len: usize) -> usize {
        len.div_ceil(self.columns)
    }

    /// The grid's height for `len` cells — zero when there is nothing in it,
    /// because the web renders no block at all in that case.
    pub fn height(&self, len: usize) -> f32 {
        let rows = self.rows(len);
        if rows == 0 {
            return 0.0;
        }
        rows as f32 * self.cell + (rows as f32 - 1.0) * self.gap
    }

    /// Cell `i`, with the grid's top-left at `(left, top)`.
    pub fn cell_rect(&self, left: f32, top: f32, i: usize) -> Rect {
        let col = i % self.columns;
        let row = i / self.columns;
        let x = left + col as f32 * (self.cell + self.gap);
        let y = top + row as f32 * (self.cell + self.gap);
        Rect::new(x, y, x + self.cell, y + self.cell)
    }

    /// Which of `len` cells `(x, y)` lands on, testing the DISC when the cells
    /// are circles (`rounded-full`) and the rectangle when they are not —
    /// border-radius clips pointer events in a browser too, so the corners of a
    /// round swatch must not answer.
    pub fn cell_at(
        &self,
        left: f32,
        top: f32,
        len: usize,
        round: bool,
        x: f32,
        y: f32,
    ) -> Option<usize> {
        (0..len).find(|&i| {
            let r = self.cell_rect(left, top, i);
            if round {
                circular_hit(r, x, y)
            } else {
                r.contains(x, y)
            }
        })
    }

    /// The cell a navigation key moves to from `i`, among `len` cells — the
    /// ARIA grid pattern: the arrows move by one cell or one row (stopping at
    /// the edges, never wrapping), Home / End go to the ends of the ROW,
    /// PageUp / PageDown to the first / last row of the same column.
    ///
    /// A desktop addition: the web renders every chip as its own `<button>`,
    /// so Tab walks all eighty of them one by one. Here the grid is ONE tab
    /// stop with a roving cursor, which is what the pattern prescribes.
    pub fn step(&self, i: usize, len: usize, key: ColorKey) -> usize {
        if len == 0 {
            return 0;
        }
        let cols = self.columns.max(1);
        let i = i.min(len - 1);
        let row_start = i - i % cols;
        match key {
            ColorKey::Left => i.saturating_sub(1),
            ColorKey::Right => (i + 1).min(len - 1),
            ColorKey::Up => i.checked_sub(cols).unwrap_or(i),
            ColorKey::Down => {
                if i + cols < len {
                    i + cols
                } else {
                    i
                }
            }
            ColorKey::Home => row_start,
            ColorKey::End => (row_start + cols - 1).min(len - 1),
            ColorKey::PageUp => i % cols,
            ColorKey::PageDown => {
                // The last row that HAS a cell in this column — a short last
                // row may not reach it.
                let col = i % cols;
                let mut row = (len - 1) / cols;
                while row > 0 && row * cols + col > len - 1 {
                    row -= 1;
                }
                row * cols + col
            }
        }
    }
}

/// How a chip says it is the current colour — the web has three spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectMark {
    /// `border: 1px solid C.accent` in place of the border — the picker's
    /// twelve fixed chips.
    Border,
    /// The accent border PLUS `0 0 0 1px C.accent` — the recent colours.
    BorderRing,
}

/// One round chip of `ColorSwatchPicker`, with the web's literal outlines:
/// `border: 1px solid rgba(0,0,0,.08)`, except pure white which gets
/// `#dadce0` (in both themes), and the selection `boxShadow: '0 0 0 2px
/// #1a73e8'` — a literal blue, drawn OUTSIDE the chip as a box-shadow is.
fn paint_dot(c: &dyn Canvas, rect: Rect, colour: Color, selected: bool, grow: bool, alpha: f32) {
    let (rect, radius) = {
        let d = if grow { (rect.right - rect.left) * m::HOVER_GROW } else { 0.0 };
        let r = rect.inflate(d, d);
        (r, crate::metrics::pill(r.right - r.left))
    };
    swatch_fill(c, rect, radius, colour, alpha);
    let edge = if colour.rgb.channels() == (255, 255, 255) {
        D2D1_COLOR_F { r: 218.0 / 255.0, g: 220.0 / 255.0, b: 224.0 / 255.0, a: alpha }
    } else {
        D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.08 * alpha }
    };
    c.stroke_rounded(&rect, radius, &edge);
    if selected {
        let ring = rect.inflate(m::SELECT_RING, m::SELECT_RING);
        let blue = D2D1_COLOR_F { r: 26.0 / 255.0, g: 115.0 / 255.0, b: 232.0 / 255.0, a: alpha };
        c.stroke_rounded_w(&ring, radius + m::SELECT_RING, &blue, m::SELECT_RING);
    }
}

/// Paints one chip: the colour over the chequer, its hairline (`C.border`),
/// and the accent border / ring a selected one wears (`ColorPicker`'s fixed
/// and recent chips).
fn paint_chip(
    c: &dyn Canvas,
    rect: Rect,
    colour: Color,
    radius: f32,
    selected: Option<SelectMark>,
    grow: bool,
    alpha: f32,
) {
    let t = c.theme();
    // `hover:scale-110`: the chip grows 10 % about its centre. A transform the
    // canvas has no primitive for, but a scale about the centre of a filled
    // shape IS a larger shape — so it is drawn larger, radius included.
    let (rect, radius) = if grow {
        let d = (rect.right - rect.left) * m::HOVER_GROW;
        (rect.inflate(d, d), radius * (1.0 + 2.0 * m::HOVER_GROW))
    } else {
        (rect, radius)
    };
    swatch_fill(c, rect, radius, colour, alpha);
    let border = match selected {
        Some(SelectMark::Border | SelectMark::BorderRing) => t.accent,
        _ => t.card_stroke,
    };
    c.stroke_rounded(&rect, radius, &fade(border, alpha));
    let spread = match selected {
        Some(SelectMark::BorderRing) => m::BORDER,
        _ => 0.0,
    };
    if spread > 0.0 {
        let ring = rect.inflate(spread, spread);
        c.stroke_rounded_w(&ring, radius + spread, &fade(t.accent, alpha), spread);
    }
}

/// The keyboard focus on a chip — the browser's own outline on a focused
/// `<button>`, drawn OUTSIDE the selection ring so the two never hide each
/// other, and in the primary ink so it reads against the accent ring.
fn chip_focus(c: &dyn Canvas, rect: Rect, radius: f32) {
    let out = m::SELECT_RING + m::FOCUS_RING;
    let ring = rect.inflate(out, out);
    c.stroke_rounded_w(&ring, radius + out, &c.theme().text_primary, m::FOCUS_RING);
}

// ═════════════════════════════════════════════════════════════════════════════
// SwatchPicker
// ═════════════════════════════════════════════════════════════════════════════

/// The quick picker — `ColorSwatchPicker.tsx`: a grid of round chips, a
/// « Personnalisé » section under it, and a `+` that opens the full picker.
///
/// Its model is [`kubuno_controls::containers::Panel`], the toolkit's own
/// surface: `padding`, `border_style`, `dock`, `anchor`, `back_color` and the
/// child list are the replica's, through [`Deref`]. What this adds is the
/// palette, the selection and the Kubuno pixels.
#[derive(Clone)]
pub struct SwatchPicker {
    inner: PanelModel,
    /// The fixed palette. Defaults to [`docs_swatches`], the web's own eighty.
    pub colors: Vec<Color>,
    /// The user's own colours — the web persists these in `localStorage` under
    /// `kubuno:picker:custom-swatches` and shows at most twenty.
    pub custom: Vec<Color>,
    /// The caption over the custom row (`customLabel = 'Personnalisé'`).
    pub custom_label: String,
    /// Which chip is current. The web compares HEX, not identity, and so does
    /// [`SwatchPicker::index_of`].
    pub selected: Option<usize>,
    /// The chip under the pointer, in the fixed palette.
    pub hot: Option<usize>,
    pub columns: usize,
    /// The colour the picker is showing — the web's `norm`, which the custom
    /// row compares against too. [`SwatchPicker::select`] sets it.
    pub current: Option<Color>,
    /// The cell of the custom row under the pointer (the last one is `+`).
    pub hot_custom: Option<usize>,
    /// The keyboard cursor, as a FLAT cell index: the palette first, then the
    /// custom row — see [`SwatchPicker::cell_count`]. The grid is one tab stop;
    /// the arrows move this ([`SwatchPicker::step_cursor`]).
    pub focus: Option<usize>,
    /// Whether the cursor's ring shows (`:focus-visible`).
    pub focus_visible: bool,
    /// Whether the eyedropper cell follows `+` (the web shows it wherever
    /// `window.EyeDropper` exists; the desktop always can).
    pub eyedropper: bool,
    /// The screen eyedropper that cell arms.
    pub eye: ScreenEyedropper,
}

impl Default for SwatchPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl SwatchPicker {
    pub fn new() -> Self {
        Self {
            inner: PanelModel::new(),
            colors: docs_swatches(),
            custom: Vec::new(),
            custom_label: "Personnalisé".to_string(),
            selected: None,
            hot: None,
            columns: m::SWATCHES_COLS,
            current: None,
            hot_custom: None,
            focus: None,
            focus_visible: false,
            eyedropper: true,
            eye: ScreenEyedropper::default(),
        }
    }

    /// `custom.slice(0, 20)`, plus the `+` cell the web always appends.
    pub const CUSTOM_MAX: usize = 20;

    /// Selects the chip whose hex matches `colour`, if the palette holds one —
    /// the web's `c.toLowerCase() === norm` test.
    pub fn select(&mut self, colour: Color) {
        self.selected = self.index_of(colour);
        self.current = Some(colour);
    }

    /// `addCustom(hex)`: the colour goes FIRST, any older copy of it (by hex)
    /// is dropped, and the row is capped at twenty.
    pub fn add_custom(&mut self, colour: Color) {
        self.custom.retain(|c| !c.same_swatch(colour));
        self.custom.insert(0, colour);
        self.custom.truncate(Self::CUSTOM_MAX);
    }

    /// Every cell, palette and custom row together — the flat index space of
    /// [`SwatchPicker::focus`] and [`SwatchPicker::cell_at`].
    pub fn cell_count(&self) -> usize {
        self.colors.len() + self.custom_cells()
    }

    /// The flat index of the `+` button, right after the custom colours.
    pub fn add_index(&self) -> usize {
        self.colors.len() + self.custom.len().min(Self::CUSTOM_MAX)
    }

    /// The flat index of the eyedropper cell, when it is shown (after `+`).
    pub fn eyedropper_index(&self) -> Option<usize> {
        self.eyedropper.then(|| self.add_index() + 1)
    }

    /// The colour cell `i` holds; `None` for the `+` button and past the end.
    pub fn colour_at(&self, i: usize) -> Option<Color> {
        let n = self.colors.len();
        if i < n {
            return self.colors.get(i).copied();
        }
        if i >= self.add_index() {
            return None;
        }
        self.custom.get(i - n).copied()
    }

    /// Cell `i`'s rectangle, flat index.
    pub fn cell_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        let content = self.content(bounds);
        let grid = self.grid(bounds);
        let n = self.colors.len();
        if i < n {
            Some(grid.cell_rect(content.left, content.top, i))
        } else if i < self.cell_count() {
            Some(grid.cell_rect(content.left, self.custom_top(bounds), i - n))
        } else {
            None
        }
    }

    /// The flat cell under `(x, y)`, in either grid.
    pub fn cell_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        self.swatch_at(bounds, x, y)
            .or_else(|| self.custom_at(bounds, x, y).map(|i| i + self.colors.len()))
    }

    /// Where a navigation key moves the cursor from flat cell `i`: within a
    /// grid as [`SwatchGrid::step`] does, and ACROSS the caption between them
    /// — Down from the palette's last row lands on the custom row in the same
    /// column (or its last cell), Up from the custom row's first row lands
    /// back on the palette.
    pub fn step_cursor(&self, i: usize, key: ColorKey) -> usize {
        let n = self.colors.len();
        let cols = self.columns.max(1);
        let grid = SwatchGrid { columns: cols, gap: 0.0, cell: 0.0 };
        let custom = self.custom_cells();
        if n == 0 {
            return n + grid.step(i.saturating_sub(n), custom, key);
        }
        if i < n {
            let last_row_start = ((n - 1) / cols) * cols;
            if key == ColorKey::Down && i >= last_row_start {
                return n + (i % cols).min(custom - 1);
            }
            return grid.step(i, n, key);
        }
        let j = (i - n).min(custom - 1);
        if key == ColorKey::Up && j < cols {
            let last_row_start = ((n - 1) / cols) * cols;
            return (last_row_start + j).min(n - 1);
        }
        n + grid.step(j, custom, key)
    }

    /// Where `colour` sits in the fixed palette, by hex.
    pub fn index_of(&self, colour: Color) -> Option<usize> {
        self.colors.iter().position(|c| c.same_swatch(colour))
    }

    /// The panel's interior — `p-3` inside its border.
    pub fn content(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.left + pm::INSET,
            bounds.top + pm::INSET,
            bounds.right - pm::INSET,
            bounds.bottom - pm::INSET,
        )
    }

    fn grid(&self, bounds: Rect) -> SwatchGrid {
        let content = self.content(bounds);
        SwatchGrid::fitting(content.right - content.left, self.columns, m::GRID_GAP)
    }

    /// How many cells the custom row holds: the stored colours, capped, plus
    /// the `+` button.
    fn custom_cells(&self) -> usize {
        self.custom.len().min(Self::CUSTOM_MAX) + 1 + self.eyedropper as usize
    }

    /// The top of the custom row's grid inside `bounds`.
    fn custom_top(&self, bounds: Rect) -> f32 {
        let content = self.content(bounds);
        let grid = self.grid(bounds);
        content.top + grid.height(self.colors.len()) + m::CUSTOM_GAP_TOP + m::CUSTOM_LABEL_H + m::CUSTOM_GAP_BOTTOM
    }

    /// Which chip of the fixed palette is under `(x, y)`.
    pub fn swatch_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let content = self.content(bounds);
        self.grid(bounds).cell_at(content.left, content.top, self.colors.len(), true, x, y)
    }

    /// Which cell of the custom row is under `(x, y)`. The LAST index is the
    /// `+` button, never a colour — see [`SwatchPicker::custom_cells`].
    pub fn custom_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let content = self.content(bounds);
        self.grid(bounds).cell_at(
            content.left,
            self.custom_top(bounds),
            self.custom_cells(),
            true,
            x,
            y,
        )
    }

    /// The panel's height at `width` — everything below the fixed grid depends
    /// on how many custom colours there are.
    pub fn height_for_width(&self, width: f32) -> f32 {
        let inner = width - 2.0 * pm::INSET;
        let grid = SwatchGrid::fitting(inner, self.columns, m::GRID_GAP);
        2.0 * pm::INSET
            + grid.height(self.colors.len())
            + m::CUSTOM_GAP_TOP
            + m::CUSTOM_LABEL_H
            + m::CUSTOM_GAP_BOTTOM
            + grid.height(self.custom_cells())
    }
}

impl Widget for SwatchPicker {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> ControlSize {
        ControlSize::new(m::SWATCHES_W, self.height_for_width(m::SWATCHES_W))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // A floating panel: no opaque ground under its rounded corners — they
        // belong to whatever the panel floats over, as the web's `rounded-lg`
        // corners do. The panel's own fill covers everything else.
        let t = canvas.theme();
        let dead = state.disabled;
        let alpha = if dead { DISABLED_ALPHA } else { 1.0 };

        // `p-3 rounded-lg shadow-lg border`, on the toolbar surface.
        canvas.draw_shadow(&bounds, m::SWATCHES_RADIUS, &SHADOW_LG, SHADOW_BLACK);
        canvas.fill_rounded(&bounds, m::SWATCHES_RADIUS, &t.toolbar_background);
        canvas.stroke_rounded(&bounds, m::SWATCHES_RADIUS, &t.card_stroke);

        let content = self.content(bounds);
        let grid = self.grid(bounds);
        let radius = crate::metrics::pill(grid.cell);
        // Everything below is clipped to the panel's interior, so a narrow
        // panel (a grid squeezed below its intended width) can never spill.
        canvas.push_clip_rounded(&bounds, m::SWATCHES_RADIUS);

        for (i, colour) in self.colors.iter().enumerate() {
            let rect = grid.cell_rect(content.left, content.top, i);
            let chosen = self.selected == Some(i)
                || (self.selected.is_none() && self.current.is_some_and(|c| c.same_swatch(*colour)));
            let grow = self.hot == Some(i) && !dead;
            paint_dot(canvas, rect, *colour, chosen, grow, alpha);
        }

        let label_top = content.top + grid.height(self.colors.len()) + m::CUSTOM_GAP_TOP;
        // `text-[11px] font-semibold uppercase tracking-wide`, `color: C.title`.
        picker::text(
            canvas,
            &self.custom_label.to_uppercase(),
            Rect::new(content.left, label_top, content.right, label_top + m::CUSTOM_LABEL_H),
            pm::TEXT_MD,
            true,
            fade(t.text_secondary, alpha),
            crate::graphics::StringAlignment::Near,
        );

        let custom_top = self.custom_top(bounds);
        for (i, colour) in self.custom.iter().take(Self::CUSTOM_MAX).enumerate() {
            let rect = grid.cell_rect(content.left, custom_top, i);
            let chosen = self.current.is_some_and(|c| c.same_swatch(*colour));
            let grow = self.hot_custom == Some(i) && !dead;
            paint_dot(canvas, rect, *colour, chosen, grow, alpha);
        }
        // The `+` cell (and the eyedropper after it): an outlined circle
        // (`borderColor: C.border`, `color: C.textDim`) whose ground turns
        // `C.surface` under the pointer.
        let n = self.colors.len();
        let mut actions = vec![(self.add_index(), "Plus", m::PLUS_ICON)];
        if let Some(i) = self.eyedropper_index() {
            actions.push((i, "Pipette", m::ICON_SM));
        }
        for (flat, icon, size) in actions {
            let local = flat - n;
            let cell = grid.cell_rect(content.left, custom_top, local);
            let lit = (self.hot_custom == Some(local) || (icon == "Pipette" && self.eye.is_picking())) && !dead;
            if lit {
                canvas.fill_rounded(&cell, radius, &t.surface_2);
            }
            canvas.stroke_rounded(&cell, radius, &fade(t.card_stroke, alpha));
            canvas.vector_icon(icon, &cell, size, &fade(t.text_secondary, alpha));
        }

        if let (Some(i), true, false) = (self.focus, self.focus_visible, dead) {
            if let Some(rect) = self.cell_rect(bounds, i) {
                chip_focus(canvas, rect, radius);
            }
        }
        canvas.pop_clip_rounded();
    }

    fn type_name(&self) -> &'static str {
        "SwatchPicker"
    }
}

impl Deref for SwatchPicker {
    type Target = PanelModel;
    fn deref(&self) -> &PanelModel {
        &self.inner
    }
}
impl DerefMut for SwatchPicker {
    fn deref_mut(&mut self) -> &mut PanelModel {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// GradientPicker
// ═════════════════════════════════════════════════════════════════════════════

/// Where every part of a [`GradientPicker`] is.
#[derive(Clone, Copy)]
pub struct GradientLayout {
    pub panel: Rect,
    pub linear: Rect,
    pub radial: Rect,
    pub close: Rect,
    /// The preview bar. The stop markers hang BELOW it — see
    /// [`GradientPicker::stop_rect`].
    pub bar: Rect,
    /// The angle row, `None` on a radial gradient (`{grad.type === 'linear' &&
    /// …}`).
    pub angle: Option<Rect>,
    pub rule: Rect,
    /// The selected stop's colour field, position label and box, and the bin.
    pub field: Rect,
    pub position: Rect,
    pub bin: Option<Rect>,
    pub opacity: Rect,
    pub add: Rect,
}

/// What a point on a [`GradientPicker`] lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientPart {
    Linear,
    Radial,
    Close,
    /// A stop marker. A press here selects and starts dragging it.
    Stop(usize),
    /// The bar itself — a press here INSERTS a stop, which is what
    /// `cursor-copy` announces.
    Bar,
    /// The angle row's slider.
    Angle,
    /// The selected stop's position box (and its caption).
    Position,
    Bin,
    /// The opacity row's slider.
    Opacity,
    Add,
    /// The angle row's numeric box.
    AngleBox,
    /// The opacity row's numeric box.
    OpacityBox,
    /// The selected stop's `ColorField`, which opens a `ColorPicker`.
    Field,
}

impl GradientPart {
    /// A stable slot per TAB STOP, for a focus id. Every stop marker shares
    /// one: the markers are one stop whose arrows pick and move them.
    pub fn focus_slot(self) -> usize {
        match self {
            Self::Linear => 0,
            Self::Radial => 1,
            Self::Close => 2,
            Self::Stop(_) | Self::Bar => 3,
            Self::Angle => 4,
            Self::AngleBox => 5,
            Self::Field => 6,
            Self::Position => 7,
            Self::Bin => 8,
            Self::Opacity => 9,
            Self::OpacityBox => 10,
            Self::Add => 11,
        }
    }

    /// Whether the part is a numeric box a keyboard types into.
    pub fn is_text(self) -> bool {
        matches!(self, Self::AngleBox | Self::Position | Self::OpacityBox)
    }
}

/// The gradient builder — `GradientPicker.tsx`.
///
/// Its model is [`kubuno_controls::containers::Panel`] for the surface,
/// [`Gradient`] for the value, and [`crate::range::Slider`] — itself a
/// `TrackBar` — for the angle and opacity rows, which the web draws with its
/// own `RangeSlider`. Only the preview bar and its markers are new code.
#[derive(Clone)]
pub struct GradientPicker {
    inner: PanelModel,
    pub gradient: Gradient,
    /// The stop being edited. The web keeps an index and clamps it on render
    /// (`grad.stops[Math.min(sel, stops.length - 1)]`), which is what
    /// [`GradientPicker::selected_stop`] reproduces.
    pub selected: usize,
    pub hot_stop: Option<usize>,
    /// The part holding the keyboard focus, and whether its ring shows.
    pub focus: Option<GradientPart>,
    pub focus_visible: bool,
    /// The part under the pointer (the `✕` lights on hover).
    pub hot: Option<GradientPart>,
    /// The box being typed into, and the caret's blink phase.
    pub edit: Option<(GradientPart, FieldDraft)>,
    pub caret_on: bool,
    /// Whether the selected stop's `ColorField` has its picker open — its
    /// border turns accent, as the field's own does.
    pub field_open: bool,
    /// Whether the header shows the ✕ (`onClose` given — the web's
    /// `GradientField` popover passes one; an inline panel does not).
    pub closable: bool,
}

impl Default for GradientPicker {
    fn default() -> Self {
        Self::new(Gradient::default())
    }
}

impl GradientPicker {
    pub fn new(gradient: Gradient) -> Self {
        Self {
            inner: PanelModel::new(),
            gradient,
            selected: 0,
            hot_stop: None,
            focus: None,
            focus_visible: false,
            hot: None,
            edit: None,
            caret_on: false,
            field_open: false,
            closable: false,
        }
    }

    /// The selected index, clamped the way the web clamps it on render.
    pub fn selected_index(&self) -> usize {
        self.selected.min(self.gradient.stops.len().saturating_sub(1))
    }

    /// The stop indices in DRAWING order — the web maps `sorted` to its
    /// markers, so the marker furthest right is the last in the DOM and the
    /// one on top where two overlap.
    pub fn draw_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.gradient.stops.len()).collect();
        order.sort_by(|&a, &b| {
            let pa = self.gradient.stops[a].position;
            let pb = self.gradient.stops[b].position;
            pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
        });
        order
    }

    /// `removeStop(idx)` then `setSel(0)`: the editor jumps back to the first
    /// stored stop. Refused at two stops, like the model.
    pub fn remove_selected(&mut self) -> bool {
        let i = self.selected_index();
        if self.gradient.remove_stop(i) {
            self.selected = 0;
            true
        } else {
            false
        }
    }

    /// The slider's own rectangle inside a labelled row — between the
    /// `width: 48` caption and the `w-14` box, `gap-2` on both sides.
    pub fn slider_track(row: Rect) -> Rect {
        let boxed = Self::row_box(row);
        Rect::new(row.left + m::GRAD_LABEL_W + m::CHAN_GAP, row.top, (boxed.left - m::CHAN_GAP).max(row.left), row.bottom)
    }

    /// The `w-14` numeric box at the end of a labelled row.
    pub fn row_box(row: Rect) -> Rect {
        Rect::new(row.right - m::NUM_W, row.top, row.right, row.bottom)
    }

    /// The selected stop's position box, `gap-1` after its « Position »
    /// caption (`flex items-center gap-1 flex-1`), vertically centred in
    /// the row.
    pub fn position_box(position_row: Rect) -> Rect {
        let top = position_row.top + (m::FIELD_H - m::NUM_H) / 2.0;
        let left = position_row.left + m::POS_LABEL_W + m::TYPE_GAP;
        Rect::new(left, top, (left + m::NUM_SMALL_W).min(position_row.right.max(left)), top + m::NUM_H)
    }

    /// The value a pointer at `x` gives a `RangeSlider` over `track`
    /// (the native range spans the whole track), rounded to its step.
    fn range_value_at(track: Rect, x: f32, max: f64) -> f64 {
        let w = (track.right - track.left).max(1.0);
        js_round((((x - track.left) / w).clamp(0.0, 1.0) as f64) * max)
    }

    /// The angle a press or drag at `x` on the angle row designates.
    pub fn angle_at(&self, bounds: Rect, x: f32) -> Option<f64> {
        let row = self.layout(bounds).angle?;
        Some(Self::range_value_at(Self::slider_track(row), x, 360.0))
    }

    /// The opacity a press or drag at `x` on the opacity row designates.
    pub fn opacity_at(&self, bounds: Rect, x: f32) -> f64 {
        Self::range_value_at(Self::slider_track(self.layout(bounds).opacity), x, 100.0)
    }

    /// The parts a Tab walks through, in visual order, with the rectangle
    /// each one registers for the focus manager. The markers are ONE stop,
    /// registered over the bar, at the selected marker.
    pub fn tab_stops(&self, bounds: Rect) -> Vec<(GradientPart, Rect)> {
        let g = self.layout(bounds);
        let mut out = vec![(GradientPart::Linear, g.linear), (GradientPart::Radial, g.radial)];
        if self.closable {
            out.push((GradientPart::Close, g.close));
        }
        if !self.gradient.stops.is_empty() {
            let sel = self.selected_index();
            let r = self.stop_rect(bounds, sel).unwrap_or(g.bar);
            out.push((GradientPart::Stop(sel), r));
        }
        if let Some(row) = g.angle {
            out.push((GradientPart::Angle, Self::slider_track(row)));
            out.push((GradientPart::AngleBox, Self::row_box(row)));
        }
        if self.selected_stop().is_some() {
            out.push((GradientPart::Field, g.field));
            out.push((GradientPart::Position, Self::position_box(g.position)));
        }
        if let Some(bin) = g.bin {
            out.push((GradientPart::Bin, bin));
        }
        out.push((GradientPart::Opacity, Self::slider_track(g.opacity)));
        out.push((GradientPart::OpacityBox, Self::row_box(g.opacity)));
        out.push((GradientPart::Add, g.add));
        out
    }

    /// Applies a navigation key to the focused `part`. Returns whether
    /// anything changed.
    ///
    /// * the markers: Left / Right move the selected stop by 1 % (10 % with
    ///   Shift), Home / End put it at 0 % / 100 %, Up / Down select the
    ///   previous / next stop along the bar — a desktop addition, the web's
    ///   markers take no keyboard;
    /// * the two sliders: a native `<input type="range">`'s keys, through
    ///   [`channel_step`];
    /// * a numeric box: Up / Down spin it by one.
    pub fn key(&mut self, part: GradientPart, key: ColorKey, shift: bool) -> bool {
        let before = (self.gradient.clone(), self.selected);
        let sel = self.selected_index();
        match part {
            GradientPart::Stop(_) => {
                let order = self.draw_order();
                let rank = order.iter().position(|&i| i == sel).unwrap_or(0);
                let st = if shift { m::STOP_STEP_BIG } else { m::STOP_STEP };
                match key {
                    ColorKey::Up => self.selected = order[rank.saturating_sub(1)],
                    ColorKey::Down => self.selected = order[(rank + 1).min(order.len() - 1)],
                    _ => {
                        if let Some(stop) = self.gradient.stops.get_mut(sel) {
                            stop.position = match key {
                                ColorKey::Home => 0.0,
                                ColorKey::End => 1.0,
                                k => (stop.position + k.sign() * st).clamp(0.0, 1.0),
                            };
                        }
                    }
                }
            }
            GradientPart::Angle => {
                self.gradient.angle = channel_step(js_round(self.gradient.angle), 360.0, key, shift);
            }
            GradientPart::Opacity => {
                if let Some(stop) = self.gradient.stops.get_mut(sel) {
                    stop.opacity = channel_step(js_round(stop.opacity), 100.0, key, shift);
                }
            }
            GradientPart::AngleBox | GradientPart::Position | GradientPart::OpacityBox => {
                let delta = match key {
                    ColorKey::Up => 1.0,
                    ColorKey::Down => -1.0,
                    _ => return false,
                };
                let value = self.box_value(part) + delta;
                self.set_box_value(part, value);
                self.begin_edit(part);
            }
            _ => return false,
        }
        before != (self.gradient.clone(), self.selected)
    }

    /// The whole number a box shows: the angle, the position in percent, the
    /// opacity.
    pub fn box_value(&self, part: GradientPart) -> f64 {
        let stop = self.selected_stop();
        match part {
            GradientPart::AngleBox => js_round(self.gradient.angle),
            GradientPart::Position => stop.map(|s| js_round(s.position * 100.0)).unwrap_or(0.0),
            GradientPart::OpacityBox => stop.map(|s| js_round(s.opacity)).unwrap_or(100.0),
            _ => 0.0,
        }
    }

    /// Writes a box's value into the model, clamped as the web's `onChange`
    /// clamps it: the angle to `0..=360`, the position to `0..=100` %, the
    /// opacity to `0..=100`.
    pub fn set_box_value(&mut self, part: GradientPart, value: f64) {
        let sel = self.selected_index();
        match part {
            GradientPart::AngleBox => self.gradient.angle = value.clamp(0.0, 360.0),
            GradientPart::Position => {
                if let Some(s) = self.gradient.stops.get_mut(sel) {
                    s.position = (value / 100.0).clamp(0.0, 1.0);
                }
            }
            GradientPart::OpacityBox => {
                if let Some(s) = self.gradient.stops.get_mut(sel) {
                    s.opacity = value.clamp(0.0, 100.0);
                }
            }
            _ => {}
        }
    }

    /// Starts typing into a box, its value selected.
    pub fn begin_edit(&mut self, part: GradientPart) {
        if !part.is_text() {
            return;
        }
        let mut d = FieldDraft::new(DraftKind::Digits, &format!("{}", self.box_value(part)));
        d.last_input_ms = host::now_ms();
        self.edit = Some((part, d));
    }

    /// Applies the draft — `Number(e.target.value)`, clamped, on every
    /// keystroke; an empty box is zero, as `Number('')` is.
    pub fn apply_edit(&mut self) -> bool {
        let Some((part, d)) = self.edit.clone() else { return false };
        self.set_box_value(part, d.number().unwrap_or(0.0));
        true
    }

    pub fn end_edit(&mut self) {
        self.edit = None;
    }

    /// The draft for `part`, if that box is being typed into.
    pub fn draft(&self, part: GradientPart) -> Option<&FieldDraft> {
        self.edit.as_ref().filter(|(p, _)| *p == part).map(|(_, d)| d)
    }

    fn ring_on(&self, part: GradientPart) -> bool {
        self.focus_visible && self.focus.is_some_and(|f| f.focus_slot() == part.focus_slot())
    }

    fn has_focus(&self, part: GradientPart) -> bool {
        self.focus.is_some_and(|f| f.focus_slot() == part.focus_slot())
    }

    /// The stop the editor at the bottom is editing.
    pub fn selected_stop(&self) -> Option<GradientStop> {
        if self.gradient.stops.is_empty() {
            return None;
        }
        let i = self.selected.min(self.gradient.stops.len() - 1);
        self.gradient.stops.get(i).copied()
    }

    /// The angle slider, 0..=360 — `RangeSlider min={0} max={360}`.
    pub fn angle_slider(&self) -> Slider {
        let mut s = Slider::new();
        s.set_minimum(0);
        s.set_maximum(360);
        let _ = s.set_value(js_round(self.gradient.angle).clamp(0.0, 360.0) as i32);
        s
    }

    /// The selected stop's opacity slider, 0..=100.
    pub fn opacity_slider(&self) -> Slider {
        let mut s = Slider::new();
        s.set_minimum(0);
        s.set_maximum(100);
        let v = self.selected_stop().map(|st| st.opacity).unwrap_or(100.0);
        let _ = s.set_value(js_round(v).clamp(0.0, 100.0) as i32);
        s
    }

    /// Everything's rectangle.
    pub fn layout(&self, bounds: Rect) -> GradientLayout {
        // `border: 1px` + `p-3`.
        let l = bounds.left + pm::INSET;
        let r = bounds.right - pm::INSET;
        let mut y = bounds.top + pm::INSET;

        // `linear` / `radial` (`gap-1`), then the ✕ at the far end — a
        // `text-[11px] px-1` glyph button, centred in the 21 DIP row.
        let linear = Rect::new(l, y, l + m::TYPE_W[0], y + m::TYPE_BTN_H);
        let radial = Rect::new(linear.right + m::TYPE_GAP, y, linear.right + m::TYPE_GAP + m::TYPE_W[1], y + m::TYPE_BTN_H);
        let cy = y + m::TYPE_BTN_H / 2.0;
        let close = if self.closable {
            Rect::new(r - pm::CLOSE_W, cy - pm::HEADER_H / 2.0, r, cy + pm::HEADER_H / 2.0)
        } else {
            Rect::new(r, cy, r, cy)
        };
        y += m::TYPE_BTN_H + m::ROW_GAP;

        let bar = Rect::new(l, y, r, y + m::BAR_H);
        y += m::BAR_H + m::BAR_GAP;

        let angle = if self.gradient.kind == GradientKind::Linear {
            let a = Rect::new(l, y, r, y + m::NUM_H);
            y += m::NUM_H + m::ROW_GAP;
            Some(a)
        } else {
            None
        };

        let rule = Rect::new(l, y, r, y + m::BORDER);
        y += m::BORDER + m::RULE_GAP;

        // The stop row: a `ColorField`, the position label + box, and the bin
        // (only when a stop may be removed).
        let field = Rect::new(l, y, l + m::FIELD_W, y + m::FIELD_H);
        let bin = (self.gradient.stops.len() > 2)
            .then(|| {
                let cy = y + m::FIELD_H / 2.0;
                Rect::new(r - m::ICON_MD, cy - m::ICON_MD / 2.0, r, cy + m::ICON_MD / 2.0)
            });
        let pos_right = bin.map(|b| b.left - m::CHAN_GAP).unwrap_or(r);
        let position = Rect::new(field.right + m::CHAN_GAP, y, pos_right, y + m::FIELD_H);
        y += m::FIELD_H + m::ROW_GAP;

        let opacity = Rect::new(l, y, r, y + m::NUM_H);
        y += m::NUM_H + m::ROW_GAP;

        // `mt-2` above the « add a stop » button.
        let add = Rect::new(l, y, (l + m::ADD_W).min(r), y + m::ADD_H);

        GradientLayout { panel: bounds, linear, radial, close, bar, angle, rule, field, position, bin, opacity, add }
    }

    pub fn height_for_width(&self, width: f32) -> f32 {
        self.layout(Rect::new(0.0, 0.0, width, 0.0)).add.bottom + pm::INSET
    }

    /// The marker for stop `i`: 12 DIP square, centred on its position along
    /// the bar and hanging 4 DIP below it (`-bottom-1`, `left: calc(p% - 6px)`).
    pub fn stop_rect(&self, bounds: Rect, i: usize) -> Option<Rect> {
        let stop = self.gradient.stops.get(i)?;
        let bar = self.layout(bounds).bar;
        let x = bar.left + (stop.position.clamp(0.0, 1.0) as f32) * (bar.right - bar.left);
        let top = bar.bottom + m::STOP_DROP - m::STOP;
        Some(Rect::new(x - m::STOP / 2.0, top, x + m::STOP / 2.0, top + m::STOP))
    }

    /// Which stop marker `(x, y)` lands on.
    ///
    /// Markers overlap when two stops sit close together, and the web resolves
    /// that by DOM order: the last one rendered is on top and takes the press.
    /// The web renders them SORTED by position ([`GradientPicker::draw_order`]),
    /// and so does `paint`; this searches that order from its end — so the
    /// marker that answers is the one the user can see.
    pub fn stop_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        self.draw_order()
            .into_iter()
            .rev()
            .find(|&i| self.stop_rect(bounds, i).is_some_and(|r| r.contains(x, y)))
    }

    /// The position a press at `x` designates — `posFromEvent`, clamped 0..=1.
    pub fn position_at(&self, bounds: Rect, x: f32) -> f64 {
        let bar = self.layout(bounds).bar;
        let w = (bar.right - bar.left).max(1.0);
        (((x - bar.left) / w) as f64).clamp(0.0, 1.0)
    }

    /// Which part `(x, y)` lands on. The stop markers are tested BEFORE the
    /// bar, because the web stops their press from reaching it
    /// (`e.stopPropagation()`) — otherwise every grab of a marker would also
    /// insert a stop.
    pub fn part_at(&self, bounds: Rect, x: f32, y: f32) -> Option<GradientPart> {
        let g = self.layout(bounds);
        if let Some(i) = self.stop_at(bounds, x, y) {
            return Some(GradientPart::Stop(i));
        }
        if g.bar.contains(x, y) {
            return Some(GradientPart::Bar);
        }
        if g.linear.contains(x, y) {
            return Some(GradientPart::Linear);
        }
        if g.radial.contains(x, y) {
            return Some(GradientPart::Radial);
        }
        if g.close.contains(x, y) {
            return Some(GradientPart::Close);
        }
        if let Some(a) = g.angle.filter(|a| a.contains(x, y)) {
            return Some(if Self::row_box(a).contains(x, y) { GradientPart::AngleBox } else { GradientPart::Angle });
        }
        if g.bin.is_some_and(|b| b.contains(x, y)) {
            return Some(GradientPart::Bin);
        }
        if g.field.contains(x, y) {
            return Some(GradientPart::Field);
        }
        if g.position.contains(x, y) {
            return Some(GradientPart::Position);
        }
        if g.opacity.contains(x, y) {
            let boxed = Self::row_box(g.opacity).contains(x, y);
            return Some(if boxed { GradientPart::OpacityBox } else { GradientPart::Opacity });
        }
        if g.add.contains(x, y) {
            return Some(GradientPart::Add);
        }
        None
    }

    /// A labelled row: a `width: 48` caption, a `RangeSlider`, a `w-14`
    /// numeric box.
    #[allow(clippy::too_many_arguments)]
    fn paint_slider_row(
        &self,
        c: &dyn Canvas,
        row: Rect,
        label: &str,
        slider: &Slider,
        (track_part, box_part): (GradientPart, GradientPart),
        alpha: f32,
    ) {
        let t = c.theme();
        // `text-[10px] uppercase`, `width: 48`, `flex-shrink-0`.
        picker::text(
            c,
            &label.to_uppercase(),
            Rect::new(row.left, row.top, row.left + m::GRAD_LABEL_W, row.bottom),
            pm::TEXT_SM,
            false,
            fade(t.text_secondary, alpha),
            crate::graphics::StringAlignment::Near,
        );
        let track = Self::slider_track(row);
        let max = slider.maximum().max(1) as f32;
        paint_range(c, track, slider.value() as f32 / max, self.ring_on(track_part), alpha);
        let boxed = Self::row_box(row);
        let draft = self.draft(box_part).map(|d| (d, self.caret_on));
        picker::number_input(c, boxed, &slider.value().to_string(), self.has_focus(box_part), draft, alpha);
    }

    /// Paints the preview bar: the CSS gradient over the transparency
    /// chequer (`repeating-conic-gradient(#bbb 0% 25%, #fff 0% 50%)` at
    /// 10 px), `borderRadius: 3`, a hairline.
    ///
    /// With a Direct2D device the gradient is a real linear / radial brush
    /// with the CSS geometry (the line through the centre at `angle`, `0deg`
    /// pointing up; a circle reaching the farthest corner); headless, a grid
    /// of [`m::GRAD_CELL`](m) squares sampled with [`Gradient::sample_at`].
    fn paint_bar(&self, c: &dyn Canvas, bar: Rect, alpha: f32) {
        paint_gradient(c, bar, m::BAR_RADIUS, &self.gradient, alpha, true);
        c.stroke_rounded(&bar, m::BAR_RADIUS, &fade(c.theme().card_stroke, alpha));
    }
}

/// A gradient filling `bar` (rounded by `radius`), over the transparency
/// chequer when `chequered` — else over white, as `GradientField`'s
/// `backgroundColor: '#fff'` is.
fn paint_gradient(c: &dyn Canvas, bar: Rect, radius: f32, gradient: &Gradient, alpha: f32, chequered: bool) {
    {
        use crate::graphics::{Brush, GradientStop as PaintStop, Graphics, LinearGradientBrush, PointF, RadialGradientBrush};
        c.push_clip_rounded(&bar, radius);
        if chequered {
            chequer(c, bar);
        } else {
            c.fill_rounded(&bar, 0.0, &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: alpha });
        }
        let g = Graphics::new(c);
        let sorted = gradient.sorted();
        if g.has_device() && sorted.len() >= 2 {
            let stops: Vec<PaintStop> = sorted
                .iter()
                .map(|s| PaintStop::new((js_round(s.position * 100.0) / 100.0) as f32, fade(s.as_color().to_d2d(), alpha).into()))
                .collect();
            let (first, last) = (stops[0].color, stops[stops.len() - 1].color);
            let brush = match gradient.kind {
                GradientKind::Linear => {
                    let angle = js_round(gradient.angle) as f32 - 90.0;
                    Brush::Linear(LinearGradientBrush::with_angle(bar, first, last, angle).with_stops(&stops))
                }
                GradientKind::Radial => {
                    let (w, h) = (bar.right - bar.left, bar.bottom - bar.top);
                    let r = ((w / 2.0).powi(2) + (h / 2.0).powi(2)).sqrt();
                    let centre = PointF::new((bar.left + bar.right) / 2.0, (bar.top + bar.bottom) / 2.0);
                    Brush::Radial(RadialGradientBrush::new(centre, r, r, first, last).with_stops(&stops))
                }
            };
            g.fill_rectangle(brush, bar);
        } else {
            let w = bar.right - bar.left;
            let h = bar.bottom - bar.top;
            let cols = (w / m::GRAD_CELL).ceil().max(1.0) as usize;
            let rows = (h / m::GRAD_CELL).ceil().max(1.0) as usize;
            for row in 0..rows {
                for col in 0..cols {
                    let x0 = bar.left + col as f32 * m::GRAD_CELL;
                    let y0 = bar.top + row as f32 * m::GRAD_CELL;
                    let cell =
                        Rect::new(x0, y0, (x0 + m::GRAD_CELL).min(bar.right), (y0 + m::GRAD_CELL).min(bar.bottom));
                    let u = ((x0 - bar.left + m::GRAD_CELL / 2.0) / w.max(1.0)) as f64;
                    let v = ((y0 - bar.top + m::GRAD_CELL / 2.0) / h.max(1.0)) as f64;
                    let colour = gradient.sample_at(u, v, w as f64, h as f64);
                    c.fill_rounded(&cell, 0.0, &fade(colour.to_d2d(), alpha));
                }
            }
        }
        c.pop_clip_rounded();
    }
}

/// `@ui/RangeSlider`'s bubble variant, as `GradientPicker` configures it
/// (`accent={C.accent} trackColor={C.border}`): an `h-1.5 rounded-full`
/// track, the accent fill up to the value, and a 12 DIP accent thumb wearing
/// `0 0 0 2px #fff, 0 1px 3px rgba(0,0,0,.35)`. The native range under it
/// spans the whole track.
fn paint_range(c: &dyn Canvas, track: Rect, fraction: f32, focused: bool, alpha: f32) {
    use crate::graphics::{Brush, Graphics};
    let t = c.theme();
    let cy = (track.top + track.bottom) / 2.0;
    let rail = Rect::new(track.left, cy - 3.0, track.right, cy + 3.0);
    c.fill_rounded(&rail, 3.0, &fade(t.card_stroke, alpha));
    let x = track.left + fraction.clamp(0.0, 1.0) * (track.right - track.left);
    if x > rail.left {
        c.fill_rounded(&Rect::new(rail.left, rail.top, x, rail.bottom), 3.0, &fade(t.accent, alpha));
    }
    let g = Graphics::new(c);
    // `0 1px 3px rgba(0,0,0,.35)`, approximated by a soft disc one DIP down.
    let d = 6.0;
    g.fill_ellipse(
        Brush::solid(D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.18 * alpha }),
        Rect::new(x - d - 1.0, cy - d, x + d + 1.0, cy + d + 2.0),
    );
    g.fill_ellipse(Brush::solid(D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: alpha }), Rect::new(x - 8.0, cy - 8.0, x + 8.0, cy + 8.0));
    g.fill_ellipse(Brush::solid(fade(t.accent, alpha)), Rect::new(x - d, cy - d, x + d, cy + d));
    if focused {
        let ring = Rect::new(x - 10.0, cy - 10.0, x + 10.0, cy + 10.0);
        c.stroke_rounded_w(&ring, 10.0, &t.accent, m::FOCUS_RING);
    }
}

impl Widget for GradientPicker {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> ControlSize {
        ControlSize::new(m::GRAD_W, self.height_for_width(m::GRAD_W))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        use crate::graphics::StringAlignment;
        // A floating panel: no opaque ground under its rounded corners.
        let t = canvas.theme();
        let dead = state.disabled;
        let alpha = if dead { DISABLED_ALPHA } else { 1.0 };
        let g = self.layout(bounds);
        let white = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: alpha };

        // `shadow-2xl p-3`, `background: C.toolbar`, `border: 1px solid C.border`,
        // `borderRadius: 4`.
        canvas.draw_shadow(&bounds, m::PANEL_RADIUS, &crate::datetime::SHADOW_2XL, SHADOW_BLACK);
        canvas.fill_rounded(&bounds, m::PANEL_RADIUS, &t.toolbar_background);
        canvas.stroke_rounded(&bounds, m::PANEL_RADIUS, &t.card_stroke);
        canvas.push_clip_rounded(&bounds, m::PANEL_RADIUS);
        canvas.push_bg(t.toolbar_background);

        // The two type buttons: `background: active ? C.accent : C.surface`,
        // `color: active ? '#fff' : C.textDim`, `border: 1px solid C.border`.
        for (rect, label, on, part) in [
            (g.linear, "Linéaire", self.gradient.kind == GradientKind::Linear, GradientPart::Linear),
            (g.radial, "Radial", self.gradient.kind == GradientKind::Radial, GradientPart::Radial),
        ] {
            let fill = if on { t.accent } else { t.surface_2 };
            let ink = if on { white } else { fade(t.text_secondary, alpha) };
            canvas.fill_rounded(&rect, m::TYPE_RADIUS, &fade(fill, alpha));
            canvas.stroke_rounded(&rect, m::TYPE_RADIUS, &fade(t.card_stroke, alpha));
            picker::text(canvas, label, rect, pm::TEXT_SM, true, ink, StringAlignment::Center);
            if self.ring_on(part) {
                focus_ring(canvas, rect, m::TYPE_RADIUS);
            }
        }
        if self.closable {
            if self.hot == Some(GradientPart::Close) && !dead {
                canvas.fill_rounded(&g.close, crate::metrics::radius::SM, &D2D1_COLOR_F { a: 0.1, ..white });
            }
            picker::text(canvas, "✕", g.close, pm::TEXT_MD, false, fade(t.text_secondary, alpha), StringAlignment::Center);
            if self.ring_on(GradientPart::Close) {
                focus_ring(canvas, g.close, crate::metrics::radius::SM);
            }
        }

        self.paint_bar(canvas, g.bar, alpha);

        // The markers, in `sorted` order so the leftmost is painted first and
        // the web's own overlap order is preserved: `border: 2px solid (sel ?
        // C.accent : #fff)`, `boxShadow: 0 0 0 1px rgba(0,0,0,.5)`.
        let sel = self.selected_index();
        for i in self.draw_order() {
            let Some(rect) = self.stop_rect(bounds, i) else { continue };
            let Some(stop) = self.gradient.stops.get(i) else { continue };
            let out = rect.inflate(m::HANDLE_SHADOW, m::HANDLE_SHADOW);
            canvas.stroke_rounded(&out, m::STOP_RADIUS + m::HANDLE_SHADOW, &fade(t.picker_handle_shadow, alpha));
            swatch_fill(canvas, rect, m::STOP_RADIUS, stop.as_color(), alpha);
            let ring = if i == sel { t.accent } else { t.picker_handle };
            canvas.stroke_rounded_w(&rect, m::STOP_RADIUS, &fade(ring, alpha), m::STOP_RING);
            if i == sel && self.ring_on(GradientPart::Stop(i)) {
                focus_ring(canvas, out, m::STOP_RADIUS + m::HANDLE_SHADOW);
            }
        }

        if let Some(row) = g.angle {
            let parts = (GradientPart::Angle, GradientPart::AngleBox);
            self.paint_slider_row(canvas, row, "Angle", &self.angle_slider(), parts, alpha);
        }

        canvas.fill_rounded(&g.rule, 0.0, &fade(t.card_stroke, alpha));

        if let Some(stop) = self.selected_stop() {
            let field = ColorField::new(stop.as_color()).open(self.field_open);
            let fs = WidgetState { disabled: dead, ..WidgetState::REST }
                .focused(self.has_focus(GradientPart::Field))
                .focus_visible(self.ring_on(GradientPart::Field));
            field.paint(canvas, g.field, fs);

            let pos_box = Self::position_box(g.position);
            picker::text(
                canvas,
                "POSITION",
                Rect::new(g.position.left, g.position.top, pos_box.left - m::TYPE_GAP, g.position.bottom),
                pm::TEXT_SM,
                false,
                fade(t.text_secondary, alpha),
                StringAlignment::Near,
            );
            let draft = self.draft(GradientPart::Position).map(|d| (d, self.caret_on));
            let value = format!("{}", js_round(stop.position * 100.0));
            picker::number_input(canvas, pos_box, &value, self.has_focus(GradientPart::Position), draft, alpha);
        }
        if let Some(bin) = g.bin {
            canvas.vector_icon("Trash2", &bin, m::ICON_MD, &fade(t.text_secondary, alpha));
            if self.ring_on(GradientPart::Bin) {
                focus_ring(canvas, bin, crate::metrics::radius::SM);
            }
        }

        let parts = (GradientPart::Opacity, GradientPart::OpacityBox);
        self.paint_slider_row(canvas, g.opacity, "Opacité", &self.opacity_slider(), parts, alpha);

        // « Ajouter un arrêt » — `px-1.5 py-1 text-[10px] rounded`,
        // `background: C.surface`, `color: C.textDim`, a `Plus size={11}`, `gap-1`.
        canvas.fill_rounded(&g.add, crate::metrics::radius::SM, &fade(t.surface_2, alpha));
        let glyph = Rect::new(g.add.left + m::NUM_PAD_X, g.add.top, g.add.left + m::NUM_PAD_X + m::ICON_SM, g.add.bottom);
        canvas.vector_icon("Plus", &glyph, m::ICON_SM, &fade(t.text_secondary, alpha));
        picker::text(
            canvas,
            "Ajouter un arrêt",
            Rect::new(glyph.right + m::GRID_GAP, g.add.top, g.add.right, g.add.bottom),
            pm::TEXT_SM,
            false,
            fade(t.text_secondary, alpha),
            StringAlignment::Near,
        );
        if self.ring_on(GradientPart::Add) {
            focus_ring(canvas, g.add, crate::metrics::radius::SM);
        }
        canvas.pop_bg();
        canvas.pop_clip_rounded();
    }

    fn type_name(&self) -> &'static str {
        "GradientPicker"
    }
}

impl Deref for GradientPicker {
    type Target = PanelModel;
    fn deref(&self) -> &PanelModel {
        &self.inner
    }
}
impl DerefMut for GradientPicker {
    fn deref_mut(&mut self) -> &mut PanelModel {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// GradientField
// ═════════════════════════════════════════════════════════════════════════════

/// The gradient swatch button — `GradientField` in `GradientPicker.tsx`: a
/// `32 × 24` button whose face is the gradient (`backgroundImage:
/// gradientToCss(value), backgroundColor: '#fff'`), `border: 1px solid (open
/// ? C.accent : C.border)`, `borderRadius: 4`. A click opens a
/// [`GradientPicker`] (with its ✕, `onClose`) in a floating popover placed
/// exactly like [`ColorField`]'s ([`ColorField::popover_rect`]).
#[derive(Clone)]
pub struct GradientField {
    inner: replica_buttons::Button,
    pub gradient: Gradient,
    /// Whether the picker attached to it is showing.
    pub open: bool,
    /// `width = 32`, `height = 24`.
    pub width: f32,
    pub height: f32,
}

impl GradientField {
    pub fn new(gradient: Gradient) -> Self {
        Self { inner: replica_buttons::Button::new(), gradient, open: false, width: m::FIELD_W, height: m::FIELD_H }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// The picker a click opens, on this field's gradient, with its ✕.
    pub fn picker(&self) -> GradientPicker {
        let mut p = GradientPicker::new(self.gradient.clone());
        p.closable = true;
        p
    }
}

impl Widget for GradientField {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> ControlSize {
        ControlSize::new(self.width, self.height)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        canvas.fill_rounded(&bounds, 0.0, &canvas.current_bg());
        let t = canvas.theme();
        let dead = !self.inner.enabled || state.disabled;
        let alpha = if dead { DISABLED_ALPHA } else { 1.0 };
        paint_gradient(canvas, bounds, m::FIELD_RADIUS, &self.gradient, alpha, false);
        let border = if self.open && !dead { t.accent } else { t.card_stroke };
        canvas.stroke_rounded(&bounds, m::FIELD_RADIUS, &fade(border, alpha));
        if state.show_focus_ring() && !dead {
            canvas.stroke_rounded_w(&bounds, m::FIELD_RADIUS, &t.text_primary, m::FOCUS_RING);
            let inner = bounds.inflate(-m::FOCUS_RING, -m::FOCUS_RING);
            canvas.stroke_rounded(&inner, (m::FIELD_RADIUS - m::FOCUS_RING).max(0.0), &t.picker_handle);
        }
    }

    fn type_name(&self) -> &'static str {
        "GradientField"
    }
}

impl Deref for GradientField {
    type Target = replica_buttons::Button;
    fn deref(&self) -> &replica_buttons::Button {
        &self.inner
    }
}
impl DerefMut for GradientField {
    fn deref_mut(&mut self) -> &mut replica_buttons::Button {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
//
// Four kinds, in this order: the ARITHMETIC (both directions, on values whose
// answer is known independently), the NOTATIONS (each one the web accepts, and
// the refusal of everything else), the GEOMETRY (thumb ⇄ value, the 2-D area at
// its four corners, the chip under a point), and the gradient MODEL
// (interpolation, insertion, the two-stop floor).
//
// The paint bodies are not covered, and cannot be from a `--lib` test: a
// `Canvas` carries a `TextFormats`, a set of COM `IDWriteTextFormat` objects
// built from a DirectWrite factory that a unit test has no way to create. The
// gallery's `color` page is what exercises those, from a live canvas.
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// How close two channels have to be to count as the same. The web works
    /// in `f64` and so does this port, so the agreement is far tighter than
    /// this; the tolerance exists so a test states a colour in decimal without
    /// having to spell out a float bit for bit.
    const EPS: f64 = 1e-9;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < EPS
    }

    /// The looser tolerance a value that has been through a `f32` coordinate
    /// needs: geometry is single precision, so `0.6` of a track comes back as
    /// `0.60000002`.
    fn close_px(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn rgb_close(a: Rgb, b: Rgb) -> bool {
        close(a.r, b.r) && close(a.g, b.g) && close(a.b, b.b)
    }

    // ── The rounding rule ───────────────────────────────────────────────────

    /// `Math.round` is not `f64::round`. This is the whole reason [`js_round`]
    /// exists, and the one case where the two disagree is a tie on a negative
    /// number.
    #[test]
    fn rounding_is_javascripts_not_rusts() {
        assert_eq!(js_round(0.5), 1.0);
        assert_eq!(js_round(1.5), 2.0);
        assert_eq!(js_round(2.4), 2.0);
        // The divergence: JS gives -0, Rust's own `round` gives -1.
        assert_eq!(js_round(-0.5), 0.0);
        assert_eq!((-0.5f64).round(), -1.0);
        assert_eq!(js_round(-1.5), -1.0);
        assert_eq!(js_round(-2.5), -2.0);
    }

    // ── RGB ⇄ HSV ───────────────────────────────────────────────────────────

    /// The six vertices of the hue hexagon, in both directions. These are the
    /// values the conversion has to get exactly right: every ramp on the page
    /// passes through them.
    #[test]
    fn the_six_hue_vertices_convert_both_ways() {
        let vertices: [(f64, Rgb); 6] = [
            (0.0, Rgb::new(255.0, 0.0, 0.0)),
            (60.0, Rgb::new(255.0, 255.0, 0.0)),
            (120.0, Rgb::new(0.0, 255.0, 0.0)),
            (180.0, Rgb::new(0.0, 255.0, 255.0)),
            (240.0, Rgb::new(0.0, 0.0, 255.0)),
            (300.0, Rgb::new(255.0, 0.0, 255.0)),
        ];
        for (h, rgb) in vertices {
            let made = hsv_to_rgb(h, 1.0, 1.0);
            assert!(rgb_close(made, rgb), "hsv({h},1,1) gave {made:?}, wanted {rgb:?}");
            let back = rgb_to_hsv(rgb.r, rgb.g, rgb.b);
            assert!(close(back.h, h), "hue of {rgb:?} was {}", back.h);
            assert!(close(back.s, 1.0) && close(back.v, 1.0));
        }
    }

    /// Magenta is the case the `% 6` remainder decides: `(g - b)/d` is negative
    /// there, and only the `if (h < 0) h += 360` that follows puts it at 300
    /// instead of at −60.
    #[test]
    fn a_negative_red_branch_wraps_to_three_hundred() {
        let h = rgb_to_hsv(255.0, 0.0, 255.0).h;
        assert!(close(h, 300.0), "magenta came out at {h}");
        // Any red-dominant colour with more blue than green lands in the same
        // branch: `(0 − 128/255) × 60` is −30.1, and the wrap makes it 329.9.
        let pink = rgb_to_hsv(255.0, 0.0, 128.0).h;
        assert!(close(pink, 360.0 - 128.0 / 255.0 * 60.0), "it came out at {pink}");
    }

    /// Black, white and the greys: saturation zero, hue zero, and a round trip
    /// that does not move.
    #[test]
    fn greys_have_no_hue_and_survive_the_round_trip() {
        for level in [0.0, 1.0, 51.0, 128.0, 200.0, 254.0, 255.0] {
            let hsv = rgb_to_hsv(level, level, level);
            assert!(close(hsv.h, 0.0), "a grey must have hue 0, {level} gave {}", hsv.h);
            assert!(close(hsv.s, 0.0), "a grey must have saturation 0");
            assert!(close(hsv.v, level / 255.0));
            let back = hsv_to_rgb(hsv.h, hsv.s, hsv.v);
            assert!(rgb_close(back, Rgb::new(level, level, level)), "{level} came back {back:?}");
        }
        // Black is the one case `s` cannot be derived from `d / max`: max is 0.
        assert!(close(rgb_to_hsv(0.0, 0.0, 0.0).s, 0.0));
    }

    /// A saturation of zero keeps whatever hue it was given, in both
    /// directions — the property the picker's own state depends on (drag the
    /// SV handle to the left edge and the hue slider must not jump to red).
    #[test]
    fn zero_saturation_keeps_its_hue() {
        for h in [0.0, 37.0, 210.0, 359.0] {
            let rgb = hsv_to_rgb(h, 0.0, 0.7);
            assert!(rgb_close(rgb, Rgb::new(178.5, 178.5, 178.5)), "{h}° gave {rgb:?}");
        }
    }

    /// A full round trip through hex on values a user actually types.
    #[test]
    fn hex_survives_a_trip_through_hsv() {
        for hex in ["#000000", "#ffffff", "#1a73e8", "#d93025", "#f9ab00", "#4a90d9", "#9b59b6"] {
            let start = parse(hex).expect("a valid six-digit hex");
            let hsv = start.to_hsv();
            let back = hsv_to_rgb(hsv.h, hsv.s, hsv.v);
            assert_eq!(back.to_hex(), hex, "{hex} came back as {}", back.to_hex());
        }
    }

    /// Every one of the two palettes the web ships survives the same trip —
    /// ninety-two colours, which is a far better sweep of the space than a
    /// handful of chosen ones.
    #[test]
    fn every_shipped_swatch_survives_the_round_trip() {
        for hex in PICKER_SWATCHES.iter().chain(DOCS_SWATCHES.iter()) {
            let c = parse(hex).unwrap_or_else(|| panic!("{hex} is not a valid swatch literal"));
            assert_eq!(c.rgb.to_hex(), *hex, "the constant itself is malformed");
            let hsv = c.to_hsv();
            assert_eq!(hsv_to_rgb(hsv.h, hsv.s, hsv.v).to_hex(), *hex);
            let hsl = rgb_to_hsl(c.rgb.r, c.rgb.g, c.rgb.b);
            assert_eq!(hsl_to_rgb(hsl.h, hsl.s, hsl.l).to_hex(), *hex, "HSL trip of {hex}");
            let k = rgb_to_cmyk(c.rgb.r, c.rgb.g, c.rgb.b);
            assert_eq!(cmyk_to_rgb(k.c, k.m, k.y, k.k).to_hex(), *hex, "CMYK trip of {hex}");
        }
    }

    // ── RGB ⇄ HSL ───────────────────────────────────────────────────────────

    /// HSL's own known values, including the two the branches turn on:
    /// `l > 0.5`, and the `s === 0` short circuit that returns `l * 255`
    /// without touching `hue2rgb`.
    #[test]
    fn hsl_matches_its_known_values() {
        let red = rgb_to_hsl(255.0, 0.0, 0.0);
        assert!(close(red.h, 0.0) && close(red.s, 1.0) && close(red.l, 0.5));

        // A light red: lightness above a half, which flips the saturation
        // formula to `d / (2 - max - min)`.
        let pale = rgb_to_hsl(255.0, 128.0, 128.0);
        assert!(close(pale.h, 0.0), "hue was {}", pale.h);
        assert!(close(pale.l, (255.0 + 128.0) / 2.0 / 255.0));
        assert!(pale.s > 0.99, "a pale red is still fully saturated in HSL");

        // The short circuit: `hslToRgb(anything, 0, l)` is a grey of `l*255`,
        // and is NOT routed through `hue2rgb`.
        for l in [0.0, 0.25, 0.5, 1.0] {
            let grey = hsl_to_rgb(123.0, 0.0, l);
            assert!(rgb_close(grey, Rgb::new(l * 255.0, l * 255.0, l * 255.0)));
        }
        // And the hue wraps the way `((h % 360) + 360) % 360` says.
        assert!(rgb_close(hsl_to_rgb(-120.0, 1.0, 0.5), hsl_to_rgb(240.0, 1.0, 0.5)));
        assert!(rgb_close(hsl_to_rgb(480.0, 1.0, 0.5), hsl_to_rgb(120.0, 1.0, 0.5)));
    }

    /// `hsvToRgb` wraps its hue the same way.
    #[test]
    fn hsv_wraps_its_hue() {
        assert!(rgb_close(hsv_to_rgb(-60.0, 1.0, 1.0), hsv_to_rgb(300.0, 1.0, 1.0)));
        assert!(rgb_close(hsv_to_rgb(720.0, 1.0, 1.0), hsv_to_rgb(0.0, 1.0, 1.0)));
    }

    /// CMYK's short circuit: pure black divides by zero without it.
    #[test]
    fn cmyk_short_circuits_on_black() {
        let k = rgb_to_cmyk(0.0, 0.0, 0.0);
        assert_eq!((k.c, k.m, k.y, k.k), (0.0, 0.0, 0.0, 100.0));
        assert!(rgb_close(cmyk_to_rgb(0.0, 0.0, 0.0, 100.0), Rgb::new(0.0, 0.0, 0.0)));
        // White is the other end: no ink at all.
        let w = rgb_to_cmyk(255.0, 255.0, 255.0);
        assert!(close(w.c, 0.0) && close(w.k, 0.0));
    }

    // ── Notations ───────────────────────────────────────────────────────────

    /// Every notation the family reads, and what it reads it as.
    #[test]
    fn the_accepted_notations_parse() {
        // The web's hex field: three digits doubled, six taken as they are,
        // with or without the `#`, and trimmed.
        assert_eq!(parse("#abc").unwrap().rgb.to_hex(), "#aabbcc");
        assert_eq!(parse("abc").unwrap().rgb.to_hex(), "#aabbcc");
        assert_eq!(parse("  #1A73E8  ").unwrap().rgb.to_hex(), "#1a73e8");
        assert_eq!(parse("1a73e8").unwrap().rgb.to_hex(), "#1a73e8");
        // Opacity: absent from every hex the web writes, so 100.
        assert_eq!(parse("#1a73e8").unwrap().opacity, 100.0);

        // The eight-digit form — this family's own addition, for the alpha
        // slider.
        let half = parse("#1a73e880").unwrap();
        assert_eq!(half.rgb.to_hex(), "#1a73e8");
        assert!(close(half.opacity, 128.0 / 255.0 * 100.0));
        assert_eq!(parse("#1a73e8ff").unwrap().opacity, 100.0);
        assert_eq!(parse("#1a73e800").unwrap().opacity, 0.0);

        // `rgba(…)` — the web's OWN output, from `rgbaFromHex`.
        let c = parse("rgba(26, 115, 232, 0.5)").unwrap();
        assert_eq!(c.rgb.to_hex(), "#1a73e8");
        assert!(close(c.opacity, 50.0));
        assert_eq!(parse("rgb(26,115,232)").unwrap().rgb.to_hex(), "#1a73e8");
        assert_eq!(parse("RGB(26, 115, 232)").unwrap().rgb.to_hex(), "#1a73e8");

        // `hsl(…)`, the second addition.
        assert_eq!(parse("hsl(0, 100%, 50%)").unwrap().rgb.to_hex(), "#ff0000");
        assert_eq!(parse("hsl(240, 100%, 50%)").unwrap().rgb.to_hex(), "#0000ff");
        let a = parse("hsla(120, 100%, 50%, 0.25)").unwrap();
        assert_eq!(a.rgb.to_hex(), "#00ff00");
        assert!(close(a.opacity, 25.0));
    }

    /// And everything else is refused, rather than guessed at.
    #[test]
    fn invalid_notations_are_refused() {
        for bad in [
            "",            // nothing
            "#",           // a hash and nothing else
            "#ab",         // two digits
            "#abcd",       // the four-digit shorthand — deliberately not read
            "#abcde",      // five
            "#abcdefa",    // seven
            "#abcdefabc",  // nine
            "#gggggg",     // not hex
            "rebeccapurple", // a named colour: the web has no name table
            "rgb(1,2)",    // too few arguments
            "rgb(1,2,3,4,5)", // too many
            "rgb(a,b,c)",  // not numbers
            "rgb(1,2,3",   // unterminated
            "hsl(0, 100, 50)", // the percent signs are required
            "hsl(0, 100%)",    // too few
            "##123456",    // only ONE leading hash is stripped
        ] {
            assert!(parse(bad).is_none(), "« {bad} » must be refused, it was not");
        }
    }

    /// The hex FIELD is stricter than [`parse`]: it is the web's own regex
    /// pair, and it accepts three or six digits and nothing else — not the
    /// eight-digit form this family added, and not a `rgb()` string.
    #[test]
    fn the_hex_field_takes_only_what_the_web_takes() {
        assert_eq!(parse_web_hex("abc").unwrap().to_hex(), "#aabbcc");
        assert_eq!(parse_web_hex("#AABBCC").unwrap().to_hex(), "#aabbcc");
        assert_eq!(parse_web_hex(" #1a73e8 ").unwrap().to_hex(), "#1a73e8");
        for refused in ["#1a73e8ff", "rgb(1,2,3)", "#abcd", "", "#12345"] {
            assert!(parse_web_hex(refused).is_none(), "the field must refuse « {refused} »");
        }

        // Through the picker, which is what a host actually calls.
        let mut p = ColorPicker::new(Color::default());
        assert!(p.apply_hex("#ff9900"));
        assert_eq!(p.color().rgb.to_hex(), "#ff9900");
        assert!(!p.apply_hex("#zz"), "a refused string leaves the colour alone");
        assert_eq!(p.color().rgb.to_hex(), "#ff9900");
        // The field shows six upper-case digits and no `#`.
        assert_eq!(p.hex_text(), "FF9900");
    }

    // ── Serialising ─────────────────────────────────────────────────────────

    /// `rgbToHex` clamps AFTER it rounds, and pads to two digits.
    #[test]
    fn hex_rounds_then_clamps() {
        assert_eq!(Rgb::new(0.4, 0.5, 1.5).to_hex(), "#000102");
        assert_eq!(Rgb::new(-20.0, 300.0, 255.4).to_hex(), "#00ffff");
        assert_eq!(Rgb::new(9.0, 10.0, 15.0).to_hex(), "#090a0f");
    }

    /// `rgbaFromHex(hex, opacity)` — the string the web hands to CSS.
    #[test]
    fn the_css_serialiser_matches_the_webs() {
        assert_eq!(Color::opaque(Rgb::new(26.0, 115.0, 232.0)).to_css(), "rgba(26, 115, 232, 1)");
        assert_eq!(Color::new(Rgb::new(0.0, 0.0, 0.0), 50.0).to_css(), "rgba(0, 0, 0, 0.5)");
        assert_eq!(Color::new(Rgb::new(0.0, 0.0, 0.0), 0.0).to_css(), "rgba(0, 0, 0, 0)");
        // Out of range on either side is clamped, as `rgbaFromHex` clamps.
        assert_eq!(Color::new(Rgb::default(), 250.0).to_css(), "rgba(0, 0, 0, 1)");
        assert_eq!(Color::new(Rgb::default(), -4.0).to_css(), "rgba(0, 0, 0, 0)");
    }

    /// `gradientToCss` — sorted by position, percentages rounded, and the two
    /// shapes spelled the way the web spells them.
    #[test]
    fn the_gradient_serialiser_matches_the_webs() {
        let g = Gradient::default();
        assert_eq!(
            g.to_css(),
            "linear-gradient(90deg, rgba(74, 144, 217, 1) 0%, rgba(155, 89, 182, 1) 100%)"
        );

        // Stops come out sorted even when they were stored out of order, and
        // the percentage is `Math.round(p * 100)`.
        let mut out = Gradient {
            kind: GradientKind::Radial,
            angle: 0.0,
            stops: vec![
                GradientStop::new(Rgb::new(255.0, 255.0, 255.0), 0.755, 40.0),
                GradientStop::new(Rgb::new(0.0, 0.0, 0.0), 0.0, 100.0),
            ],
        };
        assert_eq!(
            out.to_css(),
            "radial-gradient(circle, rgba(0, 0, 0, 1) 0%, rgba(255, 255, 255, 0.4) 76%)"
        );
        out.kind = GradientKind::Linear;
        out.angle = 45.4;
        assert!(out.to_css().starts_with("linear-gradient(45deg, "), "{}", out.to_css());
    }

    // ── Slider geometry ─────────────────────────────────────────────────────




    // ── The saturation/value area ───────────────────────────────────────────



    // ── Chips ───────────────────────────────────────────────────────────────

    /// Which chip is under a point — including the fact that a ROUND chip does
    /// not answer for its corners, and that the gap between two chips answers
    /// for neither.
    #[test]
    fn the_chip_under_a_point_is_the_one_drawn_there() {
        // Ten columns of 20 with a 4 gap: 10*20 + 9*4 = 236.
        let grid = SwatchGrid::fitting(236.0, 10, 4.0);
        assert_eq!(grid.cell, 20.0);
        assert_eq!(grid.rows(80), 8);
        assert_eq!(grid.height(80), 8.0 * 20.0 + 7.0 * 4.0);
        assert_eq!(grid.height(0), 0.0);

        let first = grid.cell_rect(0.0, 0.0, 0);
        assert_eq!((first.left, first.top, first.right, first.bottom), (0.0, 0.0, 20.0, 20.0));
        let eleventh = grid.cell_rect(0.0, 0.0, 10);
        assert_eq!((eleventh.left, eleventh.top), (0.0, 24.0), "the eleventh starts row two");
        let last_of_row = grid.cell_rect(0.0, 0.0, 9);
        assert_eq!(last_of_row.right, 236.0);

        // Square chips: the whole cell answers, the gap does not.
        assert_eq!(grid.cell_at(0.0, 0.0, 80, false, 1.0, 1.0), Some(0));
        assert_eq!(grid.cell_at(0.0, 0.0, 80, false, 25.0, 1.0), Some(1));
        assert_eq!(grid.cell_at(0.0, 0.0, 80, false, 21.0, 1.0), None, "the 4 DIP gap");
        assert_eq!(grid.cell_at(0.0, 0.0, 80, false, 1.0, 250.0), None, "below the last row");
        // Only the cells that exist answer.
        assert_eq!(grid.cell_at(0.0, 0.0, 3, false, 1.0, 25.0), None);

        // Round chips: the centre answers, the corner does not — a
        // `rounded-full` button does not receive a click on its corner in a
        // browser either.
        assert_eq!(grid.cell_at(0.0, 0.0, 80, true, 10.0, 10.0), Some(0));
        assert_eq!(grid.cell_at(0.0, 0.0, 80, true, 1.0, 1.0), None);
    }

    /// The picker's chip row is a FIXED 16 DIP cell that wraps, not a stretched
    /// one — `flex flex-wrap gap-1` over `w-4 h-4`.
    #[test]
    fn the_fixed_chip_row_wraps_instead_of_stretching() {
        let grid = SwatchGrid::fixed(236.0, 16.0, 4.0);
        assert_eq!(grid.cell, 16.0);
        // (236 + 4) / 20 = 12 columns.
        assert_eq!(grid.columns, 12);
        assert_eq!(grid.rows(12), 1);
        assert_eq!(grid.rows(13), 2);
        // A panel too narrow for even one chip still reports one column, so no
        // layout ever divides by zero.
        assert_eq!(SwatchGrid::fixed(1.0, 16.0, 4.0).columns, 1);
    }

    /// The quick picker finds its own colours by HEX, as the web does, and not
    /// by float identity.
    #[test]
    fn the_swatch_picker_selects_by_hex() {
        let mut s = SwatchPicker::new();
        assert_eq!(s.colors.len(), 80);
        s.select(Color::opaque(Rgb::new(255.0, 153.0, 0.0))); // #ff9900
        assert_eq!(s.selected, Some(12), "row two, column three of the Docs palette");
        // The same colour arrived at by arithmetic, not by parsing, still
        // matches: the comparison quantises both sides first.
        s.select(Color::opaque(Rgb::new(254.9, 152.6, 0.4)));
        assert_eq!(s.selected, Some(12));
        s.select(Color::opaque(Rgb::new(1.0, 2.0, 3.0)));
        assert_eq!(s.selected, None);
        // Opacity plays no part, exactly as in the web's own string compare.
        assert!(Color::new(Rgb::new(1.0, 2.0, 3.0), 10.0)
            .same_swatch(Color::new(Rgb::new(1.0, 2.0, 3.0), 90.0)));
    }

    /// A chip in the grid, and the `+` cell that always closes the custom row.
    #[test]
    fn the_custom_row_ends_with_the_add_button() {
        let mut s = SwatchPicker::new();
        let bounds = Rect::new(0.0, 0.0, m::SWATCHES_W, s.height_for_width(m::SWATCHES_W));
        // Empty: the row is just the `+`.
        assert_eq!(s.custom_at(bounds, -50.0, -50.0), None);
        s.custom = vec![Color::default(), Color::opaque(Rgb::new(255.0, 255.0, 255.0))];
        let content = s.content(bounds);
        let grid = SwatchGrid::fitting(content.right - content.left, s.columns, m::GRID_GAP);
        let top = s.custom_top(bounds);
        let plus = grid.cell_rect(content.left, top, 2);
        let cx = (plus.left + plus.right) / 2.0;
        let cy = (plus.top + plus.bottom) / 2.0;
        assert_eq!(s.custom_at(bounds, cx, cy), Some(2), "the third cell is the + button");
        // The panel grew by exactly nothing: two colours and the `+` still fit
        // in one row of ten.
        assert_eq!(s.height_for_width(m::SWATCHES_W), bounds.bottom);
    }

    // ── The picker's own layout ─────────────────────────────────────────────





    // ── The gradient model ──────────────────────────────────────────────────

    /// `sampleStop` at the ends, in the middle, and on an existing stop.
    #[test]
    fn sampling_a_gradient_lands_between_its_stops() {
        let g = Gradient {
            kind: GradientKind::Linear,
            angle: 90.0,
            stops: vec![
                GradientStop::new(Rgb::new(0.0, 0.0, 0.0), 0.0, 0.0),
                GradientStop::new(Rgb::new(255.0, 255.0, 255.0), 1.0, 100.0),
            ],
        };
        let mid = g.sample_stop(0.5);
        assert_eq!(mid.color.to_hex(), "#808080", "the midpoint of black → white");
        assert!(close(mid.opacity, 50.0));
        assert!(close(mid.position, 0.5));

        // A quarter of the way: 0.25 * 255 = 63.75, which `rgbToHex` rounds to
        // 64 — the quantisation the web performs on an inserted stop.
        assert_eq!(g.sample_stop(0.25).color.to_hex(), "#404040");
        // The opacity is rounded too, and by `Math.round`.
        assert!(close(g.sample_stop(0.255).opacity, 26.0));

        // Outside the range, the end stop is COPIED and only moved.
        let below = g.sample_stop(-0.5);
        assert_eq!(below.color.to_hex(), "#000000");
        assert!(close(below.position, -0.5), "the position is taken as given");
        let above = g.sample_stop(2.0);
        assert_eq!(above.color.to_hex(), "#ffffff");
        assert!(close(above.opacity, 100.0));

        // Landing exactly on a stop gives that stop back.
        assert_eq!(g.sample_stop(0.0).color.to_hex(), "#000000");
        assert_eq!(g.sample_stop(1.0).color.to_hex(), "#ffffff");
    }

    /// Sampling reads the SORTED order, so a gradient stored out of order
    /// still interpolates between the right pair.
    #[test]
    fn sampling_ignores_the_stored_order() {
        let g = Gradient {
            kind: GradientKind::Linear,
            angle: 0.0,
            stops: vec![
                GradientStop::new(Rgb::new(255.0, 255.0, 255.0), 1.0, 100.0),
                GradientStop::new(Rgb::new(0.0, 0.0, 0.0), 0.0, 100.0),
            ],
        };
        assert_eq!(g.sample_stop(0.5).color.to_hex(), "#808080");
    }

    /// Inserting selects the new stop; removing is refused at two.
    #[test]
    fn stops_are_inserted_freely_and_removed_down_to_two() {
        let mut g = Gradient::default();
        assert_eq!(g.stops.len(), 2);

        let i = g.add_stop(0.5);
        assert_eq!(i, 2, "the new stop is appended, and is the one to select");
        assert_eq!(g.stops.len(), 3);
        assert!(close(g.stops[i].position, 0.5));
        // It blends in: the midpoint of #4a90d9 → #9b59b6, whose channels land
        // on 114.5 / 116.5 / 199.5 and are rounded UP by `Math.round`.
        assert_eq!(g.stops[i].color.to_hex(), "#7375c8");

        assert!(g.remove_stop(1));
        assert_eq!(g.stops.len(), 2);
        assert!(!g.remove_stop(0), "a gradient needs two ends");
        assert_eq!(g.stops.len(), 2);
        assert!(!g.remove_stop(99), "and an index that does not exist changes nothing");
    }

    /// A linear gradient runs along its angle: 90° left to right, 0° bottom to
    /// top, 180° top to bottom — the CSS definition, which is what the browser
    /// applies to the web's own `gradientToCss` output.
    #[test]
    fn a_linear_gradient_runs_along_its_angle() {
        let mut g = Gradient {
            kind: GradientKind::Linear,
            angle: 90.0,
            stops: vec![
                GradientStop::new(Rgb::new(0.0, 0.0, 0.0), 0.0, 100.0),
                GradientStop::new(Rgb::new(255.0, 255.0, 255.0), 1.0, 100.0),
            ],
        };
        // 90°: the left edge is the first stop, the right edge the last.
        assert_eq!(g.sample_at(0.0, 0.5, 100.0, 100.0).rgb.to_hex(), "#000000");
        assert_eq!(g.sample_at(1.0, 0.5, 100.0, 100.0).rgb.to_hex(), "#ffffff");
        assert_eq!(g.sample_at(0.5, 0.5, 100.0, 100.0).rgb.to_hex(), "#808080");

        // 0° points UP: the bottom is the first stop.
        g.angle = 0.0;
        assert_eq!(g.sample_at(0.5, 1.0, 100.0, 100.0).rgb.to_hex(), "#000000");
        assert_eq!(g.sample_at(0.5, 0.0, 100.0, 100.0).rgb.to_hex(), "#ffffff");

        // 180° points down.
        g.angle = 180.0;
        assert_eq!(g.sample_at(0.5, 0.0, 100.0, 100.0).rgb.to_hex(), "#000000");
        assert_eq!(g.sample_at(0.5, 1.0, 100.0, 100.0).rgb.to_hex(), "#ffffff");

        // A radial one runs from the centre to the farthest corner.
        g.kind = GradientKind::Radial;
        assert_eq!(g.sample_at(0.5, 0.5, 100.0, 100.0).rgb.to_hex(), "#000000");
        assert_eq!(g.sample_at(0.0, 0.0, 100.0, 100.0).rgb.to_hex(), "#ffffff");
        assert_eq!(g.sample_at(1.0, 1.0, 100.0, 100.0).rgb.to_hex(), "#ffffff");
    }

    // ── The gradient panel ──────────────────────────────────────────────────

    /// A marker sits over the position it holds, and hangs below the bar.
    #[test]
    fn a_stop_marker_sits_on_its_position() {
        let p = GradientPicker::new(Gradient::default());
        let h = p.height_for_width(m::GRAD_W);
        let bounds = Rect::new(0.0, 0.0, m::GRAD_W, h);
        let bar = p.layout(bounds).bar;

        let first = p.stop_rect(bounds, 0).expect("stop 0");
        let last = p.stop_rect(bounds, 1).expect("stop 1");
        assert_eq!((first.left + first.right) / 2.0, bar.left, "position 0 is the bar's left");
        assert_eq!((last.left + last.right) / 2.0, bar.right, "position 1 is its right");
        assert_eq!(first.right - first.left, m::STOP);
        assert_eq!(first.bottom, bar.bottom + m::STOP_DROP, "`-bottom-1`");
        assert!(p.stop_rect(bounds, 2).is_none(), "there is no third stop");
    }

    /// A press on the bar reads a position; a press on a marker grabs the
    /// marker instead, because the web stops that event reaching the bar.
    #[test]
    fn a_marker_takes_the_press_before_the_bar_does() {
        let mut g = Gradient::default();
        g.stops[0].position = 0.25;
        let p = GradientPicker::new(g);
        let h = p.height_for_width(m::GRAD_W);
        let bounds = Rect::new(0.0, 0.0, m::GRAD_W, h);
        let bar = p.layout(bounds).bar;

        let marker = p.stop_rect(bounds, 0).expect("stop 0");
        let (mx, my) = ((marker.left + marker.right) / 2.0, (marker.top + marker.bottom) / 2.0);
        assert_eq!(p.stop_at(bounds, mx, my), Some(0));
        assert_eq!(p.part_at(bounds, mx, my), Some(GradientPart::Stop(0)));

        // The bar itself, well away from any marker.
        let bx = bar.left + (bar.right - bar.left) * 0.6;
        let by = (bar.top + bar.bottom) / 2.0;
        assert_eq!(p.part_at(bounds, bx, by), Some(GradientPart::Bar));
        assert!(close_px(p.position_at(bounds, bx), 0.6), "{}", p.position_at(bounds, bx));
        // And the position is clamped, wherever the pointer wandered.
        assert!(close(p.position_at(bounds, -900.0), 0.0));
        assert!(close(p.position_at(bounds, 9000.0), 1.0));
    }

    /// The angle row exists only on a linear gradient, and the bin only when a
    /// stop may actually be removed — both `&&`-guarded in the web.
    #[test]
    fn the_gradient_panel_hides_what_does_not_apply() {
        let mut p = GradientPicker::new(Gradient::default());
        let bounds = Rect::new(0.0, 0.0, m::GRAD_W, p.height_for_width(m::GRAD_W));
        assert!(p.layout(bounds).angle.is_some(), "a linear gradient has an angle");
        assert!(p.layout(bounds).bin.is_none(), "two stops cannot be reduced");

        p.gradient.add_stop(0.5);
        let bounds = Rect::new(0.0, 0.0, m::GRAD_W, p.height_for_width(m::GRAD_W));
        assert!(p.layout(bounds).bin.is_some(), "three stops can");

        p.gradient.kind = GradientKind::Radial;
        let taller = p.height_for_width(m::GRAD_W);
        p.gradient.kind = GradientKind::Linear;
        assert!(
            p.height_for_width(m::GRAD_W) > taller,
            "dropping the angle row makes the panel shorter"
        );
    }

    /// The selected index is CLAMPED on read, the way the web clamps it —
    /// removing the last stop must not leave the editor pointing past the end.
    #[test]
    fn the_selected_stop_is_clamped_not_trusted() {
        let mut p = GradientPicker::new(Gradient::default());
        p.selected = 99;
        assert_eq!(p.selected_stop().map(|s| s.position), Some(1.0), "clamped to the last");
        p.gradient.stops.clear();
        assert!(p.selected_stop().is_none(), "and an empty gradient answers nothing");
    }

    /// The two sliders are `TrackBar`s with the web's ranges, built from the
    /// model so they cannot drift from it.
    #[test]
    fn the_gradient_sliders_carry_the_models_values() {
        let mut p = GradientPicker::new(Gradient::default());
        p.gradient.angle = 217.6;
        assert_eq!(p.angle_slider().minimum(), 0);
        assert_eq!(p.angle_slider().maximum(), 360);
        assert_eq!(p.angle_slider().value(), 218, "rounded the way JavaScript rounds");

        p.gradient.stops[0].opacity = 42.0;
        p.selected = 0;
        assert_eq!(p.opacity_slider().maximum(), 100);
        assert_eq!(p.opacity_slider().value(), 42);
        // Out of range is clamped rather than refused by the replica.
        p.gradient.angle = 900.0;
        assert_eq!(p.angle_slider().value(), 360);
    }

    // ── The widgets' own surface ────────────────────────────────────────────

    /// Each primitive names itself, measures at the web's size, and reaches
    /// its replica through `Deref`.
    #[test]
    fn each_primitive_owns_a_replica() {
        let mut field = ColorField::new(Color::default());
        assert_eq!(field.type_name(), "ColorField");
        // Straight from `kubuno_controls::buttons::Button`, untouched.
        assert!(field.enabled);
        assert!(field.tab_stop);
        field.text = "Couleur du trait".to_string();
        assert_eq!(field.text, "Couleur du trait");
        field.enabled = false;
        assert!(!field.enabled);

        // A panel is not a tab stop and has no border — the replica's own
        // overrides, reached without being restated.
        let picker = ColorPicker::new(Color::default());
        assert_eq!(picker.type_name(), "ColorPicker");
        assert!(!picker.tab_stop);
        assert_eq!(SwatchPicker::new().type_name(), "SwatchPicker");
        assert_eq!(GradientPicker::default().type_name(), "GradientPicker");
        assert!(!GradientPicker::default().tab_stop);
    }

    /// The default sizes are the web's `width:` declarations, and the height
    /// is whatever the content comes to.
    #[test]
    fn the_intrinsic_widths_are_the_webs() {
        assert_eq!(m::FIELD_W, 32.0);
        assert_eq!(m::FIELD_H, 24.0);
        assert_eq!(m::PICKER_W, 312.0);
        assert_eq!(m::SWATCHES_W, 232.0);
        assert_eq!(m::GRAD_W, 260.0);
        // The SV square is the one derived number: round((212 - 44 - 12)/√2).
        assert_eq!(m::SV_SIDE, js_round((212.0 - 2.0 * 22.0 - 12.0) / 2.0_f64.sqrt()) as f32);
    }

    /// A `ColorField` answers for the rectangle it was painted into — the
    /// trait's default, half-open on the right and bottom.
    #[test]
    fn a_field_hit_test_is_its_rectangle() {
        let f = ColorField::new(Color::default());
        let r = Rect::new(10.0, 10.0, 42.0, 34.0);
        assert!(f.hit_test(r, 10.0, 10.0));
        assert!(f.hit_test(r, 41.9, 33.9));
        assert!(!f.hit_test(r, 42.0, 20.0));
        assert!(!f.hit_test(r, 9.9, 20.0));
    }

    // ── The popover's placement (`reposition()`) ────────────────────────────

    fn rect_eq(r: Rect, l: f32, t: f32, rr: f32, b: f32) -> bool {
        (r.left - l).abs() < 1e-4 && (r.top - t).abs() < 1e-4 && (r.right - rr).abs() < 1e-4 && (r.bottom - b).abs() < 1e-4
    }

    /// Room on the left: the popover opens there, 8 DIP from the swatch, top
    /// aligned with it.
    #[test]
    fn the_popover_prefers_the_left_of_its_swatch() {
        let screen = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let swatch = Rect::new(600.0, 100.0, 632.0, 124.0);
        let r = ColorField::popover_rect(swatch, (312.0, 400.0), screen);
        assert!(rect_eq(r, 600.0 - 312.0 - 8.0, 100.0, 600.0 - 8.0, 500.0), "{:?}", (r.left, r.top));
    }

    /// No room on the left: it opens to the RIGHT of the swatch instead.
    #[test]
    fn the_popover_falls_back_to_the_right() {
        let screen = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let swatch = Rect::new(100.0, 100.0, 132.0, 124.0);
        let r = ColorField::popover_rect(swatch, (312.0, 400.0), screen);
        assert_eq!(r.left, 132.0 + 8.0);
    }

    /// Too tall for the space below: pulled UP to keep an 8 DIP margin; and
    /// taller than the whole viewport: pinned to the top.
    #[test]
    fn the_popover_is_clamped_to_the_viewport() {
        let screen = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let swatch = Rect::new(600.0, 700.0, 632.0, 724.0);
        let r = ColorField::popover_rect(swatch, (312.0, 400.0), screen);
        assert_eq!(r.bottom, 800.0 - 8.0);
        let giant = ColorField::popover_rect(swatch, (312.0, 2000.0), screen);
        assert_eq!(giant.top, 8.0, "pinned to the top when it cannot fit");
        // Wider than the viewport too: pinned to the left margin.
        let wide = ColorField::popover_rect(swatch, (2000.0, 100.0), screen);
        assert_eq!(wide.left, 8.0);
        // A viewport that does not start at the origin — a second monitor, or
        // a work area in client coordinates — is honoured, not assumed.
        let off = Rect::new(-1200.0, -50.0, -200.0, 750.0);
        let s2 = Rect::new(-1150.0, -40.0, -1118.0, -16.0);
        let r2 = ColorField::popover_rect(s2, (312.0, 400.0), off);
        assert_eq!(r2.left, -1118.0 + 8.0, "no room on the left of the monitor");
        assert_eq!(r2.top, -40.0);
    }

    // ── Keys ────────────────────────────────────────────────────────────────

    #[test]
    fn the_navigation_keys_map_from_their_codes() {
        assert_eq!(ColorKey::from_vk(vk::LEFT), Some(ColorKey::Left));
        assert_eq!(ColorKey::from_vk(vk::PAGE_DOWN), Some(ColorKey::PageDown));
        assert_eq!(ColorKey::from_vk(vk::HOME), Some(ColorKey::Home));
        assert_eq!(ColorKey::from_vk(vk::ENTER), None);
    }

    /// `ColorChan`'s steps: 1, 10 with Shift, clamped; plus the ARIA ends.
    #[test]
    fn a_channel_steps_like_colorchan() {
        assert_eq!(channel_step(100.0, 360.0, ColorKey::Right, false), 101.0);
        assert_eq!(channel_step(100.0, 360.0, ColorKey::Up, true), 110.0);
        assert_eq!(channel_step(100.0, 360.0, ColorKey::Down, false), 99.0);
        assert_eq!(channel_step(5.0, 360.0, ColorKey::Left, true), 0.0, "clamped at zero");
        assert_eq!(channel_step(355.0, 360.0, ColorKey::Right, true), 360.0, "and at max");
        assert_eq!(channel_step(40.0, 100.0, ColorKey::Home, false), 0.0);
        assert_eq!(channel_step(40.0, 100.0, ColorKey::End, false), 100.0);
        assert_eq!(channel_step(40.0, 100.0, ColorKey::PageUp, false), 50.0);
    }

    // ── The edit buffer ─────────────────────────────────────────────────────

    #[test]
    fn a_draft_starts_with_everything_selected() {
        let d = FieldDraft::new(DraftKind::Hex, "1A73E8");
        assert_eq!(d.selection(), (0, 6));
        assert_eq!(d.selected_text(), "1A73E8");
        // Typing replaces the selection.
        let mut d = d;
        assert!(d.insert("f"));
        assert_eq!(d.text, "f");
        assert_eq!((d.caret, d.anchor), (1, 1));
    }

    /// The hex box refuses what is not hex, admits one `#` in front only, and
    /// stops at seven characters.
    #[test]
    fn the_hex_draft_filters_what_it_is_given() {
        let mut d = FieldDraft::new(DraftKind::Hex, "");
        assert!(d.insert("#12zz3G4"));
        assert_eq!(d.text, "#1234", "letters beyond f are dropped");
        assert!(!d.insert("#"), "a second # is refused");
        d.insert("abcdef");
        assert_eq!(d.text, "#1234ab", "capped at # + six digits");
        assert_eq!(d.digits(), "1234ab");
        // A `#` typed in the middle is refused too.
        let mut e = FieldDraft::new(DraftKind::Hex, "abc");
        e.move_to(1, false);
        assert!(!e.insert("#"));
        assert_eq!(e.text, "abc");
    }

    #[test]
    fn the_digits_draft_takes_digits_only() {
        let mut d = FieldDraft::new(DraftKind::Digits, "");
        d.insert("4a5-6.7");
        assert_eq!(d.text, "456", "letters, signs and a decimal point are refused; three digits max");
        assert_eq!(d.number(), Some(456.0));
        let empty = FieldDraft::new(DraftKind::Digits, "");
        assert_eq!(empty.number(), None);
    }

    #[test]
    fn backspace_delete_and_the_arrows_edit_like_a_text_box() {
        let mut d = FieldDraft::new(DraftKind::Hex, "abcdef");
        d.move_to(3, false);
        assert!(d.backspace());
        assert_eq!((d.text.as_str(), d.caret), ("abdef", 2));
        assert!(d.delete());
        assert_eq!(d.text, "abef");
        d.move_to(0, false);
        assert!(!d.backspace(), "nothing before the start");
        d.move_to(4, false);
        assert!(!d.delete(), "nothing after the end");

        // Shift+Left selects; a plain Right collapses to the selection's END.
        let mut d = FieldDraft::new(DraftKind::Hex, "abcdef");
        d.move_to(4, false);
        d.left(true);
        d.left(true);
        assert_eq!(d.selection(), (2, 4));
        d.right(false);
        assert_eq!((d.caret, d.anchor), (4, 4));
        // And a plain Left collapses to its START.
        d.left(true);
        d.left(false);
        assert_eq!((d.caret, d.anchor), (3, 3));
        // Backspace over a selection removes just the selection.
        d.select_all();
        assert!(d.backspace());
        assert_eq!(d.text, "");
    }

    /// A click lands the caret between the two characters it falls between.
    #[test]
    fn a_click_places_the_caret_nearest_to_it() {
        let d = FieldDraft::new(DraftKind::Hex, "abcd");
        // Every character 7 DIP wide.
        let w = |s: &str| s.len() as f32 * 7.0;
        assert_eq!(d.index_at(-5.0, w), 0);
        assert_eq!(d.index_at(3.0, w), 0);
        assert_eq!(d.index_at(4.0, w), 1);
        assert_eq!(d.index_at(15.0, w), 2);
        assert_eq!(d.index_at(999.0, w), 4);
    }

    // ── Grids and cursors ───────────────────────────────────────────────────

    #[test]
    fn a_grid_cursor_moves_by_cells_and_rows_and_stops_at_the_edges() {
        let g = SwatchGrid { columns: 10, gap: 4.0, cell: 20.0 };
        assert_eq!(g.step(0, 80, ColorKey::Left), 0, "no wrap at the start");
        assert_eq!(g.step(0, 80, ColorKey::Right), 1);
        assert_eq!(g.step(9, 80, ColorKey::Right), 10, "right runs on to the next row");
        assert_eq!(g.step(79, 80, ColorKey::Right), 79, "no wrap at the end");
        assert_eq!(g.step(3, 80, ColorKey::Up), 3);
        assert_eq!(g.step(3, 80, ColorKey::Down), 13);
        assert_eq!(g.step(73, 80, ColorKey::Down), 73);
        assert_eq!(g.step(35, 80, ColorKey::Home), 30);
        assert_eq!(g.step(35, 80, ColorKey::End), 39);
        assert_eq!(g.step(35, 80, ColorKey::PageUp), 5);
        assert_eq!(g.step(35, 80, ColorKey::PageDown), 75);
        // A short last row: End stops at the last cell, Down from above it
        // stays put when there is nothing under it.
        assert_eq!(g.step(20, 23, ColorKey::End), 22);
        assert_eq!(g.step(15, 23, ColorKey::Down), 15);
        assert_eq!(g.step(12, 23, ColorKey::Down), 22);
        assert_eq!(g.step(5, 23, ColorKey::PageDown), 15, "column 5 has no cell on the short row");
        assert_eq!(g.step(0, 0, ColorKey::Right), 0, "an empty grid");
    }

    /// The quick picker's cursor crosses from the palette into the custom row
    /// and back, keeping its column.
    #[test]
    fn the_swatch_cursor_crosses_between_the_two_grids() {
        let mut s = SwatchPicker::new();
        s.eyedropper = false; // the web without `window.EyeDropper`
        s.custom = vec![Color::default(); 3]; // three colours + the `+`
        let n = s.colors.len();
        assert_eq!(s.cell_count(), n + 4);
        assert_eq!(s.add_index(), n + 3);
        // Down from the palette's last row, column 2 → custom cell 2.
        assert_eq!(s.step_cursor(72, ColorKey::Down), n + 2);
        // Column 7 has no custom cell: lands on the last one (the `+`).
        assert_eq!(s.step_cursor(77, ColorKey::Down), n + 3);
        // Up from the custom row → the palette's last row, same column.
        assert_eq!(s.step_cursor(n + 1, ColorKey::Up), 71);
        // Within the custom row.
        assert_eq!(s.step_cursor(n, ColorKey::Right), n + 1);
        assert_eq!(s.step_cursor(n + 3, ColorKey::Right), n + 3);
        // Within the palette, the grid's own rules.
        assert_eq!(s.step_cursor(0, ColorKey::Down), 10);
        // Cells report their colour, and the `+` none.
        assert!(s.colour_at(0).is_some());
        assert!(s.colour_at(n + 2).is_some());
        assert!(s.colour_at(n + 3).is_none());
    }

    /// `addCustom`: newest first, no duplicate by hex, twenty at most.
    #[test]
    fn a_custom_colour_goes_first_once() {
        let mut s = SwatchPicker::new();
        let red = Color::opaque(Rgb::new(255.0, 0.0, 0.0));
        let blue = Color::opaque(Rgb::new(0.0, 0.0, 255.0));
        s.add_custom(red);
        s.add_custom(blue);
        s.add_custom(Color::new(Rgb::new(255.0, 0.0, 0.0), 50.0));
        assert_eq!(s.custom.len(), 2, "red came back to the front, not twice");
        assert!(s.custom[0].same_swatch(red));
        for i in 0..30 {
            s.add_custom(Color::opaque(Rgb::new(i as f64, 1.0, 2.0)));
        }
        assert_eq!(s.custom.len(), SwatchPicker::CUSTOM_MAX);
    }

    /// The flat index space: palette then custom row, each cell where it is
    /// drawn.
    #[test]
    fn the_flat_cells_answer_where_they_are_drawn() {
        let mut s = SwatchPicker::new();
        s.eyedropper = false;
        s.custom = vec![Color::default()];
        let bounds = Rect::new(0.0, 0.0, m::SWATCHES_W, s.height_for_width(m::SWATCHES_W));
        let n = s.colors.len();
        for i in [0, 9, 42, n, n + 1] {
            let r = s.cell_rect(bounds, i).expect("a cell");
            let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
            assert_eq!(s.cell_at(bounds, cx, cy), Some(i), "cell {i}");
        }
        assert!(s.cell_rect(bounds, n + 2).is_none());
    }

    // ── The picker's keyboard and boxes ─────────────────────────────────────









    // ── The gradient's keyboard ─────────────────────────────────────────────

    fn three_stops() -> GradientPicker {
        let mut g = Gradient::default();
        g.stops.push(GradientStop::new(Rgb::new(1.0, 2.0, 3.0), 0.5, 100.0));
        GradientPicker::new(g)
    }

    /// Markers are drawn in POSITION order, and the one on top where two
    /// overlap is the furthest right, whatever the stored order.
    #[test]
    fn markers_stack_in_position_order() {
        let mut p = three_stops();
        assert_eq!(p.draw_order(), vec![0, 2, 1]);
        // Stop 0 moved on top of stop 2: stop 2 (0.5) and stop 0 (0.51) overlap;
        // 0 is further right, so it is drawn last and answers the press.
        p.gradient.stops[0].position = 0.51;
        let b = Rect::new(0.0, 0.0, m::GRAD_W, p.height_for_width(m::GRAD_W));
        let r = p.stop_rect(b, 2).expect("stop 2");
        let (x, y) = ((r.left + r.right) / 2.0 + 1.0, (r.top + r.bottom) / 2.0);
        assert_eq!(p.stop_at(b, x, y), Some(0));
    }

    #[test]
    fn the_markers_take_the_arrows() {
        let mut p = three_stops();
        p.selected = 2; // position 0.5
        assert!(p.key(GradientPart::Stop(2), ColorKey::Right, false));
        assert!(close_px(p.gradient.stops[2].position, 0.51));
        p.key(GradientPart::Stop(2), ColorKey::Left, true);
        assert!(close_px(p.gradient.stops[2].position, 0.41));
        p.key(GradientPart::Stop(2), ColorKey::End, false);
        assert_eq!(p.gradient.stops[2].position, 1.0);
        p.gradient.stops[2].position = 0.5;
        // Up / Down walk the stops along the bar: 0 → 2 → 1.
        p.selected = 0;
        p.key(GradientPart::Stop(0), ColorKey::Down, false);
        assert_eq!(p.selected, 2);
        p.key(GradientPart::Stop(2), ColorKey::Down, false);
        assert_eq!(p.selected, 1);
        assert!(!p.key(GradientPart::Stop(1), ColorKey::Down, false), "the last stays the last");
        p.key(GradientPart::Stop(1), ColorKey::Up, false);
        assert_eq!(p.selected, 2);
    }

    #[test]
    fn the_gradient_sliders_and_boxes_take_the_keyboard() {
        let mut p = three_stops();
        p.gradient.angle = 90.0;
        p.key(GradientPart::Angle, ColorKey::Right, true);
        assert_eq!(p.gradient.angle, 100.0);
        p.selected = 1;
        p.key(GradientPart::Opacity, ColorKey::Home, false);
        assert_eq!(p.gradient.stops[1].opacity, 0.0);
        p.key(GradientPart::Position, ColorKey::Down, false);
        assert!(close(p.gradient.stops[1].position, 0.99));
        assert_eq!(p.draft(GradientPart::Position).map(|d| d.text.clone()), Some("99".to_string()));
        // Typed values are clamped the way the web's `onChange` clamps them.
        p.set_box_value(GradientPart::AngleBox, 999.0);
        assert_eq!(p.gradient.angle, 360.0);
        p.set_box_value(GradientPart::Position, 250.0);
        assert_eq!(p.gradient.stops[1].position, 1.0);
        p.begin_edit(GradientPart::OpacityBox);
        if let Some((_, d)) = p.edit.as_mut() {
            d.insert("42");
        }
        p.apply_edit();
        assert_eq!(p.gradient.stops[1].opacity, 42.0);
    }

    /// `removeStop` then `setSel(0)`.
    #[test]
    fn removing_the_selected_stop_selects_the_first() {
        let mut p = three_stops();
        p.selected = 2;
        assert!(p.remove_selected());
        assert_eq!(p.selected, 0);
        assert!(!p.remove_selected(), "two stops remain, and stay");
    }

    /// Every stop of the panel is reachable, in visual order; the boxes and
    /// the field are parts of their own.
    #[test]
    fn the_gradient_panel_has_every_stop_in_order() {
        let mut p = three_stops();
        p.closable = true; // the popover's picker, with its ✕
        let b = Rect::new(0.0, 0.0, m::GRAD_W, p.height_for_width(m::GRAD_W));
        let slots: Vec<usize> = p.tab_stops(b).iter().map(|(part, _)| part.focus_slot()).collect();
        assert_eq!(slots, (0..12).collect::<Vec<_>>());
        let g = p.layout(b);
        let mid = |r: Rect| ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
        let (x, y) = mid(g.field);
        assert_eq!(p.part_at(b, x, y), Some(GradientPart::Field));
        let (x, y) = mid(GradientPicker::row_box(g.opacity));
        assert_eq!(p.part_at(b, x, y), Some(GradientPart::OpacityBox));
        let (x, y) = mid(GradientPicker::slider_track(g.opacity));
        assert_eq!(p.part_at(b, x, y), Some(GradientPart::Opacity));
        let angle = g.angle.expect("linear");
        let (x, y) = mid(GradientPicker::row_box(angle));
        assert_eq!(p.part_at(b, x, y), Some(GradientPart::AngleBox));
        // A drag across the angle track covers the whole range.
        let track = GradientPicker::slider_track(angle);
        assert_eq!(p.angle_at(b, track.left - 50.0), Some(0.0));
        assert_eq!(p.angle_at(b, track.right + 50.0), Some(360.0));
    }
}
