//! Kubuno primitives — the button family.
//!
//! [`Button`], [`IconButton`], [`CheckBox`], [`RadioButton`], [`ToggleButton`]
//! and [`Switch`]: everything that is clicked once and reports a decision.
//!
//! ## What is owned and what is reached through `Deref`
//!
//! Each primitive owns its WinForms replica and derefs to it, so `text`,
//! `enabled`, `padding`, `dock`, `anchor`, `min_size`, `check_state`,
//! `three_state`, `auto_check`, `check_align`, `appearance` and the click
//! state machines (`perform_click`, the three-state cycle,
//! [`kubuno_desktop_controls::buttons::select_radio`]) are **the replica's** — there is
//! no second copy of them here. What this layer adds is what .NET has no
//! concept of: a [`Variant`], a [`Size`], and Kubuno pixels.
//!
//! ## The reference is the web
//!
//! Every number and colour is read from `core/frontend/src/ui/`:
//! `Button.tsx` (the `VARIANT` / `SIZE` maps and the `BASE` class list),
//! `Checkbox.tsx` + `checkboxCanvas.ts`, `Radio.tsx` + `radioCanvas.ts`, and
//! `Toggle.tsx` + `toggleCanvas.ts` for the switch. The canvas painters are the
//! code the browser actually runs, so their constants are transcribed rather
//! than measured.
//!
//! Three of these had a hand-written predecessor the shell and Drive painted
//! with ([`kubuno_drive_desktop_app_controls::button::draw`],
//! [`kubuno_drive_desktop_app_controls::button::draw_icon_button`],
//! [`kubuno_drive_desktop_app_controls::switch::draw`]). The geometry is still theirs (the
//! tests pin it), but where the predecessor and the web disagree the web wins:
//! the `secondary` outline is `border-border-strong`, the switch's off track is
//! `--color-surface-3` and a disabled switch is `opacity-50`, and the whole
//! family paints the web's `focus-visible:ring-2 ring-primary ring-offset-1`
//! — only when [`WidgetState::show_focus_ring`] says so, so a mouse click
//! never leaves a ring behind.
//!
//! ## Composition rules
//!
//! * **Transparent ground.** Nothing here paints a background it does not
//!   own: a ghost button at rest, an idle round button, and the label area of
//!   a check box, radio or switch show their parent's surface, as the web's
//!   `bg-transparent` does. A control nested in a Card reads as part of it.
//! * **Text never spills.** A button label that does not fit its content box
//!   is ellipsised; a check box, radio or switch label wraps (the web's
//!   `min-w-0` flex column) when the caller gives it the height
//!   [`CheckBox::height_for_width`] asks for, and ellipsises its last visible
//!   line otherwise.
//! * **The focus ring is the one declared overflow**: `ring-offset-1 ring-2`
//!   sits 1 to 3 DIP OUTSIDE the control, exactly as the web's box-shadow
//!   does. Leave [`FOCUS_OUTSET`] of room around a focusable control.

use std::ops::{Deref, DerefMut};

use kubuno_drive_desktop_app_controls::button as web_button;
use kubuno_drive_desktop_app_controls::themes::shape::{ShadowLayer, SHADOW_BLACK};
use kubuno_drive_desktop_app_controls::{Canvas, Rect, Theme};
use kubuno_desktop_controls::buttons as replica;
use kubuno_desktop_controls::enums::{Appearance, CheckState, ContentAlignment, Size as ControlSize};
use kubuno_desktop_controls::host::{vk, Cursor};
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{IDWriteTextFormat, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING};

use crate::metrics::{control, height, pill, radius};
use crate::widget::{Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────────
// The family's own metrics.
//
// `crate::metrics` re-exports the shape tokens and adds what the web never
// described. What is left below is what the web DOES describe but the token
// table does not carry — a button's per-size padding, the switch's track — each
// with the file it was read from. Nothing here is multiplied by `Canvas::scale`.
// ─────────────────────────────────────────────────────────────────────────────

/// `rounded-md`, which this design system resolves to **4** (`radius::SM`), not
/// the 6 the Tailwind name suggests.
///
/// Never overridable, and a `const` for that reason: `Button.tsx` says so in a
/// comment (« rounded-md is FIXED — border radius is never overridable ») and
/// its stylesheet order makes it true. Identical to
/// [`kubuno_drive_desktop_app_controls::button::RADIUS`], which a test pins.
pub const RADIUS: f32 = radius::SM;

/// How far the focus ring reaches OUTSIDE a control's bounds:
/// `ring-offset-1` + `ring-2` = 3 DIP. A layout that packs focusable controls
/// edge to edge leaves at least this much room, or the ring is overpainted.
pub const FOCUS_OUTSET: f32 = focus_metrics::OFFSET + focus_metrics::RING;

/// The alpha `disabled:opacity-50` comes to — `Button.tsx`'s BASE class list,
/// and `Checkbox.tsx`/`Radio.tsx`/`Toggle.tsx` apply the same 0.5 to their
/// whole label.
const DISABLED_ALPHA: f32 = 0.5;

/// A `Danger` button does not change hue on hover, it fades:
/// `hover:opacity-90 active:opacity-80`.
const DANGER_HOVER_ALPHA: f32 = 0.9;
const DANGER_ACTIVE_ALPHA: f32 = 0.8;

/// `focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-1`
/// — `Button.tsx`'s BASE, and the canvas element of `Checkbox.tsx`,
/// `Radio.tsx` and `Toggle.tsx`.
mod focus_metrics {
    pub const RING: f32 = 2.0;
    pub const OFFSET: f32 = 1.0;
}

/// The loading state of a [`Button`]: `h-4 w-4 rounded-full border-2
/// border-current border-t-transparent animate-spin` (`Button.tsx`).
mod loading_metrics {
    pub const BOX: f32 = 16.0;
    pub const STROKE: f32 = 2.0;
    /// The longest step, in DIP of arc, between two dabs of the rasterised
    /// ring, as a fraction of the stroke — the value
    /// [`crate::feedback::Spinner::DAB_PITCH`] settled on, for the same reason
    /// (`Canvas` has no arc primitive).
    pub const DAB_PITCH: f32 = 0.5;
}

/// The glyph controls' text block.
mod glyph_metrics {
    /// The label's line box. The label is the body size (`text::BODY`, the
    /// web's 13.5) over a 20 line — the same line height the replica layer
    /// measures a `Body` role with, and the web's `leading-snug` rounded.
    pub const LINE: f32 = 20.0;
    /// `mt-0.5` between the label and its description (`Checkbox.tsx`).
    pub const DESC_GAP: f32 = 2.0;
    /// Measured widths are fractional and the text layout that paints them may
    /// round the other way: one DIP of slack stops the last glyph from being
    /// trimmed (« Clai|r »).
    pub const SLACK: f32 = 1.0;
    /// A line « fits » when it overflows the bounds by less than this —
    /// absorbs float noise from callers that add heights up.
    pub const FIT_EPS: f32 = 0.5;

    /// The indeterminate dash's height, as `paintCheckbox` computes it:
    /// `max(2, round(border))`. Written as the painter writes it rather than
    /// as the 2 it currently comes to, so a thicker border stays legal.
    pub fn dash_height() -> f32 {
        crate::metrics::control::CHECK_BORDER.round().max(2.0)
    }
}

/// The switch's text block — `Toggle.tsx`.
mod switch_text_metrics {
    /// `gap-2.5` between the track and the text column.
    pub const GAP: f32 = 10.0;
    /// `mt-0.5` on the track when a label is present, aligning it with the
    /// first line.
    pub const TRACK_TOP: f32 = 2.0;
    /// `text-sm leading-5` label line.
    pub const LINE: f32 = 20.0;
    /// `text-xs` description line, under a `gap-0.5`.
    pub const DESC_LINE: f32 = 16.0;
    pub const DESC_GAP: f32 = 2.0;
}

/// The thumb's `shadowColor rgb(0 0 0 / 12%)`, `shadowBlur 2`,
/// `shadowOffsetY 1` (`toggleCanvas.ts`).
const THUMB_SHADOW: ShadowLayer = ShadowLayer { dy: 1.0, blur: 2.0, spread: 0.0, opacity: 0.12 };

/// The same colour at a different alpha — how this design system dims.
/// Identical to the predecessor's private `dim` and to the public
/// [`kubuno_drive_desktop_app_controls::switch::fade`], which a test pins.
fn fade(c: D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * alpha, ..c }
}

// ─────────────────────────────────────────────────────────────────────────────
// Motion — the check, radio and switch transitions.
// ─────────────────────────────────────────────────────────────────────────────

/// `Checkbox.tsx` / `Radio.tsx`: `const DURATION = 100`.
pub const CHECK_TRANSITION_MS: u32 = 100;
/// `Toggle.tsx`: `const DURATION = 150`.
pub const SWITCH_TRANSITION_MS: u32 = 150;

/// Samples `cubic-bezier(x1, y1, x2, y2)` at time fraction `t` — a port of
/// `cubicBezier` in `core/frontend/src/ui/easing.ts` (Newton-Raphson, then
/// bisection where the curve is flat).
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let sample_x = |u: f32| ((ax * u + bx) * u + cx) * u;
    let sample_y = |u: f32| ((ay * u + by) * u + cy) * u;
    let slope_x = |u: f32| (3.0 * ax * u + 2.0 * bx) * u + cx;
    const EPSILON: f32 = 1e-6;

    let mut u = t;
    for _ in 0..8 {
        let dx = sample_x(u) - t;
        if dx.abs() < EPSILON {
            return sample_y(u);
        }
        let slope = slope_x(u);
        if slope.abs() < EPSILON {
            break;
        }
        u -= dx / slope;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    u = t;
    for _ in 0..32 {
        if hi - lo <= EPSILON {
            break;
        }
        let dx = sample_x(u) - t;
        if dx.abs() < EPSILON {
            break;
        }
        if dx > 0.0 {
            hi = u;
        } else {
            lo = u;
        }
        u = (lo + hi) / 2.0;
    }
    sample_y(u)
}

/// The design system's standard curve, `cubic-bezier(0.4, 0, 0.2, 1)` —
/// `easeStandard` in `easing.ts`.
pub fn ease_standard(t: f32) -> f32 {
    cubic_bezier(0.4, 0.0, 0.2, 1.0, t)
}

/// A value easing from `from` to `to` over `duration_ms` along
/// [`ease_standard`] — what `animateTo` in the web's canvas controls does.
///
/// The control owns no clock: the caller keeps one of these per animated
/// control, [`Transition::retarget`]s it when the model changes, and paints
/// with [`Transition::value`] (e.g. [`CheckBox::paint_progress`]), asking for
/// a repaint while [`Transition::running`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    pub from: f32,
    pub to: f32,
    pub start_ms: u64,
    pub duration_ms: u32,
}

impl Transition {
    /// At rest on `value` — the first paint shows the final state outright,
    /// as the web's `painted` ref ensures.
    pub const fn settled(value: f32) -> Self {
        Self { from: value, to: value, start_ms: 0, duration_ms: 0 }
    }

    /// Where the value is at `now_ms`.
    pub fn value(&self, now_ms: u64) -> f32 {
        if self.duration_ms == 0 || now_ms >= self.start_ms + u64::from(self.duration_ms) {
            return self.to;
        }
        let elapsed = now_ms.saturating_sub(self.start_ms) as f32;
        let t = elapsed / self.duration_ms as f32;
        self.from + (self.to - self.from) * ease_standard(t)
    }

    /// Whether it is still moving at `now_ms`.
    pub fn running(&self, now_ms: u64) -> bool {
        self.duration_ms > 0 && now_ms < self.start_ms + u64::from(self.duration_ms)
    }

    /// Heads for `to`, starting from wherever it is now (an interrupted
    /// animation reverses smoothly, like the web's `from = progress.current`).
    /// A no-op when it already heads there.
    pub fn retarget(&mut self, to: f32, now_ms: u64, duration_ms: u32) {
        if to == self.to {
            return;
        }
        self.from = self.value(now_ms);
        self.to = to;
        self.start_ms = now_ms;
        self.duration_ms = duration_ms;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Keyboard — the native `<button>`, `<input type=checkbox|radio>` behaviour.
// ─────────────────────────────────────────────────────────────────────────────

/// A focused `<button>` is activated by Enter and by Space.
pub fn is_button_activation(key: u16) -> bool {
    key == vk::ENTER || key == vk::SPACE
}

/// A focused check box or switch (`<input type=checkbox>`) toggles on Space
/// only — Enter submits the form instead, it does not toggle.
pub fn is_check_toggle(key: u16) -> bool {
    key == vk::SPACE
}

/// Where an arrow key moves the selection inside a radio group, as the
/// browser does for same-`name` radios: Down/Right go to the next enabled
/// option, Up/Left to the previous one, wrapping at both ends and skipping
/// disabled options. `None` for any other key, or when no other option is
/// enabled.
pub fn radio_arrow_target(current: usize, enabled: &[bool], key: u16) -> Option<usize> {
    let n = enabled.len();
    if n == 0 {
        return None;
    }
    let forward = match key {
        vk::DOWN | vk::RIGHT => true,
        vk::UP | vk::LEFT => false,
        _ => return None,
    };
    let start = current.min(n - 1);
    for step in 1..n {
        let i = if forward { (start + step) % n } else { (start + n - step) % n };
        if enabled[i] {
            return Some(i);
        }
    }
    None
}

/// The one option of a radio group that is a Tab stop (roving tabindex): the
/// checked one when it is enabled, else the first enabled option.
pub fn radio_tab_stop(checked: Option<usize>, enabled: &[bool]) -> Option<usize> {
    match checked {
        Some(i) if enabled.get(i).copied().unwrap_or(false) => Some(i),
        _ => enabled.iter().position(|&e| e),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Variant, Size, State — what Kubuno adds on top of the replica.
// ─────────────────────────────────────────────────────────────────────────────

/// The web's `VARIANT` map (`core/frontend/src/ui/Button.tsx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    /// Filled accent: the action that carries a page.
    #[default]
    Primary,
    /// A surface with a border.
    Secondary,
    /// No fill until hovered.
    Ghost,
    /// The accent WITHOUT a fill — a dialog's confirming action next to a
    /// `Ghost` cancel.
    Text,
    /// Filled danger.
    Danger,
    /// Destructive without a fill. Its own variant on the web too, because two
    /// colour utilities on one element are settled by stylesheet order and the
    /// override silently lost.
    TextDanger,
}

impl From<Variant> for web_button::Variant {
    /// So a caller — and the gallery's parity page — can drive the predecessor
    /// from the same value.
    fn from(v: Variant) -> Self {
        match v {
            Variant::Primary => web_button::Variant::Primary,
            Variant::Secondary => web_button::Variant::Secondary,
            Variant::Ghost => web_button::Variant::Ghost,
            Variant::Text => web_button::Variant::Text,
            Variant::Danger => web_button::Variant::Danger,
            Variant::TextDanger => web_button::Variant::TextDanger,
        }
    }
}

/// The web's `SIZE` map: `h-8/h-9/h-11` over `px-3/px-4/px-5`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Size {
    Sm,
    #[default]
    Md,
    Lg,
}

impl Size {
    /// From the shape tokens, so a height is never stated twice.
    pub fn height(self) -> f32 {
        match self {
            Size::Sm => height::BUTTON_SM,
            Size::Md => height::BUTTON_MD,
            Size::Lg => height::BUTTON_LG,
        }
    }

    /// `px-3 / px-4 / px-5`. The spacing scale has no 20, so the three are
    /// stated here rather than assembled from `space::*` — two of them would
    /// be a token and the third a literal, which is worse than three literals
    /// with one source.
    pub fn pad_x(self) -> f32 {
        match self {
            Size::Sm => 12.0,
            Size::Md => 16.0,
            Size::Lg => 20.0,
        }
    }

    /// `gap-1.5` on the small size, `gap-2` on the others.
    pub fn gap(self) -> f32 {
        match self {
            Size::Sm => 6.0,
            Size::Md | Size::Lg => 8.0,
        }
    }

    /// The glyph inside a button: `size-4` on `sm`, else 18. Measured by the
    /// predecessor (its `Button::icon_size` is private, so this number is
    /// taken from its doc comment rather than compared against it in a test).
    pub fn icon(self) -> f32 {
        match self {
            Size::Sm => 16.0,
            Size::Md | Size::Lg => 18.0,
        }
    }
}

impl From<Size> for web_button::Size {
    fn from(s: Size) -> Self {
        match s {
            Size::Sm => web_button::Size::Sm,
            Size::Md => web_button::Size::Md,
            Size::Lg => web_button::Size::Lg,
        }
    }
}

/// The four states this family's fill distinguishes — the predecessor's own
/// [`kubuno_drive_desktop_app_controls::button::State`], reached from a [`WidgetState`] plus
/// the model's `enabled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Rest,
    Hover,
    Active,
    Disabled,
}

impl State {
    /// The ladder, in the toolkit's order and for the toolkit's reasons:
    /// **disabled first**, because a dead control is never hot or pressed (the
    /// toolkit stops routing mouse messages to it, which is what the replica's
    /// own `effective_state` encodes); then **pressed before hot**, because the
    /// pointer is necessarily over what it is pressing, so testing hot first
    /// would make a press unreachable.
    ///
    /// `focused` does not appear: focus changes no fill. The ring is painted on
    /// top, from [`WidgetState::show_focus_ring`].
    pub fn of(enabled: bool, state: WidgetState) -> Self {
        if !enabled || state.disabled {
            State::Disabled
        } else if state.pressed {
            State::Active
        } else if state.hot {
            State::Hover
        } else {
            State::Rest
        }
    }
}

impl From<State> for web_button::State {
    fn from(s: State) -> Self {
        match s {
            State::Rest => web_button::State::Rest,
            State::Hover => web_button::State::Hover,
            State::Active => web_button::State::Active,
            State::Disabled => web_button::State::Disabled,
        }
    }
}

/// The cursor a control shows under the pointer. Tailwind v4's preflight
/// leaves `<button>` on the default arrow; the check box, radio and switch
/// labels ask for `cursor: pointer`; every disabled control says
/// `cursor-not-allowed`.
fn cursor_of(clickable_label: bool, disabled: bool) -> Cursor {
    if disabled {
        Cursor::NotAllowed
    } else if clickable_label {
        Cursor::Hand
    } else {
        Cursor::Arrow
    }
}

/// The fill, the label colour and the border, for one state — the shape of
/// the predecessor's private `Paint`.
struct Ink {
    fill: Option<D2D1_COLOR_F>,
    label: D2D1_COLOR_F,
    border: Option<D2D1_COLOR_F>,
}

/// The variant × state colour table, transcribed from `Button.tsx`'s `VARIANT`
/// map.
fn ink_for(variant: Variant, t: &Theme, state: State) -> Ink {
    let hovered = matches!(state, State::Hover);
    let pressed = matches!(state, State::Active);
    let p = match variant {
        // `hover:bg-primary-hover active:bg-primary-hover` — one colour for
        // both, as the web writes it.
        Variant::Primary => Ink {
            fill: Some(if hovered || pressed { t.accent_hover } else { t.accent }),
            label: t.accent_foreground,
            border: None,
        },
        // `bg-white border border-border-strong hover:bg-surface-1
        // active:bg-surface-2` — the STRONG border: « a button is an
        // actionable target and needs a firmer outline than the hairlines ».
        Variant::Secondary => Ink {
            fill: Some(if pressed {
                t.surface_2
            } else if hovered {
                t.card_background
            } else {
                t.layer_background
            }),
            label: t.text_primary,
            border: Some(t.border_strong),
        },
        Variant::Ghost => Ink {
            fill: if pressed {
                Some(t.surface_3)
            } else if hovered {
                Some(t.surface_2)
            } else {
                None
            },
            label: t.text_secondary,
            border: None,
        },
        Variant::Text => Ink {
            fill: (hovered || pressed).then_some(t.accent_light),
            label: t.accent,
            border: None,
        },
        // `hover:opacity-90 active:opacity-80`: the fill itself fades rather
        // than changing hue.
        Variant::Danger => Ink {
            fill: Some(if pressed {
                fade(t.danger, DANGER_ACTIVE_ALPHA)
            } else if hovered {
                fade(t.danger, DANGER_HOVER_ALPHA)
            } else {
                t.danger
            }),
            label: t.accent_foreground,
            border: None,
        },
        Variant::TextDanger => Ink {
            fill: (hovered || pressed).then_some(t.danger_light),
            label: t.danger,
            border: None,
        },
    };
    if matches!(state, State::Disabled) {
        return dim_ink(p);
    }
    p
}

/// `disabled:opacity-50` over the whole control — fill, label and border.
fn dim_ink(p: Ink) -> Ink {
    Ink {
        fill: p.fill.map(|f| fade(f, DISABLED_ALPHA)),
        label: fade(p.label, DISABLED_ALPHA),
        border: p.border.map(|b| fade(b, DISABLED_ALPHA)),
    }
}

/// The focus ring: `ring-2 ring-primary ring-offset-1` around `shape`, whose
/// corner radius is `corner`. The ring lies 1 to 3 DIP outside `shape`, with
/// its corners grown by the same amount so it stays concentric.
fn paint_focus_ring(c: &dyn Canvas, shape: Rect, corner: f32, colour: &D2D1_COLOR_F) {
    let out = FOCUS_OUTSET;
    let ring = shape.inflate(out, out);
    c.stroke_rounded_w(&ring, corner + out, colour, focus_metrics::RING);
}

// ═════════════════════════════════════════════════════════════════════════════
// Button
// ═════════════════════════════════════════════════════════════════════════════

/// A command button.
///
/// The model — `text`, `enabled`, `padding`, `dock`, `anchor`, `min_size`,
/// `dialog_result`… — is [`kubuno_desktop_controls::buttons::Button`], reached through
/// [`Deref`]. Only `variant`, `size`, the leading `icon`, the overridable
/// `pad_x` and the `loading` state are declared here, because .NET has no
/// concept for any of them.
///
/// ```ignore
/// let mut b = kubuno_desktop_ui::buttons::Button::new("Envoyer");
/// b.text = "Envoyer maintenant".into();   // the replica's field
/// b.variant = Variant::Primary;           // Kubuno's
/// ```
pub struct Button {
    inner: replica::Button,
    pub variant: Variant,
    pub size: Size,
    /// A leading icon, drawn before the label. `'static` because an icon name
    /// is a compile-time constant, as [`Canvas::vector_icon`] requires.
    pub icon: Option<&'static str>,
    /// Overrides the size's horizontal padding — the one metric the web lets a
    /// caller change (the waffle's edit buttons use `px-5` and `px-6`).
    pub pad_x: Option<f32>,
    /// `loading` on the web: the content is replaced by a spinning ring in the
    /// label's colour and the button is disabled (`disabled || loading`).
    pub loading: bool,
    /// Where the loading ring is in its turn, `0.0..1.0` (wraps). The host
    /// advances it — `animate-spin` is one turn per second, so
    /// `(now_ms % 1000) / 1000` is the web's cadence.
    pub loading_phase: f32,
    /// Room for an image (`ButtonBase.Image`, in DIP) the caller draws itself at
    /// [`Button::image_rect`]; the label moves by the replica's `ImageAlign` and
    /// `TextImageRelation`.
    pub image_size: Option<(f32, f32)>,
    /// The label's character to underline (a `&Save` mnemonic, while Alt is held).
    pub mnemonic: Option<usize>,
    /// The icon's own size in DIP (`IconSize`), instead of the size's ([`Size::icon`]).
    pub icon_size: Option<f32>,
    /// The gap between the icon (or the image) and the label (`IconSpacing`), instead of the size's.
    pub gap: Option<f32>,
    /// The icon is laid out like an image — by `ImageAlign` and `TextImageRelation`, see
    /// [`aligned_face`] — rather than before the label.
    pub icon_aligned: bool,
}

/// Where the text and the image of a button face go when the face is laid out
/// the WinForms way (`TextAlign`, `ImageAlign`, `TextImageRelation`) — see
/// [`aligned_face`].
#[derive(Clone, Copy)]
pub struct AlignedFace {
    /// The label's box (exactly its width, one line high).
    pub text: Option<Rect>,
    pub image: Option<Rect>,
}

/// Lays out a label of `text_w` × `line_h` and an image of `image` inside `rect` (inset by `pad`
/// horizontally): each placed by its own alignment when they overlay, else the image before/after or
/// above/below the text as one block placed by `text_align`, `gap` apart. Pure.
#[allow(clippy::too_many_arguments)]
pub fn aligned_face(
    rect: Rect,
    pad: f32,
    text_w: Option<f32>,
    line_h: f32,
    text_align: ContentAlignment,
    image: Option<(f32, f32)>,
    image_align: ContentAlignment,
    relation: replica::TextImageRelation,
    gap: f32,
) -> AlignedFace {
    use replica::TextImageRelation as R;
    let content = Rect::new((rect.left + pad).min(rect.right), rect.top + 2.0, (rect.right - pad).max(rect.left), (rect.bottom - 2.0).max(rect.top));
    let place = |w: f32, h: f32, a: ContentAlignment| {
        let (fx, fy) = a.fractions();
        let x = content.left + (content.right - content.left - w) * fx;
        let y = content.top + (content.bottom - content.top - h) * fy;
        Rect::new(x, y, x + w, y + h)
    };
    let text_w = text_w.map(|w| w.min(content.right - content.left));
    match (text_w, image) {
        (None, None) => AlignedFace { text: None, image: None },
        (Some(w), None) => AlignedFace { text: Some(place(w, line_h, text_align)), image: None },
        (None, Some((iw, ih))) => AlignedFace { text: None, image: Some(place(iw, ih, image_align)) },
        (Some(w), Some((iw, ih))) => match relation {
            R::Overlay => AlignedFace { text: Some(place(w, line_h, text_align)), image: Some(place(iw, ih, image_align)) },
            R::ImageBeforeText | R::TextBeforeImage => {
                let block = place(iw + gap + w, ih.max(line_h), text_align);
                let mid = (block.top + block.bottom) / 2.0;
                let image_first = relation == R::ImageBeforeText;
                let (ix, tx) = if image_first { (block.left, block.left + iw + gap) } else { (block.left + w + gap, block.left) };
                AlignedFace {
                    text: Some(Rect::new(tx, mid - line_h / 2.0, tx + w, mid + line_h / 2.0)),
                    image: Some(Rect::new(ix, mid - ih / 2.0, ix + iw, mid + ih / 2.0)),
                }
            }
            R::ImageAboveText | R::TextAboveImage => {
                let block = place(iw.max(w), ih + gap + line_h, text_align);
                let centre = (block.left + block.right) / 2.0;
                let image_first = relation == R::ImageAboveText;
                let (iy, ty) = if image_first { (block.top, block.top + ih + gap) } else { (block.top + line_h + gap, block.top) };
                AlignedFace {
                    text: Some(Rect::new(centre - w / 2.0, ty, centre + w / 2.0, ty + line_h)),
                    image: Some(Rect::new(centre - iw / 2.0, iy, centre + iw / 2.0, iy + ih)),
                }
            }
        },
    }
}

/// Where a button's content lands inside its bounds — see [`face_layout`].
#[derive(Clone, Copy)]
pub struct FaceLayout {
    /// The rectangle the icon glyph is centred in, if there is an icon.
    pub icon: Option<Rect>,
    /// The label's rectangle.
    pub label: Rect,
    /// The label is centred over the whole face (no icon and it fits).
    pub centred: bool,
    /// The content is wider than the content box: the label is ellipsised.
    pub truncated: bool,
}

/// Lays a button face out: `inline-flex items-center justify-center` with a
/// `gap`, inside `pad` of horizontal padding.
///
/// Content that fits is centred as one group (icon + gap + label). Content
/// that does not fit starts at the left padding and the label is ellipsised
/// against the right one, so the text never crosses the button's edge. Pure,
/// so both shapes are unit tests.
pub fn face_layout(
    rect: Rect,
    pad: f32,
    icon: Option<f32>,
    gap: f32,
    text_w: Option<f32>,
) -> FaceLayout {
    let content_left = (rect.left + pad).min((rect.left + rect.right) / 2.0);
    let content_right = (rect.right - pad).max(content_left);
    let avail = content_right - content_left;
    let Some(text_w) = text_w else {
        // Icon alone (or nothing): the glyph is centred on the face.
        return FaceLayout { icon: icon.map(|_| rect), label: rect, centred: true, truncated: false };
    };
    let icon_w = icon.map(|s| s + gap).unwrap_or(0.0);
    let total = icon_w + text_w;
    if total <= avail + glyph_metrics::FIT_EPS {
        if icon.is_none() {
            return FaceLayout { icon: None, label: rect, centred: true, truncated: false };
        }
        let start = (rect.left + rect.right) / 2.0 - total / 2.0;
        let icon_rect = icon.map(|s| Rect::new(start, rect.top, start + s, rect.bottom));
        let label = Rect::new(
            start + icon_w,
            rect.top,
            (content_right + glyph_metrics::SLACK).min(rect.right).max(start + total),
            rect.bottom,
        );
        return FaceLayout { icon: icon_rect, label, centred: false, truncated: false };
    }
    let icon_rect = icon.map(|s| Rect::new(content_left, rect.top, content_left + s, rect.bottom));
    let label_left = (content_left + icon_w).min(content_right);
    FaceLayout {
        icon: icon_rect,
        label: Rect::new(label_left, rect.top, content_right, rect.bottom),
        centred: false,
        truncated: true,
    }
}

/// The centres of the dabs the loading ring is rasterised from: a
/// `border-2` ring of [`loading_metrics::BOX`] whose TOP quarter is
/// transparent (`border-t-transparent`), turned by `phase` of a revolution.
/// Canvas space, y down, so angles run clockwise.
pub fn loading_dabs(centre: (f32, f32), phase: f32) -> Vec<(f32, f32)> {
    use std::f32::consts::{FRAC_PI_4, TAU};
    let turn = if phase.is_finite() { phase.rem_euclid(1.0) } else { 0.0 };
    let r = (loading_metrics::BOX - loading_metrics::STROKE) / 2.0;
    // The top border owns the arc between the two upper diagonals
    // (−135°..−45°); the visible three quarters run from −45° clockwise to
    // 225°.
    let from = -FRAC_PI_4 + turn * TAU;
    let to = from + 0.75 * TAU;
    let arc_len = (to - from) * r;
    let steps = (arc_len / (loading_metrics::STROKE * loading_metrics::DAB_PITCH)).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|i| {
            let a = from + (to - from) * (i as f32 / steps as f32);
            (centre.0 + r * a.cos(), centre.1 + r * a.sin())
        })
        .collect()
}

impl Button {
    pub fn new(label: &str) -> Self {
        let mut inner = replica::Button::new();
        inner.text = label.to_string();
        Self {
            inner,
            variant: Variant::default(),
            size: Size::default(),
            icon: None,
            pad_x: None,
            loading: false,
            loading_phase: 0.0,
            image_size: None,
            mnemonic: None,
            icon_size: None,
            gap: None,
            icon_aligned: false,
        }
    }

    /// The icon's size in force.
    pub fn icon_px(&self) -> f32 {
        self.icon_size.unwrap_or_else(|| self.size.icon())
    }

    /// The gap between the icon (or the image) and the label in force.
    pub fn gap_px(&self) -> f32 {
        self.gap.unwrap_or_else(|| self.size.gap())
    }

    /// What the aligned face lays out as its image: the caller's image, else the icon when it is
    /// laid out like one ([`Button::icon_aligned`]).
    fn aligned_image(&self) -> Option<(f32, f32)> {
        self.image_size.or_else(|| (self.icon.is_some() && (self.icon_aligned || self.inner.text_align != ContentAlignment::MiddleCenter)).then(|| (self.icon_px(), self.icon_px())))
    }

    /// The icon is laid out above or below the label.
    fn stacked(&self) -> bool {
        use replica::TextImageRelation as R;
        self.aligned_image().is_some() && matches!(self.inner.text_image_relation, R::ImageAboveText | R::TextAboveImage)
    }

    /// Whether the face is laid out the WinForms way (a `TextAlign` other than the centre, or an
    /// image): see [`aligned_face`].
    fn aligned(&self) -> bool {
        self.aligned_image().is_some() || self.inner.text_align != ContentAlignment::MiddleCenter
    }

    /// Where the caller draws the button's image ([`Button::image_size`]) inside `bounds`.
    pub fn image_rect(&self, canvas: &dyn Canvas, bounds: Rect) -> Option<Rect> {
        let text_w = (!self.inner.text.is_empty()).then(|| canvas.measure(&self.inner.text, &canvas.formats().body));
        aligned_face(bounds, self.padding_x(), text_w, LABEL_LINE, self.inner.text_align, self.aligned_image(), self.inner.image_align, self.inner.text_image_relation, self.gap_px())
            .image
    }

    pub fn variant(mut self, v: Variant) -> Self {
        self.variant = v;
        self
    }

    pub fn size(mut self, s: Size) -> Self {
        self.size = s;
        self
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn pad_x(mut self, pad: f32) -> Self {
        self.pad_x = Some(pad);
        self
    }

    /// Builder: the loading state (see [`Button::loading`]).
    pub fn loading(mut self, on: bool) -> Self {
        self.loading = on;
        self
    }

    /// Builder: the loading ring's phase (see [`Button::loading_phase`]).
    pub fn loading_phase(mut self, phase: f32) -> Self {
        self.loading_phase = phase;
        self
    }

    /// The horizontal padding actually in force.
    pub fn padding_x(&self) -> f32 {
        self.pad_x.unwrap_or_else(|| self.size.pad_x())
    }

    /// The intrinsic width once the label has been measured: padding, the icon
    /// and its gap, then the label — the same content-driven sizing the web
    /// has, and the same sum as [`kubuno_drive_desktop_app_controls::button::width`]. A
    /// loading button holds only its 16 DIP ring, as the web's does.
    ///
    /// Split from [`Button::width`] so the arithmetic is testable without a
    /// live [`Canvas`] (a `TextFormats` is a set of COM objects a unit test
    /// cannot build).
    pub fn width_of(&self, text_width: f32) -> f32 {
        if self.loading {
            return self.padding_x() * 2.0 + loading_metrics::BOX;
        }
        let label = if self.text.is_empty() { 0.0 } else { text_width };
        if self.stacked() {
            // Above or below the label: as wide as the wider of the two.
            let image = self.aligned_image().map_or(0.0, |(w, _)| w);
            return self.padding_x() * 2.0 + image.max(label);
        }
        let icon = match self.aligned_image().or_else(|| self.icon.map(|_| (self.icon_px(), self.icon_px()))) {
            Some((w, _)) if self.text.is_empty() => w,
            Some((w, _)) => w + self.gap_px(),
            None => 0.0,
        };
        self.padding_x() * 2.0 + icon + label
    }

    /// The intrinsic width, measured against the real font.
    pub fn width(&self, c: &dyn Canvas) -> f32 {
        self.width_of(c.measure(&self.text, &c.formats().body))
    }

    /// Lays the button out at its intrinsic width with its top-left at
    /// `(x, y)` — [`kubuno_drive_desktop_app_controls::button::rect_at`].
    pub fn rect_at(&self, c: &dyn Canvas, x: f32, y: f32) -> Rect {
        Rect::new(x, y, x + self.width(c), y + self.size.height())
    }

    /// Lays it out with its top-RIGHT at `(right, y)` —
    /// [`kubuno_drive_desktop_app_controls::button::rect_ending_at`].
    pub fn rect_ending_at(&self, c: &dyn Canvas, right: f32, y: f32) -> Rect {
        Rect::new(right - self.width(c), y, right, y + self.size.height())
    }

    /// The state this button paints in, given what the caller reports and what
    /// the model says about `enabled` — a loading button is a disabled one.
    pub fn state(&self, state: WidgetState) -> State {
        State::of(self.inner.enabled && !self.loading, state)
    }

    /// Whether it takes part in keyboard focus (register it with the focus
    /// ring only when this is true — a disabled `<button>` is skipped by Tab).
    pub fn focusable(&self) -> bool {
        self.inner.enabled && !self.loading
    }

    /// The cursor to show over it: the arrow, or `not-allowed` when disabled.
    pub fn cursor(&self, state: WidgetState) -> Cursor {
        cursor_of(false, self.state(state) == State::Disabled)
    }
}

/// What a button face shows.
struct Face<'a> {
    label: &'a str,
    icon: Option<&'static str>,
    icon_size: f32,
    gap: f32,
    pad: f32,
    /// `Some(phase)` replaces the content with the loading ring.
    loading: Option<f32>,
    /// The WinForms layout (`TextAlign`, an image), when the face uses it — see [`aligned_face`].
    aligned: Option<AlignedSpec>,
    /// The label's character to underline (a mnemonic, while Alt is held).
    mnemonic: Option<usize>,
}

/// What [`aligned_face`] needs beyond the [`Face`].
#[derive(Clone, Copy)]
struct AlignedSpec {
    text_align: ContentAlignment,
    /// The face's icon is drawn where the image goes (the button has no image of its own).
    icon_as_image: bool,
    image: Option<(f32, f32)>,
    image_align: ContentAlignment,
    relation: replica::TextImageRelation,
}

/// The line box of a button label, what an aligned face stacks by.
const LABEL_LINE: f32 = 20.0;

/// Paints a Kubuno button face into `rect`. Shared by [`Button`] and by a
/// check/radio whose `Appearance` is `Button`, so the two cannot disagree about
/// what a button looks like.
///
/// Fill (only when the variant has one — `bg-transparent` shows the parent),
/// border, the content per [`face_layout`], then the focus ring.
fn paint_face(c: &dyn Canvas, rect: Rect, face: &Face, ink: &Ink, ring: bool) {
    let t = c.theme();
    let f = c.formats();

    if let Some(fill) = ink.fill.as_ref() {
        c.fill_rounded(&rect, RADIUS, fill);
    }
    if let Some(border) = ink.border.as_ref() {
        c.stroke_rounded(&rect, RADIUS, border);
    }

    if let Some(phase) = face.loading {
        let centre = ((rect.left + rect.right) / 2.0, (rect.top + rect.bottom) / 2.0);
        // The same three quarters `loading_dabs` samples, drawn as ONE smooth
        // arc on the stroke's centre line — a row of dots read as a beaded,
        // shimmering ring once it turned.
        let turn = if phase.is_finite() { phase.rem_euclid(1.0) } else { 0.0 };
        let from = -std::f32::consts::FRAC_PI_4 + turn * std::f32::consts::TAU;
        let r = (loading_metrics::BOX - loading_metrics::STROKE) / 2.0;
        c.stroke_arc(centre, r, from, 0.75 * std::f32::consts::TAU, loading_metrics::STROKE, &ink.label);
    } else if let Some(spec) = face.aligned {
        // The WinForms layout: the label (and the image the caller draws) by their alignments.
        let text_w = (!face.label.is_empty()).then(|| c.measure(face.label, &f.body));
        let laid = aligned_face(rect, face.pad, text_w, LABEL_LINE, spec.text_align, spec.image, spec.image_align, spec.relation, face.gap);
        if let (true, Some(name), Some(at)) = (spec.icon_as_image, face.icon, laid.image) {
            c.vector_icon(name, &at, face.icon_size, &ink.label);
        }
        if let Some(band) = laid.text {
            // One DIP of slack each side: a box exactly as wide as the measured label can still trim it.
            let band = Rect::new(band.left - 1.0, band.top, band.right + 1.0, band.bottom);
            c.text_ellipsis(face.label, &band, &f.body, &ink.label);
            if let Some(i) = face.mnemonic {
                crate::mnemonic::underline(c, face.label, i, &band, &f.body, &ink.label, DWRITE_TEXT_ALIGNMENT_LEADING);
            }
        }
    } else {
        let text_w = (!face.label.is_empty()).then(|| c.measure(face.label, &f.body));
        let layout =
            face_layout(rect, face.pad, face.icon.map(|_| face.icon_size), face.gap, text_w);
        if let (Some(name), Some(icon_rect)) = (face.icon, layout.icon) {
            c.vector_icon(name, &icon_rect, face.icon_size, &ink.label);
        }
        if text_w.is_some() {
            if layout.centred {
                c.text(face.label, &layout.label, &f.body, &ink.label, true);
            } else {
                c.text_ellipsis(face.label, &layout.label, &f.body, &ink.label);
            }
            if let Some(i) = face.mnemonic.filter(|_| !layout.truncated) {
                let align = if layout.centred { DWRITE_TEXT_ALIGNMENT_CENTER } else { DWRITE_TEXT_ALIGNMENT_LEADING };
                crate::mnemonic::underline(c, face.label, i, &layout.label, &f.body, &ink.label, align);
            }
        }
    }

    if ring {
        paint_focus_ring(c, rect, RADIUS, &t.accent);
    }
}

impl Widget for Button {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    /// The Kubuno size, not the replica's `preferred_size`: a token height and
    /// a token padding, rather than a caption plus a system-metric border.
    fn measure(&self, canvas: &dyn Canvas) -> ControlSize {
        // Above or below the label, the icon makes the button taller.
        let height = match self.aligned_image().filter(|_| self.stacked()) {
            Some((_, h)) if !self.text.is_empty() => self.size.height().max(h + self.gap_px() + LABEL_LINE + 8.0),
            Some((_, h)) => self.size.height().max(h + 8.0),
            None => self.size.height(),
        };
        ControlSize::new(self.width(canvas).ceil(), height)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let s = self.state(state);
        let ink = ink_for(self.variant, canvas.theme(), s);
        let face = Face {
            label: &self.text,
            icon: self.icon,
            icon_size: self.icon_px(),
            gap: self.gap_px(),
            pad: self.padding_x(),
            loading: self.loading.then_some(self.loading_phase),
            aligned: self.aligned().then_some(AlignedSpec {
                text_align: self.inner.text_align,
                icon_as_image: self.image_size.is_none() && self.aligned_image().is_some(),
                image: self.aligned_image(),
                image_align: self.inner.image_align,
                relation: self.inner.text_image_relation,
            }),
            mnemonic: self.mnemonic,
        };
        let ring = state.show_focus_ring() && s != State::Disabled;
        paint_face(canvas, bounds, &face, &ink, ring);
    }

    fn type_name(&self) -> &'static str {
        "Button"
    }
}

impl Deref for Button {
    type Target = replica::Button;
    fn deref(&self) -> &replica::Button {
        &self.inner
    }
}
impl DerefMut for Button {
    fn deref_mut(&mut self) -> &mut replica::Button {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// IconButton
// ═════════════════════════════════════════════════════════════════════════════

/// A round icon button — the header's 36 px controls and the waffle's 40 px
/// pencil.
///
/// These ARE circles on the web (`rounded-full`), unlike the text buttons; the
/// two shapes are easy to mix up, which is why the predecessor keeps them in
/// one file and why this one narrows [`Widget::hit_test`] to a disc.
pub struct IconButton {
    inner: replica::Button,
    pub icon: &'static str,
    /// The circle's diameter: 36 in the header, 40 for the waffle pencil.
    pub diameter: f32,
    /// The glyph inside it: 18 in the header, 16 for the pencil.
    pub glyph: f32,
    /// Whether it carries a resting fill (the pencil sits on surface-2, the
    /// header's controls on nothing).
    pub filled: bool,
}

/// Whether `(x, y)` lands on a **round** icon button filling `bounds` — the
/// disc inscribed in it, never the square.
///
/// It is a free function, and not only [`IconButton`]'s `hit_test`, because a
/// page's hit-testing runs where no button has been built: the shell derives
/// `edit_rect(row)` and asks whether the pointer is inside it, without ever
/// constructing the control it is about to paint there. Every one of those
/// sites was testing the SQUARE, so the corners of a circular button lit up —
/// three separate reviews of the migration reported it independently. Exposing
/// the rule on its own is what lets those callers agree with the paint.
///
/// This matches the web, where `border-radius` clips pointer events too: a
/// click in the corner of a `rounded-full` control does not reach it.
pub fn circular_hit(bounds: Rect, x: f32, y: f32) -> bool {
    let cx = (bounds.left + bounds.right) / 2.0;
    let cy = (bounds.top + bounds.bottom) / 2.0;
    let r = (bounds.right - bounds.left).min(bounds.bottom - bounds.top) / 2.0;
    let (dx, dy) = (x - cx, y - cy);
    dx * dx + dy * dy <= r * r
}

impl IconButton {
    /// The header's control: 36 px circle, 18 px glyph, no resting fill.
    pub fn header(icon: &'static str) -> Self {
        Self { inner: replica::Button::new(), icon, diameter: 36.0, glyph: 18.0, filled: false }
    }

    /// The waffle's pencil: a tinted circle on surface-2.
    pub fn tinted(icon: &'static str, diameter: f32, glyph: f32) -> Self {
        Self { inner: replica::Button::new(), icon, diameter, glyph, filled: true }
    }

    /// A circle with no resting fill, at a size [`IconButton::header`] does not
    /// offer — the row actions of the admin lists (28 px over a 15 px glyph).
    ///
    /// This constructor exists because the shell asked for it: adopting the
    /// primitive turned six call sites into `tinted(…)` immediately followed by
    /// `filled = false`, which reads as building a tinted button in order to
    /// un-tint it. When a caller has to undo the constructor it just called,
    /// the constructor is the thing that is missing.
    pub fn plain(icon: &'static str, diameter: f32, glyph: f32) -> Self {
        Self { inner: replica::Button::new(), icon, diameter, glyph, filled: false }
    }

    pub fn state(&self, state: WidgetState) -> State {
        State::of(self.inner.enabled, state)
    }

    /// The cursor to show over it: the arrow, or `not-allowed` when disabled.
    pub fn cursor(&self, state: WidgetState) -> Cursor {
        cursor_of(false, self.state(state) == State::Disabled)
    }
}

impl Widget for IconButton {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> ControlSize {
        ControlSize::new(self.diameter, self.diameter)
    }

    /// The predecessor's `draw_icon_button`: the radius is `pill(height)` — a
    /// real circle, whatever rectangle the caller passes — hover and press
    /// share `surface_3`, and a resting tinted button sits on `surface_2`. A
    /// resting plain one paints NOTHING behind its glyph, so it sits on its
    /// parent's surface (a Card, a toolbar) rather than on a window-coloured
    /// disc. The focus ring is the family's, concentric with the disc.
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let s = self.state(state);
        let r = pill(bounds.bottom - bounds.top);
        let fill = match s {
            State::Active | State::Hover => Some(t.surface_3),
            _ if self.filled => Some(t.surface_2),
            _ => None,
        };
        if let Some(fill) = fill {
            let fill = if s == State::Disabled { fade(fill, DISABLED_ALPHA) } else { fill };
            canvas.fill_rounded(&bounds, r, &fill);
        }
        let colour = if matches!(s, State::Disabled) {
            fade(t.text_secondary, DISABLED_ALPHA)
        } else {
            t.text_secondary
        };
        canvas.vector_icon(self.icon, &bounds, self.glyph, &colour);
        if state.show_focus_ring() && s != State::Disabled {
            paint_focus_ring(canvas, bounds, r, &t.accent);
        }
    }

    /// The disc inscribed in `bounds`. A round button whose corners answered
    /// would light up from a pointer that is visibly off it.
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        circular_hit(bounds, x, y)
    }

    fn type_name(&self) -> &'static str {
        "IconButton"
    }
}

impl Deref for IconButton {
    type Target = replica::Button;
    fn deref(&self) -> &replica::Button {
        &self.inner
    }
}
impl DerefMut for IconButton {
    fn deref_mut(&mut self) -> &mut replica::Button {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// The labelled controls' text block — shared by CheckBox, RadioButton, Switch.
// ═════════════════════════════════════════════════════════════════════════════

/// One laid-out line of a label + description block, relative to the block's
/// top.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub text: String,
    /// Offset from the block's top.
    pub top: f32,
    pub height: f32,
    /// A description line (secondary colour, its own format) rather than a
    /// label line.
    pub secondary: bool,
    /// The block ran out of height here: the line carries the rest of its
    /// paragraph and is painted with an ellipsis.
    pub truncated: bool,
}

/// One paragraph of a text block: the label, or the description under it.
#[derive(Debug, Clone, Copy)]
pub struct Paragraph<'a> {
    pub text: &'a str,
    pub line_h: f32,
    /// Space above it when something precedes it (`mt-0.5`).
    pub gap_before: f32,
    pub secondary: bool,
}

/// Wraps the paragraphs into `width` and keeps what fits in `max_h`.
///
/// The web's label column is `flex flex-col min-w-0`, so a long label wraps
/// word by word ([`crate::dialogs::wrap`], the browser's greedy fit). The
/// desktop cannot grow a caller's rectangle, so when the block is taller than
/// `max_h` the last line that fits takes the rest of its paragraph and is
/// flagged `truncated` (painted with an ellipsis), and a paragraph with no
/// room at all is dropped. The first line is always kept, whatever `max_h`
/// says: a check box never loses its whole label. `measure` receives the
/// paragraph's `secondary` flag to pick its font.
pub fn layout_text_block(
    paras: &[Paragraph],
    width: f32,
    max_h: f32,
    measure: &dyn Fn(&str, bool) -> f32,
) -> Vec<TextLine> {
    let mut out: Vec<TextLine> = Vec::new();
    let mut y = 0.0f32;
    for p in paras {
        if p.text.trim().is_empty() {
            continue;
        }
        let lines = crate::dialogs::wrap(p.text, width.max(0.0), &|s: &str| measure(s, p.secondary));
        let gap = if out.is_empty() { 0.0 } else { p.gap_before };
        let first = out.len();
        for (i, line) in lines.iter().enumerate() {
            let top = y + gap + i as f32 * p.line_h;
            let fits = top + p.line_h <= max_h + glyph_metrics::FIT_EPS;
            if !fits && !out.is_empty() {
                if out.len() > first {
                    let rest = lines[i..].join(" ");
                    if let Some(last) = out.last_mut() {
                        last.text = format!("{} {}", last.text, rest);
                        last.truncated = true;
                    }
                }
                return out;
            }
            out.push(TextLine {
                text: line.clone(),
                top,
                height: p.line_h,
                secondary: p.secondary,
                truncated: false,
            });
        }
        y = y + gap + lines.len() as f32 * p.line_h;
    }
    out
}

/// The height a laid-out block occupies.
pub fn text_block_height(lines: &[TextLine]) -> f32 {
    lines.last().map(|l| l.top + l.height).unwrap_or(0.0)
}

/// Where a leading glyph (box, disc or track) and its text block land.
#[derive(Clone, Copy)]
struct Leading {
    glyph: Rect,
    /// The text column: its `left`/`right` bound the lines, its `top` is the
    /// block's top.
    text: Rect,
}

/// Places a `side_w × side_h` glyph and a `block_h` text block in `bounds`.
///
/// `items-start`: the glyph sits `lead` below the block's top (`mt-px` for
/// the check box and radio, `mt-0.5` for the switch), never centred on a
/// multi-line block. The whole group is then placed vertically by
/// `CheckAlign`'s fraction — a single-line control in a taller row sits in
/// its middle, which is what a `MiddleLeft` caller expects. `CheckAlign` on
/// the right mirrors it, the rule the replica's own `split_glyph` has.
fn place_leading(
    bounds: Rect,
    align: ContentAlignment,
    side: (f32, f32),
    gap: f32,
    lead: f32,
    block_h: f32,
) -> Leading {
    let (side_w, side_h) = side;
    let (fh, fv) = align.fractions();
    let bounds_h = bounds.bottom - bounds.top;
    // A caller that gave less than a line keeps the glyph inside its bounds.
    let lead = if block_h > 0.0 { lead.min((bounds_h - side_h).max(0.0)) } else { 0.0 };
    let content_h = block_h.max(lead + side_h);
    let slack = (bounds_h - content_h).max(0.0);
    let top = bounds.top + slack * fv;
    let block_bottom = top + block_h.max(side_h);

    if fh >= 1.0 {
        let left = bounds.right - side_w;
        Leading {
            glyph: Rect::new(left, top + lead, bounds.right, top + lead + side_h),
            text: Rect::new(bounds.left, top, (left - gap).max(bounds.left), block_bottom),
        }
    } else {
        let left = bounds.left;
        Leading {
            glyph: Rect::new(left, top + lead, left + side_w, top + lead + side_h),
            text: Rect::new((left + side_w + gap).min(bounds.right), top, bounds.right, block_bottom),
        }
    }
}

/// Paints laid-out lines into the text column `at`.
fn paint_text_lines(
    c: &dyn Canvas,
    lines: &[TextLine],
    at: Rect,
    formats: (&IDWriteTextFormat, &IDWriteTextFormat),
    colours: (&D2D1_COLOR_F, &D2D1_COLOR_F),
) {
    for l in lines {
        let r = Rect::new(at.left, at.top + l.top, at.right, at.top + l.top + l.height);
        let (fmt, colour) =
            if l.secondary { (formats.1, colours.1) } else { (formats.0, colours.0) };
        // Always through the ellipsis path: a single word wider than the column
        // (which `wrap` keeps whole) is trimmed rather than spilling.
        c.text_ellipsis(&l.text, &r, fmt, colour);
    }
}

/// The natural single-line width of a label + description column, with the
/// one DIP of slack that keeps the last glyph from being trimmed.
fn natural_text_width(c: &dyn Canvas, label: &str, desc: Option<(&str, &IDWriteTextFormat)>) -> f32 {
    let f = c.formats();
    let mut w = if label.is_empty() { 0.0 } else { c.measure(label, &f.body) };
    if let Some((d, fmt)) = desc {
        if !d.is_empty() {
            w = w.max(c.measure(d, fmt));
        }
    }
    if w > 0.0 {
        w.ceil() + glyph_metrics::SLACK
    } else {
        0.0
    }
}

/// The check box / radio paragraphs: `text-sm text-text-primary leading-snug`
/// then `text-sm text-text-secondary leading-snug mt-0.5`.
fn glyph_paragraphs<'a>(label: &'a str, desc: Option<&'a str>) -> [Paragraph<'a>; 2] {
    [
        Paragraph { text: label, line_h: glyph_metrics::LINE, gap_before: 0.0, secondary: false },
        Paragraph {
            text: desc.unwrap_or(""),
            line_h: glyph_metrics::LINE,
            gap_before: glyph_metrics::DESC_GAP,
            secondary: true,
        },
    ]
}

/// What a glyph control wants to be: box + gap + text column, at least a line
/// tall — plus one line per description.
fn glyph_measure_parts(text_width: f32, has_desc: bool) -> ControlSize {
    let text_h = if has_desc {
        glyph_metrics::LINE * 2.0 + glyph_metrics::DESC_GAP
    } else {
        glyph_metrics::LINE
    };
    let gap = if text_width > 0.0 { control::CHECK_GAP } else { 0.0 };
    ControlSize::new(control::CHECK_BOX + gap + text_width, control::CHECK_BOX.max(text_h))
}

/// Everything the check box and the radio paint around their glyph.
struct GlyphText<'a> {
    label: &'a str,
    description: Option<&'a str>,
}

impl GlyphText<'_> {
    /// The lines that fit in `bounds`, and the resulting layout.
    fn layout(&self, c: &dyn Canvas, bounds: Rect, align: ContentAlignment) -> (Leading, Vec<TextLine>) {
        let side = control::CHECK_BOX;
        let gap = control::CHECK_GAP;
        let width = (bounds.right - bounds.left - side - gap).max(0.0);
        let f = c.formats();
        let paras = glyph_paragraphs(self.label, self.description);
        let lines = layout_text_block(&paras, width, bounds.bottom - bounds.top, &|s, _| {
            c.measure(s, &f.body)
        });
        let lead = ((glyph_metrics::LINE - side) / 2.0).max(0.0);
        let leading = place_leading(bounds, align, (side, side), gap, lead, text_block_height(&lines));
        (leading, lines)
    }

    /// The height a column `width` wide needs to show every line.
    fn height_for_width(&self, c: &dyn Canvas, width: f32) -> f32 {
        let side = control::CHECK_BOX;
        let text_w = (width - side - control::CHECK_GAP).max(0.0);
        let f = c.formats();
        let paras = glyph_paragraphs(self.label, self.description);
        let lines = layout_text_block(&paras, text_w, f32::INFINITY, &|s, _| c.measure(s, &f.body));
        let lead = ((glyph_metrics::LINE - side) / 2.0).max(0.0);
        text_block_height(&lines).max(lead + side).max(glyph_metrics::LINE)
    }

    fn paint(&self, c: &dyn Canvas, lines: &[TextLine], at: Rect, alpha: f32) {
        let t = c.theme();
        let f = c.formats();
        paint_text_lines(
            c,
            lines,
            at,
            (&f.body, &f.body),
            (&fade(t.text_primary, alpha), &fade(t.text_secondary, alpha)),
        );
    }
}

/// The unchecked outline's colour: `--color-border`, or `--color-border-strong`
/// under the pointer (`readCheckboxPalette` / `readRadioPalette`). A press
/// keeps the pointer over the label, so it keeps the hover colour.
fn glyph_border(t: &Theme, state: State) -> D2D1_COLOR_F {
    if matches!(state, State::Hover | State::Active) {
        t.border_strong
    } else {
        t.card_stroke
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// CheckBox (and ToggleButton, which is the same control)
// ═════════════════════════════════════════════════════════════════════════════

/// A two- or three-state check box.
///
/// Everything about *being* a check box is the replica's: `check_state` is the
/// single source of truth (`checked()` is a projection of it, exactly as in the
/// toolkit), `three_state` and `auto_check` govern the click, and
/// `perform_click()` walks the toolkit's own cycle — `Unchecked → Checked →
/// Unchecked`, or `Unchecked → Checked → Indeterminate → Unchecked` when
/// `three_state` is on. None of that is re-implemented here.
///
/// What Kubuno adds is what the web's `Checkbox` props add over a bare
/// `<input>`: a `description` under the label, and an accent `color`.
///
/// ## Where every number comes from
///
/// `core/frontend/src/ui/checkboxCanvas.ts`, which is the painter the browser
/// runs: `{ size: 18, border: 2, radius: 4, tick: 11 }`, an accent fill when
/// checked, a white mark, `--color-border` / `--color-border-strong` for the
/// resting and hovered outline, and a dash of `tick × max(2, border)` for the
/// indeterminate state. The label column is `Checkbox.tsx`'s: `gap-2`,
/// `items-start`, `mt-px`, wrapping.
///
/// The one substitution: the web's tick is a bespoke six-point polygon
/// (`clip-path`), and [`Canvas`] has no polygon primitive — it draws glyphs as
/// geometries by name. The mark is therefore the shared `Check` icon, in the
/// same 11 DIP square the polygon occupied.
pub struct CheckBox {
    inner: replica::CheckBox,
    /// `description`: a secondary line under the label.
    pub description: Option<String>,
    /// `color`: the accent when checked (a calendar's colour). `None` = the
    /// theme's primary.
    pub accent: Option<D2D1_COLOR_F>,
}

/// A toggle button IS a check box — `Appearance::Button` is a property of the
/// same .NET control, not a second one — so this is an alias rather than a
/// type. Build one with [`CheckBox::toggle_button`].
pub type ToggleButton = CheckBox;

impl CheckBox {
    pub fn new(label: &str) -> Self {
        let mut inner = replica::CheckBox::new();
        inner.text = label.to_string();
        Self { inner, description: None, accent: None }
    }

    /// The same control, painted as a pressable button
    /// (`Appearance::Button`) — see [`ToggleButton`].
    pub fn toggle_button(label: &str) -> Self {
        let mut b = Self::new(label);
        b.inner.appearance = Appearance::Button;
        b
    }

    /// Starts it out three-state, so a click can reach `Indeterminate`.
    pub fn tri_state(mut self) -> Self {
        self.inner.three_state = true;
        self
    }

    /// Sets the initial [`CheckState`].
    ///
    /// Deliberately NOT called `checked`: the replica already publishes
    /// `checked()` (the getter, a projection of `check_state`) and
    /// `set_checked`, and a builder of the same name would shadow the getter
    /// through `Deref` — the exact drift this crate's first rule exists to
    /// prevent.
    pub fn check(mut self, state: CheckState) -> Self {
        self.inner.check_state = state;
        self
    }

    /// Builder: the secondary line under the label.
    pub fn description(mut self, text: &str) -> Self {
        self.description = Some(text.to_string());
        self
    }

    /// Builder: the checked accent (the web's `color` prop).
    pub fn accent(mut self, colour: D2D1_COLOR_F) -> Self {
        self.accent = Some(colour);
        self
    }

    pub fn state(&self, state: WidgetState) -> State {
        State::of(self.inner.enabled, state)
    }

    /// `cursor: pointer` over the label, `not-allowed` when disabled.
    pub fn cursor(&self, state: WidgetState) -> Cursor {
        let disabled = self.state(state) == State::Disabled;
        cursor_of(self.inner.appearance == Appearance::Normal, disabled)
    }

    fn text(&self) -> GlyphText<'_> {
        GlyphText { label: &self.inner.text, description: self.description.as_deref() }
    }

    /// The height this check box needs at `width` to show its whole label and
    /// description, wrapped — the web's behaviour. Give it that height and it
    /// wraps; give it less and the last visible line is ellipsised.
    pub fn height_for_width(&self, canvas: &dyn Canvas, width: f32) -> f32 {
        if self.inner.appearance == Appearance::Button {
            return Size::Md.height();
        }
        self.text().height_for_width(canvas, width)
    }

    /// Where the 18 DIP box lands inside `bounds` — for a caller that anchors
    /// something on it (a tooltip, a help bubble).
    pub fn glyph_rect(&self, canvas: &dyn Canvas, bounds: Rect) -> Rect {
        self.text().layout(canvas, bounds, self.inner.check_align).0.glyph
    }

    /// Paints it with the check transition at `progress` (0 = unchecked, 1 =
    /// checked): the accent fill cross-fades in and the tick scales from the
    /// centre, as `paintCheckbox` does. An indeterminate box ignores it (the
    /// dash is drawn outright). Drive it with a [`Transition`];
    /// [`Widget::paint`] is this at 0 or 1.
    pub fn paint_progress(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState, progress: f32) {
        let t = canvas.theme();
        let s = self.state(state);
        let alpha = if matches!(s, State::Disabled) { DISABLED_ALPHA } else { 1.0 };

        if self.inner.appearance == Appearance::Button {
            // A toggle button has no transition of its own on the web.
            let ink = toggle_ink(t, s, progress >= 0.5);
            let face = Face {
                label: &self.inner.text,
                icon: None,
                icon_size: Size::Md.icon(),
                gap: Size::Md.gap(),
                pad: Size::Md.pad_x(),
                loading: None,
                aligned: None,
                mnemonic: None,
            };
            paint_face(canvas, bounds, &face, &ink, state.show_focus_ring() && s != State::Disabled);
            return;
        }

        // No ground: the label area shows the parent's surface, as the web's
        // `<label>` does.
        let text = self.text();
        let (at, lines) = text.layout(canvas, bounds, self.inner.check_align);
        let accent = self.accent.unwrap_or(t.accent);
        let glyph = CheckGlyph {
            indeterminate: self.inner.check_state == CheckState::Indeterminate,
            progress: progress.clamp(0.0, 1.0),
            state: s,
            accent,
            alpha,
        };
        paint_check_glyph(canvas, at.glyph, &glyph);
        if state.show_focus_ring() && !matches!(s, State::Disabled) {
            paint_focus_ring(canvas, at.glyph, control::CHECK_RADIUS, &t.accent);
        }
        text.paint(canvas, &lines, at.text, alpha);
    }
}

/// The ink a pressed toggle button wears: `bg-primary-light text-primary`.
///
/// Measured off the web's own toggle buttons — `drive/fileView.tsx`'s view-mode
/// group and `core/admin/storage/QuotaField.tsx`'s unit group both write
/// exactly `active ? 'bg-primary-light text-primary' : 'text-text-secondary
/// hover:bg-surface-…'`. An unpressed one is therefore a [`Variant::Ghost`]
/// button, and a pressed one keeps its fill under the pointer (the web states
/// no hover rule on the active branch, so hover does not reach it).
fn toggle_ink(t: &Theme, state: State, checked: bool) -> Ink {
    if !checked {
        return ink_for(Variant::Ghost, t, state);
    }
    let p = Ink { fill: Some(t.accent_light), label: t.accent, border: None };
    if matches!(state, State::Disabled) {
        return dim_ink(p);
    }
    p
}

/// What the 18 DIP box shows.
struct CheckGlyph {
    indeterminate: bool,
    progress: f32,
    state: State,
    accent: D2D1_COLOR_F,
    alpha: f32,
}

/// Paints the box, `paintCheckbox`'s way: the outline always, the accent fill
/// over it at `filled` alpha, then the dash or the scaled tick.
fn paint_check_glyph(c: &dyn Canvas, glyph: Rect, g: &CheckGlyph) {
    let t = c.theme();
    c.stroke_rounded_w(
        &glyph,
        control::CHECK_RADIUS,
        &fade(glyph_border(t, g.state), g.alpha),
        control::CHECK_BORDER,
    );
    // An indeterminate box is filled like a checked one, whatever `checked`
    // says — `paintCheckbox` decides the fill with `indeterminate ? 1 : progress`.
    let filled = if g.indeterminate { 1.0 } else { g.progress };
    if filled > 0.0 {
        c.fill_rounded(&glyph, control::CHECK_RADIUS, &fade(g.accent, g.alpha * filled));
    }

    // The mark is `#ffffff` in the painter, which in a token palette is « what
    // sits on the accent » — and the dark theme's pale accent needs a dark mark,
    // which is exactly what `accent_foreground` carries.
    let mark = fade(t.accent_foreground, g.alpha);
    if g.indeterminate {
        let w = control::CHECK_TICK;
        let h = glyph_metrics::dash_height();
        let x = glyph.left + (control::CHECK_BOX - w) / 2.0;
        let y = glyph.top + (control::CHECK_BOX - h) / 2.0;
        c.fill_rounded(&Rect::new(x, y, x + w, y + h), h / 2.0, &mark);
    } else if g.progress > 0.0 {
        // « Tick — scales from the centre, like the CSS transform: scale() ».
        c.vector_icon("Check", &glyph, control::CHECK_TICK * g.progress, &mark);
    }
}

impl Widget for CheckBox {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> ControlSize {
        match self.inner.appearance {
            // A toggle button measures like a plain button — the same rule the
            // replica's own `preferred_size` follows.
            Appearance::Button => {
                let text_w = canvas.measure(&self.inner.text, &canvas.formats().body);
                ControlSize::new((Size::Md.pad_x() * 2.0 + text_w).ceil(), Size::Md.height())
            }
            Appearance::Normal => {
                let desc = self.description.as_deref().filter(|d| !d.is_empty());
                let body = &canvas.formats().body;
                let w = natural_text_width(canvas, &self.inner.text, desc.map(|d| (d, body)));
                glyph_measure_parts(w, desc.is_some())
            }
        }
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let progress = if self.inner.check_state == CheckState::Unchecked { 0.0 } else { 1.0 };
        self.paint_progress(canvas, bounds, state, progress);
    }

    fn type_name(&self) -> &'static str {
        if self.inner.appearance == Appearance::Button {
            "ToggleButton"
        } else {
            "CheckBox"
        }
    }
}

impl Deref for CheckBox {
    type Target = replica::CheckBox;
    fn deref(&self) -> &replica::CheckBox {
        &self.inner
    }
}
impl DerefMut for CheckBox {
    fn deref_mut(&mut self) -> &mut replica::CheckBox {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// RadioButton
// ═════════════════════════════════════════════════════════════════════════════

/// A mutually-exclusive option button.
///
/// As with [`CheckBox`], the model is entirely the replica's: `checked`,
/// `auto_check`, `check_align`, and a `perform_click()` that selects without
/// ever un-selecting — a radio does not toggle off, it is cleared by a sibling
/// being chosen. That container rule lives on the models
/// ([`kubuno_desktop_controls::buttons::select_radio`]) and is deliberately not
/// restated here. The group's keyboard rules are [`radio_arrow_target`] and
/// [`radio_tab_stop`].
///
/// ## Where every number comes from
///
/// `core/frontend/src/ui/radioCanvas.ts`: `{ size: 18, ring: 2, dot: 10 }`, the
/// ring in `--color-border` (`--color-border-strong` hovered) and in the accent
/// once selected, and a dot that grows from the centre — so a fully selected
/// radio is a 10 DIP accent disc inside a 2 DIP accent ring.
pub struct RadioButton {
    inner: replica::RadioButton,
    /// `description`: a secondary line under the label.
    pub description: Option<String>,
    /// `color`: the accent when selected. `None` = the theme's primary.
    pub accent: Option<D2D1_COLOR_F>,
}

impl RadioButton {
    pub fn new(label: &str) -> Self {
        let mut inner = replica::RadioButton::new();
        inner.text = label.to_string();
        Self { inner, description: None, accent: None }
    }

    /// Selects it. Named `selected` rather than `checked` because the replica
    /// already stores `checked` as a FIELD, and a method of that name would sit
    /// confusingly beside it.
    pub fn selected(mut self, on: bool) -> Self {
        self.inner.checked = on;
        self
    }

    /// Builder: the secondary line under the label.
    pub fn description(mut self, text: &str) -> Self {
        self.description = Some(text.to_string());
        self
    }

    /// Builder: the selected accent (the web's `color` prop).
    pub fn accent(mut self, colour: D2D1_COLOR_F) -> Self {
        self.accent = Some(colour);
        self
    }

    pub fn state(&self, state: WidgetState) -> State {
        State::of(self.inner.enabled, state)
    }

    /// `cursor: pointer` over the label, `not-allowed` when disabled.
    pub fn cursor(&self, state: WidgetState) -> Cursor {
        let disabled = self.state(state) == State::Disabled;
        cursor_of(self.inner.appearance == Appearance::Normal, disabled)
    }

    fn text(&self) -> GlyphText<'_> {
        GlyphText { label: &self.inner.text, description: self.description.as_deref() }
    }

    /// See [`CheckBox::height_for_width`].
    pub fn height_for_width(&self, canvas: &dyn Canvas, width: f32) -> f32 {
        if self.inner.appearance == Appearance::Button {
            return Size::Md.height();
        }
        self.text().height_for_width(canvas, width)
    }

    /// Where the 18 DIP disc lands inside `bounds`.
    pub fn glyph_rect(&self, canvas: &dyn Canvas, bounds: Rect) -> Rect {
        self.text().layout(canvas, bounds, self.inner.check_align).0.glyph
    }

    /// Paints it with the selection transition at `progress`: the accent ring
    /// cross-fades over the neutral one and the dot grows from the centre, as
    /// `paintRadio` does. [`Widget::paint`] is this at 0 or 1.
    pub fn paint_progress(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState, progress: f32) {
        let t = canvas.theme();
        let s = self.state(state);
        let alpha = if matches!(s, State::Disabled) { DISABLED_ALPHA } else { 1.0 };

        if self.inner.appearance == Appearance::Button {
            let ink = toggle_ink(t, s, progress >= 0.5);
            let face = Face {
                label: &self.inner.text,
                icon: None,
                icon_size: Size::Md.icon(),
                gap: Size::Md.gap(),
                pad: Size::Md.pad_x(),
                loading: None,
                aligned: None,
                mnemonic: None,
            };
            paint_face(canvas, bounds, &face, &ink, state.show_focus_ring() && s != State::Disabled);
            return;
        }

        let text = self.text();
        let (at, lines) = text.layout(canvas, bounds, self.inner.check_align);
        let accent = self.accent.unwrap_or(t.accent);
        paint_radio_glyph(canvas, at.glyph, progress.clamp(0.0, 1.0), s, accent, alpha);
        if state.show_focus_ring() && !matches!(s, State::Disabled) {
            paint_focus_ring(canvas, at.glyph, pill(control::RADIO_BOX), &t.accent);
        }
        text.paint(canvas, &lines, at.text, alpha);
    }
}

/// Paints the ring and the dot at `progress`.
///
/// The web strokes the ring centred on a path of radius `(size - ring) / 2`,
/// i.e. the ring occupies the outer 2 DIP of the 18 box —
/// [`Canvas::stroke_rounded_w`] insets by half the width and shrinks the radius
/// by the same half, which is the identical shape.
fn paint_radio_glyph(
    c: &dyn Canvas,
    glyph: Rect,
    progress: f32,
    state: State,
    accent: D2D1_COLOR_F,
    alpha: f32,
) {
    let t = c.theme();
    let corner = pill(control::RADIO_BOX);
    c.stroke_rounded_w(&glyph, corner, &fade(glyph_border(t, state), alpha), control::RADIO_RING);
    if progress > 0.0 {
        c.stroke_rounded_w(&glyph, corner, &fade(accent, alpha * progress), control::RADIO_RING);
        let dot = control::RADIO_DOT * progress;
        let inset = (control::RADIO_BOX - dot) / 2.0;
        let r = glyph.inflate(-inset, -inset);
        c.fill_rounded(&r, pill(dot), &fade(accent, alpha));
    }
}

impl Widget for RadioButton {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> ControlSize {
        match self.inner.appearance {
            Appearance::Button => {
                let text_w = canvas.measure(&self.inner.text, &canvas.formats().body);
                ControlSize::new((Size::Md.pad_x() * 2.0 + text_w).ceil(), Size::Md.height())
            }
            Appearance::Normal => {
                let desc = self.description.as_deref().filter(|d| !d.is_empty());
                let body = &canvas.formats().body;
                let w = natural_text_width(canvas, &self.inner.text, desc.map(|d| (d, body)));
                glyph_measure_parts(w, desc.is_some())
            }
        }
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_progress(canvas, bounds, state, if self.inner.checked { 1.0 } else { 0.0 });
    }

    fn type_name(&self) -> &'static str {
        "RadioButton"
    }
}

impl Deref for RadioButton {
    type Target = replica::RadioButton;
    fn deref(&self) -> &replica::RadioButton {
        &self.inner
    }
}
impl DerefMut for RadioButton {
    fn deref_mut(&mut self) -> &mut replica::RadioButton {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Switch
// ═════════════════════════════════════════════════════════════════════════════

/// The switch's two sizes — `Toggle.tsx`'s `size?: 'sm' | 'md'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SwitchSize {
    Sm,
    #[default]
    Md,
}

/// One row of `TOGGLE_GEOMETRY` (`toggleCanvas.ts`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwitchGeometry {
    pub width: f32,
    pub height: f32,
    pub track_radius: f32,
    pub thumb: f32,
    pub thumb_radius: f32,
    /// Gap between the track edge and the thumb, on all four sides.
    pub inset: f32,
}

impl SwitchSize {
    /// `sm: { 28, 16, 5, 12, 3, 2 }`, `md: { 36, 20, 6, 14, 4, 3 }`.
    pub const fn geometry(self) -> SwitchGeometry {
        match self {
            SwitchSize::Sm => SwitchGeometry {
                width: 28.0,
                height: 16.0,
                track_radius: 5.0,
                thumb: 12.0,
                thumb_radius: 3.0,
                inset: 2.0,
            },
            SwitchSize::Md => SwitchGeometry {
                width: 36.0,
                height: 20.0,
                track_radius: 6.0,
                thumb: 14.0,
                thumb_radius: 4.0,
                inset: 3.0,
            },
        }
    }
}

/// The toggle switch — `Toggle.tsx`: a 36×20 track with a 14 thumb (or the
/// 28×16 `sm`), and an optional label + description to its right.
///
/// **Its model is a check box**, because that is what a switch is in the
/// toolkit: a two-state control whose click cycles `Unchecked ↔ Checked`. It
/// therefore derefs to [`kubuno_desktop_controls::buttons::CheckBox`] and gets
/// `checked()`, `set_checked`, `auto_check`, `perform_click()` — and `text`,
/// which is the switch's `label` — with no state machine of its own.
/// (`three_state` is honoured by the model but has no rendering — keep it off.)
///
/// With no label it paints only its track, centred in `bounds` (so a caller
/// can hand it a row). With a label the track leads and the text column
/// follows at `gap-2.5`, wrapping like the check box's.
pub struct Switch {
    inner: replica::CheckBox,
    pub size: SwitchSize,
    /// `description`: `text-xs text-text-secondary` under the label.
    pub description: Option<String>,
}

impl Switch {
    /// The `md` track's width — public so a settings row can reserve it
    /// without building a switch.
    pub const WIDTH: f32 = SwitchSize::Md.geometry().width;
    /// The `md` track's height.
    pub const HEIGHT: f32 = SwitchSize::Md.geometry().height;

    pub fn new() -> Self {
        Self { inner: replica::CheckBox::new(), size: SwitchSize::default(), description: None }
    }

    pub fn on(mut self, on: bool) -> Self {
        self.inner.set_checked(on);
        self
    }

    /// Builder: the label to the switch's right (the model's `text`).
    pub fn label(mut self, text: &str) -> Self {
        self.inner.text = text.to_string();
        self
    }

    /// Builder: the description under the label.
    pub fn description(mut self, text: &str) -> Self {
        self.description = Some(text.to_string());
        self
    }

    /// Builder: the size.
    pub fn with_size(mut self, size: SwitchSize) -> Self {
        self.size = size;
        self
    }

    /// The `md` track, centred in `bounds`.
    ///
    /// Centring is what lets a caller hand it a row; handed the rectangle
    /// [`Widget::measure`] asks for, it returns that rectangle unchanged, which
    /// is what makes it identical to the predecessor's origin-anchored `draw`.
    pub fn track(bounds: Rect) -> Rect {
        Self::track_for(bounds, SwitchSize::Md)
    }

    /// [`Switch::track`] for either size.
    pub fn track_for(bounds: Rect, size: SwitchSize) -> Rect {
        let g = size.geometry();
        let cx = (bounds.left + bounds.right) / 2.0;
        let cy = (bounds.top + bounds.bottom) / 2.0;
        let left = cx - g.width / 2.0;
        let top = cy - g.height / 2.0;
        Rect::new(left, top, left + g.width, top + g.height)
    }

    /// The `md` thumb inside a track, at either end of its travel.
    pub fn thumb(track: Rect, on: bool) -> Rect {
        Self::thumb_at(track, SwitchSize::Md, if on { 1.0 } else { 0.0 })
    }

    /// The thumb at `progress` of its travel (0 = off, 1 = on) — `thumbInset +
    /// travel × progress` in `paintToggle`.
    pub fn thumb_at(track: Rect, size: SwitchSize, progress: f32) -> Rect {
        let g = size.geometry();
        let travel = g.width - g.thumb - g.inset * 2.0;
        let left = track.left + g.inset + travel * progress.clamp(0.0, 1.0);
        Rect::new(left, track.top + g.inset, left + g.thumb, track.top + g.inset + g.thumb)
    }

    /// Where the switch sits when it is right-aligned on a settings row —
    /// [`kubuno_drive_desktop_app_controls::switch::bounds_right_aligned`].
    pub fn bounds_right_aligned(row: Rect, margin: f32) -> Rect {
        let left = row.right - margin - Self::WIDTH;
        let top = (row.top + row.bottom) / 2.0 - Self::HEIGHT / 2.0;
        Rect::new(left, top, left + Self::WIDTH, top + Self::HEIGHT)
    }

    /// `cursor-pointer`, `cursor-not-allowed` when disabled (`Toggle.tsx`).
    pub fn cursor(&self, state: WidgetState) -> Cursor {
        cursor_of(true, !self.inner.enabled || state.disabled)
    }

    fn has_text(&self) -> bool {
        !self.inner.text.is_empty() || self.description.as_deref().is_some_and(|d| !d.is_empty())
    }

    /// The label then the `text-xs` description, `gap-0.5` apart.
    fn paragraphs(&self) -> [Paragraph<'_>; 2] {
        [
            Paragraph {
                text: &self.inner.text,
                line_h: switch_text_metrics::LINE,
                gap_before: 0.0,
                secondary: false,
            },
            Paragraph {
                text: self.description.as_deref().unwrap_or(""),
                line_h: switch_text_metrics::DESC_LINE,
                gap_before: switch_text_metrics::DESC_GAP,
                secondary: true,
            },
        ]
    }

    fn layout(&self, c: &dyn Canvas, bounds: Rect) -> (Leading, Vec<TextLine>) {
        let g = self.size.geometry();
        if !self.has_text() {
            let track = Self::track_for(bounds, self.size);
            return (Leading { glyph: track, text: Rect::new(track.right, track.top, track.right, track.top) }, Vec::new());
        }
        let f = c.formats();
        let width = (bounds.right - bounds.left - g.width - switch_text_metrics::GAP).max(0.0);
        let paras = self.paragraphs();
        let lines = layout_text_block(&paras, width, bounds.bottom - bounds.top, &|s, secondary| {
            c.measure(s, if secondary { &f.caption } else { &f.body })
        });
        let leading = place_leading(
            bounds,
            self.inner.check_align,
            (g.width, g.height),
            switch_text_metrics::GAP,
            switch_text_metrics::TRACK_TOP,
            text_block_height(&lines),
        );
        (leading, lines)
    }

    /// The height this switch needs at `width` to show its whole label and
    /// description, wrapped.
    pub fn height_for_width(&self, canvas: &dyn Canvas, width: f32) -> f32 {
        let g = self.size.geometry();
        if !self.has_text() {
            return g.height;
        }
        let f = canvas.formats();
        let text_w = (width - g.width - switch_text_metrics::GAP).max(0.0);
        let paras = self.paragraphs();
        let lines = layout_text_block(&paras, text_w, f32::INFINITY, &|s, secondary| {
            canvas.measure(s, if secondary { &f.caption } else { &f.body })
        });
        text_block_height(&lines).max(switch_text_metrics::TRACK_TOP + g.height)
    }

    /// Where the track lands inside `bounds`.
    pub fn track_rect(&self, canvas: &dyn Canvas, bounds: Rect) -> Rect {
        self.layout(canvas, bounds).0.glyph
    }

    /// Paints it with the thumb at `progress` of its travel — `paintToggle`:
    /// the off track (`--color-surface-3` filled, `--color-border` outlined),
    /// the on track over it at `progress` alpha, then the thumb with its
    /// 1 DIP drop shadow. [`Widget::paint`] is this at 0 or 1.
    pub fn paint_progress(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState, progress: f32) {
        let t = canvas.theme();
        let g = self.size.geometry();
        let progress = progress.clamp(0.0, 1.0);
        let enabled = self.inner.enabled && !state.disabled;
        // `opacity-50` on the whole `<label>` (`Toggle.tsx`).
        let alpha = if enabled { 1.0 } else { DISABLED_ALPHA };
        let (at, lines) = self.layout(canvas, bounds);
        let track = at.glyph;

        canvas.fill_rounded(&track, g.track_radius, &fade(t.surface_3, alpha));
        canvas.stroke_rounded(&track, g.track_radius, &fade(t.card_stroke, alpha));
        if progress > 0.0 {
            canvas.fill_rounded(&track, g.track_radius, &fade(t.accent, alpha * progress));
        }

        let thumb = Self::thumb_at(track, self.size, progress);
        let shadow = ShadowLayer { opacity: THUMB_SHADOW.opacity * alpha, ..THUMB_SHADOW };
        canvas.draw_shadow(&thumb, g.thumb_radius, &[shadow], SHADOW_BLACK);
        canvas.fill_rounded(&thumb, g.thumb_radius, &fade(t.accent_foreground, alpha));

        if state.show_focus_ring() && enabled {
            paint_focus_ring(canvas, track, g.track_radius, &t.accent);
        }

        let f = canvas.formats();
        paint_text_lines(
            canvas,
            &lines,
            at.text,
            (&f.body, &f.caption),
            (&fade(t.text_primary, alpha), &fade(t.text_secondary, alpha)),
        );
    }
}

impl Default for Switch {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Switch {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> ControlSize {
        let g = self.size.geometry();
        if !self.has_text() {
            return ControlSize::new(g.width, g.height);
        }
        let f = canvas.formats();
        let desc = self.description.as_deref().filter(|d| !d.is_empty());
        let w = natural_text_width(canvas, &self.inner.text, desc.map(|d| (d, &f.caption)));
        let mut text_h = if self.inner.text.is_empty() { 0.0 } else { switch_text_metrics::LINE };
        if desc.is_some() {
            if text_h > 0.0 {
                text_h += switch_text_metrics::DESC_GAP;
            }
            text_h += switch_text_metrics::DESC_LINE;
        }
        ControlSize::new(
            g.width + switch_text_metrics::GAP + w,
            text_h.max(switch_text_metrics::TRACK_TOP + g.height),
        )
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.paint_progress(canvas, bounds, state, if self.inner.checked() { 1.0 } else { 0.0 });
    }

    fn type_name(&self) -> &'static str {
        "Switch"
    }
}

impl Deref for Switch {
    type Target = replica::CheckBox;
    fn deref(&self) -> &replica::CheckBox {
        &self.inner
    }
}
impl DerefMut for Switch {
    fn deref_mut(&mut self) -> &mut replica::CheckBox {
        &mut self.inner
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
//
// Four kinds, in this order: PARITY with the predecessor's geometry,
// measurement and layout (pure functions over a measuring closure — a `--lib`
// test cannot build the COM `TextFormats` a live `Canvas` carries), keyboard
// and motion, and the state machines the replica owns. The gallery's `buttons`
// page closes the gap by painting everything from one live canvas.
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::text as text_size;
    use kubuno_drive_desktop_app_controls::switch as web_switch;

    fn same_colour(a: D2D1_COLOR_F, b: D2D1_COLOR_F) -> bool {
        a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a
    }

    fn ltrb(r: Rect) -> (f32, f32, f32, f32) {
        (r.left, r.top, r.right, r.bottom)
    }

    /// A fake font: every character is 7 DIP wide.
    fn mono(s: &str, _secondary: bool) -> f32 {
        s.chars().count() as f32 * 7.0
    }

    // ── Parity with the predecessor ────────────────────────────────────────

    /// The radius is the predecessor's, and no builder can move it.
    #[test]
    fn the_radius_is_the_predecessors_and_is_not_overridable() {
        assert_eq!(RADIUS, web_button::RADIUS);
        assert_eq!(RADIUS, 4.0, "`rounded-md` resolves to 4 in this design system");
        let b = Button::new("OK").variant(Variant::Danger).size(Size::Lg).pad_x(24.0);
        assert_eq!(b.padding_x(), 24.0);
        assert_eq!(RADIUS, web_button::RADIUS);
    }

    /// Every term of the size ramp equals the predecessor's, size by size.
    #[test]
    fn the_size_ramp_matches_the_predecessor() {
        for (mine, theirs) in [
            (Size::Sm, web_button::Size::Sm),
            (Size::Md, web_button::Size::Md),
            (Size::Lg, web_button::Size::Lg),
        ] {
            assert_eq!(mine.height(), theirs.height(), "height of {mine:?}");
            assert_eq!(mine.pad_x(), theirs.pad_x(), "pad_x of {mine:?}");
            assert_eq!(mine.gap(), theirs.gap(), "gap of {mine:?}");
        }
    }

    /// The intrinsic width is the predecessor's sum — `2·pad + icon + gap +
    /// label` — in each of its four shapes, and a loading button holds its ring.
    #[test]
    fn the_intrinsic_width_is_the_predecessors_sum() {
        let text = 40.0;

        let plain = Button::new("OK");
        assert_eq!(plain.width_of(text), 16.0 * 2.0 + text);

        let with_icon = Button::new("OK").icon("Plus");
        assert_eq!(with_icon.width_of(text), 16.0 * 2.0 + 18.0 + 8.0 + text);

        let icon_only = Button::new("").icon("Plus");
        assert_eq!(icon_only.width_of(text), 16.0 * 2.0 + 18.0);

        let small = Button::new("OK").size(Size::Sm).icon("Plus");
        assert_eq!(small.width_of(text), 12.0 * 2.0 + 16.0 + 6.0 + text);

        let padded = Button::new("OK").pad_x(24.0);
        assert_eq!(padded.width_of(text), 24.0 * 2.0 + text);

        let loading = Button::new("Envoyer").loading(true);
        assert_eq!(loading.width_of(text), 16.0 * 2.0 + 16.0, "`h-4 w-4` replaces the content");
    }

    /// The switch's track and travel are the predecessor's; the public
    /// constants are the `md` row of `TOGGLE_GEOMETRY`.
    #[test]
    fn the_switch_geometry_matches_the_predecessor() {
        assert_eq!(Switch::WIDTH, web_switch::WIDTH);
        assert_eq!(Switch::HEIGHT, web_switch::HEIGHT);

        let at = Rect::new(10.0, 20.0, 10.0 + web_switch::WIDTH, 20.0 + web_switch::HEIGHT);
        let track = Switch::track(at);
        assert_eq!((track.left, track.top), (10.0, 20.0));
        assert_eq!(track.right - track.left, web_switch::WIDTH);
        assert_eq!(track.bottom - track.top, web_switch::HEIGHT);

        // The travel the predecessor's own test pins: 36 − 14 − 2·3 = 16.
        let g = SwitchSize::Md.geometry();
        let off = Switch::thumb(track, false);
        let on = Switch::thumb(track, true);
        assert_eq!(on.left - off.left, 16.0);
        assert_eq!(off.left - track.left, g.inset);
        assert_eq!(track.right - on.right, g.inset);
        assert_eq!(off.top, on.top, "the thumb travels horizontally only");
        assert_eq!(off.bottom - off.top, g.thumb);

        let row = Rect::new(0.0, 100.0, 400.0, 140.0);
        let b = Switch::bounds_right_aligned(row, 16.0);
        let theirs = web_switch::bounds_right_aligned(&row, 16.0);
        assert_eq!((b.left, b.top, b.right, b.bottom), (theirs.left, theirs.top, theirs.right, theirs.bottom));
    }

    /// `sm` is `toggleCanvas.ts`'s row, and the thumb interpolates linearly in
    /// progress between its two ends.
    #[test]
    fn the_small_switch_and_the_thumb_travel() {
        let g = SwitchSize::Sm.geometry();
        assert_eq!((g.width, g.height, g.track_radius, g.thumb, g.thumb_radius, g.inset), (28.0, 16.0, 5.0, 12.0, 3.0, 2.0));
        let track = Switch::track_for(Rect::new(0.0, 0.0, 28.0, 16.0), SwitchSize::Sm);
        assert_eq!((track.left, track.top, track.right, track.bottom), (0.0, 0.0, 28.0, 16.0));
        let off = Switch::thumb_at(track, SwitchSize::Sm, 0.0);
        let half = Switch::thumb_at(track, SwitchSize::Sm, 0.5);
        let on = Switch::thumb_at(track, SwitchSize::Sm, 1.0);
        assert_eq!(off.left, 2.0);
        assert_eq!(on.right, 26.0);
        assert!((half.left - (off.left + on.left) / 2.0).abs() < 1e-4);
        assert_eq!(ltrb(Switch::thumb_at(track, SwitchSize::Sm, 7.0)), ltrb(on), "progress is clamped");
    }

    /// Dimming is the same arithmetic the predecessor publishes.
    #[test]
    fn fading_matches_the_predecessors_fade() {
        let c = D2D1_COLOR_F { r: 0.1, g: 0.2, b: 0.3, a: 0.8 };
        for alpha in [1.0, DISABLED_ALPHA, 0.4, 0.0] {
            assert!(same_colour(fade(c, alpha), web_switch::fade(&c, alpha)), "alpha {alpha}");
        }
    }

    /// The state ladder puts disabled first; focus changes no fill.
    #[test]
    fn the_state_ladder_puts_disabled_first() {
        let all = WidgetState::REST.hot(true).pressed(true).focused(true);
        assert_eq!(State::of(false, all), State::Disabled);
        assert_eq!(State::of(true, all.disabled(true)), State::Disabled);
        assert_eq!(State::of(true, all), State::Active, "pressed outranks hot");
        assert_eq!(State::of(true, WidgetState::REST.hot(true)), State::Hover);
        assert_eq!(State::of(true, WidgetState::REST), State::Rest);
        assert_eq!(State::of(true, WidgetState::REST.focused(true)), State::Rest);

        for (mine, theirs) in [
            (State::Rest, web_button::State::Rest),
            (State::Hover, web_button::State::Hover),
            (State::Active, web_button::State::Active),
            (State::Disabled, web_button::State::Disabled),
        ] {
            assert_eq!(web_button::State::from(mine), theirs);
        }
    }

    /// A disabled model is disabled however the caller reports the pointer, and
    /// so is a loading one (`disabled || loading`).
    #[test]
    fn a_disabled_or_loading_button_never_lights_up() {
        let mut b = Button::new("OK");
        b.enabled = false;
        assert_eq!(b.state(WidgetState::REST.hot(true).pressed(true)), State::Disabled);
        assert!(!b.focusable());

        let l = Button::new("OK").loading(true);
        assert_eq!(l.state(WidgetState::REST.hot(true)), State::Disabled);
        assert!(!l.focusable());
        assert_eq!(l.cursor(WidgetState::REST.hot(true)), Cursor::NotAllowed);
        assert_eq!(Button::new("OK").cursor(WidgetState::REST.hot(true)), Cursor::Arrow);
    }

    // ── Measurement and layout ─────────────────────────────────────────────

    /// A button measures as a token height, never as the replica's
    /// system-metric `preferred_size`.
    #[test]
    fn a_button_measures_the_kubuno_height() {
        assert_eq!(Size::Sm.height(), 32.0);
        assert_eq!(Size::Md.height(), 36.0);
        assert_eq!(Size::Lg.height(), 44.0);
        assert!(Size::Sm.height() > 23.0);
    }

    /// Content that fits is centred as one group; content that does not is
    /// pinned to the left padding and ellipsised against the right one, so the
    /// label never crosses the face's edge.
    #[test]
    fn the_face_centres_what_fits_and_ellipsises_what_does_not() {
        let r = Rect::new(0.0, 0.0, 200.0, 36.0);

        let plain = face_layout(r, 16.0, None, 8.0, Some(60.0));
        assert!(plain.centred && !plain.truncated);

        let with_icon = face_layout(r, 16.0, Some(18.0), 8.0, Some(60.0));
        let icon = with_icon.icon.expect("an icon rect");
        // Group = 18 + 8 + 60 = 86, centred on 100.
        assert_eq!(icon.left, 100.0 - 43.0);
        assert_eq!(with_icon.label.left, icon.left + 26.0);
        assert!(!with_icon.truncated);

        let narrow = Rect::new(0.0, 0.0, 120.0, 36.0);
        let long = face_layout(narrow, 16.0, None, 8.0, Some(300.0));
        assert!(long.truncated && !long.centred);
        assert_eq!((long.label.left, long.label.right), (16.0, 104.0));

        let long_icon = face_layout(narrow, 16.0, Some(18.0), 8.0, Some(300.0));
        assert_eq!(long_icon.icon.map(|i| i.left), Some(16.0));
        assert_eq!((long_icon.label.left, long_icon.label.right), (42.0, 104.0));

        // Narrower than its own padding: nothing is placed outside the face.
        let tiny = face_layout(Rect::new(0.0, 0.0, 20.0, 36.0), 16.0, Some(18.0), 8.0, Some(50.0));
        assert!(tiny.label.left >= 0.0 && tiny.label.right <= 20.0);
        assert!(tiny.label.left <= tiny.label.right);

        let icon_only = face_layout(r, 16.0, Some(18.0), 8.0, None);
        assert_eq!(icon_only.icon.map(ltrb), Some(ltrb(r)));
    }

    /// The loading ring's dabs sit on the 7 DIP centre line and leave the top
    /// quarter empty at phase 0.
    #[test]
    fn the_loading_ring_leaves_its_top_quarter_transparent() {
        let dabs = loading_dabs((50.0, 50.0), 0.0);
        assert!(dabs.len() > 8);
        for (x, y) in &dabs {
            let d = ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt();
            assert!((d - 7.0).abs() < 1e-3);
        }
        // Straight up (50, 43) is in the transparent quarter.
        assert!(dabs.iter().all(|(x, y)| (x - 50.0).abs() > 1.0 || *y > 45.0));
        // Straight down is painted.
        assert!(dabs.iter().any(|(x, y)| (x - 50.0).abs() < 1.0 && *y > 56.0));
        // Half a turn later the gap is at the bottom.
        let turned = loading_dabs((50.0, 50.0), 0.5);
        assert!(turned.iter().any(|(x, y)| (x - 50.0).abs() < 1.0 && *y < 44.0));
        assert_eq!(loading_dabs((0.0, 0.0), f32::NAN), loading_dabs((0.0, 0.0), 0.0));
    }

    /// A glyph control is box + gap + label, at least a line tall; one line
    /// more (plus `mt-0.5`) with a description.
    #[test]
    fn a_glyph_control_measures_box_gap_label() {
        let s = glyph_measure_parts(61.0, false);
        assert_eq!(s.width, 18.0 + 8.0 + 61.0);
        assert_eq!(s.height, 20.0, "the 20 line box is taller than the 18 box");
        let d = glyph_measure_parts(61.0, true);
        assert_eq!(d.height, 20.0 + 2.0 + 20.0);
        let bare = glyph_measure_parts(0.0, false);
        assert_eq!(bare.width, 18.0, "no label, no gap");
        assert_eq!(control::CHECK_BOX, 18.0);
        assert_eq!(control::CHECK_GAP, 8.0);
    }

    /// The box sits on the first line (`items-start mt-px`), the group is
    /// centred in a taller single-line row, and `CheckAlign` right mirrors it.
    #[test]
    fn the_leading_glyph_is_placed_by_check_align() {
        let bounds = Rect::new(100.0, 200.0, 400.0, 224.0);
        let side = (18.0, 18.0);

        let l = place_leading(bounds, ContentAlignment::MiddleLeft, side, 8.0, 1.0, 20.0);
        assert_eq!(l.glyph.left, 100.0);
        assert_eq!(l.glyph.right - l.glyph.left, 18.0);
        assert_eq!((l.glyph.top + l.glyph.bottom) / 2.0, (bounds.top + bounds.bottom) / 2.0);
        assert_eq!(l.text.left, l.glyph.right + 8.0);
        assert_eq!(l.text.right, bounds.right);

        let r = place_leading(bounds, ContentAlignment::MiddleRight, side, 8.0, 1.0, 20.0);
        assert_eq!(r.glyph.right, bounds.right);
        assert_eq!(r.text.left, bounds.left);
        assert_eq!(r.text.right, r.glyph.left - 8.0);

        // A three-line block: the box stays on the FIRST line.
        let tall = Rect::new(0.0, 0.0, 300.0, 60.0);
        let m = place_leading(tall, ContentAlignment::MiddleLeft, side, 8.0, 1.0, 60.0);
        assert_eq!(m.glyph.top, 1.0);
        assert_eq!(m.text.top, 0.0);

        // Bounds shorter than a line keep the box inside them.
        let short = Rect::new(0.0, 0.0, 300.0, 18.0);
        let s = place_leading(short, ContentAlignment::MiddleLeft, side, 8.0, 1.0, 20.0);
        assert_eq!((s.glyph.top, s.glyph.bottom), (0.0, 18.0));
    }

    /// A long label wraps when given the height and ellipsises its last
    /// visible line when not; the description drops out before the label.
    #[test]
    fn a_long_label_wraps_or_ellipsises() {
        let paras = glyph_paragraphs("aaaa bbbb cccc dddd", Some("eeee ffff"));
        // 10 chars = 70 DIP wide: two words per line.
        let all = layout_text_block(&paras, 70.0, f32::INFINITY, &mono);
        let texts: Vec<&str> = all.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["aaaa bbbb", "cccc dddd", "eeee ffff"]);
        assert_eq!(all[2].top, 40.0 + 2.0, "the description sits `mt-0.5` under the label");
        assert!(all[2].secondary);
        assert_eq!(text_block_height(&all), 62.0);

        // One line of room: the label's first line takes the rest, ellipsised.
        let one = layout_text_block(&paras, 70.0, 20.0, &mono);
        assert_eq!(one.len(), 1);
        assert!(one[0].truncated);
        assert_eq!(one[0].text, "aaaa bbbb cccc dddd");

        // Two lines of room: the whole label, no description.
        let two = layout_text_block(&paras, 70.0, 40.0, &mono);
        assert_eq!(two.len(), 2);
        assert!(!two[1].truncated && !two[1].secondary);

        // Less than a line: the first line is still there.
        assert_eq!(layout_text_block(&paras, 70.0, 5.0, &mono).len(), 1);
        // An empty label with a description starts with the description.
        let d = layout_text_block(&glyph_paragraphs("", Some("x")), 70.0, 99.0, &mono);
        assert_eq!((d.len(), d[0].top, d[0].secondary), (1, 0.0, true));
    }

    /// The check box and radio geometries are the web painters'.
    #[test]
    fn the_glyph_metrics_are_the_web_painters() {
        assert_eq!(control::CHECK_BOX, 18.0);
        assert_eq!(control::CHECK_BORDER, 2.0);
        assert_eq!(control::CHECK_RADIUS, 4.0);
        assert_eq!(control::CHECK_TICK, 11.0);
        assert_eq!(control::RADIO_BOX, 18.0);
        assert_eq!(control::RADIO_RING, 2.0);
        assert_eq!(control::RADIO_DOT, 10.0);
        assert_eq!(glyph_metrics::dash_height(), 2.0);
        const { assert!(control::RADIO_DOT <= control::RADIO_BOX - 2.0 * control::RADIO_RING) };
        assert_eq!(FOCUS_OUTSET, 3.0, "`ring-offset-1` + `ring-2`");
    }

    // ── Hit-testing ────────────────────────────────────────────────────────

    #[test]
    fn an_icon_button_hit_test_is_a_disc() {
        let b = IconButton::header("Search");
        let r = Rect::new(0.0, 0.0, 36.0, 36.0);
        assert!(b.hit_test(r, 18.0, 18.0), "the centre");
        assert!(b.hit_test(r, 18.0, 0.5), "just inside the top edge");
        assert!(!b.hit_test(r, 1.0, 1.0), "the top-left corner is outside the circle");
        assert!(!b.hit_test(r, 35.0, 35.0), "and so is the bottom-right one");
        assert!(!b.hit_test(r, 100.0, 18.0));
    }

    #[test]
    fn a_text_button_hit_test_is_its_rectangle() {
        let b = Button::new("OK");
        let r = Rect::new(10.0, 10.0, 100.0, 46.0);
        assert!(b.hit_test(r, 10.0, 10.0));
        assert!(b.hit_test(r, 99.9, 45.9));
        assert!(!b.hit_test(r, 100.0, 20.0));
        assert!(!b.hit_test(r, 9.9, 20.0));
    }

    // ── Keyboard and motion ────────────────────────────────────────────────

    /// Enter and Space activate a button; only Space toggles a check box.
    #[test]
    fn activation_keys_follow_the_native_controls() {
        assert!(is_button_activation(vk::ENTER));
        assert!(is_button_activation(vk::SPACE));
        assert!(!is_button_activation(vk::TAB));
        assert!(is_check_toggle(vk::SPACE));
        assert!(!is_check_toggle(vk::ENTER));
    }

    /// Arrows walk a radio group with wrap-around, skipping disabled options;
    /// the Tab stop is the checked option, else the first enabled one.
    #[test]
    fn radio_group_keyboard() {
        let all = [true, true, true];
        assert_eq!(radio_arrow_target(0, &all, vk::DOWN), Some(1));
        assert_eq!(radio_arrow_target(0, &all, vk::RIGHT), Some(1));
        assert_eq!(radio_arrow_target(2, &all, vk::DOWN), Some(0), "wraps forward");
        assert_eq!(radio_arrow_target(0, &all, vk::UP), Some(2), "wraps backward");
        assert_eq!(radio_arrow_target(0, &all, vk::LEFT), Some(2));
        assert_eq!(radio_arrow_target(0, &all, vk::ENTER), None);

        let holes = [true, false, true];
        assert_eq!(radio_arrow_target(0, &holes, vk::DOWN), Some(2), "skips the disabled one");
        assert_eq!(radio_arrow_target(0, &[true, false], vk::DOWN), None);
        assert_eq!(radio_arrow_target(0, &[], vk::DOWN), None);

        assert_eq!(radio_tab_stop(Some(2), &all), Some(2));
        assert_eq!(radio_tab_stop(None, &all), Some(0));
        assert_eq!(radio_tab_stop(Some(1), &holes), Some(0), "a disabled checked option is no stop");
        assert_eq!(radio_tab_stop(None, &[false, false]), None);
    }

    /// `easeStandard` leaves at once and settles, and hits both ends exactly.
    #[test]
    fn the_standard_curve() {
        assert_eq!(ease_standard(0.0), 0.0);
        assert_eq!(ease_standard(1.0), 1.0);
        assert_eq!(ease_standard(-3.0), 0.0);
        assert_eq!(ease_standard(9.0), 1.0);
        // cubic-bezier(0.4, 0, 0.2, 1) is 0.7756 at the midpoint (the value a
        // browser's `transition-timing-function` sampler gives).
        let mid = ease_standard(0.5);
        assert!((mid - 0.7756).abs() < 0.002, "{mid}");
        let mut prev = 0.0;
        for i in 1..=20 {
            let v = ease_standard(i as f32 / 20.0);
            assert!(v >= prev, "monotonic");
            prev = v;
        }
    }

    /// A transition starts where it is and reverses smoothly when retargeted.
    #[test]
    fn a_transition_eases_and_retargets_from_where_it_is() {
        let mut t = Transition::settled(0.0);
        assert!(!t.running(0));
        assert_eq!(t.value(123), 0.0);
        t.retarget(1.0, 1000, SWITCH_TRANSITION_MS);
        assert!(t.running(1000));
        assert_eq!(t.value(1000), 0.0);
        assert_eq!(t.value(1150), 1.0);
        assert!(!t.running(1150));
        let mid = t.value(1075);
        assert!(mid > 0.5 && mid < 1.0);
        // Retarget mid-way: it starts back from `mid`, not from 1.
        t.retarget(0.0, 1075, SWITCH_TRANSITION_MS);
        assert!((t.value(1075) - mid).abs() < 1e-5);
        // Retargeting to where it already heads changes nothing.
        let before = t;
        t.retarget(0.0, 1100, SWITCH_TRANSITION_MS);
        assert_eq!(t, before);
    }

    // ── The replica's state machines, reused rather than rewritten ──────────

    #[test]
    fn clicking_walks_the_replicas_check_cycle() {
        let mut b = CheckBox::new("Partager");
        assert_eq!(b.check_state, CheckState::Unchecked);
        b.perform_click();
        assert_eq!(b.check_state, CheckState::Checked);
        assert!(b.checked());
        b.perform_click();
        assert_eq!(b.check_state, CheckState::Unchecked);

        let mut t = CheckBox::new("Partager").tri_state();
        t.perform_click();
        t.perform_click();
        assert_eq!(t.check_state, CheckState::Indeterminate);
        assert!(t.checked(), "Indeterminate reports as checked, as in the toolkit");
        t.perform_click();
        assert_eq!(t.check_state, CheckState::Unchecked);
    }

    #[test]
    fn auto_check_off_freezes_the_state() {
        let mut b = CheckBox::new("Partager");
        b.auto_check = false;
        b.perform_click();
        assert_eq!(b.check_state, CheckState::Unchecked);
    }

    #[test]
    fn a_radio_selects_but_never_unselects_itself() {
        let mut r = RadioButton::new("Liste");
        assert!(!r.checked);
        r.perform_click();
        assert!(r.checked);
        r.perform_click();
        assert!(r.checked, "a second click does not clear it");
        assert!(!RadioButton::new("Grille").tab_stop);
    }

    #[test]
    fn a_switch_is_a_two_state_check_box() {
        let mut s = Switch::new();
        assert!(!s.checked());
        s.perform_click();
        assert!(s.checked());
        s.perform_click();
        assert!(!s.checked());
        assert_eq!(Switch::new().on(true).check_state, CheckState::Checked);
        // Its label is the model's text.
        assert_eq!(Switch::new().label("Wi-Fi").text, "Wi-Fi");
        assert_eq!(Switch::new().cursor(WidgetState::REST), Cursor::Hand);
        assert_eq!(Switch::new().cursor(WidgetState::REST.disabled(true)), Cursor::NotAllowed);
    }

    #[test]
    fn a_toggle_button_is_a_check_box_in_button_appearance() {
        let plain = CheckBox::new("Grille");
        assert_eq!(plain.appearance, Appearance::Normal);
        assert_eq!(plain.type_name(), "CheckBox");
        assert_eq!(plain.cursor(WidgetState::REST), Cursor::Hand);

        let mut toggle: ToggleButton = CheckBox::toggle_button("Grille");
        assert_eq!(toggle.appearance, Appearance::Button);
        assert_eq!(toggle.type_name(), "ToggleButton");
        assert_eq!(toggle.cursor(WidgetState::REST), Cursor::Arrow);
        toggle.perform_click();
        assert!(toggle.checked());
    }

    #[test]
    fn the_defaults_are_the_webs_over_the_replicas() {
        let b = Button::new("Envoyer");
        assert_eq!(b.variant, Variant::Primary);
        assert_eq!(b.size, Size::Md);
        assert!(b.icon.is_none());
        assert!(!b.loading);
        assert_eq!(b.padding_x(), 16.0);
        assert!(b.enabled);
        assert_eq!(b.text, "Envoyer");
        assert_eq!(b.text_align, ContentAlignment::MiddleCenter);
        let c = CheckBox::new("Partager");
        assert_eq!(c.check_align, ContentAlignment::MiddleLeft);
        assert_eq!(c.text_align, ContentAlignment::MiddleLeft);
        assert!(c.description.is_none() && c.accent.is_none());
        assert_eq!(Switch::new().size, SwitchSize::Md);
    }

    #[test]
    fn the_body_text_is_the_web_body_step_in_the_control_line_box() {
        // The default text is the web's body step (`--kb-text-body`, 13.5 px).
        // The control line box stays 20 px.
        assert_eq!(text_size::BODY, 13.5);
        assert_eq!(glyph_metrics::LINE, 20.0, "the control line box is unchanged");
    }

    /// `TextAlign`, `ImageAlign` and `TextImageRelation`, laid out the WinForms way.
    #[test]
    fn an_aligned_face_places_text_and_image_like_winforms() {
        use replica::TextImageRelation as R;
        let r = Rect::new(0.0, 0.0, 200.0, 44.0);
        // Text alone, middle left: at the padding, vertically centred.
        let f = aligned_face(r, 10.0, Some(50.0), 20.0, ContentAlignment::MiddleLeft, None, ContentAlignment::MiddleCenter, R::Overlay, 6.0);
        let t = f.text.unwrap();
        assert_eq!((t.left, t.right, t.top, t.bottom), (10.0, 60.0, 12.0, 32.0));
        // Bottom right.
        let t = aligned_face(r, 10.0, Some(50.0), 20.0, ContentAlignment::BottomRight, None, ContentAlignment::MiddleCenter, R::Overlay, 6.0).text.unwrap();
        assert_eq!((t.right, t.bottom), (190.0, 42.0));
        // Image before text, the block centred: [16][6][50] = 72 wide, from 64.
        let f = aligned_face(r, 10.0, Some(50.0), 20.0, ContentAlignment::MiddleCenter, Some((16.0, 16.0)), ContentAlignment::MiddleCenter, R::ImageBeforeText, 6.0);
        let (i, t) = (f.image.unwrap(), f.text.unwrap());
        assert_eq!((i.left, i.right, t.left, t.right), (64.0, 80.0, 86.0, 136.0));
        // Text before image.
        let f = aligned_face(r, 10.0, Some(50.0), 20.0, ContentAlignment::MiddleCenter, Some((16.0, 16.0)), ContentAlignment::MiddleCenter, R::TextBeforeImage, 6.0);
        assert!(f.text.unwrap().right < f.image.unwrap().left);
        // Image above text: stacked, centred on one axis.
        let tall = Rect::new(0.0, 0.0, 200.0, 80.0);
        let f = aligned_face(tall, 10.0, Some(50.0), 20.0, ContentAlignment::MiddleCenter, Some((24.0, 24.0)), ContentAlignment::MiddleCenter, R::ImageAboveText, 6.0);
        assert!(f.image.unwrap().bottom <= f.text.unwrap().top);
        assert_eq!((f.image.unwrap().left + f.image.unwrap().right) / 2.0, 100.0);
        // Overlay: each by its own alignment.
        let f = aligned_face(r, 10.0, Some(50.0), 20.0, ContentAlignment::MiddleRight, Some((16.0, 16.0)), ContentAlignment::MiddleLeft, R::Overlay, 6.0);
        assert_eq!((f.image.unwrap().left, f.text.unwrap().right), (10.0, 190.0));
    }

    /// The default face (centred text, no image) keeps the historical layout.
    #[test]
    fn a_default_button_is_not_aligned() {
        let b = Button::new("Ok");
        assert!(!b.aligned());
        let mut b = Button::new("Ok");
        b.text_align = ContentAlignment::TopLeft;
        assert!(b.aligned());
    }

    /// Every `TextImageRelation` × every alignment keeps the icon and the label inside the face, in
    /// the order the relation says, `gap` apart, and never overlapping unless they overlay.
    #[test]
    fn icon_and_label_layout_for_every_relation_and_alignment() {
        use replica::TextImageRelation as R;
        use ContentAlignment as A;
        let rect = Rect::new(0.0, 0.0, 200.0, 90.0);
        let (text_w, line, icon, gap) = (60.0, LABEL_LINE, (24.0, 24.0), 6.0);
        let aligns = [A::TopLeft, A::TopCenter, A::TopRight, A::MiddleLeft, A::MiddleCenter, A::MiddleRight, A::BottomLeft, A::BottomCenter, A::BottomRight];
        let inside = |r: Rect| r.left >= rect.left - 0.01 && r.right <= rect.right + 0.01 && r.top >= rect.top - 0.01 && r.bottom <= rect.bottom + 0.01;
        for relation in [R::Overlay, R::ImageAboveText, R::TextAboveImage, R::ImageBeforeText, R::TextBeforeImage] {
            for text_align in aligns {
                for image_align in aligns {
                    let f = aligned_face(rect, 8.0, Some(text_w), line, text_align, Some(icon), image_align, relation, gap);
                    let (t, i) = (f.text.expect("a label"), f.image.expect("an icon"));
                    assert!(inside(t) && inside(i), "{relation:?} {text_align:?} {image_align:?}");
                    assert_eq!((i.right - i.left, i.bottom - i.top), icon);
                    match relation {
                        R::ImageBeforeText => assert!((t.left - i.right - gap).abs() < 0.01),
                        R::TextBeforeImage => assert!((i.left - t.right - gap).abs() < 0.01),
                        R::ImageAboveText => assert!((t.top - i.bottom - gap).abs() < 0.01),
                        R::TextAboveImage => assert!((i.top - t.bottom - gap).abs() < 0.01),
                        R::Overlay => {
                            // Each by its own alignment.
                            let (fx, _) = image_align.fractions();
                            let expected = 8.0 + (200.0 - 16.0 - 24.0) * fx;
                            assert!((i.left - expected).abs() < 0.01, "{image_align:?}");
                        }
                    }
                }
            }
        }
    }

    /// An icon laid out like an image: its own size and spacing count in the width, above the label it
    /// makes the button as wide as the wider of the two.
    #[test]
    fn an_aligned_icon_sizes_the_button() {
        let mut b = Button::new("Save").icon("Save");
        b.icon_size = Some(32.0);
        b.gap = Some(4.0);
        b.icon_aligned = true;
        b.text_image_relation = replica::TextImageRelation::ImageBeforeText;
        assert_eq!(b.width_of(40.0), 16.0 * 2.0 + 32.0 + 4.0 + 40.0);
        b.text_image_relation = replica::TextImageRelation::ImageAboveText;
        assert_eq!(b.width_of(40.0), 16.0 * 2.0 + 40.0);
        assert_eq!(b.width_of(10.0), 16.0 * 2.0 + 32.0);
        // Not aligned: the icon still before the label, at its own size and spacing.
        let mut plain = Button::new("Save").icon("Save");
        plain.icon_size = Some(20.0);
        plain.gap = Some(2.0);
        assert_eq!(plain.width_of(40.0), 16.0 * 2.0 + 20.0 + 2.0 + 40.0);
    }
}
