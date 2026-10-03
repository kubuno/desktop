//! Kubuno primitives — **feedback**: [`Spinner`], [`EmptyState`], [`Callout`],
//! [`Accordion`], [`Stepper`].
//!
//! The five surfaces that tell the user what is going on: something is loading,
//! there is nothing here, this block deserves a caveat, this stack folds, this
//! wizard is three steps from done.
//!
//! ## This family is an ASSEMBLY, not a fifth set of primitives
//!
//! None of these draws its own text, its own glyph or its own card. They are
//! built out of what the neighbouring families already publish, and that is the
//! point — a title painted here and a title painted by [`crate::display`] must
//! be the same title:
//!
//! | this module | uses |
//! |---|---|
//! | every text run | [`crate::display::Label`] + [`crate::display::Role`] (the web type scale) |
//! | every glyph | [`crate::display::Icon`] — geometry, never a character |
//! | a section's count pill | [`crate::display::Badge`] |
//! | the rule under an accordion header | [`crate::display::Separator`] |
//! | an accordion section's card | [`crate::containers::Panel`] + [`crate::containers::Surface::Card`] |
//! | an empty state's buttons | [`crate::buttons::Button`] |
//!
//! What is left — and all that is left — is *arithmetic*: where the medallion
//! goes, how tall a folded stack is, where the five bullets of a stepper land.
//! Every one of those is a pure function on this page, so it is a unit test and
//! not a screenshot.
//!
//! ## Where the pixels come from
//!
//! No desktop predecessor exists for any of the five, so the reference is the
//! **web** design system, **read** (none of these surfaces was reachable from
//! this machine to measure):
//!
//! | primitive | web source |
//! |---|---|
//! | [`Spinner`] | `core/frontend/src/ui/Spinner.tsx` |
//! | [`EmptyState`] | `core/frontend/src/ui/EmptyState.tsx` |
//! | [`Callout`] | `core/frontend/src/ui/Callout.tsx` |
//! | [`Accordion`] | `core/frontend/src/ui/Accordion.tsx` |
//! | [`Stepper`] | `core/frontend/src/ui/Stepper.tsx` |
//!
//! Every constant below names the class or the prop it was read from. The ones
//! with **no** source say so out loud: [`SPINNER_ARC`], [`Spinner::DAB_PITCH`]
//! and [`Stepper::VERTICAL_ROW_GAP`]'s justification.
//!
//! ## Animation: the phase is a PARAMETER
//!
//! [`Spinner`] is the only animated primitive here, and it owns no clock —
//! there is none in [`Canvas`] to read, and a painter that read one would draw a
//! different picture for the same inputs, which is the end of both parity
//! testing and pixel diffing.
//!
//! It therefore follows the convention this crate already set in
//! [`crate::range::ProgressBar`], whose indeterminate marquee carries a public
//! `phase: f32` the host advances between frames. Same name, same range
//! (`0.0..=1.0` is one whole cycle), same division of labour. The single
//! deliberate difference is documented on [`Spinner::phase`]: a marquee runs
//! once and therefore **clamps**, a rotation is cyclic and therefore **wraps**.

use std::ops::{Deref, DerefMut};

use kubuno_drive_desktop_app_controls::{Canvas, Rect, Theme};
use kubuno_desktop_controls::containers::Panel as PanelModel;
use kubuno_desktop_controls::enums::{ContentAlignment, Size};
use kubuno_desktop_controls::labels as kc;
use kubuno_desktop_controls::{Control, ControlBase};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::buttons::{Button, Size as ButtonSize, Variant as ButtonVariant};
use crate::containers::{Panel, Surface};
use crate::display::{Badge, BadgeSize, BadgeVariant, Icon, Label, Role, Separator};
use crate::metrics::{control, pill, radius, space};
use crate::{Widget, WidgetState};

// ═════════════════════════════════════════════════════════════════════════════
// Shared metrics and helpers
// ═════════════════════════════════════════════════════════════════════════════

/// A line of `leading-relaxed` body text, in DIP.
///
/// Tailwind's `leading-relaxed` is `line-height: 1.625`, and both `@ui/Callout`
/// and `@ui/EmptyState` set it on their prose (and only on their prose — a title
/// keeps the normal leading). The body face is the web's,
/// [`crate::metrics::text::BODY`] = 13.5, so the relaxed line is
/// `13.5 × 1.625 = 21.94`, ceiled the way `Font.Height` is ceiled everywhere
/// else in this crate: **22**.
pub const RELAXED_LINE: f32 = 22.0;

/// `focus-visible:ring-2` — the ring every focusable part of this family wears
/// (the callout's action and close button, an accordion header, a stepper
/// step, the empty state's documentation link). Same value as the other
/// families' local `FOCUS_RING`.
pub const FOCUS_RING: f32 = 2.0;

/// Paints a `ring-2 ring-primary` INSIDE `rect`.
///
/// Inset rather than outset on purpose: a focus ring painted outside the part
/// would leave the bounds the caller gave the widget (the accordion header is
/// flush with its card, a stepper row with the trail's top edge), and on the
/// web the accordion's `overflow-hidden` card clips an outset ring down to the
/// same inset band anyway.
fn focus_ring(c: &dyn Canvas, rect: Rect, corner: f32) {
    c.stroke_rounded_w(&rect, corner, &c.theme().accent, FOCUS_RING);
}

/// Paints one line in an explicit DirectWrite face — for the `font-medium` /
/// `font-semibold` runs [`Role`] has no member for (`body_strong`,
/// `caption_strong`). Always trimmed with an ellipsis: a line reaching this
/// helper has already been wrapped, so the trim only ever bites on a caller
/// that handed it less room than it measured.
fn strong_line(
    c: &dyn Canvas,
    band: Rect,
    text: &str,
    format: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    centred: bool,
    colour: D2D1_COLOR_F,
) {
    if text.is_empty() || band.right <= band.left {
        return;
    }
    if centred {
        c.text_ellipsis_center(text, &band, format, &colour);
    } else {
        c.text_ellipsis(text, &band, format, &colour);
    }
}

/// Whether a widget must paint as inert — the Kubuno state or the replica's own
/// `Control.Enabled`. Both, because a container greys a subtree through the
/// state without mutating its children.
fn is_inert(model: &ControlBase, state: WidgetState) -> bool {
    state.disabled || !model.enabled
}

/// Greedy word wrap: the lines `text` breaks into when laid out `max_width`
/// wide, measured by `measure`.
///
/// `measure` is a closure and not a [`Canvas`] on purpose — a `TextFormats` is a
/// set of COM objects a unit test cannot build, so injecting the measurement is
/// what makes wrapping (and therefore every height that depends on it) testable
/// without a device. It is the same split [`crate::buttons::Button::width_of`]
/// makes.
///
/// Breaks on whitespace, and on explicit line breaks (`\n`, `\r\n`): each
/// paragraph starts a new line, the way a `<br>` or a second `<p>` does.
///
/// A single word wider than `max_width` is **hard-broken** between characters
/// — the web's `overflow-wrap: break-word` (`break-words`), which is what a
/// browser does to an unbreakable run inside a `min-w-0` flex child: the word
/// first moves to a line of its own, then splits wherever the line is full. No
/// text is ever dropped and no intermediate line is ever ellipsized; every
/// returned line fits `max_width` (a line always holds at least one character,
/// so a column narrower than one glyph still makes progress).
pub fn wrap_lines(text: &str, max_width: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            // Measure the CANDIDATE line whole rather than summing word
            // widths: kerning and the space's own advance are part of what fits.
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if measure(&candidate) <= max_width {
                current = candidate;
                continue;
            }
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if measure(word) <= max_width {
                current.push_str(word);
                continue;
            }
            // `break-words`: split the run; its last piece stays open so the
            // next word may still join it.
            let mut pieces = break_word(word, max_width, &measure);
            if let Some(last) = pieces.pop() {
                lines.extend(pieces);
                current = last;
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

/// Splits one unbreakable run into the longest prefixes that fit `max_width`,
/// on `char` boundaries. Every piece holds at least one character.
fn break_word(word: &str, max_width: f32, measure: &impl Fn(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut piece = String::new();
    for ch in word.chars() {
        piece.push(ch);
        if measure(&piece) > max_width && piece.chars().count() > 1 {
            piece.pop();
            out.push(std::mem::take(&mut piece));
            piece.push(ch);
        }
    }
    if !piece.is_empty() {
        out.push(piece);
    }
    out
}

/// Paints one line of text through [`crate::display::Label`], vertically centred
/// in `band`.
///
/// Everything this family shows is a `Label`: one role, one colour, one
/// alignment, optionally trimmed. Routing every run through the same primitive
/// is what keeps a stepper's caption and a card's caption the same caption.
fn line(
    c: &dyn Canvas,
    band: Rect,
    text: &str,
    role: Role,
    align: ContentAlignment,
    colour: D2D1_COLOR_F,
    trim: bool,
) {
    if text.is_empty() {
        return;
    }
    let mut l = Label::new(text).role(role).align(align);
    l.fore_color = Some(colour);
    l.auto_ellipsis = trim;
    l.paint(c, band, WidgetState::REST);
}

/// Paints a glyph through [`crate::display::Icon`], at exactly `size` DIP with
/// its top-left at `(x, y)`.
///
/// The rectangle is the glyph's own box because `PictureBoxSizeMode::AutoSize`
/// draws at native size from the content's TOP-LEFT (the replica's
/// `labels::image_rect`), so handing it a bigger rectangle would silently
/// left-align the glyph instead of centring it.
fn glyph(c: &dyn Canvas, name: &'static str, x: f32, y: f32, size: f32, colour: D2D1_COLOR_F) {
    let icon = Icon::sized(name, size).tint(Some(colour));
    icon.paint(c, Rect::new(x, y, x + size, y + size), WidgetState::REST);
}

// ═════════════════════════════════════════════════════════════════════════════
// Spinner
// ═════════════════════════════════════════════════════════════════════════════

/// The fraction of the ring the accent arc covers.
///
/// **NO WEB NUMBER**, and none is possible: `@ui/Spinner` is a CSS *border*
/// trick — `rounded-full border-border border-t-primary` — so the accent is
/// whatever the browser's border mitring paints on the top edge of a circle,
/// which is a quarter of the circumference between the two 45° mitre lines. A
/// quarter is therefore the geometry the CSS produces, not a value chosen here;
/// it is written down because a rasteriser has to be told, and a browser does
/// not.
pub const SPINNER_ARC: f32 = 0.25;

/// The four sizes `@ui/Spinner` ships, and no others.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SpinnerSize {
    /// `h-3 w-3 border`.
    Xs,
    /// `h-4 w-4 border-2`.
    Sm,
    /// `h-6 w-6 border-2` — the component's own default.
    #[default]
    Md,
    /// `h-8 w-8 border-[3px]`.
    Lg,
}

/// Every size, smallest first — for the gallery and the tests.
pub const SPINNER_SIZES: [SpinnerSize; 4] =
    [SpinnerSize::Xs, SpinnerSize::Sm, SpinnerSize::Md, SpinnerSize::Lg];

impl SpinnerSize {
    /// The ring's outer box: `h-3` = 12, `h-4` = 16, `h-6` = 24, `h-8` = 32.
    pub const fn box_size(self) -> f32 {
        match self {
            Self::Xs => 12.0,
            Self::Sm => 16.0,
            Self::Md => 24.0,
            Self::Lg => 32.0,
        }
    }

    /// The ring's thickness: `border` = 1, `border-2` = 2, `border-[3px]` = 3.
    pub const fn stroke(self) -> f32 {
        match self {
            Self::Xs => 1.0,
            Self::Sm | Self::Md => 2.0,
            Self::Lg => 3.0,
        }
    }
}

/// The indeterminate activity ring — `@ui/Spinner`.
///
/// A `Spinner` has no WinForms counterpart at all (the toolkit's answer to
/// "working…" is a `ProgressBar` in `Marquee` style, which is a bar and not a
/// ring), so — like [`crate::display::Badge`], `Tooltip` and `Separator` before
/// it — it composes the closest replica that carries the properties a *layout*
/// needs, an empty [`kubuno_desktop_controls::labels::Label`], rather than declaring a
/// rival property block. `bounds`, `dock`, `anchor`, `minimum_size`, `visible`
/// and `enabled` are therefore the replica's, through [`Deref`].
pub struct Spinner {
    inner: kc::Label,
    pub size: SpinnerSize,
    /// Where in its turn the ring is, `0.0..=1.0` for one full revolution.
    ///
    /// The host advances it; this control owns no clock (see the module header).
    /// `animate-spin` is Tailwind's `1s linear infinite`, so a host that wants
    /// the web's cadence advances by `dt_seconds` and nothing else — which is
    /// exactly what [`Spinner::advanced`] does.
    ///
    /// Unlike [`crate::range::ProgressBar::phase`], which clamps because a
    /// marquee travels once and stops, this one **wraps**: a rotation is cyclic,
    /// and clamping it would freeze the ring at the top of its turn the moment
    /// the host's accumulator passed 1.
    pub phase: f32,
}

impl Deref for Spinner {
    type Target = kc::Label;
    fn deref(&self) -> &kc::Label {
        &self.inner
    }
}
impl DerefMut for Spinner {
    fn deref_mut(&mut self) -> &mut kc::Label {
        &mut self.inner
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Spinner {
    /// The longest step, in DIP of arc, between two dabs of the rasterised arc.
    ///
    /// **NO WEB SOURCE**, and there cannot be one: the browser strokes an arc
    /// and [`Canvas`] publishes fills, strokes, text and named geometries but no
    /// arc, so the accent quarter is rasterised here — the same move
    /// [`crate::display::Tooltip`]'s arrow makes with its stack of one-pixel
    /// bands. Half the stroke width is the widest spacing at which consecutive
    /// round dabs still overlap into a continuous line at every size, which is
    /// what makes it the value rather than a taste.
    pub const DAB_PITCH: f32 = 0.5;

    /// A spinner at [`SpinnerSize::Md`], at the start of its turn.
    pub fn new() -> Self {
        Self { inner: kc::Label::new(), size: SpinnerSize::default(), phase: 0.0 }
    }

    /// Builder: the size.
    pub fn with_size(mut self, size: SpinnerSize) -> Self {
        self.size = size;
        self
    }

    /// Builder: the phase.
    pub fn with_phase(mut self, phase: f32) -> Self {
        self.phase = phase;
        self
    }

    /// The phase folded into `0.0..1.0`.
    ///
    /// `rem_euclid` and not `%`: a host that ran its accumulator backwards (a
    /// reversed animation, a clock correction) must land on a real angle rather
    /// than a negative one. A non-finite phase — the shape a division by a zero
    /// frame time takes — reads as the start of the turn instead of poisoning
    /// every coordinate downstream with NaN.
    pub fn turn(&self) -> f32 {
        if self.phase.is_finite() {
            self.phase.rem_euclid(1.0)
        } else {
            0.0
        }
    }

    /// This spinner `dt` turns further on. `dt` is in TURNS, i.e. in seconds at
    /// `animate-spin`'s own `1s linear` cadence.
    pub fn advanced(&self, dt: f32) -> f32 {
        let next = self.turn() + dt;
        if next.is_finite() {
            next.rem_euclid(1.0)
        } else {
            0.0
        }
    }

    /// `animate-spin`'s period: Tailwind's `spin 1s linear infinite`.
    pub const PERIOD_MS: u64 = 1000;

    /// The repaint cadence a host asks for while a spinner is on screen
    /// (`host::request_repaint_after(Spinner::FRAME_MS)`), about 60 frames a
    /// second — the rate a browser runs a CSS animation at.
    pub const FRAME_MS: u32 = 16;

    /// The phase `animate-spin` is at `ms` milliseconds into a monotonic clock
    /// (`host::now_ms()`): one turn per [`Spinner::PERIOD_MS`], wrapping.
    ///
    /// Pure, so the control still owns no clock — the host reads its own and
    /// passes the answer in through [`Spinner::phase`].
    pub fn phase_at(ms: u64) -> f32 {
        (ms % Self::PERIOD_MS) as f32 / Self::PERIOD_MS as f32
    }

    /// Builder: the phase `animate-spin` is at on the clock reading `ms`.
    pub fn at_time(self, ms: u64) -> Self {
        self.with_phase(Self::phase_at(ms))
    }

    /// The accent arc, as `(start, end)` angles in radians, in canvas space
    /// (x right, y **down**, so angles run clockwise).
    ///
    /// At [`Spinner::phase`] 0 the arc is centred on the top of the ring, which
    /// is where `border-t-primary` puts it before the animation starts. It
    /// spans [`SPINNER_ARC`] of a turn, half on either side of that centre.
    pub fn arc(&self) -> (f32, f32) {
        let tau = std::f32::consts::TAU;
        // −TAU/4 is straight up when y grows downward.
        let centre = -tau / 4.0 + self.turn() * tau;
        let half = SPINNER_ARC * tau / 2.0;
        (centre - half, centre + half)
    }

    /// The ring's outer square, centred in `bounds`.
    ///
    /// A caller that sized the spinner from [`Widget::measure`] gets the same
    /// rectangle back; a caller that gave it a whole row gets it centred, which
    /// is what `inline-block` inside a centring flex row reads as.
    pub fn ring_rect(&self, bounds: Rect) -> Rect {
        let s = self.size.box_size();
        let cx = (bounds.left + bounds.right) / 2.0;
        let cy = (bounds.top + bounds.bottom) / 2.0;
        Rect::new(cx - s / 2.0, cy - s / 2.0, cx + s / 2.0, cy + s / 2.0)
    }

    /// The centres of the dabs the accent arc is rasterised from, in the order
    /// they are painted. Pure, so the arc's placement is a test.
    pub fn dab_centres(&self, bounds: Rect) -> Vec<(f32, f32)> {
        let ring = self.ring_rect(bounds);
        let stroke = self.size.stroke();
        // The dabs sit on the stroke's CENTRE line, which is where
        // `stroke_rounded_w` — an inward stroke — puts the ring underneath them.
        let r = (self.size.box_size() - stroke) / 2.0;
        let cx = (ring.left + ring.right) / 2.0;
        let cy = (ring.top + ring.bottom) / 2.0;
        let (from, to) = self.arc();
        let arc_len = (to - from) * r;
        let steps = (arc_len / (stroke * Self::DAB_PITCH)).ceil().max(1.0) as usize;
        (0..=steps)
            .map(|i| {
                let a = from + (to - from) * (i as f32 / steps as f32);
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    }
}

impl Widget for Spinner {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        let s = self.size.box_size();
        self.inner.clamp(Size::new(s, s))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let ring = self.ring_rect(bounds);
        let stroke = self.size.stroke();
        let inert = is_inert(&self.inner.control, state);

        // `border-border`: the full ring, in the same token every hairline in
        // this design system uses.
        canvas.stroke_rounded_w(&ring, pill(self.size.box_size()), &t.card_stroke, stroke);

        // `border-t-primary`: the quarter that makes the rotation visible. A
        // disabled spinner keeps turning in grey rather than stopping — the
        // work is still happening, it is the surface that is inert.
        let accent = if inert { t.text_tertiary } else { t.accent };
        // One smooth arc on the stroke's centre line (where the inward ring
        // stroke above sits), round-capped. It used to be a row of dots — the
        // `dab_centres` — which read as a beaded, shimmering edge once it turned.
        let r = (self.size.box_size() - stroke) / 2.0;
        let centre = ((ring.left + ring.right) / 2.0, (ring.top + ring.bottom) / 2.0);
        let (from, to) = self.arc();
        canvas.stroke_arc(centre, r, from, to - from, stroke, &accent);
    }

    /// A ring is not its bounding square: the hole in the middle belongs to
    /// whatever is behind it, and the corners never were the control.
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        let ring = self.ring_rect(bounds);
        let outer = self.size.box_size() / 2.0;
        let inner = outer - self.size.stroke();
        let cx = (ring.left + ring.right) / 2.0;
        let cy = (ring.top + ring.bottom) / 2.0;
        let d2 = (x - cx).powi(2) + (y - cy).powi(2);
        d2 <= outer * outer && d2 >= inner * inner
    }

    fn type_name(&self) -> &'static str {
        "Spinner"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Callout
// ═════════════════════════════════════════════════════════════════════════════

/// A callout's severity — one-to-one with `@ui/Callout`'s `CalloutVariant`,
/// names included.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CalloutVariant {
    /// `bg-primary-light text-primary`, glyph `Info`.
    #[default]
    Info,
    /// `bg-success-light text-success`, glyph `CheckCircle2`.
    Success,
    /// `bg-warning-light text-warning`, glyph `AlertTriangle`.
    Warning,
    /// `bg-danger-light text-danger`, glyph `AlertCircle`.
    Danger,
}

/// Every variant, in the order `@ui/Callout`'s `SKIN` map declares them.
pub const CALLOUT_VARIANTS: [CalloutVariant; 4] = [
    CalloutVariant::Info,
    CalloutVariant::Success,
    CalloutVariant::Warning,
    CalloutVariant::Danger,
];

impl CalloutVariant {
    /// The tinted ground the severity is carried by.
    ///
    /// The web's own comment is worth keeping: the tint carries the severity
    /// because the project forbids a left accent bar, and the four `*-light`
    /// tokens are remapped by the dark palette (`--color-warning-light` becomes
    /// `#3d3218`), so this reads correctly in both without a branch on
    /// [`crate::ThemeMode`]. All four tokens already existed — nothing was added
    /// to the palette for this family.
    pub fn ground(self, t: &Theme) -> D2D1_COLOR_F {
        match self {
            Self::Info => t.accent_light,
            Self::Success => t.success_light,
            Self::Warning => t.warning_light,
            Self::Danger => t.danger_light,
        }
    }

    /// The hue, used for the GLYPH and the inline action only.
    ///
    /// Never for the prose: the web spells out why — `--color-warning` (#f9ab00)
    /// on `--color-warning-light` (#fef7e0) fails contrast outright, so body
    /// text stays `text_primary` and the accent colour is reserved for marks
    /// where contrast is not a legibility requirement.
    pub fn hue(self, t: &Theme) -> D2D1_COLOR_F {
        match self {
            Self::Info => t.accent,
            Self::Success => t.success,
            Self::Warning => t.warning,
            Self::Danger => t.danger,
        }
    }

    /// The lucide glyph, from `SKIN`'s `Glyph` column.
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Success => "CheckCircle2",
            Self::Warning => "AlertTriangle",
            Self::Danger => "AlertCircle",
        }
    }
}

/// An inline banner qualifying the block it sits in — `@ui/Callout`.
///
/// Not a toast (transient, floating) and not a dialog (blocking): a callout is
/// *in* the flow, and it is as wide as whatever it was given.
///
/// Its model is the body's [`crate::display::Label`], so `text`, `padding`,
/// `enabled`, `bounds`, `dock` and `anchor` are the replica's and a callout
/// drops into the same layout pass as anything else.
pub struct Callout {
    body: Label,
    pub variant: CalloutVariant,
    /// `title` — the `font-medium` first line. `None` renders body only.
    pub title: Option<String>,
    /// The single inline action ("Retry", "Configure", "See the log").
    ///
    /// Stored as its label and optional glyph rather than as a
    /// [`crate::buttons::Button`]: the web draws it as a bare button tinted with
    /// the **variant's** hue, and `buttons::Variant` has no member for "the
    /// accent of an arbitrary severity" — `Text` is always the primary accent,
    /// which would put a blue action on an amber callout. Adding one would mean
    /// editing another family's file.
    pub action: Option<CalloutAction>,
    /// Adds the close button (`dismissible`).
    pub dismissible: bool,
    /// Overrides the variant's glyph. `None` keeps it; see
    /// [`Callout::without_icon`] to drop it entirely (the web's `icon={null}`).
    pub icon: Option<&'static str>,
    /// Whether a glyph is drawn at all.
    pub show_icon: bool,
    /// The inline action's OWN state (hover, focus-visible ring, disabled).
    ///
    /// [`Widget::paint`] receives one [`WidgetState`] for the whole banner, so
    /// on its own it cannot say « the pointer is on the close button, not on
    /// the action ». A host that tracks the two parts (see
    /// [`Callout::part_at`]) sets this and [`Callout::dismiss_state`]; when
    /// both are `None` the banner-wide state lights both, which is the
    /// behaviour this type always had.
    pub action_state: Option<WidgetState>,
    /// The close button's own state — see [`Callout::action_state`].
    pub dismiss_state: Option<WidgetState>,
}

/// The two interactive parts of a [`Callout`] — the web's two `<button>`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutPart {
    /// The inline action.
    Action,
    /// The close button (`dismissible`).
    Dismiss,
}

/// Where everything in a [`Callout`] lands for one set of bounds — computed
/// once, read by the paint pass, the height and the hit test alike, so the
/// three can never disagree.
#[derive(Clone)]
pub struct CalloutLayout {
    /// The title, wrapped into the text column.
    pub title_lines: Vec<String>,
    /// The body, wrapped into the text column.
    pub body_lines: Vec<String>,
    /// The inline action's box, if any.
    pub action: Option<Rect>,
    /// The close button's box, if any.
    pub dismiss: Option<Rect>,
    /// The height the content needs (padding included).
    pub height: f32,
}

/// A callout's inline action: a label and an optional leading glyph.
#[derive(Debug, Clone)]
pub struct CalloutAction {
    pub label: String,
    pub icon: Option<&'static str>,
}

impl CalloutAction {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), icon: None }
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }
}

impl Deref for Callout {
    type Target = kc::Label;
    fn deref(&self) -> &kc::Label {
        &self.body
    }
}
impl DerefMut for Callout {
    fn deref_mut(&mut self) -> &mut kc::Label {
        &mut self.body
    }
}

impl Callout {
    /// `rounded-lg` — and note that this design system's `--radius-lg` is 6,
    /// whose own token doc names « dialogs, callouts » as what it is for.
    pub const RADIUS: f32 = radius::LG;
    /// `px-3`.
    pub const PAD_X: f32 = space::MD;
    /// `py-2.5`.
    pub const PAD_Y: f32 = 10.0;
    /// `gap-2.5` between the glyph column and the text column.
    pub const GAP: f32 = 10.0;
    /// `<Glyph size={16} />`.
    pub const ICON: f32 = 16.0;
    /// `mt-px` on the glyph — one DIP, so a 16 px mark sits on the text's
    /// x-height rather than on its ascender.
    pub const ICON_TOP: f32 = 1.0;
    /// `mt-0.5` between the title and the body.
    pub const TITLE_GAP: f32 = space::XXS;
    /// `mt-1.5` above the action.
    pub const ACTION_GAP: f32 = 6.0;
    /// The action's own box: `px-2 py-1` around a body line, `rounded-md`.
    pub const ACTION_PAD_X: f32 = space::SM;
    pub const ACTION_PAD_Y: f32 = space::XS;
    /// `gap-1.5` between the action's glyph and its label.
    pub const ACTION_GLYPH_GAP: f32 = 6.0;
    /// `-ml-2`: the action's padding hangs outside the text column so its LABEL
    /// still lines up with the prose above it.
    pub const ACTION_OUTDENT: f32 = space::SM;
    /// `<X size={14} />` inside a `p-1` box.
    pub const DISMISS: f32 = 14.0;
    pub const DISMISS_PAD: f32 = space::XS;

    /// A callout with a body and no title.
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            body: Label::new(body).role(Role::Body).align(ContentAlignment::MiddleLeft),
            variant: CalloutVariant::default(),
            title: None,
            action: None,
            dismissible: false,
            icon: None,
            show_icon: true,
            action_state: None,
            dismiss_state: None,
        }
    }

    pub fn with_variant(mut self, v: CalloutVariant) -> Self {
        self.variant = v;
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_action(mut self, action: CalloutAction) -> Self {
        self.action = Some(action);
        self
    }

    pub fn with_dismiss(mut self, on: bool) -> Self {
        self.dismissible = on;
        self
    }

    /// The web's `icon={null}`.
    pub fn without_icon(mut self) -> Self {
        self.show_icon = false;
        self
    }

    /// The glyph actually drawn, if any.
    pub fn glyph(&self) -> Option<&'static str> {
        self.show_icon.then(|| self.icon.unwrap_or(self.variant.icon()))
    }

    /// The action's own height — a body line inside `py-1`.
    pub const fn action_height() -> f32 {
        Role::Body.line_height() + Self::ACTION_PAD_Y * 2.0
    }

    /// The dismiss button's box — the glyph inside `p-1`.
    pub const fn dismiss_box() -> f32 {
        Self::DISMISS + Self::DISMISS_PAD * 2.0
    }

    /// The text column inside `bounds`: what is left after the padding, the
    /// glyph column and the dismiss column.
    pub fn text_column(&self, bounds: Rect) -> Rect {
        let lead = if self.glyph().is_some() { Self::ICON + Self::GAP } else { 0.0 };
        let trail = if self.dismissible { Self::dismiss_box() + Self::GAP } else { 0.0 };
        let left = bounds.left + Self::PAD_X + lead;
        Rect::new(
            left,
            bounds.top + Self::PAD_Y,
            (bounds.right - Self::PAD_X - trail).max(left),
            bounds.bottom - Self::PAD_Y,
        )
    }

    /// The dismiss button's rectangle, or `None` when there is none.
    ///
    /// `-mr-1 -mt-0.5` in the web: the padded hit box hangs outside the content
    /// box so the GLYPH lands on the padding's corner.
    pub fn dismiss_rect(&self, bounds: Rect) -> Option<Rect> {
        if !self.dismissible {
            return None;
        }
        let box_ = Self::dismiss_box();
        let right = bounds.right - Self::PAD_X + Self::DISMISS_PAD;
        let top = bounds.top + Self::PAD_Y - space::XXS;
        Some(Rect::new(right - box_, top, right, top + box_))
    }

    /// How tall a callout is for a body wrapped into `body_lines` lines.
    ///
    /// Pure, so every combination of title / body / action is a unit test rather
    /// than a screenshot. The floor is the glyph column: a one-word callout with
    /// no title is still as tall as the mark beside it.
    pub fn height_for(&self, body_lines: usize) -> f32 {
        self.height_for_lines(usize::from(self.title.is_some()), body_lines)
    }

    /// [`Callout::height_for`] with the title's own line count — a long title
    /// wraps like the web's `<p>` does rather than being cut.
    pub fn height_for_lines(&self, title_lines: usize, body_lines: usize) -> f32 {
        let mut content = 0.0_f32;
        let titled = self.title.is_some() && title_lines > 0;
        if titled {
            content += title_lines as f32 * Role::Body.line_height();
        }
        if body_lines > 0 {
            if titled {
                content += Self::TITLE_GAP;
            }
            content += body_lines as f32 * RELAXED_LINE;
        }
        if self.action.is_some() {
            content += Self::ACTION_GAP + Self::action_height();
        }
        let floor = if self.glyph().is_some() { Self::ICON_TOP + Self::ICON } else { 0.0 };
        Self::PAD_Y * 2.0 + content.max(floor)
    }

    /// The inline action's rectangle: outdented by `-ml-2`, sized to its
    /// content, and never wider than the column it hangs off.
    pub fn action_rect(&self, canvas: &dyn Canvas, column: Rect, top: f32) -> Rect {
        let label = self.action.as_ref().map(|a| a.label.as_str()).unwrap_or_default();
        let icon = self.action.as_ref().and_then(|a| a.icon);
        let text_w = canvas.measure(label, Role::Body.format(canvas.formats())).ceil();
        let lead = if icon.is_some() { Self::ICON + Self::ACTION_GLYPH_GAP } else { 0.0 };
        let left = column.left - Self::ACTION_OUTDENT;
        let width = (Self::ACTION_PAD_X * 2.0 + lead + text_w).min(column.right - left);
        Rect::new(left, top, left + width.max(0.0), top + Self::action_height())
    }

    /// The height this callout needs when laid out `width` DIP wide, its body
    /// wrapped into that width.
    ///
    /// [`Widget::measure`] answers the same question from the replica's own
    /// designer width; this one is for the caller who already knows the column
    /// it is about to put the callout in — which is most of them, a callout
    /// being `w-full`.
    pub fn height_at(&self, canvas: &dyn Canvas, width: f32) -> f32 {
        let probe = Rect::new(0.0, 0.0, width.max(1.0), 0.0);
        self.layout(canvas, probe).height
    }

    /// Everything's position inside `bounds`: the wrapped title and body, the
    /// action's and the close button's boxes, and the height it all needs.
    pub fn layout(&self, canvas: &dyn Canvas, bounds: Rect) -> CalloutLayout {
        let column = self.text_column(bounds);
        let width = (column.right - column.left).max(1.0);
        let title_lines = match &self.title {
            Some(title) => {
                let fmt = &canvas.formats().body_strong;
                wrap_lines(title, width, |s| canvas.measure(s, fmt))
            }
            None => Vec::new(),
        };
        let fmt = Role::Body.format(canvas.formats());
        let body_lines = wrap_lines(&self.body.shown_text(), width, |s| canvas.measure(s, fmt));

        let height = self.height_for_lines(title_lines.len(), body_lines.len());
        let action = self.action.as_ref().map(|_| {
            let top = bounds.top + height - Self::PAD_Y - Self::action_height();
            self.action_rect(canvas, column, top)
        });
        CalloutLayout { title_lines, body_lines, action, dismiss: self.dismiss_rect(bounds), height }
    }

    /// The interactive part under `(x, y)`, if any — what a host hit-tests
    /// before routing a click, and feeds back through
    /// [`Callout::action_state`] / [`Callout::dismiss_state`] for the hover.
    pub fn part_at(&self, canvas: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<CalloutPart> {
        let layout = self.layout(canvas, bounds);
        if layout.dismiss.is_some_and(|r| r.contains(x, y)) {
            return Some(CalloutPart::Dismiss);
        }
        if layout.action.is_some_and(|r| r.contains(x, y)) {
            return Some(CalloutPart::Action);
        }
        None
    }

    /// The state one part paints in: its own when the host set it, otherwise
    /// the banner-wide one.
    fn part_state(&self, part: CalloutPart, banner: WidgetState) -> WidgetState {
        let own = match part {
            CalloutPart::Action => self.action_state,
            CalloutPart::Dismiss => self.dismiss_state,
        };
        let tracked = self.action_state.is_some() || self.dismiss_state.is_some();
        match own {
            Some(s) => WidgetState { disabled: s.disabled || banner.disabled, ..s },
            // The host tracks the parts, and this one is not the hot one.
            None if tracked => WidgetState::REST.disabled(banner.disabled),
            None => banner,
        }
    }
}

impl Widget for Callout {
    fn model(&self) -> &dyn Control {
        self.body.model()
    }

    /// A callout is `w-full` — it has no intrinsic width, so the replica's
    /// designer width is kept and only the HEIGHT is computed, from the body
    /// wrapped into that width.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let width = self.body.width();
        self.body.clamp(Size::new(width, self.height_at(canvas, width)))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let inert = is_inert(&self.body.control, state);

        // The tinted ground, and a NEUTRAL hairline: the web keeps
        // `border-border` because an accent border would need an opacity
        // modifier, and every Tailwind opacity modifier bakes a static
        // light-theme hex as its fallback.
        canvas.fill_rounded(&bounds, Self::RADIUS, &self.variant.ground(t));
        canvas.stroke_rounded(&bounds, Self::RADIUS, &t.card_stroke);

        let hue = if inert { t.text_tertiary } else { self.variant.hue(t) };
        let ink = if inert { t.text_tertiary } else { t.text_primary };

        if let Some(name) = self.glyph() {
            glyph(
                canvas,
                name,
                bounds.left + Self::PAD_X,
                bounds.top + Self::PAD_Y + Self::ICON_TOP,
                Self::ICON,
                hue,
            );
        }

        let column = self.text_column(bounds);
        let layout = self.layout(canvas, bounds);
        let mut y = column.top;

        // `overflow-hidden` is not on the web's box, but nothing of a callout
        // may leave the rectangle it was given either: a caller that reserved
        // less than `height_at` gets the text clipped to the rounded ground
        // rather than printed over the next block.
        canvas.push_clip_rounded(&bounds, Self::RADIUS);

        if !layout.title_lines.is_empty() {
            // `font-medium text-text-primary` — the shared `body_strong` face,
            // wrapped like the web's `<p>` rather than cut.
            let fmt = &canvas.formats().body_strong;
            for text in &layout.title_lines {
                let band = Rect::new(column.left, y, column.right, y + Role::Body.line_height());
                strong_line(canvas, band, text, fmt, false, ink);
                y = band.bottom;
            }
            y += Self::TITLE_GAP;
        }

        for text in &layout.body_lines {
            let band = Rect::new(column.left, y, column.right, y + RELAXED_LINE);
            line(canvas, band, text, Role::Body, ContentAlignment::MiddleLeft, ink, true);
            y = band.bottom;
        }

        if let (Some(action), Some(rect)) = (&self.action, layout.action) {
            let st = self.part_state(CalloutPart::Action, state);
            // `hover:bg-[var(--kb-black-08)]` — an 8 % black overlay, which this
            // palette has no token for. `control_fill_hover` is its answer to
            // the same question ("a control under the pointer") and, unlike a
            // literal, it has a dark-palette value.
            if (st.hot || st.pressed) && !inert && !st.disabled {
                canvas.fill_rounded(&rect, radius::SM, &t.control_fill_hover);
            }
            let mut x = rect.left + Self::ACTION_PAD_X;
            let mid = (rect.top + rect.bottom) / 2.0;
            if let Some(name) = action.icon {
                glyph(canvas, name, x, mid - Self::ICON / 2.0, Self::ICON, hue);
                x += Self::ICON + Self::ACTION_GLYPH_GAP;
            }
            let band = Rect::new(x, rect.top, rect.right - Self::ACTION_PAD_X, rect.bottom);
            line(canvas, band, &action.label, Role::Body, ContentAlignment::MiddleLeft, hue, true);
            // `focus-visible:ring-2 ring-primary` on `rounded-md`.
            if st.show_focus_ring() && !inert {
                focus_ring(canvas, rect, radius::SM);
            }
        }

        if let Some(rect) = layout.dismiss {
            let st = self.part_state(CalloutPart::Dismiss, state);
            let lit = (st.hot || st.pressed) && !inert && !st.disabled;
            if lit {
                canvas.fill_rounded(&rect, radius::SM, &t.control_fill_hover);
            }
            // `text-text-secondary hover:text-text-primary`.
            let ink = if inert || st.disabled {
                t.text_tertiary
            } else if lit {
                t.text_primary
            } else {
                t.text_secondary
            };
            glyph(
                canvas,
                "X",
                rect.left + Self::DISMISS_PAD,
                rect.top + Self::DISMISS_PAD,
                Self::DISMISS,
                ink,
            );
            if st.show_focus_ring() && !inert {
                focus_ring(canvas, rect, radius::SM);
            }
        }

        canvas.pop_clip_rounded();
    }

    fn type_name(&self) -> &'static str {
        "Callout"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// EmptyState
// ═════════════════════════════════════════════════════════════════════════════

/// The four situations an empty area can be in — `@ui/EmptyState`'s
/// `EmptyStateVariant`, names and meanings included.
///
/// The web component's own doc is the specification, and it is not decoration:
/// the four look similar and are routinely conflated, yet they call for
/// different words and different actions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EmptyStateVariant {
    /// The collection is genuinely empty and nothing is filtered — the ONLY
    /// variant that invites creation. `bg-primary-light text-primary`.
    #[default]
    FirstUse,
    /// Rows exist but the filters match none. The way out is to WIDEN the
    /// query, so its action is secondary and never a creation.
    /// `bg-surface-2 text-text-secondary`.
    NoResults,
    /// The data could not be loaded — nothing is known about the collection, so
    /// "no user" would be a lie. `bg-danger-light text-danger`.
    Error,
    /// The feature exists but is out of reach: module absent, rights missing.
    /// Usually no action at all. `bg-surface-2 text-text-tertiary`.
    Unavailable,
}

/// Every variant, in the order `@ui/EmptyState` declares them.
pub const EMPTY_STATE_VARIANTS: [EmptyStateVariant; 4] = [
    EmptyStateVariant::FirstUse,
    EmptyStateVariant::NoResults,
    EmptyStateVariant::Error,
    EmptyStateVariant::Unavailable,
];

impl EmptyStateVariant {
    /// `(medallion ground, glyph)` — the `MEDALLION` map, as tokens.
    pub fn medallion(self, t: &Theme) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
        match self {
            Self::FirstUse => (t.accent_light, t.accent),
            Self::NoResults => (t.surface_2, t.text_secondary),
            Self::Error => (t.danger_light, t.danger),
            Self::Unavailable => (t.surface_2, t.text_tertiary),
        }
    }

    /// The main action's default style — the `ACTION_VARIANT` map. Only
    /// `first-use` gets a primary button, which is the rule the component exists
    /// to enforce.
    pub const fn action_variant(self) -> ButtonVariant {
        match self {
            Self::FirstUse => ButtonVariant::Primary,
            _ => ButtonVariant::Secondary,
        }
    }
}

/// The centred « there is nothing here » block — `@ui/EmptyState`.
///
/// Its model is the title's [`crate::display::Label`]; its buttons are
/// [`crate::buttons::Button`]s (the web renders literal `<Button size="sm">`s,
/// so this family must not draw its own).
pub struct EmptyState {
    title: Label,
    pub icon: &'static str,
    pub variant: EmptyStateVariant,
    /// `description` — wrapped into [`EmptyState::MAX_TEXT`].
    pub description: Option<String>,
    /// `compact`: half the vertical breathing room, for a small card.
    pub compact: bool,
    /// The main way out. Its default style is the variant's; overriding it is
    /// the caller's business, exactly as on the web.
    pub action: Option<Button>,
    pub secondary_action: Option<Button>,
    /// The « learn more » link — documentation, not an action.
    pub doc_label: Option<String>,
    /// Each action button's OWN state — `[main, secondary]`.
    ///
    /// [`Widget::paint`] receives one [`WidgetState`] for the whole block; a
    /// host that tracks the buttons (see [`EmptyState::action_rects`]) sets
    /// these so only the button under the pointer lights and only the focused
    /// one wears its ring. `None` in both slots keeps the block-wide state for
    /// both, the behaviour this type always had.
    pub action_state: [Option<WidgetState>; 2],
    /// The documentation link's own state (`hover:underline`, focus ring).
    pub doc_state: Option<WidgetState>,
}

/// Where everything in an [`EmptyState`] lands for one set of bounds —
/// computed once and shared by the paint pass, the height and the hit tests.
#[derive(Clone)]
pub struct EmptyStateLayout {
    /// The icon medallion.
    pub medallion: Rect,
    /// The `max-w-sm` text column (left and right edges; the lines are centred
    /// in it).
    pub column: Rect,
    /// The title, wrapped into the column.
    pub title_lines: Vec<String>,
    pub title_top: f32,
    /// The description, wrapped into the column.
    pub description_lines: Vec<String>,
    pub description_top: f32,
    /// Each button's box, `0` = main action, `1` = secondary — on one row, or
    /// several when `flex-wrap` has to break it.
    pub actions: Vec<(usize, Rect)>,
    /// The documentation link's box — its measured label, centred.
    pub doc: Option<Rect>,
    /// The height the block needs (padding included).
    pub height: f32,
}

impl Deref for EmptyState {
    type Target = kc::Label;
    fn deref(&self) -> &kc::Label {
        &self.title
    }
}
impl DerefMut for EmptyState {
    fn deref_mut(&mut self) -> &mut kc::Label {
        &mut self.title
    }
}

impl EmptyState {
    /// The icon medallion: `h-14 w-14`, or `h-11 w-11` when compact.
    pub const MEDALLION: f32 = 56.0;
    pub const MEDALLION_COMPACT: f32 = 44.0;
    /// The glyph inside it. The web passes an « already-sized lucide element,
    /// e.g. `<Users size={26} />` » and 26 is the example it gives; the compact
    /// medallion keeps the same ratio, `round(26 × 44 / 56) = 20`, which is also
    /// [`crate::display::Icon::DEFAULT`].
    pub const GLYPH: f32 = 26.0;
    pub const GLYPH_COMPACT: f32 = 20.0;
    /// `px-6 py-12`, or `px-4 py-6` when compact.
    pub const PAD_X: f32 = space::XL;
    pub const PAD_X_COMPACT: f32 = space::LG;
    pub const PAD_Y: f32 = 48.0;
    pub const PAD_Y_COMPACT: f32 = space::XL;
    /// `gap-3`, or `gap-2` when compact — between the medallion, the text block
    /// and the actions.
    pub const GAP: f32 = space::MD;
    pub const GAP_COMPACT: f32 = space::SM;
    /// `mt-1` between the title and the description, and again above the
    /// actions.
    pub const TEXT_GAP: f32 = space::XS;
    /// `gap-2` between the two action buttons.
    pub const ACTION_GAP: f32 = space::SM;
    /// `max-w-sm` on the text block — Tailwind's `24rem`.
    pub const MAX_TEXT: f32 = 384.0;
    /// `underline-offset-2` under a meta line: how far above the line box's
    /// bottom the hover underline sits.
    pub const DOC_UNDERLINE_DROP: f32 = 2.0;

    pub fn new(icon: &'static str, title: impl Into<String>) -> Self {
        Self {
            title: Label::new(title).role(Role::Heading).align(ContentAlignment::MiddleCenter),
            icon,
            variant: EmptyStateVariant::default(),
            description: None,
            compact: false,
            action: None,
            secondary_action: None,
            doc_label: None,
            action_state: [None, None],
            doc_state: None,
        }
    }

    pub fn with_variant(mut self, v: EmptyStateVariant) -> Self {
        self.variant = v;
        self
    }

    pub fn with_description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// The main action. Its variant is left to the caller; [`EmptyState::action_or_default`]
    /// is what applies the variant's rule when the caller did not.
    pub fn with_action(mut self, action: Button) -> Self {
        self.action = Some(action);
        self
    }

    pub fn with_secondary_action(mut self, action: Button) -> Self {
        self.secondary_action = Some(action);
        self
    }

    pub fn with_doc(mut self, label: impl Into<String>) -> Self {
        self.doc_label = Some(label.into());
        self
    }

    /// Halves the vertical breathing room and drops the title one type step —
    /// the web's `compact`, which also swaps `--kb-text-heading` for
    /// `--kb-text-body`.
    pub fn with_compact(mut self, on: bool) -> Self {
        self.compact = on;
        self.title.role = if on { Role::Body } else { Role::Heading };
        self
    }

    pub fn medallion_size(&self) -> f32 {
        if self.compact {
            Self::MEDALLION_COMPACT
        } else {
            Self::MEDALLION
        }
    }

    pub fn glyph_size(&self) -> f32 {
        if self.compact {
            Self::GLYPH_COMPACT
        } else {
            Self::GLYPH
        }
    }

    pub fn pad_x(&self) -> f32 {
        if self.compact {
            Self::PAD_X_COMPACT
        } else {
            Self::PAD_X
        }
    }

    pub fn pad_y(&self) -> f32 {
        if self.compact {
            Self::PAD_Y_COMPACT
        } else {
            Self::PAD_Y
        }
    }

    pub fn gap(&self) -> f32 {
        if self.compact {
            Self::GAP_COMPACT
        } else {
            Self::GAP
        }
    }

    /// The style the main action paints in: the caller's if it set one away from
    /// the button default, otherwise the variant's rule.
    pub fn action_or_default(&self) -> ButtonVariant {
        match &self.action {
            Some(b) if b.variant != ButtonVariant::default() => b.variant,
            _ => self.variant.action_variant(),
        }
    }

    /// Whether an action row is drawn at all.
    pub fn has_actions(&self) -> bool {
        self.action.is_some() || self.secondary_action.is_some()
    }

    /// How tall the block is for a description wrapped into `description_lines`
    /// lines, with a one-line title and — when there are actions — one action
    /// row. Pure — this is the arithmetic the measurement test drives.
    pub fn height_for(&self, description_lines: usize) -> f32 {
        self.height_for_lines(1, description_lines, usize::from(self.has_actions()))
    }

    /// The full height arithmetic: a title wrapped into `title_lines`, a
    /// description into `description_lines`, and the buttons into
    /// `action_rows` rows (`flex-wrap gap-2`).
    pub fn height_for_lines(&self, title_lines: usize, description_lines: usize, action_rows: usize) -> f32 {
        let gap = self.gap();
        let mut h = self.pad_y() * 2.0
            + self.medallion_size()
            + gap
            + title_lines.max(1) as f32 * self.title.role.line_height();
        if description_lines > 0 {
            h += Self::TEXT_GAP + description_lines as f32 * RELAXED_LINE;
        }
        if action_rows > 0 {
            // `gap-3` from the text block, then the row's own `mt-1`, then the
            // wrapped rows `gap-2` apart.
            h += gap
                + Self::TEXT_GAP
                + action_rows as f32 * ButtonSize::Sm.height()
                + (action_rows - 1) as f32 * Self::ACTION_GAP;
        }
        if self.doc_label.is_some() {
            h += gap + Role::Meta.line_height();
        }
        h
    }

    /// The text column's width inside a block `width` DIP wide: the padding
    /// comes off first, then `max-w-sm` caps it.
    pub fn text_width(&self, width: f32) -> f32 {
        (width - self.pad_x() * 2.0).clamp(1.0, Self::MAX_TEXT)
    }

    /// The title's own face: `font-medium` at `--kb-text-heading`, or at
    /// `--kb-text-body` when compact.
    fn title_format<'a>(&self, canvas: &'a dyn Canvas) -> &'a windows::Win32::Graphics::DirectWrite::IDWriteTextFormat {
        if self.compact {
            &canvas.formats().body_strong
        } else {
            &canvas.formats().heading
        }
    }

    /// The buttons that exist, as `(slot, natural width)`.
    fn action_widths(&self, canvas: &dyn Canvas) -> Vec<(usize, f32)> {
        let mut out = Vec::new();
        if let Some(b) = &self.action {
            out.push((0, b.width(canvas)));
        }
        if let Some(b) = &self.secondary_action {
            out.push((1, b.width(canvas)));
        }
        out
    }

    /// The height this block needs laid out `width` DIP wide — title and
    /// description wrapped into the real column, buttons wrapped into rows.
    /// What a caller that knows its column (a card body) should reserve.
    pub fn height_at(&self, canvas: &dyn Canvas, width: f32) -> f32 {
        self.layout(canvas, Rect::new(0.0, 0.0, width.max(1.0), 0.0)).height
    }

    /// Everything's position inside `bounds`.
    ///
    /// `flex-col items-center justify-center`: the content is centred
    /// horizontally AND vertically in `bounds`; when `bounds` is shorter than
    /// the content it is pinned to the top rather than pushed above it.
    pub fn layout(&self, canvas: &dyn Canvas, bounds: Rect) -> EmptyStateLayout {
        let width = (bounds.right - bounds.left).max(1.0);
        let cx = (bounds.left + bounds.right) / 2.0;
        let text_w = self.text_width(width);

        let title_fmt = self.title_format(canvas);
        let title_lines = wrap_lines(&self.title.shown_text(), text_w, |s| canvas.measure(s, title_fmt));
        let body_fmt = Role::Body.format(canvas.formats());
        let description_lines = match &self.description {
            Some(text) => wrap_lines(text, text_w, |s| canvas.measure(s, body_fmt)),
            None => Vec::new(),
        };
        let row_w = (width - self.pad_x() * 2.0).max(1.0);
        let rows = pack_rows(&self.action_widths(canvas), row_w, Self::ACTION_GAP);

        let height = self.height_for_lines(title_lines.len(), description_lines.len(), rows.len());
        let free = (bounds.bottom - bounds.top) - height;
        let top = bounds.top + if free > 0.0 { free / 2.0 } else { 0.0 };

        let gap = self.gap();
        let mut y = top + self.pad_y();
        let m = self.medallion_size();
        let medallion = Rect::new(cx - m / 2.0, y, cx + m / 2.0, y + m);
        y += m + gap;
        let column = Rect::new(cx - text_w / 2.0, bounds.top, cx + text_w / 2.0, bounds.bottom);
        let title_top = y;
        y += title_lines.len().max(1) as f32 * self.title.role.line_height();
        if !description_lines.is_empty() {
            y += Self::TEXT_GAP;
        }
        let description_top = y;
        y += description_lines.len() as f32 * RELAXED_LINE;

        let mut actions = Vec::new();
        if !rows.is_empty() {
            y += gap + Self::TEXT_GAP;
            let h = ButtonSize::Sm.height();
            for row in &rows {
                let total: f32 = row.iter().map(|(_, w)| w.min(row_w)).sum::<f32>()
                    + Self::ACTION_GAP * row.len().saturating_sub(1) as f32;
                let mut x = cx - total / 2.0;
                for &(slot, w) in row {
                    let w = w.min(row_w);
                    actions.push((slot, Rect::new(x, y, x + w, y + h)));
                    x += w + Self::ACTION_GAP;
                }
                y += h + Self::ACTION_GAP;
            }
            y -= Self::ACTION_GAP;
        }

        let doc = self.doc_label.as_ref().map(|label| {
            let y = y + gap;
            let w = canvas.measure(label, Role::Meta.format(canvas.formats())).ceil().min(row_w);
            Rect::new(cx - w / 2.0, y, cx + w / 2.0, y + Role::Meta.line_height())
        });

        EmptyStateLayout {
            medallion,
            column,
            title_lines,
            title_top,
            description_lines,
            description_top,
            actions,
            doc,
            height,
        }
    }

    /// Each button's box inside `bounds`, `0` = main action, `1` = secondary —
    /// what a host hit-tests and registers with its focus manager, in the
    /// web's Tab order (main first).
    pub fn action_rects(&self, canvas: &dyn Canvas, bounds: Rect) -> Vec<(usize, Rect)> {
        self.layout(canvas, bounds).actions
    }

    /// The documentation link's box inside `bounds`, if there is one.
    pub fn doc_rect(&self, canvas: &dyn Canvas, bounds: Rect) -> Option<Rect> {
        self.layout(canvas, bounds).doc
    }

    /// The state one button paints in: its own when the host set it,
    /// otherwise the block-wide one.
    fn button_state(&self, slot: usize, block: WidgetState) -> WidgetState {
        let tracked = self.action_state.iter().any(Option::is_some);
        match self.action_state.get(slot).copied().flatten() {
            Some(s) => WidgetState { disabled: s.disabled || block.disabled, ..s },
            None if tracked => WidgetState::REST.disabled(block.disabled),
            None => block,
        }
    }
}

/// `flex flex-wrap gap-*`: greedy rows of items `(id, width)` no wider than
/// `max_width`, in order. An item wider than the row gets a row of its own;
/// an empty input gives no row.
pub fn pack_rows(items: &[(usize, f32)], max_width: f32, gap: f32) -> Vec<Vec<(usize, f32)>> {
    let mut rows: Vec<Vec<(usize, f32)>> = Vec::new();
    let mut used = 0.0_f32;
    for &(id, w) in items {
        match rows.last_mut() {
            Some(row) if !row.is_empty() && used + gap + w <= max_width => {
                row.push((id, w));
                used += gap + w;
            }
            _ => {
                rows.push(vec![(id, w)]);
                used = w;
            }
        }
    }
    rows
}

impl Widget for EmptyState {
    fn model(&self) -> &dyn Control {
        self.title.model()
    }

    /// The intrinsic size: the widest of the medallion, the title, the
    /// description (each capped at `max-w-sm`) and the action row, plus the
    /// padding — and the height that exact width produces, so painting into
    /// the measured rectangle wraps into the same lines this counted.
    ///
    /// A caller that paints into a NARROWER column (a card body) must reserve
    /// [`EmptyState::height_at`] of that width instead: the text wraps onto
    /// more lines there.
    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let title_fmt = self.title_format(canvas);
        let fmt_body = Role::Body.format(canvas.formats());
        let title_w = canvas.measure(&self.title.shown_text(), title_fmt).ceil();
        let desc_w = match &self.description {
            Some(text) => wrap_lines(text, Self::MAX_TEXT, |s| canvas.measure(s, fmt_body))
                .iter()
                .map(|l| canvas.measure(l, fmt_body).ceil())
                .fold(0.0_f32, f32::max),
            None => 0.0,
        };
        let text_w = title_w.max(desc_w).min(Self::MAX_TEXT);
        let buttons = self.action_widths(canvas);
        let actions_w = buttons.iter().map(|(_, w)| *w).sum::<f32>()
            + Self::ACTION_GAP * buttons.len().saturating_sub(1) as f32;
        let width = self.pad_x() * 2.0 + text_w.max(self.medallion_size()).max(actions_w);
        self.title.clamp(Size::new(width, self.height_at(canvas, width)))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let inert = is_inert(&self.title.control, state);
        let (ground, mark) = self.variant.medallion(t);
        let layout = self.layout(canvas, bounds);
        canvas.push_clip(&bounds);

        // The medallion — `rounded-full`, so the radius is half the box.
        let medallion = layout.medallion;
        let m = self.medallion_size();
        canvas.fill_rounded(&medallion, pill(m), &if inert { t.surface_2 } else { ground });
        let g = self.glyph_size();
        glyph(
            canvas,
            self.icon,
            medallion.left + (m - g) / 2.0,
            medallion.top + (m - g) / 2.0,
            g,
            if inert { t.text_tertiary } else { mark },
        );

        // The text block, centred and capped at `max-w-sm`; the title wraps
        // (`<p>`) and so does the description, one centred line at a time.
        let column = layout.column;
        let title_line = self.title.role.line_height();
        let ink = if inert { t.text_tertiary } else { t.text_primary };
        let title_fmt = self.title_format(canvas);
        let mut y = layout.title_top;
        for text in &layout.title_lines {
            strong_line(canvas, Rect::new(column.left, y, column.right, y + title_line), text, title_fmt, true, ink);
            y += title_line;
        }

        let secondary = if inert { t.text_tertiary } else { t.text_secondary };
        let mut y = layout.description_top;
        for text in &layout.description_lines {
            line(
                canvas,
                Rect::new(column.left, y, column.right, y + RELAXED_LINE),
                text,
                Role::Body,
                ContentAlignment::MiddleCenter,
                secondary,
                true,
            );
            y += RELAXED_LINE;
        }

        // The action row — real `buttons::Button`s, centred as a group and
        // wrapped onto a second row when the block is too narrow for both.
        for &(slot, rect) in &layout.actions {
            let button = if slot == 0 { self.action.as_ref() } else { self.secondary_action.as_ref() };
            if let Some(b) = button {
                // The variant's rule is applied HERE rather than stored, so a
                // caller may set `variant` and the empty-state's own variant in
                // either order and still get the same button. The secondary
                // keeps whatever it was given — the web defaults it to `ghost`
                // at the call site, not in the component.
                let target = if slot == 0 { self.action_or_default() } else { b.variant };
                restyled(b, target).paint(canvas, rect, self.button_state(slot, state));
            }
        }

        if let (Some(doc), Some(rect)) = (&self.doc_label, layout.doc) {
            let st = match self.doc_state {
                Some(s) => WidgetState { disabled: s.disabled || state.disabled, ..s },
                None => WidgetState::REST.disabled(state.disabled),
            };
            let colour = if inert { t.text_tertiary } else { t.accent };
            line(canvas, rect, doc, Role::Meta, ContentAlignment::MiddleCenter, colour, true);
            // `hover:underline underline-offset-2`: a hairline two DIP under the
            // baseline, in the link's own ink.
            if st.hot && !inert {
                let base = rect.bottom - Self::DOC_UNDERLINE_DROP;
                canvas.fill_rounded(&Rect::new(rect.left, base, rect.right, base + 1.0), 0.0, &colour);
            }
            // `rounded-sm focus-visible:ring-2 ring-primary`.
            if st.show_focus_ring() && !inert {
                focus_ring(canvas, rect.inflate(FOCUS_RING, 0.0), radius::SM);
            }
        }

        canvas.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "EmptyState"
    }
}

/// The same [`crate::buttons::Button`] in another variant.
///
/// Every field that shapes it is carried over — label, size, icon, the
/// overridable `pad_x` and the replica's `enabled` — because a copy that dropped
/// one would paint a different button from the one the caller built.
fn restyled(button: &Button, variant: ButtonVariant) -> Button {
    let mut out = Button::new(&button.text).size(button.size).variant(variant);
    out.icon = button.icon;
    out.pad_x = button.pad_x;
    out.enabled = button.enabled;
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// Accordion
// ═════════════════════════════════════════════════════════════════════════════

/// The two header densities `@ui/Accordion` offers: `px-3 py-2` and `px-4 py-3`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AccordionSize {
    Sm,
    #[default]
    Md,
}

impl AccordionSize {
    /// `px-4` / `px-3` — also the panel's own horizontal padding.
    pub const fn pad_x(self) -> f32 {
        match self {
            Self::Sm => space::MD,
            Self::Md => space::LG,
        }
    }

    /// `py-3` / `py-2`.
    pub const fn pad_y(self) -> f32 {
        match self {
            Self::Sm => space::SM,
            Self::Md => space::MD,
        }
    }

    /// The clickable header's height: its padding around a
    /// [`Role::Meta`] line — the title is `text-xs`, so 16.
    pub const fn header(self) -> f32 {
        Role::Meta.line_height() + self.pad_y() * 2.0
    }

    /// `pb-4` / `pb-3` under the panel's content.
    pub const fn pad_bottom(self) -> f32 {
        match self {
            Self::Sm => space::MD,
            Self::Md => space::LG,
        }
    }
}

/// One collapsible section — `AccordionItemDef`.
///
/// `content` is the panel's height in DIP rather than a child widget: the web
/// hands the component a `ReactNode` and lets the browser measure it, and this
/// layer has no such measurer. The caller knows how tall its own block is; the
/// accordion's job is where to put it.
#[derive(Debug, Clone)]
pub struct AccordionSection {
    pub title: String,
    /// A leading glyph — `icon`, drawn at 16.
    pub icon: Option<&'static str>,
    /// The trailing count pill — `badge`.
    pub badge: Option<String>,
    /// `disabled`: renders muted and stays collapsed.
    pub disabled: bool,
    /// The panel's content height, in DIP.
    pub content: f32,
    /// Whether the panel is showing.
    pub open: bool,
}

impl AccordionSection {
    pub fn new(title: impl Into<String>, content: f32) -> Self {
        Self { title: title.into(), icon: None, badge: None, disabled: false, content, open: false }
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn badge(mut self, text: impl Into<String>) -> Self {
        self.badge = Some(text.into());
        self
    }

    pub fn open(mut self, on: bool) -> Self {
        self.open = on;
        self
    }

    pub fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }

    /// Whether the panel is actually showing — a disabled section « stays
    /// collapsed », which is the web component's own rule and not a rendering
    /// accident.
    pub fn is_open(&self) -> bool {
        self.open && !self.disabled
    }
}

/// A stack of collapsible groups — `@ui/Accordion`.
///
/// Its model is the [`crate::containers::Panel`] that also paints each section's
/// card, so there is exactly ONE replica in the type and `padding`, `dock`,
/// `anchor`, `bounds` and the rest are reached through it.
pub struct Accordion {
    /// Reused for every section: one [`crate::containers::Surface::Card`], the
    /// `rounded-xl border border-border bg-surface-0` the web writes on each
    /// item, painted N times rather than reimplemented once.
    frame: Panel,
    pub sections: Vec<AccordionSection>,
    pub size: AccordionSize,
    /// Which section's header the pointer is on, if any.
    ///
    /// A **Kubuno concept**, and a host-driven one — the same shape
    /// [`crate::range::ScrollBar::expanded`] takes: [`Widget::paint`] receives
    /// ONE [`WidgetState`] for the whole stack, so `state.hot` cannot say which
    /// of five headers is lit. The host answers that with
    /// [`Accordion::header_at`] and parks the answer here.
    pub hovered: Option<usize>,
    /// Which section's header wears the focus ring — the header `<button>`'s
    /// `:focus-visible`. Host-driven like [`Accordion::hovered`]: the host's
    /// focus manager knows whether the focus came from the keyboard, so it
    /// sets this only when the ring must SHOW (a click leaves it `None`).
    pub focused: Option<usize>,
}

/// A key that moves the focus between accordion headers — the WAI-ARIA
/// accordion pattern's optional navigation (the web component relies on Tab
/// alone; the arrows are the desktop's native expectation for a stack of
/// headers, and they only move focus — Enter and Space are what toggle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderKey {
    /// ↓ — the next enabled header, wrapping to the first.
    Next,
    /// ↑ — the previous enabled header, wrapping to the last.
    Previous,
    /// Home.
    First,
    /// End.
    Last,
}

impl Deref for Accordion {
    type Target = PanelModel;
    fn deref(&self) -> &PanelModel {
        &self.frame
    }
}
impl DerefMut for Accordion {
    fn deref_mut(&mut self) -> &mut PanelModel {
        &mut self.frame
    }
}

impl Default for Accordion {
    fn default() -> Self {
        Self::new()
    }
}

impl Accordion {
    /// `flex flex-col gap-2` on the stack.
    pub const GAP: f32 = space::SM;
    /// `gap-3` inside a header, between the glyph, the title, the badge and the
    /// chevron.
    pub const HEADER_GAP: f32 = space::MD;
    /// `<Icon size={16} />` and `<ChevronDown size={16} />`.
    pub const ICON: f32 = 16.0;
    /// `pt-1` above the panel's content, under the rule.
    pub const PANEL_TOP: f32 = space::XS;
    /// `border-t border-border` between the header and the panel.
    pub const RULE: f32 = control::SEPARATOR;

    pub fn new() -> Self {
        Self {
            frame: Panel::new().with_surface(Surface::Card),
            sections: Vec::new(),
            size: AccordionSize::default(),
            hovered: None,
            focused: None,
        }
    }

    /// The header a navigation key moves the focus to from `from`, skipping
    /// disabled sections (a disabled `<button>` is not focusable). `None` when
    /// no section is enabled. `from` outside the list reads as « before the
    /// first » for [`HeaderKey::Next`] and « after the last » otherwise.
    pub fn header_after_key(&self, from: Option<usize>, key: HeaderKey) -> Option<usize> {
        let enabled: Vec<usize> =
            (0..self.sections.len()).filter(|&i| !self.sections[i].disabled).collect();
        let (&first, &last) = (enabled.first()?, enabled.last()?);
        let from = from.filter(|&i| i < self.sections.len());
        Some(match key {
            HeaderKey::First => first,
            HeaderKey::Last => last,
            HeaderKey::Next => match from {
                Some(i) => enabled.iter().copied().find(|&j| j > i).unwrap_or(first),
                None => first,
            },
            HeaderKey::Previous => match from {
                Some(i) => enabled.iter().rev().copied().find(|&j| j < i).unwrap_or(last),
                None => last,
            },
        })
    }

    pub fn with_size(mut self, size: AccordionSize) -> Self {
        self.size = size;
        self
    }

    /// Points [`Accordion::hovered`] at whatever header the pointer is on.
    /// One call per frame is all a host needs.
    pub fn track_pointer(&mut self, bounds: Rect, x: f32, y: f32) {
        self.hovered = self.header_at(bounds, x, y);
    }

    pub fn section(mut self, section: AccordionSection) -> Self {
        self.sections.push(section);
        self
    }

    /// How tall one section is, header and — when open — panel.
    pub fn section_height(&self, section: &AccordionSection) -> f32 {
        let mut h = self.size.header();
        if section.is_open() {
            h += Self::RULE + Self::PANEL_TOP + section.content.max(0.0) + self.size.pad_bottom();
        }
        h
    }

    /// The whole stack's height: every section, plus a [`Accordion::GAP`]
    /// between each pair. Depends on which sections are open, which is the whole
    /// point of the control.
    pub fn total_height(&self) -> f32 {
        let n = self.sections.len();
        if n == 0 {
            return 0.0;
        }
        self.sections.iter().map(|s| self.section_height(s)).sum::<f32>()
            + Self::GAP * (n - 1) as f32
    }

    /// Every section's rectangle inside `bounds`, stacked from its top edge.
    ///
    /// Each is the WHOLE section — header plus panel when open — so a caller can
    /// hit-test, scroll to, or outline one without re-deriving the stack.
    pub fn section_rects(&self, bounds: Rect) -> Vec<Rect> {
        let mut out = Vec::with_capacity(self.sections.len());
        let mut y = bounds.top;
        for s in &self.sections {
            let h = self.section_height(s);
            out.push(Rect::new(bounds.left, y, bounds.right, y + h));
            y += h + Self::GAP;
        }
        out
    }

    /// The clickable header inside a section rectangle.
    pub fn header_rect(&self, section: Rect) -> Rect {
        Rect::new(section.left, section.top, section.right, section.top + self.size.header())
    }

    /// The panel's content rectangle inside a section rectangle — empty (zero
    /// height) when the section is closed.
    pub fn panel_rect(&self, section: Rect, index: usize) -> Rect {
        let top = section.top + self.size.header() + Self::RULE + Self::PANEL_TOP;
        let open = self.sections.get(index).map(|s| s.is_open()).unwrap_or(false);
        let bottom = if open { section.bottom - self.size.pad_bottom() } else { top };
        let pad = self.size.pad_x();
        Rect::new(section.left + pad, top, section.right - pad, bottom.max(top))
    }

    /// Which section `(x, y)` lands on, in the space `bounds` was painted into.
    ///
    /// `None` in the gaps between sections and outside the stack. The test is
    /// half-open — `top` belongs to the section, `bottom` does not — so two
    /// stacked rectangles can never both claim the same DIP.
    pub fn section_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if x < bounds.left || x >= bounds.right {
            return None;
        }
        self.section_rects(bounds)
            .into_iter()
            .position(|r| y >= r.top && y < r.bottom)
    }

    /// Which section's HEADER `(x, y)` lands on — the only part that toggles.
    /// A disabled section is not a target, which is the web's `disabled` button.
    pub fn header_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let index = self.section_at(bounds, x, y)?;
        let section = *self.section_rects(bounds).get(index)?;
        let disabled = self.sections.get(index).map(|s| s.disabled).unwrap_or(false);
        (!disabled && self.header_rect(section).contains(x, y)).then_some(index)
    }

    /// Toggles a section, the web's `single = false` behaviour (several may be
    /// open at once). A disabled one does not move.
    pub fn toggle(&mut self, index: usize) {
        if let Some(s) = self.sections.get_mut(index) {
            if !s.disabled {
                s.open = !s.open;
            }
        }
    }

    /// Toggles a section with `single = true`: opening one closes the others.
    pub fn toggle_single(&mut self, index: usize) {
        let was = self.sections.get(index).map(|s| s.is_open()).unwrap_or(false);
        let disabled = self.sections.get(index).map(|s| s.disabled).unwrap_or(true);
        for s in &mut self.sections {
            s.open = false;
        }
        if !disabled && !was {
            if let Some(s) = self.sections.get_mut(index) {
                s.open = true;
            }
        }
    }
}

impl Widget for Accordion {
    fn model(&self) -> &dyn Control {
        self.frame.model()
    }

    /// `w-full`: no intrinsic width (the stack takes what it is given), and a
    /// height that is the folded state's own arithmetic.
    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        self.frame.clamp(Size::new(0.0, self.total_height()))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let rects = self.section_rects(bounds);

        for (i, section) in self.sections.iter().enumerate() {
            let Some(&rect) = rects.get(i) else { continue };
            let inert = state.disabled || section.disabled;

            // The card — the containers family's own `Surface::Card`, painted
            // through a real `Panel` rather than reimplemented here.
            self.frame.paint(canvas, rect, WidgetState::REST);

            let header = self.header_rect(rect);
            // `hover:bg-surface-2`, clipped by the card's rounded corners —
            // which is what `overflow-hidden` does on the web, and what stops a
            // square fill from spilling over the top two corners.
            if self.hovered == Some(i) && !inert {
                canvas.push_clip_rounded(&rect, Surface::Card.radius());
                canvas.fill_rounded(&header, 0.0, &t.surface_2);
                canvas.pop_clip_rounded();
            }

            let pad = self.size.pad_x();
            let mid = (header.top + header.bottom) / 2.0;
            let mut x = header.left + pad;
            // `opacity-50` on a disabled header: the palest ink step, which is
            // this design system's answer to a faded control.
            let muted = if inert { t.text_tertiary } else { t.text_secondary };

            if let Some(name) = section.icon {
                glyph(canvas, name, x, mid - Self::ICON / 2.0, Self::ICON, muted);
                x += Self::ICON + Self::HEADER_GAP;
            }

            // The chevron is GEOMETRY, and it is a different geometry rather
            // than a rotated one: `Canvas` has no transform, and `ChevronUp` IS
            // lucide's own 180° `ChevronDown`, so the web's
            // `rotate-180`-when-open lands on the same picture.
            let chevron_x = header.right - pad - Self::ICON;
            glyph(
                canvas,
                if section.is_open() { "ChevronUp" } else { "ChevronDown" },
                chevron_x,
                mid - Self::ICON / 2.0,
                Self::ICON,
                // `text-text-tertiary` on the chevron, disabled or not: the web
                // fades the whole header with `opacity-50` instead, and the
                // palest ink step is already where a disabled mark lands.
                t.text_tertiary,
            );
            let mut title_right = chevron_x - Self::HEADER_GAP;

            // The count pill — the display family's `Badge`, in its `Neutral`
            // tone. The web writes this one inline (`bg-surface-3` with
            // `text-text-secondary`) rather than reusing its own `<Badge>`;
            // collapsing it onto the shared primitive is the deliberate
            // difference, and it costs one ink step on the label.
            if let Some(text) = &section.badge {
                let badge = Badge::new(text.clone())
                    .variant(BadgeVariant::Neutral)
                    .size(BadgeSize::Sm);
                let size = badge.measure(canvas);
                let pill_rect = Rect::new(
                    title_right - size.width,
                    mid - size.height / 2.0,
                    title_right,
                    mid + size.height / 2.0,
                );
                badge.paint(canvas, pill_rect, WidgetState::REST.disabled(inert));
                title_right -= size.width + Self::HEADER_GAP;
            }

            // `text-xs font-semibold uppercase tracking-wide truncate`: the meta
            // step in the shared semibold `caption_strong` face, ellipsized
            // (`flex-1 min-w-0 truncate`). Uppercasing is the caller's — a
            // locale-aware transform is not this layer's decision.
            strong_line(
                canvas,
                Rect::new(x, header.top, title_right.max(x), header.bottom),
                &section.title,
                &canvas.formats().caption_strong,
                false,
                muted,
            );

            // The header `<button>`'s focus ring, clipped by the card like the
            // web's `overflow-hidden` clips it.
            if self.focused == Some(i) && !inert {
                canvas.push_clip_rounded(&rect, Surface::Card.radius());
                focus_ring(canvas, header, Surface::Card.radius());
                canvas.pop_clip_rounded();
            }

            if section.is_open() {
                // `border-t border-border` — the display family's rule.
                let rule_y = header.bottom;
                Separator::horizontal().paint(
                    canvas,
                    Rect::new(rect.left, rule_y, rect.right, rule_y + Self::RULE),
                    WidgetState::REST,
                );
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "Accordion"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Stepper
// ═════════════════════════════════════════════════════════════════════════════

/// The validation state of one step — `@ui/Stepper`'s `StepStatus`, all five.
///
/// The web component's own comment says what [`StepStatus::Error`] is for, and
/// it is the reason a stepper is more than a decoration: « a wizard whose third
/// step failed validation must SAY so on the indicator, otherwise the user walks
/// forward and discovers it at submit time ». It is therefore carried here in
/// full — bullet, glyph, label colour and connector — and not folded into
/// `pending`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepStatus {
    /// Not reached yet.
    #[default]
    Pending,
    /// Where the user is.
    Current,
    /// Done.
    Complete,
    /// Failed validation.
    Error,
    /// Out of reach entirely.
    Disabled,
}

/// Every status, in the order the web's `BULLET` map declares them.
pub const STEP_STATUSES: [StepStatus; 5] = [
    StepStatus::Complete,
    StepStatus::Current,
    StepStatus::Error,
    StepStatus::Pending,
    StepStatus::Disabled,
];

impl StepStatus {
    /// `(fill, ink, border)` — the `BULLET` map, as tokens.
    ///
    /// `text-white` on the two filled bullets is `accent_foreground`, which is
    /// the token « what sits ON the accent » — the same choice
    /// [`crate::buttons::Variant::Danger`] already makes for a filled danger
    /// button, so a red bullet and a red button carry the same ink.
    pub fn bullet(self, t: &Theme) -> (D2D1_COLOR_F, D2D1_COLOR_F, D2D1_COLOR_F) {
        match self {
            Self::Complete => (t.accent, t.accent_foreground, t.accent),
            Self::Current => (t.layer_background, t.accent, t.accent),
            Self::Error => (t.danger, t.accent_foreground, t.danger),
            Self::Pending => (t.layer_background, t.text_tertiary, t.card_stroke),
            Self::Disabled => (t.surface_2, t.text_tertiary, t.card_stroke),
        }
    }

    /// The label's colour — the `LABEL` map.
    pub fn label(self, t: &Theme) -> D2D1_COLOR_F {
        match self {
            Self::Complete => t.text_primary,
            Self::Current => t.accent,
            Self::Error => t.danger,
            Self::Pending => t.text_secondary,
            Self::Disabled => t.text_tertiary,
        }
    }

    /// The glyph inside the bullet, with its size — `Check` at 14 when complete,
    /// `AlertTriangle` at 13 when failed, and the step's NUMBER otherwise.
    pub const fn mark(self) -> Option<(&'static str, f32)> {
        match self {
            Self::Complete => Some(("Check", Stepper::BULLET_TICK)),
            Self::Error => Some(("AlertTriangle", Stepper::BULLET_ALERT)),
            _ => None,
        }
    }
}

/// One step — `StepDef`.
#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub description: Option<String>,
    /// An explicit status. `None` derives it from the position relative to the
    /// current step, which covers the linear happy path with no bookkeeping at
    /// the call site — the web's own rule.
    pub status: Option<StepStatus>,
    /// `optional`: appends the caller's own « (optional) » marker.
    pub optional: bool,
}

impl Step {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), description: None, status: None, optional: false }
    }

    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn status(mut self, status: StepStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn optional(mut self, on: bool) -> Self {
        self.optional = on;
        self
    }
}

/// Which way the trail runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepperOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// The progress spine of a multi-step assistant — `@ui/Stepper`.
///
/// Its model is a [`crate::containers::Panel`], because that is what a stepper
/// is to a layout: a band that holds an ordered list.
pub struct Stepper {
    frame: Panel,
    pub steps: Vec<Step>,
    /// The step the user is on, by index. Clamped into the list on every read,
    /// like the web's `currentIndex`.
    pub current: usize,
    pub orientation: StepperOrientation,
    /// `allowForward`: whether a step past the current one may be clicked. Off
    /// by default — a wizard's later steps usually depend on data that does not
    /// exist yet.
    pub allow_forward: bool,
    /// Which step's row the pointer is on — lit `hover:bg-surface-2` when
    /// that step is reachable (a `<button>` on the web). Host-driven, like
    /// [`Accordion::hovered`]; see [`Stepper::step_at`].
    pub hovered: Option<usize>,
    /// Which step's row wears the focus ring (`focus-visible:ring-2`). Set by
    /// the host only when the ring must show.
    pub focused: Option<usize>,
    /// The two words of the compact summary's counter, `(« Step », « of »)`,
    /// which paints « Step 2 of 5 » — the web's `ui.st_step` / `ui.st_of`.
    /// A primitive invents no sentence, so without them the counter is the
    /// language-neutral « 2 / 5 ».
    pub counter_words: Option<(String, String)>,
    /// The word an optional step's marker carries — the web's
    /// `ui.st_optional`, painted « (word) » after the label. `None` paints no
    /// marker ([`Step::optional`] is still carried for the host).
    pub optional_marker: Option<String>,
}

/// What a [`Stepper`] actually paints in a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepperView {
    /// The full trail of bullets.
    Trail,
    /// « Step 2 of 5 », the current label and a rail — the web's replacement
    /// for a trail that would not fit.
    Compact,
}

impl Deref for Stepper {
    type Target = PanelModel;
    fn deref(&self) -> &PanelModel {
        &self.frame
    }
}
impl DerefMut for Stepper {
    fn deref_mut(&mut self) -> &mut PanelModel {
        &mut self.frame
    }
}

impl Default for Stepper {
    fn default() -> Self {
        Self::new()
    }
}

impl Stepper {
    /// `h-7 w-7 rounded-full border` — the bullet and its hairline.
    pub const BULLET: f32 = 28.0;
    pub const BULLET_BORDER: f32 = 1.0;
    /// `<Check size={14} />` and `<AlertTriangle size={13} />`. The two differ
    /// on the web, and they differ here: a triangle's ink sits higher in its box
    /// than a tick's, so matching them would make the warning look bigger.
    pub const BULLET_TICK: f32 = 14.0;
    pub const BULLET_ALERT: f32 = 13.0;
    /// `gap-2.5` between the bullet and the label block.
    pub const BULLET_GAP: f32 = 10.0;
    /// `px-1 py-1` around each step's clickable row.
    pub const ITEM_PAD: f32 = space::XS;
    /// `gap-1` — between two `<li>`s, and inside one between its content and its
    /// connector.
    pub const CELL_GAP: f32 = space::XS;
    /// `min-w-4` on a horizontal connector, `h-px` thick.
    pub const CONNECTOR_MIN: f32 = space::LG;
    pub const CONNECTOR: f32 = control::SEPARATOR;
    /// A vertical connector: `ml-4 h-4 w-px`.
    pub const VERTICAL_CONNECTOR_X: f32 = space::LG;
    pub const VERTICAL_CONNECTOR_H: f32 = space::LG;
    /// `flex-col gap-3` between two rows of a vertical trail.
    pub const VERTICAL_ROW_GAP: f32 = space::MD;
    /// Below this container width the horizontal trail would wrap onto several
    /// lines — a shape the project explicitly rejects — and the web replaces it
    /// with « Step 2 of 5 » and a rail.
    ///
    /// [`Widget::paint`] makes that switch itself, from the width it is given
    /// (the web measures its container, not the window). The counter's words
    /// are the caller's ([`Stepper::counter_words`]) — a primitive in this
    /// crate paints the strings it is handed rather than composing its own.
    ///
    /// The web applies it to a vertical trail too; here a vertical trail keeps
    /// its bullets at any width (see [`Stepper::is_compact`]), because the
    /// desktop hosts put vertical wizards in side columns narrower than this
    /// and a vertical trail never wraps in the first place.
    pub const COMPACT_WIDTH: f32 = 560.0;
    /// The compact summary: `gap-3` between the label and the counter.
    pub const COMPACT_GAP: f32 = space::MD;
    /// `mt-0.5` above the current step's description.
    pub const COMPACT_DESC_TOP: f32 = space::XXS;
    /// `mt-2` above the rail, `h-1` rail, `gap-1` between its segments.
    pub const RAIL_TOP: f32 = space::SM;
    pub const RAIL: f32 = space::XS;
    pub const RAIL_GAP: f32 = space::XS;
    /// `rounded-md` on a reachable step's row (its hover fill and ring).
    pub const ROW_RADIUS: f32 = radius::SM;
    /// `ml-1` before the « (optional) » marker.
    pub const OPTIONAL_GAP: f32 = space::XS;

    pub fn new() -> Self {
        Self {
            frame: Panel::new(),
            steps: Vec::new(),
            current: 0,
            orientation: StepperOrientation::default(),
            allow_forward: false,
            hovered: None,
            focused: None,
            counter_words: None,
            optional_marker: None,
        }
    }

    /// Builder: the compact counter's two words, e.g. `("Étape", "sur")`.
    pub fn with_counter_words(mut self, step: impl Into<String>, of: impl Into<String>) -> Self {
        self.counter_words = Some((step.into(), of.into()));
        self
    }

    /// Builder: the optional marker's word, e.g. `"optionnel"`.
    pub fn with_optional_marker(mut self, word: impl Into<String>) -> Self {
        self.optional_marker = Some(word.into());
        self
    }

    /// What this stepper paints in a container `width` DIP wide: the compact
    /// summary below [`Stepper::COMPACT_WIDTH`] (horizontal only — see
    /// [`Stepper::is_compact`]), the trail otherwise.
    pub fn view_at(&self, width: f32) -> StepperView {
        if self.is_compact(width) {
            StepperView::Compact
        } else {
            StepperView::Trail
        }
    }

    /// The compact counter's text: « Step 2 of 5 », or « 2 / 5 ».
    pub fn counter_text(&self) -> String {
        let n = self.current_index() + 1;
        let total = self.steps.len();
        match &self.counter_words {
            Some((step, of)) => format!("{step} {n} {of} {total}"),
            None => format!("{n} / {total}"),
        }
    }

    /// The compact summary's height: the label line, the current step's
    /// description when it has one (`mt-0.5`, meta), then the rail (`mt-2`,
    /// `h-1`).
    pub fn compact_height(&self) -> f32 {
        let desc = self
            .steps
            .get(self.current_index())
            .is_some_and(|s| s.description.is_some());
        Role::Body.line_height()
            + if desc { Self::COMPACT_DESC_TOP + Role::Meta.line_height() } else { 0.0 }
            + Self::RAIL_TOP
            + Self::RAIL
    }

    /// The height this stepper needs in a container `width` DIP wide — the
    /// compact summary's or the trail's, whichever [`Stepper::view_at`] picks.
    pub fn height_at(&self, width: f32) -> f32 {
        match self.view_at(width) {
            StepperView::Compact => self.compact_height(),
            StepperView::Trail => self.total_height(),
        }
    }

    /// The compact rail's segments inside `bounds` — `flex gap-1`, one
    /// `h-1 flex-1 rounded-full` segment per step, at the bottom of the
    /// summary.
    pub fn rail_rects(&self, bounds: Rect) -> Vec<Rect> {
        let n = self.steps.len();
        if n == 0 {
            return Vec::new();
        }
        let top = bounds.top + self.compact_height() - Self::RAIL;
        let total = (bounds.right - bounds.left).max(0.0);
        let seg = ((total - Self::RAIL_GAP * (n - 1) as f32) / n as f32).max(0.0);
        (0..n)
            .map(|i| {
                let left = bounds.left + i as f32 * (seg + Self::RAIL_GAP);
                Rect::new(left, top, left + seg, top + Self::RAIL)
            })
            .collect()
    }

    /// The reachable steps, in order — the Tab stops of the trail (the web
    /// renders only these as `<button>`s, so « the Tab order only ever offers
    /// what can actually be activated »).
    pub fn reachable_steps(&self) -> Vec<usize> {
        (0..self.steps.len()).filter(|&i| self.is_reachable(i)).collect()
    }

    pub fn step(mut self, step: Step) -> Self {
        self.steps.push(step);
        self
    }

    pub fn at(mut self, current: usize) -> Self {
        self.current = current;
        self
    }

    pub fn vertical(mut self) -> Self {
        self.orientation = StepperOrientation::Vertical;
        self
    }

    pub fn with_allow_forward(mut self, on: bool) -> Self {
        self.allow_forward = on;
        self
    }

    fn is_horizontal(&self) -> bool {
        self.orientation == StepperOrientation::Horizontal
    }

    /// The current index, clamped into the list — the web's `currentIndex`.
    pub fn current_index(&self) -> usize {
        if self.steps.is_empty() {
            0
        } else {
            self.current.min(self.steps.len() - 1)
        }
    }

    /// The status of step `index`: its own if it declared one, otherwise derived
    /// from its position relative to the current step.
    pub fn status(&self, index: usize) -> StepStatus {
        let Some(step) = self.steps.get(index) else { return StepStatus::Pending };
        if let Some(explicit) = step.status {
            return explicit;
        }
        let current = self.current_index();
        match index.cmp(&current) {
            std::cmp::Ordering::Less => StepStatus::Complete,
            std::cmp::Ordering::Equal => StepStatus::Current,
            std::cmp::Ordering::Greater => StepStatus::Pending,
        }
    }

    /// Whether step `index` may be clicked — `reachable` in the web.
    pub fn is_reachable(&self, index: usize) -> bool {
        index < self.steps.len()
            && self.status(index) != StepStatus::Disabled
            && (self.allow_forward || index <= self.current_index())
    }

    /// Whether ANY step is in error. What a wizard's « Next » button reads to
    /// refuse to move on.
    pub fn has_error(&self) -> bool {
        (0..self.steps.len()).any(|i| self.status(i) == StepStatus::Error)
    }

    /// Whether a container this wide must fall back to the compact summary.
    pub fn is_compact(&self, width: f32) -> bool {
        self.is_horizontal() && width < Self::COMPACT_WIDTH
    }

    /// The height of one step's row: its padding around the taller of the bullet
    /// and the label block.
    pub fn row_height(&self) -> f32 {
        let text = Role::Body.line_height()
            + if self.steps.iter().any(|s| s.description.is_some()) {
                Role::Meta.line_height()
            } else {
                0.0
            };
        Self::ITEM_PAD * 2.0 + Self::BULLET.max(text)
    }

    /// The whole trail's height.
    pub fn total_height(&self) -> f32 {
        if self.is_horizontal() {
            self.row_height()
        } else {
            let n = self.steps.len();
            if n == 0 {
                return 0.0;
            }
            n as f32 * self.row_height()
                + (n - 1) as f32 * (Self::VERTICAL_ROW_GAP + Self::VERTICAL_CONNECTOR_H)
        }
    }

    /// Each step's CLICKABLE row inside `bounds`, connectors excluded.
    ///
    /// Horizontally the `<ol>` splits into equal `flex-1` cells with
    /// [`Stepper::CELL_GAP`] between them; inside a cell the content and its
    /// connector are `flex-1` too, so they halve what is left — with the
    /// connector floored at [`Stepper::CONNECTOR_MIN`], and the last cell
    /// keeping the whole width because it has no connector.
    pub fn step_rects(&self, bounds: Rect) -> Vec<Rect> {
        let n = self.steps.len();
        let mut out = Vec::with_capacity(n);
        if n == 0 {
            return out;
        }
        if self.is_horizontal() {
            let total = (bounds.right - bounds.left).max(0.0);
            let cell = ((total - Self::CELL_GAP * (n - 1) as f32) / n as f32).max(0.0);
            let h = self.row_height();
            for i in 0..n {
                let left = bounds.left + i as f32 * (cell + Self::CELL_GAP);
                let width = if i + 1 == n { cell } else { cell - Self::CELL_GAP - self.connector_len(cell) };
                out.push(Rect::new(left, bounds.top, left + width.max(0.0), bounds.top + h));
            }
        } else {
            let h = self.row_height();
            let mut y = bounds.top;
            for _ in 0..n {
                out.push(Rect::new(bounds.left, y, bounds.right, y + h));
                y += h + Self::VERTICAL_CONNECTOR_H + Self::VERTICAL_ROW_GAP;
            }
        }
        out
    }

    /// A horizontal connector's length inside a cell of `cell` DIP: half of what
    /// is left after the inner gap, floored at `min-w-4`.
    fn connector_len(&self, cell: f32) -> f32 {
        (((cell - Self::CELL_GAP) / 2.0).max(Self::CONNECTOR_MIN)).min((cell - Self::CELL_GAP).max(0.0))
    }

    /// The `n − 1` connectors, in step order.
    pub fn connector_rects(&self, bounds: Rect) -> Vec<Rect> {
        let rects = self.step_rects(bounds);
        let mut out = Vec::new();
        for i in 0..rects.len().saturating_sub(1) {
            let a = rects[i];
            if self.is_horizontal() {
                let y = a.top + self.row_height() / 2.0;
                out.push(Rect::new(
                    a.right + Self::CELL_GAP,
                    y - Self::CONNECTOR / 2.0,
                    rects[i + 1].left - Self::CELL_GAP,
                    y + Self::CONNECTOR / 2.0,
                ));
            } else {
                let x = a.left + Self::VERTICAL_CONNECTOR_X;
                out.push(Rect::new(
                    x,
                    a.bottom,
                    x + Self::CONNECTOR,
                    a.bottom + Self::VERTICAL_CONNECTOR_H,
                ));
            }
        }
        out
    }

    /// Which step `(x, y)` lands on. `None` between cells and outside the trail.
    pub fn step_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        self.step_rects(bounds)
            .into_iter()
            .position(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
    }

    /// The bullet's rectangle inside a step's row.
    pub fn bullet_rect(&self, row: Rect) -> Rect {
        let top = row.top + Self::ITEM_PAD + ((row.bottom - row.top - Self::ITEM_PAD * 2.0) - Self::BULLET) / 2.0;
        let left = row.left + Self::ITEM_PAD;
        Rect::new(left, top, left + Self::BULLET, top + Self::BULLET)
    }

    /// The web's `compactView`: the current label (`font-medium`, truncated)
    /// and the counter on one baseline, the current description under them,
    /// then the rail. Pinned to the top of `bounds`; the description is
    /// dropped rather than overflowing when `bounds` has no room for it (a
    /// caller that reserved [`Stepper::total_height`] instead of
    /// [`Stepper::height_at`]).
    fn paint_compact(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let current = self.current_index();
        let Some(step) = self.steps.get(current) else { return };
        let inert = state.disabled;

        let line_h = Role::Body.line_height();
        let counter = self.counter_text();
        let meta = Role::Meta.format(canvas.formats());
        let counter_w = canvas.measure(&counter, meta).ceil().min(bounds.right - bounds.left);
        let band = Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + line_h);
        // `shrink-0 tabular-nums text-text-secondary`, meta.
        line(
            canvas,
            Rect::new(band.right - counter_w, band.top, band.right, band.bottom),
            &counter,
            Role::Meta,
            ContentAlignment::MiddleRight,
            if inert { t.text_tertiary } else { t.text_secondary },
            false,
        );
        // `min-w-0 truncate font-medium text-text-primary`.
        strong_line(
            canvas,
            Rect::new(band.left, band.top, (band.right - counter_w - Self::COMPACT_GAP).max(band.left), band.bottom),
            &step.label,
            &canvas.formats().body_strong,
            false,
            if inert { t.text_tertiary } else { t.text_primary },
        );

        let fits = bounds.bottom - bounds.top >= self.compact_height();
        if let (Some(desc), true) = (&step.description, fits) {
            let top = band.bottom + Self::COMPACT_DESC_TOP;
            line(
                canvas,
                Rect::new(band.left, top, band.right, top + Role::Meta.line_height()),
                desc,
                Role::Meta,
                ContentAlignment::MiddleLeft,
                if inert { t.text_tertiary } else { t.text_secondary },
                true,
            );
        }

        // The rail: `bg-danger` on a failed step, `bg-primary` up to and
        // including the current one, `bg-surface-3` after it.
        // Without room for the description the rail follows the label line.
        let rail_top = if fits {
            bounds.top + self.compact_height() - Self::RAIL
        } else {
            band.bottom + Self::RAIL_TOP
        };
        for (i, r) in self.rail_rects(bounds).iter().enumerate() {
            let seg = &Rect::new(r.left, rail_top, r.right, rail_top + Self::RAIL);
            let colour = if inert {
                t.card_stroke
            } else if self.status(i) == StepStatus::Error {
                t.danger
            } else if i <= current {
                t.accent
            } else {
                t.surface_3
            };
            canvas.fill_rounded(seg, pill(Self::RAIL), &colour);
        }
    }
}

impl Widget for Stepper {
    fn model(&self) -> &dyn Control {
        self.frame.model()
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        // No intrinsic width: a horizontal trail is `flex-1` over whatever it is
        // given, and a vertical one is as wide as its column.
        self.frame.clamp(Size::new(0.0, self.total_height()))
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        // The web swaps the trail for the summary below `COMPACT_WIDTH` of
        // ITS container: « narrow containers do not shrink the trail — they
        // replace it ». Squeezing five bullets into a card is how the labels
        // ended up as « S… », « C… ».
        if self.view_at(bounds.right - bounds.left) == StepperView::Compact {
            self.paint_compact(canvas, bounds, state);
            return;
        }

        let t = canvas.theme();
        let rows = self.step_rects(bounds);
        let connectors = self.connector_rects(bounds);
        let current = self.current_index();

        // The connectors first, so a bullet always sits on top of its line.
        for (i, rect) in connectors.iter().enumerate() {
            // The web colours a connector by POSITION (`i < currentIndex`), not
            // by status: it says how far the user has walked, not whether the
            // step it leaves behind was valid.
            let colour = if i < current { t.accent } else { t.card_stroke };
            canvas.fill_rounded(rect, 0.0, &colour);
        }

        for (i, step) in self.steps.iter().enumerate() {
            let Some(&row) = rows.get(i) else { continue };
            let status = if state.disabled { StepStatus::Disabled } else { self.status(i) };
            let (fill, ink, border) = status.bullet(t);
            let reachable = !state.disabled && self.is_reachable(i);

            // `hover:bg-surface-2 rounded-md` — only on a step that is a real
            // `<button>`, i.e. a reachable one.
            if reachable && self.hovered == Some(i) {
                canvas.fill_rounded(&row, Self::ROW_RADIUS, &t.surface_2);
            }

            let bullet = self.bullet_rect(row);
            canvas.fill_rounded(&bullet, pill(Self::BULLET), &fill);
            canvas.stroke_rounded(&bullet, pill(Self::BULLET), &border);

            match status.mark() {
                Some((name, size)) => glyph(
                    canvas,
                    name,
                    (bullet.left + bullet.right) / 2.0 - size / 2.0,
                    (bullet.top + bullet.bottom) / 2.0 - size / 2.0,
                    size,
                    ink,
                ),
                // The step's NUMBER, one-based, at the meta step.
                None => line(
                    canvas,
                    bullet,
                    &(i + 1).to_string(),
                    Role::Meta,
                    ContentAlignment::MiddleCenter,
                    ink,
                    false,
                ),
            }

            // The label block: a body line, then the optional description at the
            // meta step. Both trimmed — the web writes `truncate` on each.
            let left = bullet.right + Self::BULLET_GAP;
            let right = (row.right - Self::ITEM_PAD).max(left);
            let label_line = Role::Body.line_height();
            let desc_line =
                if step.description.is_some() { Role::Meta.line_height() } else { 0.0 };
            let block = label_line + desc_line;
            let top = (row.top + row.bottom) / 2.0 - block / 2.0;

            // `text-primary font-medium` on the current and the failed step:
            // the shared semibold body face; the others keep the regular one.
            let fmt = if matches!(status, StepStatus::Current | StepStatus::Error) {
                &canvas.formats().body_strong
            } else {
                Role::Body.format(canvas.formats())
            };
            let label_band = Rect::new(left, top, right, top + label_line);
            // `optional`: the web appends « (optional) » inside the same
            // truncated span, `ml-1`, meta, tertiary. The word is the caller's
            // ([`Stepper::optional_marker`]); the label keeps priority, so the
            // marker only shows when the label leaves it room.
            let marker = match (&self.optional_marker, step.optional) {
                (Some(word), true) => Some(format!("({word})")),
                _ => None,
            };
            let label_w = canvas.measure(&step.label, fmt).ceil();
            strong_line(canvas, label_band, &step.label, fmt, false, status.label(t));
            if let Some(marker) = marker {
                let x = left + label_w + Self::OPTIONAL_GAP;
                if x < right {
                    line(
                        canvas,
                        Rect::new(x, label_band.top, right, label_band.bottom),
                        &marker,
                        Role::Meta,
                        ContentAlignment::MiddleLeft,
                        t.text_tertiary,
                        true,
                    );
                }
            }
            if reachable && self.focused == Some(i) {
                focus_ring(canvas, row, Self::ROW_RADIUS);
            }
            if let Some(desc) = &step.description {
                line(
                    canvas,
                    Rect::new(left, top + label_line, right, top + block),
                    desc,
                    Role::Meta,
                    ContentAlignment::MiddleLeft,
                    t.text_tertiary,
                    true,
                );
            }
        }
    }

    fn type_name(&self) -> &'static str {
        "Stepper"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for DirectWrite: every glyph is this wide. Injected the way
    /// `display`'s tests inject a text extent, so the arithmetic is checked
    /// without a device.
    const CHAR: f32 = 7.0;

    fn measure(s: &str) -> f32 {
        s.chars().count() as f32 * CHAR
    }

    // ── Spinner ───────────────────────────────────────────────────────────

    #[test]
    fn a_spinners_phase_walks_one_whole_turn_and_wraps() {
        let mut s = Spinner::new();
        let tau = std::f32::consts::TAU;

        s.phase = 0.0;
        let (from0, to0) = s.arc();
        // At rest the arc is centred on the top of the ring, which is where
        // `border-t-primary` paints it.
        assert!(((from0 + to0) / 2.0 + tau / 4.0).abs() < 1e-4);
        // …and it covers exactly a quarter of the ring.
        assert!(((to0 - from0) - SPINNER_ARC * tau).abs() < 1e-4);

        // A quarter turn later it has moved a quarter of the ring.
        s.phase = 0.25;
        let (from1, _) = s.arc();
        assert!(((from1 - from0) - tau / 4.0).abs() < 1e-4);

        // One full turn is the same picture as none: the phase WRAPS.
        s.phase = 1.0;
        assert!((s.turn() - 0.0).abs() < 1e-6);
        let (from2, to2) = s.arc();
        assert!((from2 - from0).abs() < 1e-4 && (to2 - to0).abs() < 1e-4);

        // And it keeps wrapping past the end rather than freezing there.
        s.phase = 2.25;
        assert!((s.turn() - 0.25).abs() < 1e-6);
        // Backwards too.
        s.phase = -0.25;
        assert!((s.turn() - 0.75).abs() < 1e-6);
        // A non-finite phase — a division by a zero frame time — reads as the
        // start of the turn rather than poisoning every coordinate with NaN.
        s.phase = f32::NAN;
        assert_eq!(s.turn(), 0.0);
        assert!(s.arc().0.is_finite());
    }

    #[test]
    fn advancing_a_spinner_is_the_phase_plus_a_delta_wrapped() {
        let s = Spinner::new().with_phase(0.9);
        assert!((s.advanced(0.05) - 0.95).abs() < 1e-6);
        assert!((s.advanced(0.2) - 0.1).abs() < 1e-5);
        assert!((s.advanced(f32::INFINITY) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn the_arc_is_rasterised_into_overlapping_dabs_on_the_stroke_centreline() {
        for size in SPINNER_SIZES {
            let s = Spinner::new().with_size(size);
            let box_ = size.box_size();
            let bounds = Rect::new(0.0, 0.0, box_, box_);
            let dabs = s.dab_centres(bounds);
            assert!(dabs.len() >= 2, "{size:?}: an arc needs more than one dab");

            let r = (box_ - size.stroke()) / 2.0;
            for (x, y) in &dabs {
                let d = ((x - box_ / 2.0).powi(2) + (y - box_ / 2.0).powi(2)).sqrt();
                assert!((d - r).abs() < 1e-3, "{size:?}: a dab left the stroke's centre line");
            }
            // Consecutive dabs overlap, which is what makes the chain read as a
            // continuous stroke rather than a dotted one.
            for pair in dabs.windows(2) {
                let step = ((pair[1].0 - pair[0].0).powi(2) + (pair[1].1 - pair[0].1).powi(2)).sqrt();
                assert!(
                    step <= size.stroke() * Spinner::DAB_PITCH + 1e-3,
                    "{size:?}: {step} between dabs of a {} stroke",
                    size.stroke()
                );
            }
        }
    }

    #[test]
    fn a_spinner_is_a_ring_and_not_a_square() {
        let s = Spinner::new().with_size(SpinnerSize::Lg);
        let b = Rect::new(0.0, 0.0, 32.0, 32.0);
        // On the ring.
        assert!(s.hit_test(b, 16.0, 0.5));
        // In the hole.
        assert!(!s.hit_test(b, 16.0, 16.0));
        // In a corner.
        assert!(!s.hit_test(b, 0.0, 0.0));
    }

    // ── wrap ──────────────────────────────────────────────────────────────

    #[test]
    fn wrapping_breaks_on_words_and_never_loses_one() {
        // 7 DIP a glyph: "aaa bbb" is 7 chars = 49.
        let lines = wrap_lines("aaa bbb ccc", 49.0, measure);
        assert_eq!(lines, vec!["aaa bbb".to_string(), "ccc".to_string()]);

        assert!(wrap_lines("", 100.0, measure).is_empty());
        assert_eq!(wrap_lines("un deux", 1000.0, measure).len(), 1);
    }

    #[test]
    fn a_word_longer_than_the_line_is_hard_broken_like_break_words() {
        // 7 DIP a glyph, a 20 DIP column holds two glyphs a line.
        let lines = wrap_lines("aaaaaaaaaaaa bb", 20.0, measure);
        assert_eq!(lines, vec!["aa", "aa", "aa", "aa", "aa", "aa", "bb"]);
        // Every line fits, and not one character was lost.
        assert!(lines.iter().all(|l| measure(l) <= 20.0));
        assert_eq!(lines.concat(), "aaaaaaaaaaaabb");

        // The run first moves to a line of its own, and its LAST piece stays
        // open, so the next word may join it.
        let lines = wrap_lines("le Anticonstitutionnellement x", 70.0, measure);
        assert_eq!(lines[0], "le");
        assert!(lines.iter().all(|l| measure(l) <= 70.0), "{lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("ement x"));
        assert_eq!(lines.concat().replace(' ', ""), "leAnticonstitutionnellementx");

        // A column narrower than one glyph still makes progress.
        assert_eq!(wrap_lines("abc", 1.0, measure), vec!["a", "b", "c"]);
        // Multi-byte characters are split on char boundaries.
        assert_eq!(wrap_lines("ééé", 14.0, measure), vec!["éé", "é"]);
    }

    #[test]
    fn no_intermediate_line_is_ever_dropped_or_truncated() {
        // The composition audit's sentence: every word must survive, in order.
        let text = "Quand un membre partagera un dossier ou un document, il apparaîtra ici avec ses droits d'accès.";
        for width in [80.0, 140.0, 250.0, 384.0] {
            let lines = wrap_lines(text, width, measure);
            assert_eq!(lines.join(" "), text, "width {width}");
            assert!(lines.iter().all(|l| measure(l) <= width), "width {width}: {lines:?}");
        }
        // Narrower than its longest word: the words break, but every
        // character still comes out, in order.
        let lines = wrap_lines(text, 60.0, measure);
        assert!(lines.iter().all(|l| measure(l) <= 60.0));
        assert_eq!(lines.concat().replace(' ', ""), text.replace(' ', ""));
    }

    #[test]
    fn explicit_line_breaks_start_a_new_line() {
        let lines = wrap_lines("un\r\ndeux trois\nquatre", 1000.0, measure);
        assert_eq!(lines, vec!["un", "deux trois", "quatre"]);
    }

    #[test]
    fn flex_wrap_packs_buttons_into_rows() {
        let items = [(0, 100.0), (1, 80.0)];
        // Room for both and the gap: one row.
        assert_eq!(pack_rows(&items, 188.0, 8.0).len(), 1);
        // One DIP short: the second wraps.
        let rows = pack_rows(&items, 187.0, 8.0);
        assert_eq!(rows, vec![vec![(0, 100.0)], vec![(1, 80.0)]]);
        // An item wider than the row still gets a row.
        assert_eq!(pack_rows(&[(0, 500.0)], 100.0, 8.0).len(), 1);
        assert!(pack_rows(&[], 100.0, 8.0).is_empty());
    }

    // ── EmptyState ────────────────────────────────────────────────────────

    #[test]
    fn an_empty_states_height_grows_one_relaxed_line_per_wrapped_line() {
        let e = EmptyState::new("Inbox", "Rien ici").with_description("peu importe");
        let bare = e.height_for(0);
        assert_eq!(bare, EmptyState::PAD_Y * 2.0 + EmptyState::MEDALLION + EmptyState::GAP + Role::Heading.line_height());

        // One line adds the `mt-1` gap and one relaxed line…
        assert_eq!(e.height_for(1), bare + EmptyState::TEXT_GAP + RELAXED_LINE);
        // …and every further line adds exactly one more.
        for n in 2..6 {
            assert_eq!(
                e.height_for(n) - e.height_for(n - 1),
                RELAXED_LINE,
                "line {n} did not cost one relaxed line"
            );
        }
    }

    #[test]
    fn the_length_of_the_text_is_what_drives_that_height() {
        let e = EmptyState::new("Inbox", "Rien ici")
            .with_description("un deux trois quatre cinq six sept huit neuf dix");
        // The same description, wrapped into a narrow column and into a wide one.
        let narrow = wrap_lines("un deux trois quatre cinq six sept huit neuf dix", 60.0, measure);
        let wide = wrap_lines("un deux trois quatre cinq six sept huit neuf dix", 400.0, measure);
        assert!(narrow.len() > wide.len());
        assert!(e.height_for(narrow.len()) > e.height_for(wide.len()));
    }

    #[test]
    fn a_compact_empty_state_halves_its_room_and_drops_a_type_step() {
        let normal = EmptyState::new("Inbox", "Rien ici");
        let compact = EmptyState::new("Inbox", "Rien ici").with_compact(true);
        assert_eq!(compact.pad_y(), EmptyState::PAD_Y_COMPACT);
        assert_eq!(compact.medallion_size(), EmptyState::MEDALLION_COMPACT);
        // `compact ? 'var(--kb-text-body)' : 'var(--kb-text-heading)'`.
        assert_eq!(compact.title.role, Role::Body);
        assert_eq!(normal.title.role, Role::Heading);
        assert!(compact.height_for(1) < normal.height_for(1));
    }

    #[test]
    fn an_action_row_costs_a_gap_and_a_small_button() {
        let plain = EmptyState::new("Inbox", "Rien ici");
        let acting = EmptyState::new("Inbox", "Rien ici")
            .with_action(Button::new("Créer").size(ButtonSize::Sm));
        assert_eq!(
            acting.height_for(0) - plain.height_for(0),
            EmptyState::GAP + EmptyState::TEXT_GAP + ButtonSize::Sm.height()
        );
    }

    #[test]
    fn only_a_first_use_empty_state_offers_a_primary_action() {
        // The rule the component exists to enforce: a filtered-empty screen must
        // not push a creation.
        assert_eq!(EmptyStateVariant::FirstUse.action_variant(), ButtonVariant::Primary);
        for v in [
            EmptyStateVariant::NoResults,
            EmptyStateVariant::Error,
            EmptyStateVariant::Unavailable,
        ] {
            assert_eq!(v.action_variant(), ButtonVariant::Secondary, "{v:?}");
        }
    }

    // ── Callout ───────────────────────────────────────────────────────────

    #[test]
    fn every_callout_variant_has_its_own_glyph() {
        let names: Vec<&str> = CALLOUT_VARIANTS.iter().map(|v| v.icon()).collect();
        assert_eq!(names, vec!["Info", "CheckCircle2", "AlertTriangle", "AlertCircle"]);
        // …and no two variants share one, which is what makes the severity
        // readable without colour.
        for (i, a) in names.iter().enumerate() {
            for b in names.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn a_callouts_height_is_its_padding_its_lines_and_its_action() {
        let base = Callout::new("un mot");
        // No title: padding, then one relaxed line — but never less than the
        // glyph column.
        let one = base.height_for(1);
        assert_eq!(one, Callout::PAD_Y * 2.0 + RELAXED_LINE.max(Callout::ICON_TOP + Callout::ICON));
        assert_eq!(base.height_for(2) - one, RELAXED_LINE);

        let titled = Callout::new("un mot").with_title("Titre");
        assert_eq!(
            titled.height_for(1) - one,
            Role::Body.line_height() + Callout::TITLE_GAP
        );

        // The action is the same cost on every variant.
        for v in CALLOUT_VARIANTS {
            let quiet = Callout::new("un mot").with_variant(v);
            let acting = Callout::new("un mot")
                .with_variant(v)
                .with_action(CalloutAction::new("Réessayer"));
            assert_eq!(
                acting.height_for(1) - quiet.height_for(1),
                Callout::ACTION_GAP + Callout::action_height(),
                "{v:?}"
            );
        }
    }

    #[test]
    fn an_empty_callout_is_still_as_tall_as_its_mark() {
        // `flex items-start`: the glyph column is the floor, so a callout with a
        // one-line body is never shorter than the 16 px mark beside it.
        let c = Callout::new("");
        assert_eq!(c.height_for(0), Callout::PAD_Y * 2.0 + Callout::ICON_TOP + Callout::ICON);
        // Dropping the glyph removes that floor.
        let bare = Callout::new("").without_icon();
        assert_eq!(bare.height_for(0), Callout::PAD_Y * 2.0);
        assert_eq!(bare.glyph(), None);
    }

    #[test]
    fn the_glyph_and_the_dismiss_columns_narrow_the_text() {
        let bounds = Rect::new(0.0, 0.0, 300.0, 60.0);
        let plain = Callout::new("x").without_icon();
        let with_icon = Callout::new("x");
        let both = Callout::new("x").with_dismiss(true);

        let w = |c: &Callout| {
            let r = c.text_column(bounds);
            r.right - r.left
        };
        assert_eq!(w(&plain) - w(&with_icon), Callout::ICON + Callout::GAP);
        assert_eq!(w(&with_icon) - w(&both), Callout::dismiss_box() + Callout::GAP);
        assert!(both.dismiss_rect(bounds).is_some());
        assert!(with_icon.dismiss_rect(bounds).is_none());
    }

    // ── Accordion ─────────────────────────────────────────────────────────

    fn three() -> Accordion {
        Accordion::new()
            .section(AccordionSection::new("un", 40.0).open(true))
            .section(AccordionSection::new("deux", 60.0))
            .section(AccordionSection::new("trois", 30.0).open(true))
    }

    #[test]
    fn a_closed_section_is_its_header_and_an_open_one_carries_its_panel() {
        let a = three();
        let header = AccordionSize::Md.header();
        assert_eq!(a.section_height(&a.sections[1]), header);
        assert_eq!(
            a.section_height(&a.sections[0]),
            header + Accordion::RULE + Accordion::PANEL_TOP + 40.0 + AccordionSize::Md.pad_bottom()
        );
        // A disabled section « stays collapsed » whatever its `open` says.
        let stuck = AccordionSection::new("x", 100.0).open(true).disabled(true);
        assert!(!stuck.is_open());
        assert_eq!(a.section_height(&stuck), header);
    }

    #[test]
    fn the_total_height_follows_what_is_open() {
        let mut a = three();
        let header = AccordionSize::Md.header();
        let panel = |content: f32| {
            Accordion::RULE + Accordion::PANEL_TOP + content + AccordionSize::Md.pad_bottom()
        };
        assert_eq!(
            a.total_height(),
            3.0 * header + panel(40.0) + panel(30.0) + 2.0 * Accordion::GAP
        );

        // Opening the middle one adds exactly its panel.
        let before = a.total_height();
        a.toggle(1);
        assert_eq!(a.total_height(), before + panel(60.0));
        // Closing everything leaves three headers and two gaps.
        for s in &mut a.sections {
            s.open = false;
        }
        assert_eq!(a.total_height(), 3.0 * header + 2.0 * Accordion::GAP);

        // `single` closes the others.
        a.toggle_single(2);
        assert!(a.sections[2].is_open());
        a.toggle_single(0);
        assert!(a.sections[0].is_open() && !a.sections[2].is_open());
        // …and toggling the open one closes it.
        a.toggle_single(0);
        assert!(!a.sections[0].is_open());

        assert_eq!(Accordion::new().total_height(), 0.0);
    }

    #[test]
    fn section_at_is_exact_at_the_borders_and_empty_in_the_gaps() {
        let a = three();
        let bounds = Rect::new(10.0, 100.0, 310.0, 900.0);
        let rects = a.section_rects(bounds);
        assert_eq!(rects.len(), 3);

        for (i, r) in rects.iter().enumerate() {
            // The top edge belongs to the section…
            assert_eq!(a.section_at(bounds, 20.0, r.top), Some(i), "top of {i}");
            // …the bottom edge does not, so two stacked rows never both claim
            // the same DIP.
            assert_ne!(a.section_at(bounds, 20.0, r.bottom), Some(i), "bottom of {i}");
            // One DIP inside is still it.
            assert_eq!(a.section_at(bounds, 20.0, r.bottom - 0.001), Some(i));
        }

        // The gap between two sections belongs to nobody.
        let gap_y = rects[0].bottom + Accordion::GAP / 2.0;
        assert_eq!(a.section_at(bounds, 20.0, gap_y), None);
        // Above the stack, below it, and outside it horizontally.
        assert_eq!(a.section_at(bounds, 20.0, bounds.top - 1.0), None);
        assert_eq!(a.section_at(bounds, 20.0, rects[2].bottom + 1.0), None);
        assert_eq!(a.section_at(bounds, bounds.left - 1.0, rects[0].top), None);
        assert_eq!(a.section_at(bounds, bounds.right, rects[0].top), None);

        // Two sections open at once: the third still starts exactly one gap
        // after the second ends, panels included.
        assert_eq!(rects[1].top, rects[0].bottom + Accordion::GAP);
        assert_eq!(rects[2].top, rects[1].bottom + Accordion::GAP);
        assert_eq!(rects[2].bottom - bounds.top, a.total_height());
    }

    #[test]
    fn only_a_headers_own_band_toggles_and_a_disabled_one_never_does() {
        let mut a = three();
        a.sections[1].disabled = true;
        let bounds = Rect::new(0.0, 0.0, 300.0, 900.0);
        let rects = a.section_rects(bounds);
        let header = AccordionSize::Md.header();

        assert_eq!(a.header_at(bounds, 10.0, rects[0].top + 1.0), Some(0));
        // Inside the OPEN section's panel, not its header.
        assert_eq!(a.header_at(bounds, 10.0, rects[0].top + header + 5.0), None);
        // The disabled section's header is not a target.
        assert_eq!(a.header_at(bounds, 10.0, rects[1].top + 1.0), None);
        assert_eq!(a.section_at(bounds, 10.0, rects[1].top + 1.0), Some(1));
    }

    #[test]
    fn a_small_accordion_is_tighter_on_both_axes() {
        let sm = AccordionSize::Sm;
        let md = AccordionSize::Md;
        assert!(sm.header() < md.header());
        assert!(sm.pad_x() < md.pad_x());
        assert!(sm.pad_bottom() < md.pad_bottom());
        // Both hold the same 12 px title line.
        assert_eq!(md.header() - md.pad_y() * 2.0, Role::Meta.line_height());
        assert_eq!(sm.header() - sm.pad_y() * 2.0, Role::Meta.line_height());
    }

    // ── Stepper ───────────────────────────────────────────────────────────

    fn steps(n: usize) -> Stepper {
        (0..n).fold(Stepper::new(), |s, i| s.step(Step::new(format!("étape {}", i + 1))))
    }

    #[test]
    fn two_steps_split_the_width_and_the_last_one_reaches_the_end() {
        let s = steps(2);
        let bounds = Rect::new(0.0, 0.0, 600.0, 60.0);
        let rects = s.step_rects(bounds);
        assert_eq!(rects.len(), 2);
        // The `<ol>` splits into equal cells; the LAST one keeps its whole cell
        // because it has no connector to make room for.
        let cell = (600.0 - Stepper::CELL_GAP) / 2.0;
        assert_eq!(rects[1].right, bounds.right);
        assert_eq!(rects[1].right - rects[1].left, cell);
        assert!(rects[0].right < rects[1].left, "the cells must not overlap");
        // One connector, between the two.
        let links = s.connector_rects(bounds);
        assert_eq!(links.len(), 1);
        assert!(links[0].left >= rects[0].right && links[0].right <= rects[1].left);
    }

    #[test]
    fn five_steps_are_evenly_pitched_and_ordered() {
        let s = steps(5);
        let bounds = Rect::new(20.0, 0.0, 1020.0, 60.0);
        let rects = s.step_rects(bounds);
        assert_eq!(rects.len(), 5);
        let pitch = rects[1].left - rects[0].left;
        for pair in rects.windows(2) {
            assert!((pair[1].left - pair[0].left - pitch).abs() < 1e-3);
            assert!(pair[0].right <= pair[1].left);
        }
        assert_eq!(rects[0].left, bounds.left);
        assert_eq!(rects[4].right, bounds.right);
        assert_eq!(s.connector_rects(bounds).len(), 4);
    }

    #[test]
    fn where_the_current_step_is_decides_every_status_but_not_the_positions() {
        let s = steps(5);
        let bounds = Rect::new(0.0, 0.0, 800.0, 60.0);
        let at_start = s.step_rects(bounds);

        // Start: nothing done, the first is current, the rest pending.
        let s0 = steps(5).at(0);
        assert_eq!(s0.status(0), StepStatus::Current);
        assert_eq!(s0.status(4), StepStatus::Pending);

        // Middle.
        let s2 = steps(5).at(2);
        assert_eq!(s2.status(0), StepStatus::Complete);
        assert_eq!(s2.status(1), StepStatus::Complete);
        assert_eq!(s2.status(2), StepStatus::Current);
        assert_eq!(s2.status(3), StepStatus::Pending);

        // End.
        let s4 = steps(5).at(4);
        assert_eq!(s4.status(3), StepStatus::Complete);
        assert_eq!(s4.status(4), StepStatus::Current);

        // Out of range clamps, like the web's `currentIndex`.
        assert_eq!(steps(5).at(99).current_index(), 4);
        assert_eq!(steps(2).at(0).current_index(), 0);

        // …and none of that moved a single rectangle.
        for (a, b) in at_start.iter().zip(s4.step_rects(bounds).iter()) {
            assert_eq!((a.left, a.right), (b.left, b.right));
        }
    }

    #[test]
    fn a_failed_step_says_so_and_keeps_saying_it_behind_the_cursor() {
        // The reason this component is not a decoration: an error must survive
        // the user walking past it.
        let mut s = steps(5).at(3);
        s.steps[1].status = Some(StepStatus::Error);

        assert_eq!(s.status(1), StepStatus::Error, "an explicit status wins over the position");
        assert_eq!(s.status(0), StepStatus::Complete);
        assert!(s.has_error());
        // Its bullet is the danger fill and its label the danger ink — not the
        // pending grey it would have had.
        assert_eq!(StepStatus::Error.mark(), Some(("AlertTriangle", Stepper::BULLET_ALERT)));
        assert_eq!(StepStatus::Complete.mark(), Some(("Check", Stepper::BULLET_TICK)));
        assert_eq!(StepStatus::Current.mark(), None);

        // Clearing it puts the step back on the derived path.
        s.steps[1].status = None;
        assert_eq!(s.status(1), StepStatus::Complete);
        assert!(!s.has_error());
    }

    #[test]
    fn only_reachable_steps_are_clickable() {
        let s = steps(5).at(2);
        assert!(s.is_reachable(0) && s.is_reachable(2));
        // Forward is closed by default…
        assert!(!s.is_reachable(3));
        // …and opened by the flag.
        assert!(steps(5).at(2).with_allow_forward(true).is_reachable(3));
        // A disabled step never is.
        let mut s = steps(5).at(4);
        s.steps[1].status = Some(StepStatus::Disabled);
        assert!(!s.is_reachable(1));
        assert!(!s.is_reachable(9), "past the end is not a step");
    }

    #[test]
    fn step_at_lands_on_a_cell_and_not_on_the_connector_between_two() {
        let s = steps(5);
        let bounds = Rect::new(0.0, 10.0, 1000.0, 70.0);
        let rects = s.step_rects(bounds);
        for (i, r) in rects.iter().enumerate() {
            assert_eq!(s.step_at(bounds, r.left, r.top), Some(i));
            assert_eq!(s.step_at(bounds, r.right - 0.001, r.bottom - 0.001), Some(i));
            // Half-open on both axes.
            assert_ne!(s.step_at(bounds, r.right, r.top), Some(i));
        }
        // The connector's own band is nobody's cell.
        let link = s.connector_rects(bounds)[0];
        assert_eq!(s.step_at(bounds, (link.left + link.right) / 2.0, bounds.top + 1.0), None);
        assert_eq!(s.step_at(bounds, 500.0, bounds.top - 1.0), None);
    }

    #[test]
    fn a_vertical_trail_stacks_its_rows_with_a_connector_between_each() {
        let s = steps(3).vertical();
        let bounds = Rect::new(0.0, 0.0, 300.0, 400.0);
        let rects = s.step_rects(bounds);
        let row = s.row_height();
        assert_eq!(rects[1].top - rects[0].top, row + Stepper::VERTICAL_CONNECTOR_H + Stepper::VERTICAL_ROW_GAP);
        assert_eq!(s.total_height(), 3.0 * row + 2.0 * (Stepper::VERTICAL_CONNECTOR_H + Stepper::VERTICAL_ROW_GAP));
        let links = s.connector_rects(bounds);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].left, bounds.left + Stepper::VERTICAL_CONNECTOR_X);
        assert_eq!(links[0].top, rects[0].bottom);
    }

    #[test]
    fn a_description_makes_every_row_taller_and_the_bullet_stays_centred() {
        let plain = steps(3);
        let mut described = steps(3);
        described.steps[1].description = Some("détail".into());
        assert!(described.row_height() >= plain.row_height());

        let bounds = Rect::new(0.0, 0.0, 900.0, 100.0);
        for s in [&plain, &described] {
            let row = s.step_rects(bounds)[0];
            let bullet = s.bullet_rect(row);
            let above = bullet.top - (row.top + Stepper::ITEM_PAD);
            let below = (row.bottom - Stepper::ITEM_PAD) - bullet.bottom;
            assert!((above - below).abs() < 1e-3, "the bullet drifted off centre");
            assert_eq!(bullet.right - bullet.left, Stepper::BULLET);
        }
    }

    #[test]
    fn a_narrow_container_asks_for_the_compact_summary() {
        let s = steps(5);
        assert!(s.is_compact(Stepper::COMPACT_WIDTH - 1.0));
        assert!(!s.is_compact(Stepper::COMPACT_WIDTH));
        // A vertical trail never collapses: it does not wrap in the first place.
        assert!(!steps(5).vertical().is_compact(100.0));
    }

    #[test]
    fn an_empty_stepper_measures_to_nothing_and_hits_nothing() {
        let s = Stepper::new();
        let bounds = Rect::new(0.0, 0.0, 500.0, 50.0);
        assert!(s.step_rects(bounds).is_empty());
        assert!(s.connector_rects(bounds).is_empty());
        assert_eq!(s.step_at(bounds, 10.0, 10.0), None);
        assert_eq!(s.current_index(), 0);
        assert_eq!(s.status(0), StepStatus::Pending);
    }

    // ── Added: animation, per-part states, keyboard, compact view ─────────

    #[test]
    fn the_spinner_clock_turns_once_a_second() {
        assert_eq!(Spinner::phase_at(0), 0.0);
        assert!((Spinner::phase_at(250) - 0.25).abs() < 1e-6);
        assert!((Spinner::phase_at(1750) - 0.75).abs() < 1e-6);
        // It wraps rather than growing without bound.
        assert_eq!(Spinner::phase_at(3000), 0.0);
        assert!((Spinner::new().at_time(500).turn() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_wrapped_title_costs_one_body_line_per_line() {
        let c = Callout::new("x").with_title("Un titre");
        assert_eq!(c.height_for(1), c.height_for_lines(1, 1));
        assert_eq!(c.height_for_lines(3, 1) - c.height_for_lines(1, 1), 2.0 * Role::Body.line_height());
        // No title: the title count is ignored.
        let bare = Callout::new("x");
        assert_eq!(bare.height_for_lines(2, 1), bare.height_for(1));
    }

    #[test]
    fn a_callout_part_state_isolates_the_hot_part() {
        let mut c = Callout::new("x").with_dismiss(true).with_action(CalloutAction::new("Go"));
        let banner = WidgetState::REST.hot(true);
        // Legacy: nothing tracked, the banner-wide hover lights both.
        assert!(c.part_state(CalloutPart::Action, banner).hot);
        assert!(c.part_state(CalloutPart::Dismiss, banner).hot);
        // Tracked: only the part the host named is hot.
        c.dismiss_state = Some(WidgetState::REST.hot(true));
        assert!(c.part_state(CalloutPart::Dismiss, banner).hot);
        assert!(!c.part_state(CalloutPart::Action, banner).hot);
        // A disabled banner disables its parts whatever the host said.
        assert!(c.part_state(CalloutPart::Dismiss, banner.disabled(true)).disabled);
    }

    #[test]
    fn an_empty_states_title_and_buttons_wrap_into_its_height() {
        let e = EmptyState::new("Inbox", "Rien ici")
            .with_action(Button::new("Créer").size(ButtonSize::Sm))
            .with_secondary_action(Button::new("Aide").size(ButtonSize::Sm));
        let one = e.height_for_lines(1, 0, 1);
        assert_eq!(one, e.height_for(0));
        // A title on two lines costs one more title line…
        assert_eq!(e.height_for_lines(2, 0, 1) - one, Role::Heading.line_height());
        // …and a second button row one button and one gap.
        assert_eq!(e.height_for_lines(1, 0, 2) - one, ButtonSize::Sm.height() + EmptyState::ACTION_GAP);
        // The text column is the bounds minus the padding, capped at max-w-sm.
        assert_eq!(e.text_width(200.0), 200.0 - 2.0 * EmptyState::PAD_X);
        assert_eq!(e.text_width(2000.0), EmptyState::MAX_TEXT);
        assert_eq!(e.text_width(10.0), 1.0);
    }

    #[test]
    fn the_empty_state_button_state_isolates_the_hot_button() {
        let mut e = EmptyState::new("Inbox", "Rien");
        let block = WidgetState::REST.hot(true);
        assert!(e.button_state(0, block).hot && e.button_state(1, block).hot);
        e.action_state[1] = Some(WidgetState::REST.hot(true).focused(true).focus_visible(true));
        assert!(!e.button_state(0, block).hot);
        assert!(e.button_state(1, block).show_focus_ring());
    }

    #[test]
    fn arrows_walk_the_enabled_headers_and_wrap() {
        let mut a = three();
        a.sections.push(AccordionSection::new("quatre", 10.0).disabled(true));
        a.sections[1].disabled = true;
        // Enabled: 0 and 2.
        assert_eq!(a.header_after_key(Some(0), HeaderKey::Next), Some(2));
        assert_eq!(a.header_after_key(Some(2), HeaderKey::Next), Some(0), "wraps past the disabled tail");
        assert_eq!(a.header_after_key(Some(0), HeaderKey::Previous), Some(2));
        assert_eq!(a.header_after_key(Some(2), HeaderKey::Previous), Some(0));
        assert_eq!(a.header_after_key(None, HeaderKey::Next), Some(0));
        assert_eq!(a.header_after_key(None, HeaderKey::Previous), Some(2));
        assert_eq!(a.header_after_key(Some(1), HeaderKey::First), Some(0));
        assert_eq!(a.header_after_key(Some(0), HeaderKey::Last), Some(2));
        // Nothing enabled, nowhere to go.
        let dead = Accordion::new().section(AccordionSection::new("x", 1.0).disabled(true));
        assert_eq!(dead.header_after_key(None, HeaderKey::Next), None);
    }

    #[test]
    fn a_narrow_horizontal_stepper_paints_the_compact_summary() {
        let s = steps(4).at(1);
        assert_eq!(s.view_at(330.0), StepperView::Compact);
        assert_eq!(s.view_at(800.0), StepperView::Trail);
        assert_eq!(steps(4).vertical().view_at(330.0), StepperView::Trail);

        // Its height is the label line, then the rail.
        assert_eq!(s.compact_height(), Role::Body.line_height() + Stepper::RAIL_TOP + Stepper::RAIL);
        assert_eq!(s.height_at(330.0), s.compact_height());
        assert_eq!(s.height_at(800.0), s.total_height());
        // The current step's description adds its meta line.
        let mut d = steps(4).at(1);
        d.steps[1].description = Some("colonnes".into());
        assert_eq!(d.compact_height() - s.compact_height(), Stepper::COMPACT_DESC_TOP + Role::Meta.line_height());

        // The counter: language-neutral by default, the caller's words otherwise.
        assert_eq!(s.counter_text(), "2 / 4");
        assert_eq!(steps(4).at(1).with_counter_words("Étape", "sur").counter_text(), "Étape 2 sur 4");
    }

    #[test]
    fn the_compact_rail_shares_the_width_equally() {
        let s = steps(4);
        let bounds = Rect::new(10.0, 0.0, 330.0, 100.0);
        let rails = s.rail_rects(bounds);
        assert_eq!(rails.len(), 4);
        let w = rails[0].right - rails[0].left;
        for r in &rails {
            assert!(((r.right - r.left) - w).abs() < 1e-3);
            assert_eq!(r.bottom - r.top, Stepper::RAIL);
        }
        assert_eq!(rails[0].left, bounds.left);
        assert!((rails[3].right - bounds.right).abs() < 1e-3);
        assert!((rails[1].left - rails[0].right - Stepper::RAIL_GAP).abs() < 1e-3);
        // The rail ends the summary.
        assert_eq!(rails[0].bottom, bounds.top + s.compact_height());
        assert!(Stepper::new().rail_rects(bounds).is_empty());
    }

    #[test]
    fn only_reachable_steps_are_tab_stops() {
        assert_eq!(steps(5).at(2).reachable_steps(), vec![0, 1, 2]);
        assert_eq!(steps(3).at(0).with_allow_forward(true).reachable_steps(), vec![0, 1, 2]);
    }
}
