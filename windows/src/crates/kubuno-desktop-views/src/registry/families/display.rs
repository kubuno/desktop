//! Component family `display` — declared with the `component!` table (see
//! `../macros.rs`) plus, in this same file, the view nodes those components
//! build. Compiled only with the `family-display` feature while the families
//! are being written in parallel; the feature is on by default once
//! integrated.
//!
//! Nine leaf primitives from `kubuno-desktop-ui/src/{display.rs,feedback.rs,range.rs}`:
//! `Label`, `LinkLabel`, `Badge`, `Icon`, `Separator` (static text and
//! decoration), `Spinner`, `ProgressBar` (feedback on ongoing work), `Callout`,
//! `EmptyState` (feedback on a whole area). None of the nine take children —
//! every one wraps exactly one `kubuno_desktop_ui` widget and paints it directly, the
//! same leaf shape `crate::node`'s `ButtonNode`/`SwitchNode` already use (see
//! that module's doc on why a `ViewNode` is not simply `kubuno_desktop_ui::Widget`).
//!
//! ## Reaching this family's own node types from `component!`
//!
//! `../macros.rs`'s fixed `use $crate::node::{ButtonNode, CardNode, StackNode,
//! SwitchNode, TextFieldNode, ViewNode}` inside every `component!` invocation
//! does not know about this file's node types, and this family owns only this
//! file (not `../macros.rs`). The `component!` macro expands to `pub mod
//! $mod_name { … }` **inside this very module**, so `super` from inside a
//! `build:` closure is `registry::families::display` — this file — and every
//! `build:` closure below names its node type as `super::WhateverNode`
//! instead.
//!
//! ## `PaintCx::fire` / `InteractCx::fire` / `press_release`
//!
//! The interactive nodes here (`LinkLabelNode`, `CalloutNode`,
//! `EmptyStateNode`) call [`crate::node::PaintCx::fire`]/
//! [`crate::node::InteractCx::fire`] and [`crate::node::press_release`]
//! directly — both `pub(crate)`, shared by every component family instead of
//! each re-deriving its own copy.

#[allow(unused_imports)] // Used by the `component!` invocations below.
use crate::registry::macros::component;
use crate::registry::ComponentMeta;

use crate::binding::{PropSource, ViewModel};
use crate::node::{press_release, InteractCx, PaintCx, ViewEventKind, ViewNode};
use crate::events::EmptyEventArgs;

use kubuno_desktop_ui::display::{
    Badge, BadgeSize, BadgeVariant, Icon, Label, LinkLabel, Orientation, Role, Separator, TextOverflow,
};
use kubuno_desktop_ui::feedback::{
    Callout, CalloutAction, CalloutPart, CalloutVariant, EmptyState, EmptyStateVariant, Spinner, SpinnerSize,
};
use kubuno_desktop_ui::range::{ProgressBar, ProgressSize, ProgressVariant};
use kubuno_desktop_ui::{Canvas, ContentAlignment, FocusId, Rect, Size, Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────
// Shared helpers this family still owns (not moved to `crate::node`/
// `crate::icon`: no other family needs an animation clock).
// ─────────────────────────────────────────────────────────────────────────

/// Advances an indeterminate animation's phase by one assumed repaint tick
/// (16 ms, i.e. ~60 Hz), wrapping into `0.0..1.0`.
///
/// Both [`Spinner`] and [`ProgressBar`]'s marquee document that they own no
/// clock — the *host* advances `phase` between frames (see `feedback.rs`'s
/// "Animation: the phase is a PARAMETER" note and `range.rs`'s matching one on
/// `ProgressBar::phase`). [`crate::node::PaintCx`] carries no timestamp for a
/// host to read (it was not designed for an animated leaf), so this is the
/// best approximation available to a family file that must not touch that
/// shared type: assume `paint` runs roughly once per repaint. A real host
/// clock, if `PaintCx` ever grows one, replaces this outright.
fn advance_phase(phase: f32, period_ms: u64) -> f32 {
    let dt = 16.0 / period_ms.max(1) as f32;
    let next = phase + dt;
    if next.is_finite() {
        next.rem_euclid(1.0)
    } else {
        0.0
    }
}

fn parse_role(s: &str) -> Role {
    match s {
        "Micro" => Role::Micro,
        "Meta" => Role::Meta,
        "Heading" => Role::Heading,
        "Title" => Role::Title,
        "Page" => Role::Page,
        "PageAdmin" => Role::PageAdmin,
        "Badge" => Role::Badge,
        "Caption" => Role::Caption,
        "Subtitle" => Role::Subtitle,
        "Display" => Role::Display,
        _ => Role::Body,
    }
}

fn parse_alignment(s: &str) -> ContentAlignment {
    match s {
        "TopCenter" => ContentAlignment::TopCenter,
        "TopRight" => ContentAlignment::TopRight,
        "MiddleLeft" => ContentAlignment::MiddleLeft,
        "MiddleCenter" => ContentAlignment::MiddleCenter,
        "MiddleRight" => ContentAlignment::MiddleRight,
        "BottomLeft" => ContentAlignment::BottomLeft,
        "BottomCenter" => ContentAlignment::BottomCenter,
        "BottomRight" => ContentAlignment::BottomRight,
        _ => ContentAlignment::TopLeft,
    }
}

fn parse_overflow(s: &str) -> TextOverflow {
    match s {
        "Clip" => TextOverflow::Clip,
        "Wrap" => TextOverflow::Wrap,
        _ => TextOverflow::Ellipsis,
    }
}

fn parse_badge_variant(s: &str) -> BadgeVariant {
    match s {
        "Primary" => BadgeVariant::Primary,
        "Success" => BadgeVariant::Success,
        "Warning" => BadgeVariant::Warning,
        "Danger" => BadgeVariant::Danger,
        "Neutral" => BadgeVariant::Neutral,
        _ => BadgeVariant::Default,
    }
}

fn parse_badge_size(s: &str) -> BadgeSize {
    match s {
        "Sm" => BadgeSize::Sm,
        _ => BadgeSize::Md,
    }
}

fn parse_orientation(s: &str) -> Orientation {
    match s {
        "Vertical" => Orientation::Vertical,
        _ => Orientation::Horizontal,
    }
}

fn parse_spinner_size(s: &str) -> SpinnerSize {
    match s {
        "Xs" => SpinnerSize::Xs,
        "Sm" => SpinnerSize::Sm,
        "Lg" => SpinnerSize::Lg,
        _ => SpinnerSize::Md,
    }
}

fn parse_progress_variant(s: &str) -> ProgressVariant {
    match s {
        "Primary" => ProgressVariant::Primary,
        "Success" => ProgressVariant::Success,
        "Warning" => ProgressVariant::Warning,
        "Danger" => ProgressVariant::Danger,
        _ => ProgressVariant::Auto,
    }
}

fn parse_progress_size(s: &str) -> ProgressSize {
    match s {
        "Sm" => ProgressSize::Sm,
        _ => ProgressSize::Md,
    }
}

fn parse_callout_variant(s: &str) -> CalloutVariant {
    match s {
        "Success" => CalloutVariant::Success,
        "Warning" => CalloutVariant::Warning,
        "Danger" => CalloutVariant::Danger,
        _ => CalloutVariant::Info,
    }
}

fn parse_empty_state_variant(s: &str) -> EmptyStateVariant {
    match s {
        "NoResults" => EmptyStateVariant::NoResults,
        "Error" => EmptyStateVariant::Error,
        "Unavailable" => EmptyStateVariant::Unavailable,
        _ => EmptyStateVariant::FirstUse,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Label
// ─────────────────────────────────────────────────────────────────────────

pub struct LabelNode {
    pub text: PropSource<String>,
    pub role: PropSource<String>,
    pub align: PropSource<String>,
    pub overflow: PropSource<String>,
    /// `UseMnemonic` (an `&` marks the shortcut that moves to the next control).
    pub use_mnemonic: bool,
    /// `Image` (a path) and `ImageAlign`.
    pub image: Option<(String, ContentAlignment)>,
}

impl LabelNode {
    pub fn new(text: PropSource<String>, role: PropSource<String>, align: PropSource<String>, overflow: PropSource<String>) -> Self {
        Self { text, role, align, overflow, use_mnemonic: true, image: None }
    }

    fn build(&self, vm: &dyn ViewModel) -> Label {
        let text = self.text.resolve(vm);
        let text = if self.use_mnemonic { crate::common::mnemonic(&text).0 } else { text };
        Label::new(text)
            .role(parse_role(&self.role.resolve(vm)))
            .align(parse_alignment(&self.align.resolve(vm)))
            .overflow(parse_overflow(&self.overflow.resolve(vm)))
    }
}

impl ViewNode for LabelNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        // `Image`, behind the text, by `ImageAlign`.
        if let Some((path, align)) = &self.image {
            if let Some(bitmap) = kubuno_desktop_controls::styled::load_image(cx.canvas, path) {
                let rect = kubuno_desktop_controls::styled::align_image(kubuno_desktop_controls::styled::image_size(&bitmap), bounds, *align);
                cx.canvas.draw_bitmap(&bitmap, &rect, if crate::common::is_disabled() { 0.5 } else { 1.0 });
            }
        }
        let raw = self.text.resolve(cx.vm);
        let (_, underline) = cx.mnemonic_text(&raw, self.use_mnemonic, crate::common::MnemonicAction::FocusNext);
        let mut label = self.build(cx.vm);
        label.mnemonic = underline;
        label.paint(cx.canvas, bounds, crate::common::rest());
    }
}

// ─────────────────────────────────────────────────────────────────────────
// LinkLabel — the one `display` leaf with a real click: whole-widget
// press/release exactly like `ButtonNode`/`SwitchNode` (the spec's "the whole
// text is one link when neither `links` nor `link_area` is set" resolution
// makes that the common case; distinguishing which of several inline links
// was hit would need `LinkLabel::link_at`, which — like `Callout`/
// `EmptyState`'s sub-regions below — needs a `&dyn Canvas` this leaf's simple
// `InteractCx`-driven interact does not have).
// ─────────────────────────────────────────────────────────────────────────

pub struct LinkLabelNode {
    pub text: PropSource<String>,
    pub role: PropSource<String>,
    pub visited: PropSource<bool>,
    pub focus_id: Option<FocusId>,
    pub on_click: Option<String>,
    pressed: bool,
    /// `UseMnemonic`.
    pub use_mnemonic: bool,
}

impl LinkLabelNode {
    pub fn new(
        text: PropSource<String>,
        role: PropSource<String>,
        visited: PropSource<bool>,
        focus_id: Option<FocusId>,
        on_click: Option<String>,
    ) -> Self {
        Self { text, role, visited, focus_id, on_click, pressed: false, use_mnemonic: true }
    }

    fn build(&self, vm: &dyn ViewModel) -> LinkLabel {
        let text = self.text.resolve(vm);
        let text = if self.use_mnemonic { crate::common::mnemonic(&text).0 } else { text };
        LinkLabel::new(text)
            .role(parse_role(&self.role.resolve(vm)))
            .visited(self.visited.resolve(vm))
    }

    /// The canvas-independent half of a frame — see `ButtonNode::interact`,
    /// whose shape this mirrors: a whole-widget rectangle hit test (no
    /// `&dyn Canvas` needed) is enough for the "one implicit link" case.
    pub(crate) fn interact(&mut self, ix: &mut InteractCx<'_>, bounds: Rect) -> WidgetState {
        let hot = !ix.frame.pointer_outside() && bounds.contains(ix.frame.mouse.0, ix.frame.mouse.1);
        let (down_now, clicked) = press_release(&mut self.pressed, hot, ix.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| ix.focus.register(id, bounds)).unwrap_or_default();
        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        let clicked = clicked || ix.activate;
        if clicked {
            ix.fire("OnClick", self.focus_id, self.on_click.as_deref(), ViewEventKind::Clicked, &mut crate::node::click_args(ix.frame, bounds));
        }
        state
    }
}

impl ViewNode for LinkLabelNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let state = {
            let mut ix = cx.interact_cx();
            self.interact(&mut ix, bounds)
        };
        let raw = self.text.resolve(cx.vm);
        let _ = cx.mnemonic_text(&raw, self.use_mnemonic, crate::common::MnemonicAction::Activate);
        self.build(cx.vm).paint(cx.canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Badge
// ─────────────────────────────────────────────────────────────────────────

pub struct BadgeNode {
    pub text: PropSource<String>,
    pub variant: PropSource<String>,
    pub size: PropSource<String>,
    pub dot: PropSource<bool>,
    pub max_width: PropSource<f32>,
    /// `Solid`: the saturated colour with white text — a counter pinned on a button (the web
    /// header's unread bubble), rather than the tinted status pill.
    pub solid: PropSource<bool>,
}

impl BadgeNode {
    pub fn new(
        text: PropSource<String>,
        variant: PropSource<String>,
        size: PropSource<String>,
        dot: PropSource<bool>,
        max_width: PropSource<f32>,
    ) -> Self {
        Self { text, variant, size, dot, max_width, solid: PropSource::Literal(false) }
    }

    /// With `Solid` (see the field).
    pub fn with_solid(mut self, solid: PropSource<bool>) -> Self {
        self.solid = solid;
        self
    }

    fn build(&self, vm: &dyn ViewModel) -> Badge {
        let mut b = Badge::new(self.text.resolve(vm))
            .variant(parse_badge_variant(&self.variant.resolve(vm)))
            .size(parse_badge_size(&self.size.resolve(vm)))
            .dot(self.dot.resolve(vm));
        // `0` (the declared default) reads as "no cap" — `Badge::max_width`
        // itself has no such sentinel, so the mapping is made here.
        let max_width = self.max_width.resolve(vm);
        if max_width > 0.0 {
            b = b.max_width(max_width);
        }
        b
    }
}

impl ViewNode for BadgeNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    /// `Badge::paint` (`kubuno_desktop_ui::display`) fills a rounded pill across
    /// WHATEVER `bounds` it is handed, by design (its own doc: "When the
    /// caller sized the badge from `measure` this is identical to laying it
    /// out from the left") — a fixed-content-width pill, not a `w-full`
    /// widget, so it needs [`ViewNode::intrinsic_width`] to keep a `<Stack>`
    /// column from stretching it edge to edge.
    fn intrinsic_width(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Option<f32> {
        Some(self.build(vm).measure(c).width)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        if self.solid.resolve(cx.vm) {
            // The counter: a pill as tall as its box in the variant's saturated colour, the text
            // centred in the strong caption face, white.
            let c = cx.canvas;
            let t = c.theme();
            let ground = parse_badge_variant(&self.variant.resolve(cx.vm)).dot_colour(t);
            let h = bounds.bottom - bounds.top;
            c.fill_rounded(&bounds, h / 2.0, &ground);
            c.text(&self.text.resolve(cx.vm), &bounds, &c.formats().caption_strong, &t.accent_foreground, true);
            return;
        }
        self.build(cx.vm).paint(cx.canvas, bounds, crate::common::rest());
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Icon
// ─────────────────────────────────────────────────────────────────────────

pub struct IconNode {
    pub name: PropSource<String>,
    pub size: PropSource<f32>,
    /// `Disc`: the glyph on a tinted disc (the web `ConfirmDialog`'s), `None` for a bare glyph.
    pub disc: PropSource<String>,
}

impl IconNode {
    pub fn new(name: PropSource<String>, size: PropSource<f32>) -> Self {
        Self { name, size, disc: PropSource::Literal("None".to_string()) }
    }

    /// The glyph on a disc of the icon's size (`Disc`).
    pub fn with_disc(mut self, disc: PropSource<String>) -> Self {
        self.disc = disc;
        self
    }

    fn build(&self, vm: &dyn ViewModel) -> Icon {
        Icon::sized(crate::icon::resolve_or(&self.name.resolve(vm), ""), self.size.resolve(vm).max(0.0))
    }
}

impl ViewNode for IconNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let disc = self.disc.resolve(cx.vm);
        if disc == "None" || disc.is_empty() {
            self.build(cx.vm).paint(cx.canvas, bounds, crate::common::rest());
            return;
        }
        // The web `ConfirmDialog`'s disc (`w-12 h-12 rounded-full` around a `w-6 h-6` glyph), in
        // the tokens its palette steps stand for.
        let t = cx.canvas.theme();
        let (ground, ink) = match disc.as_str() {
            "Info" => (t.accent_light, t.accent),
            "Warning" => (t.warning_light, t.caution),
            "Danger" => (t.danger_light, t.danger),
            "Success" => (t.success_light, t.success),
            _ => (t.surface_2, t.text_secondary),
        };
        let d = (bounds.right - bounds.left).min(bounds.bottom - bounds.top);
        let (mx, my) = ((bounds.left + bounds.right) / 2.0, (bounds.top + bounds.bottom) / 2.0);
        let r = Rect::new(mx - d / 2.0, my - d / 2.0, mx + d / 2.0, my + d / 2.0);
        cx.canvas.fill_rounded(&r, d / 2.0, &ground);
        if let Some(name) = crate::icon::resolve(&self.name.resolve(cx.vm)) {
            cx.canvas.vector_icon(name, &r, d / 2.0, &ink);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Separator
// ─────────────────────────────────────────────────────────────────────────

pub struct SeparatorNode {
    pub orientation: PropSource<String>,
}

impl SeparatorNode {
    pub fn new(orientation: PropSource<String>) -> Self {
        Self { orientation }
    }

    fn build(&self, vm: &dyn ViewModel) -> Separator {
        match parse_orientation(&self.orientation.resolve(vm)) {
            Orientation::Horizontal => Separator::horizontal(),
            Orientation::Vertical => Separator::vertical(),
        }
    }
}

impl ViewNode for SeparatorNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        self.build(cx.vm).paint(cx.canvas, bounds, crate::common::rest());
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Spinner — animates itself; see `advance_phase`'s doc for why the tick is
// an assumption rather than a read host clock.
// ─────────────────────────────────────────────────────────────────────────

pub struct SpinnerNode {
    pub size: PropSource<String>,
    turn: f32,
}

impl SpinnerNode {
    pub fn new(size: PropSource<String>) -> Self {
        Self { size, turn: 0.0 }
    }

    fn build(&self, vm: &dyn ViewModel) -> Spinner {
        Spinner::new().with_size(parse_spinner_size(&self.size.resolve(vm))).with_phase(self.turn)
    }
}

impl ViewNode for SpinnerNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        self.build(cx.vm).paint(cx.canvas, bounds, crate::common::rest());
        // A rotation wraps (see `Spinner::phase`'s own doc), so this simply
        // keeps turning frame over frame.
        self.turn = advance_phase(self.turn, Spinner::PERIOD_MS);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// ProgressBar
// ─────────────────────────────────────────────────────────────────────────

pub struct ProgressBarNode {
    pub min: PropSource<f32>,
    pub max: PropSource<f32>,
    pub value: PropSource<f32>,
    pub indeterminate: PropSource<bool>,
    pub label: PropSource<String>,
    pub show_value: PropSource<bool>,
    pub variant: PropSource<String>,
    pub size: PropSource<String>,
    phase: f32,
}

impl ProgressBarNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        min: PropSource<f32>,
        max: PropSource<f32>,
        value: PropSource<f32>,
        indeterminate: PropSource<bool>,
        label: PropSource<String>,
        show_value: PropSource<bool>,
        variant: PropSource<String>,
        size: PropSource<String>,
    ) -> Self {
        Self { min, max, value, indeterminate, label, show_value, variant, size, phase: 0.0 }
    }

    fn build(&self, vm: &dyn ViewModel) -> ProgressBar {
        let mut p = ProgressBar::new()
            .with_variant(parse_progress_variant(&self.variant.resolve(vm)))
            .with_size(parse_progress_size(&self.size.resolve(vm)))
            .with_value_shown(self.show_value.resolve(vm));
        let label = self.label.resolve(vm);
        if !label.is_empty() {
            p = p.with_label(label);
        }
        p.set_minimum(self.min.resolve(vm).round() as i32);
        p.set_maximum(self.max.resolve(vm).round() as i32);
        p.set_value(self.value.resolve(vm).round() as i32);
        p.set_indeterminate(self.indeterminate.resolve(vm));
        p.phase = self.phase;
        p
    }
}

impl ViewNode for ProgressBarNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let p = self.build(cx.vm);
        let indeterminate = p.indeterminate();
        p.paint(cx.canvas, bounds, crate::common::rest());
        if indeterminate {
            self.phase = advance_phase(self.phase, ProgressBar::SLIDE_MS);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Callout — a single widget with two interactive sub-regions (the inline
// action, the dismiss button). Both `Callout::part_at` and
// `EmptyState::action_rects` below need a `&dyn Canvas` to lay the parts out,
// which `InteractCx` does not carry (see `crate::node::PaintCx::interact_cx`'s
// own doc: it is the CANVAS-INDEPENDENT half of a frame, built for leaves like
// `Button`/`Switch`/`LinkLabelNode` that do not need one). So — unlike every
// other node in this file — the hit test happens directly in `paint`, against
// `cx.canvas`, rather than through `interact_cx()`.
// ─────────────────────────────────────────────────────────────────────────

pub struct CalloutNode {
    pub body: PropSource<String>,
    pub title: PropSource<String>,
    pub variant: PropSource<String>,
    pub dismissible: PropSource<bool>,
    pub show_icon: PropSource<bool>,
    pub action_label: PropSource<String>,
    pub focus_id: Option<FocusId>,
    pub on_action: Option<String>,
    pub on_dismiss: Option<String>,
    action_pressed: bool,
    dismiss_pressed: bool,
}

impl CalloutNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        body: PropSource<String>,
        title: PropSource<String>,
        variant: PropSource<String>,
        dismissible: PropSource<bool>,
        show_icon: PropSource<bool>,
        action_label: PropSource<String>,
        focus_id: Option<FocusId>,
        on_action: Option<String>,
        on_dismiss: Option<String>,
    ) -> Self {
        Self {
            body,
            title,
            variant,
            dismissible,
            show_icon,
            action_label,
            focus_id,
            on_action,
            on_dismiss,
            action_pressed: false,
            dismiss_pressed: false,
        }
    }

    fn build(&self, vm: &dyn ViewModel) -> Callout {
        let mut c = Callout::new(self.body.resolve(vm)).with_variant(parse_callout_variant(&self.variant.resolve(vm)));
        let title = self.title.resolve(vm);
        if !title.is_empty() {
            c = c.with_title(title);
        }
        let label = self.action_label.resolve(vm);
        if !label.is_empty() {
            c = c.with_action(CalloutAction::new(label));
        }
        c = c.with_dismiss(self.dismissible.resolve(vm));
        if !self.show_icon.resolve(vm) {
            c = c.without_icon();
        }
        c
    }
}

impl ViewNode for CalloutNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    /// `Callout::measure` (`kubuno_desktop_ui::feedback`) is `w-full`: it has no
    /// intrinsic width of its own, so its UNCONSTRAINED [`Self::measure`]
    /// wraps its body into the replica's own designer-default width — almost
    /// never what a `<Stack>` column is about to give it (every block is the
    /// full row wide), which is how a callout used to claim a wildly wrong
    /// height and swallow whatever came after it. `Callout::height_at`
    /// already exists for exactly this ("What a caller that knows its
    /// column … should reserve") — this is that caller.
    fn measure_for_width(&self, c: &dyn Canvas, vm: &dyn ViewModel, width: f32) -> Size {
        let callout = self.build(vm);
        Size::new(width, callout.height_at(c, width))
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let probe = self.build(cx.vm);
        let part = if cx.frame.pointer_outside() {
            None
        } else {
            probe.part_at(cx.canvas, bounds, cx.frame.mouse.0, cx.frame.mouse.1)
        };
        let action_hot = part == Some(CalloutPart::Action);
        let dismiss_hot = part == Some(CalloutPart::Dismiss);
        let (action_down, action_clicked) = press_release(&mut self.action_pressed, action_hot, cx.frame.mouse_down);
        let (dismiss_down, dismiss_clicked) = press_release(&mut self.dismiss_pressed, dismiss_hot, cx.frame.mouse_down);
        if action_clicked {
            cx.fire("OnAction", self.focus_id, self.on_action.as_deref(), ViewEventKind::Clicked, &mut EmptyEventArgs);
        }
        if dismiss_clicked {
            cx.fire("OnDismiss", self.focus_id, self.on_dismiss.as_deref(), ViewEventKind::Clicked, &mut EmptyEventArgs);
        }
        // Re-resolved AFTER firing, so a handler's write-back shows up this
        // same frame — the reasoning `ButtonNode::paint` documents.
        let mut callout = self.build(cx.vm);
        callout.action_state = Some(crate::common::rest().hot(action_hot).pressed(action_down));
        callout.dismiss_state = Some(crate::common::rest().hot(dismiss_hot).pressed(dismiss_down));
        callout.paint(cx.canvas, bounds, crate::common::rest());
    }
}

// ─────────────────────────────────────────────────────────────────────────
// EmptyState — same two-region-hit-testing shape as `CalloutNode` above, over
// `EmptyState::action_rects` instead of `Callout::part_at`. The web's third
// interactive part (`doc_label`, a documentation link) is left display-only
// here: it is pure sugar over the other two actions, and — with `Callout`'s
// dismiss/action pair already exercising the "hit-test inside `paint`" shape
// once — adding a third geometrically distinct region was not worth the
// duplication for this family's first pass. `EmptyState::with_doc` is simply
// never called.
// ─────────────────────────────────────────────────────────────────────────

pub struct EmptyStateNode {
    pub icon: PropSource<String>,
    pub title: PropSource<String>,
    pub description: PropSource<String>,
    pub variant: PropSource<String>,
    pub compact: PropSource<bool>,
    pub action_label: PropSource<String>,
    pub secondary_action_label: PropSource<String>,
    pub focus_id: Option<FocusId>,
    pub on_action: Option<String>,
    pub on_secondary_action: Option<String>,
    action_pressed: bool,
    secondary_pressed: bool,
}

impl EmptyStateNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        icon: PropSource<String>,
        title: PropSource<String>,
        description: PropSource<String>,
        variant: PropSource<String>,
        compact: PropSource<bool>,
        action_label: PropSource<String>,
        secondary_action_label: PropSource<String>,
        focus_id: Option<FocusId>,
        on_action: Option<String>,
        on_secondary_action: Option<String>,
    ) -> Self {
        Self {
            icon,
            title,
            description,
            variant,
            compact,
            action_label,
            secondary_action_label,
            focus_id,
            on_action,
            on_secondary_action,
            action_pressed: false,
            secondary_pressed: false,
        }
    }

    fn build(&self, vm: &dyn ViewModel) -> EmptyState {
        let icon = crate::icon::resolve_or(&self.icon.resolve(vm), "");
        let mut e = EmptyState::new(icon, self.title.resolve(vm))
            .with_variant(parse_empty_state_variant(&self.variant.resolve(vm)))
            .with_compact(self.compact.resolve(vm));
        let description = self.description.resolve(vm);
        if !description.is_empty() {
            e = e.with_description(description);
        }
        let action = self.action_label.resolve(vm);
        if !action.is_empty() {
            e = e.with_action(kubuno_desktop_ui::buttons::Button::new(&action).size(kubuno_desktop_ui::buttons::Size::Sm));
        }
        let secondary = self.secondary_action_label.resolve(vm);
        if !secondary.is_empty() {
            e = e.with_secondary_action(kubuno_desktop_ui::buttons::Button::new(&secondary).size(kubuno_desktop_ui::buttons::Size::Sm));
        }
        e
    }
}

impl ViewNode for EmptyStateNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let probe = self.build(cx.vm);
        let (mx, my) = cx.frame.mouse;
        let mut main_hot = false;
        let mut secondary_hot = false;
        if !cx.frame.pointer_outside() {
            for (slot, rect) in probe.action_rects(cx.canvas, bounds) {
                if rect.contains(mx, my) {
                    if slot == 0 {
                        main_hot = true;
                    } else {
                        secondary_hot = true;
                    }
                }
            }
        }
        let (main_down, main_clicked) = press_release(&mut self.action_pressed, main_hot, cx.frame.mouse_down);
        let (secondary_down, secondary_clicked) = press_release(&mut self.secondary_pressed, secondary_hot, cx.frame.mouse_down);
        if main_clicked {
            cx.fire("OnAction", self.focus_id, self.on_action.as_deref(), ViewEventKind::Clicked, &mut EmptyEventArgs);
        }
        if secondary_clicked {
            cx.fire("OnSecondaryAction", self.focus_id, self.on_secondary_action.as_deref(), ViewEventKind::Clicked, &mut EmptyEventArgs);
        }
        let mut e = self.build(cx.vm);
        e.action_state[0] = Some(crate::common::rest().hot(main_hot).pressed(main_down));
        e.action_state[1] = Some(crate::common::rest().hot(secondary_hot).pressed(secondary_down));
        e.paint(cx.canvas, bounds, crate::common::rest());
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Metadata table
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: label,
    name: "Label",
    // Note: A text run at one of the five type-scale steps (`kubuno_desktop_ui::display::Label`).
    doc: "A text label.",
    ctor: kubuno_desktop_ui::display::Label::new("Titre"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text displayed."),
        PropertyMeta::new("Role",
            PropKind::Enum(&["Micro", "Meta", "Body", "Heading", "Title", "Page", "PageAdmin", "Badge", "Caption", "Subtitle", "Display"]),
            "Body",
            "Text style: small, caption, body, heading, title, page title, administration page title, badge, caption, subtitle or display.",
        ),
        PropertyMeta::new("TextAlign",
            PropKind::Enum(&[
                "TopLeft", "TopCenter", "TopRight", "MiddleLeft", "MiddleCenter", "MiddleRight", "BottomLeft",
                "BottomCenter", "BottomRight",
            ]),
            "TopLeft",
            "Position of the text inside the label.",
        ).aliases(&["Align"]),
        PropertyMeta::new("Overflow",
            PropKind::Enum(&["Ellipsis", "Clip", "Wrap"]),
            "Ellipsis",
            "What happens to text that does not fit: ellipsis, clipped, or wrapped onto several lines.",
        ),
    ],
    events: [],
    smoke: |l| {
        let l = l.role(kubuno_desktop_ui::display::Role::Heading);
        let l = l.align(kubuno_desktop_ui::ContentAlignment::MiddleCenter);
        l.overflow(kubuno_desktop_ui::display::TextOverflow::Wrap)
    },
    build: |props, cx| {
        let text = props.str("Text", "")?;
        let role = props.enum_("Role", "Body")?;
        let align = props.enum_("TextAlign", "TopLeft")?;
        let overflow = props.enum_("Overflow", "Ellipsis")?;
        let mut node = super::LabelNode::new(text, role, align, overflow);
        let literal = |name: &str| crate::common::literal_attr(props, name);
        node.use_mnemonic = literal("UseMnemonic").is_none_or(|v| v != "false");
        node.image = literal("Image").map(|p| {
            (crate::common::resolve_path(p, cx.base_dir.as_deref()), literal("ImageAlign").and_then(|a| crate::common::content_alignment(&a)).unwrap_or(kubuno_desktop_ui::ContentAlignment::MiddleCenter))
        });
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: link_label,
    name: "LinkLabel",
    // Note: A hyperlink-styled label (`kubuno_desktop_ui::display::LinkLabel`).
    doc: "A clickable link.",
    ctor: kubuno_desktop_ui::display::LinkLabel::new("En savoir plus"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text of the link."),
        PropertyMeta::new("Role",
            PropKind::Enum(&["Micro", "Meta", "Body", "Heading", "Title", "Page", "PageAdmin", "Badge", "Caption", "Subtitle", "Display"]),
            "Body",
            "Text style: small, caption, body, heading, title, page title, administration page title, badge, caption, subtitle or display.",
        ),
        // Note: Marks the link visited (the replica's `LinkVisited`).
        PropertyMeta::new("Visited", PropKind::Bool, "false", "Shows the link as already visited."),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the link is clicked.").args::<crate::events::MouseEventArgs>(),
    ],
    smoke: |l| {
        let l = l.role(kubuno_desktop_ui::display::Role::Meta);
        l.visited(true)
    },
    build: |props, _cx| {
        let text = props.str("Text", "")?;
        let role = props.enum_("Role", "Body")?;
        let visited = props.bool("Visited", false)?;
        let focus_id = props.focus_id();
        let on_click = props.event("OnClick");
        let mut node = super::LinkLabelNode::new(text, role, visited, focus_id, on_click);
        node.use_mnemonic = crate::common::literal_attr(props, "UseMnemonic").is_none_or(|v| v != "false");
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: badge,
    name: "Badge",
    // Note: A counting / status pill (`kubuno_desktop_ui::display::Badge`).
    doc: "A small pill showing a count or a status.",
    ctor: kubuno_desktop_ui::display::Badge::new("12"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text of the badge."),
        PropertyMeta::new("Variant",
            PropKind::Enum(&["Default", "Primary", "Success", "Warning", "Danger", "Neutral"]),
            "Default",
            "Colour of the badge.",
        ),
        PropertyMeta::new("Size", PropKind::Enum(&["Sm", "Md"]), "Md", "Size of the badge."),
        PropertyMeta::new("Dot", PropKind::Bool, "false", "Shows a coloured status dot before the text."),
        PropertyMeta::new("Solid", PropKind::Bool, "false", "Fills the badge with the saturated colour of its variant, its text in white: a counter pinned on a button (unread notifications).").category("Appearance"),
        // Note: The widest the pill may grow, in DIP, before its text ellipsizes inside it. `0` means no cap.
        PropertyMeta::new("MaxWidth", PropKind::F32, "0",
            "Maximum width in pixels before the text is shortened. 0 means no limit.",
        ),
    ],
    events: [],
    smoke: |b| {
        let b = b.variant(kubuno_desktop_ui::display::BadgeVariant::Success);
        let b = b.size(kubuno_desktop_ui::display::BadgeSize::Sm);
        let b = b.dot(true);
        b.max_width(120.0)
    },
    build: |props, _cx| {
        let text = props.str("Text", "")?;
        let variant = props.enum_("Variant", "Default")?;
        let size = props.enum_("Size", "Md")?;
        let dot = props.bool("Dot", false)?;
        let max_width = props.f32("MaxWidth", 0.0)?;
        let solid = props.bool("Solid", false)?;
        Ok(Box::new(super::BadgeNode::new(text, variant, size, dot, max_width).with_solid(solid)) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: icon,
    name: "Icon",
    // Note: A vector glyph from the shared icon set (`kubuno_desktop_ui::display::Icon`).
    doc: "An icon.",
    ctor: kubuno_desktop_ui::display::Icon::new("check"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Name", PropKind::String, "",
            "The icon: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.",
        ).editor("icon").category("Icon"),
        PropertyMeta::new("Size", PropKind::F32, "20", "Size of the icon, in pixels."),
        PropertyMeta::new("Disc", PropKind::Enum(&["None", "Neutral", "Info", "Warning", "Danger", "Success"]), "None",
            "Draws the icon on a coloured disc, like the icons of Kubuno's message boxes and confirmations.",
        ),
    ],
    events: [],
    smoke: |i| { i.tint(None) },
    build: |props, _cx| {
        let name = props.str("Name", "")?;
        let size = props.f32("Size", 20.0)?;
        let disc = props.enum_("Disc", "None")?;
        Ok(Box::new(super::IconNode::new(name, size).with_disc(disc)) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: separator,
    name: "Separator",
    // Note: A hairline rule (`kubuno_desktop_ui::display::Separator`).
    doc: "A thin separating line.",
    ctor: kubuno_desktop_ui::display::Separator::horizontal(),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Orientation", PropKind::Enum(&["Horizontal", "Vertical"]), "Horizontal",
            "Whether the line is horizontal or vertical.",
        ),
    ],
    events: [],
    smoke: |_s| { kubuno_desktop_ui::display::Separator::vertical() },
    build: |props, _cx| {
        let orientation = props.enum_("Orientation", "Horizontal")?;
        Ok(Box::new(super::SeparatorNode::new(orientation)) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: spinner,
    name: "Spinner",
    // Note: An indeterminate activity ring (`kubuno_desktop_ui::feedback::Spinner`).
    doc: "An animated loading indicator.",
    ctor: kubuno_desktop_ui::feedback::Spinner::new(),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Size", PropKind::Enum(&["Xs", "Sm", "Md", "Lg"]), "Md", "Size of the indicator."),
    ],
    events: [],
    smoke: |s| { s.with_size(kubuno_desktop_ui::feedback::SpinnerSize::Lg) },
    build: |props, _cx| {
        let size = props.enum_("Size", "Md")?;
        Ok(Box::new(super::SpinnerNode::new(size)) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: progress_bar,
    name: "ProgressBar",
    // Note: A determinate or indeterminate progress track (`kubuno_desktop_ui::range::ProgressBar`).
    doc: "A progress bar.",
    ctor: kubuno_desktop_ui::range::ProgressBar::new(),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Minimum", PropKind::F32, "0", "Value at which the bar is empty.").aliases(&["Min"]),
        PropertyMeta::new("Maximum", PropKind::F32, "100", "Value at which the bar is full.").aliases(&["Max"]),
        // Note: The current value, clamped to `Minimum..=Maximum`.
        PropertyMeta::new("Value", PropKind::F32, "0", "Current progress, between Minimum and Maximum."),
        // Note: Unknown progress: a travelling sliver instead of a fill (the replica's `Marquee` style).
        PropertyMeta::new("Indeterminate", PropKind::Bool, "false",
            "Shows an animation instead of a value, when the progress is unknown.",
        ),
        PropertyMeta::new("Label", PropKind::String, "", "Text shown above the bar. Leave empty for none."),
        PropertyMeta::new("ShowValue", PropKind::Bool, "false", "Shows the percentage above the bar."),
        // Note: The fill's colour. `Auto` goes amber then red past the design system's own quota thresholds.
        PropertyMeta::new("Variant",
            PropKind::Enum(&["Auto", "Primary", "Success", "Warning", "Danger"]),
            "Auto",
            "Colour of the bar. Auto turns amber, then red, as it fills up.",
        ),
        PropertyMeta::new("Size", PropKind::Enum(&["Sm", "Md"]), "Md", "Thickness of the bar."),
    ],
    events: [],
    smoke: |mut p| {
        p.set_minimum(0);
        p.set_maximum(10);
        p.set_value(4);
        let p = p.with_variant(kubuno_desktop_ui::range::ProgressVariant::Success);
        let p = p.with_size(kubuno_desktop_ui::range::ProgressSize::Sm);
        let p = p.with_label("Stockage");
        p.with_value_shown(true)
    },
    build: |props, _cx| {
        let min = props.f32("Minimum", 0.0)?;
        let max = props.f32("Maximum", 100.0)?;
        let value = props.f32("Value", 0.0)?;
        let indeterminate = props.bool("Indeterminate", false)?;
        let label = props.str("Label", "")?;
        let show_value = props.bool("ShowValue", false)?;
        let variant = props.enum_("Variant", "Auto")?;
        let size = props.enum_("Size", "Md")?;
        Ok(Box::new(super::ProgressBarNode::new(min, max, value, indeterminate, label, show_value, variant, size)) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: callout,
    name: "Callout",
    // Note: An inline severity banner (`kubuno_desktop_ui::feedback::Callout`).
    doc: "A message banner: information, success, warning or error.",
    ctor: kubuno_desktop_ui::feedback::Callout::new("Un message."),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Body", PropKind::String, "", "Message text."),
        PropertyMeta::new("Title", PropKind::String, "", "Bold title shown above the message. Leave empty for none."),
        PropertyMeta::new("Variant",
            PropKind::Enum(&["Info", "Success", "Warning", "Danger"]),
            "Info",
            "Kind of message, which sets the colour and the icon.",
        ),
        PropertyMeta::new("Dismissible", PropKind::Bool, "false", "Shows a button to close the banner."),
        PropertyMeta::new("ShowIcon", PropKind::Bool, "true", "Shows the icon of the message kind."),
        PropertyMeta::new("ActionLabel", PropKind::String, "",
            "Text of an action button in the banner. Leave empty for none.",
        ),
    ],
    events: [
        EventMeta::new("OnAction", "Occurs when the action button is clicked."),
        // Note: Raised when the close button is activated (only when `Dismissible`).
        EventMeta::new("OnDismiss", "Occurs when the banner is closed."),
    ],
    smoke: |c| {
        let c = c.with_variant(kubuno_desktop_ui::feedback::CalloutVariant::Warning);
        let c = c.with_title("Attention");
        let c = c.with_action(kubuno_desktop_ui::feedback::CalloutAction::new("Réessayer"));
        let c = c.with_dismiss(true);
        c.without_icon()
    },
    build: |props, _cx| {
        let body = props.str("Body", "")?;
        let title = props.str("Title", "")?;
        let variant = props.enum_("Variant", "Info")?;
        let dismissible = props.bool("Dismissible", false)?;
        let show_icon = props.bool("ShowIcon", true)?;
        let action_label = props.str("ActionLabel", "")?;
        let focus_id = props.focus_id();
        let on_action = props.event("OnAction");
        let on_dismiss = props.event("OnDismiss");
        Ok(Box::new(super::CalloutNode::new(
            body, title, variant, dismissible, show_icon, action_label, focus_id, on_action, on_dismiss,
        )) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: empty_state,
    name: "EmptyState",
    // Note: The centred « there is nothing here » block (`kubuno_desktop_ui::feedback::EmptyState`).
    doc: "A placeholder shown when an area has nothing to display.",
    ctor: kubuno_desktop_ui::feedback::EmptyState::new("Inbox", "Rien ici"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Icon", PropKind::String, "",
            "Icon shown at the top: a name of the Kubuno icon set, or an image file relative to the view.",
        ).editor("icon").category("Icon"),
        PropertyMeta::new("Title", PropKind::String, "", "Title of the message."),
        PropertyMeta::new("Description", PropKind::String, "", "Secondary text shown under the title."),
        // Note: Why the area is empty — only `FirstUse` gets a primary-styled main action.
        PropertyMeta::new("Variant",
            PropKind::Enum(&["FirstUse", "NoResults", "Error", "Unavailable"]),
            "FirstUse",
            "Why the area is empty: first use, no results, error or unavailable.",
        ),
        PropertyMeta::new("Compact", PropKind::Bool, "false", "Uses less vertical space, for a small area."),
        PropertyMeta::new("ActionLabel", PropKind::String, "",
            "Text of the main action button. Leave empty for none.",
        ),
        PropertyMeta::new("SecondaryActionLabel", PropKind::String, "",
            "Text of the secondary action button. Leave empty for none.",
        ),
    ],
    events: [
        EventMeta::new("OnAction", "Occurs when the main action button is clicked."),
        EventMeta::new("OnSecondaryAction", "Occurs when the secondary action button is clicked."),
    ],
    smoke: |e| {
        let e = e.with_variant(kubuno_desktop_ui::feedback::EmptyStateVariant::NoResults);
        let e = e.with_description("Essayez d'élargir la recherche.");
        e.with_compact(true)
    },
    build: |props, _cx| {
        let icon = props.str("Icon", "")?;
        let title = props.str("Title", "")?;
        let description = props.str("Description", "")?;
        let variant = props.enum_("Variant", "FirstUse")?;
        let compact = props.bool("Compact", false)?;
        let action_label = props.str("ActionLabel", "")?;
        let secondary_action_label = props.str("SecondaryActionLabel", "")?;
        let focus_id = props.focus_id();
        let on_action = props.event("OnAction");
        let on_secondary_action = props.event("OnSecondaryAction");
        Ok(Box::new(super::EmptyStateNode::new(
            icon, title, description, variant, compact, action_label, secondary_action_label, focus_id, on_action,
            on_secondary_action,
        )) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: paint_box,
    name: "PaintBox",
    // Note: A surface drawn by its Paint handler with the Graphics API (EVT-8; the design note's `<Canvas>` — `Canvas` names the drawing trait).
    doc: "A drawing surface: your Paint handler draws on it with e.graphics() (lines, shapes, gradients, text, images).",
    ctor: kubuno_desktop_ui::graphics::Graphics::recorder(),
    children: ChildrenModel::None,
    default_event: "OnPaint",
    props: [],
    events: [
        EventMeta::new("OnPaint", "Occurs when the surface is drawn: draw with e.graphics() in e.clip_rectangle.").category(crate::registry::EventCategory::Appearance).args::<crate::events::PaintEventArgs>(),
    ],
    smoke: |g| {
        g.fill_rectangle(kubuno_desktop_ui::graphics::Color::BLACK, kubuno_desktop_ui::Rect::new(0.0, 0.0, 1.0, 1.0));
        g
    },
    build: |props, _cx| {
        Ok(Box::new(crate::node::custom::PaintBoxNode::new(props.event("OnPaint"))) as Box<dyn ViewNode>)
    },
}

/// Every component this family declares, in declaration order.
pub const ALL: &[ComponentMeta] = &[
    label::META,
    link_label::META,
    badge::META,
    icon::META,
    separator::META,
    spinner::META,
    progress_bar::META,
    callout::META,
    empty_state::META,
    paint_box::META,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AstNode, Document, Element};
    use crate::binding::{HandlerTable, MapViewModel, Value};
    use crate::compile::compile;
    use crate::handlers;
    use crate::registry;
    use crate::syntax::parse;
    use kubuno_desktop_controls::host::{Frame, Modifiers};

    fn element(src: &str) -> Element {
        let p = parse(src);
        let doc = Document::cast(p.syntax()).unwrap();
        doc.root_element().unwrap()
    }

    // ── Compiles end to end, through the real registry ──────────────────

    #[test]
    fn every_display_component_compiles_with_its_declared_defaults() {
        // Each as the lone child of `<Stack>` — `Card`/`Stack` are the
        // unconditional phase-2a components, always in the registry, so this
        // exercises real nesting without depending on another feature-gated
        // family.
        let src = r#"
            <Card Title="Galerie">
              <Stack Direction="TopDown" Gap="8">
                <Label Text="Titre" Role="Heading" Align="MiddleCenter" Overflow="Wrap"/>
                <LinkLabel x:Name="doc" Text="En savoir plus" Visited="true" OnClick="doc_clicked"/>
                <Badge Text="12" Variant="Success" Size="Sm" Dot="true" MaxWidth="80"/>
                <Icon Name="check" Size="24"/>
                <Separator Orientation="Vertical"/>
                <Spinner Size="Lg"/>
                <ProgressBar Min="0" Max="10" Value="4" ShowValue="true" Variant="Warning" Size="Sm"/>
                <Callout Body="Un souci" Title="Attention" Variant="Warning" Dismissible="true"
                         ActionLabel="Réessayer" OnAction="retry" OnDismiss="dismissed"/>
                <EmptyState Icon="Inbox" Title="Rien ici" Description="Ajoutez un élément."
                            ActionLabel="Ajouter" SecondaryActionLabel="Importer"
                            OnAction="add" OnSecondaryAction="import"/>
              </Stack>
            </Card>
        "#;
        let view = compile(src).unwrap();
        let _root: &dyn ViewNode = view.root.as_ref();
    }

    #[test]
    fn every_declared_component_is_findable_and_registered() {
        for meta in ALL {
            assert_eq!(registry::lookup(meta.name).map(|m| m.name), Some(meta.name));
        }
        assert_eq!(ALL.len(), 10, "the ten components this family owns");
    }

    // ── Diagnostics: enum / bool / f32, with a byte range ────────────────

    #[test]
    fn bad_enum_value_is_a_diagnostic_with_a_range() {
        let err = match compile(r#"<Label Role="Huge"/>"#) {
            Ok(_) => panic!("expected an unknown `Role` value to fail"),
            Err(d) => d,
        };
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("Huge"), "{:?}", err[0]);
        assert!(err[0].message.contains("Heading"), "{:?}", err[0]);
        assert!(err[0].line >= 1 && err[0].column >= 1);
    }

    #[test]
    fn bad_bool_value_is_a_diagnostic() {
        let err = match compile(r#"<Callout Body="x" Dismissible="peut-etre"/>"#) {
            Ok(_) => panic!("expected an unknown `Dismissible` value to fail"),
            Err(d) => d,
        };
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("true"), "{:?}", err[0]);
    }

    #[test]
    fn bad_f32_value_is_a_diagnostic() {
        let err = match compile(r#"<ProgressBar Value="beaucoup"/>"#) {
            Ok(_) => panic!("expected an unknown `Value` value to fail"),
            Err(d) => d,
        };
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("expected a number"), "{:?}", err[0]);
    }

    // ── Props: defaults and typed reads, direct against `Props` ─────────

    #[test]
    fn label_defaults_match_the_declared_metadata() {
        let el = element(r#"<Label/>"#);
        let meta = registry::lookup("Label").unwrap();
        let props = crate::props::Props::new(&el, meta);
        assert!(matches!(props.str("Text", "").unwrap(), PropSource::Literal(ref s) if s.is_empty()));
        assert!(matches!(props.enum_("Role", "Body").unwrap(), PropSource::Literal(ref s) if s == "Body"));
        assert!(matches!(props.enum_("Align", "TopLeft").unwrap(), PropSource::Literal(ref s) if s == "TopLeft"));
        assert!(matches!(props.enum_("Overflow", "Ellipsis").unwrap(), PropSource::Literal(ref s) if s == "Ellipsis"));
    }

    #[test]
    fn progress_bar_reads_its_typed_properties() {
        let el = element(r#"<ProgressBar Min="0" Max="10" Value="4" Indeterminate="true" ShowValue="true"/>"#);
        let meta = registry::lookup("ProgressBar").unwrap();
        let props = crate::props::Props::new(&el, meta);
        assert!(matches!(props.f32("Maximum", 100.0).unwrap(), PropSource::Literal(v) if v == 10.0));
        assert!(matches!(props.f32("Value", 0.0).unwrap(), PropSource::Literal(v) if v == 4.0));
        assert!(matches!(props.bool("Indeterminate", false).unwrap(), PropSource::Literal(true)));
        assert!(matches!(props.bool("ShowValue", false).unwrap(), PropSource::Literal(true)));
    }

    #[test]
    fn empty_state_action_labels_are_read_as_string_props() {
        let el = element(r#"<EmptyState Icon="Inbox" Title="Rien" ActionLabel="Ajouter" OnAction="add"/>"#);
        let meta = registry::lookup("EmptyState").unwrap();
        let props = crate::props::Props::new(&el, meta);
        assert!(matches!(props.str("ActionLabel", "").unwrap(), PropSource::Literal(ref s) if s == "Ajouter"));
        assert_eq!(props.event("OnAction").as_deref(), Some("add"));
        assert_eq!(props.event("OnSecondaryAction"), None);
    }

    // ── Events: the one leaf whose interaction is canvas-independent ────

    fn frame_at(mouse: (f32, f32), mouse_down: bool) -> Frame {
        Frame {
            size: (400.0, 300.0),
            mouse,
            mouse_down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 400.0, 300.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: 0,
            window_focused: true,
        }
    }

    #[test]
    fn link_label_click_dispatches_its_handler_and_fires_an_event() {
        let mut node = LinkLabelNode::new(
            PropSource::Literal("En savoir plus".to_string()),
            PropSource::Literal("Body".to_string()),
            PropSource::Literal(false),
            None,
            Some("doc_clicked".to_string()),
        );
        let handlers: HandlerTable = handlers! {
            "doc_clicked" => |vm, _v| { vm.set("Opened", Value::Bool(true)); },
        };
        let mut vm = MapViewModel::new();
        let mut focus = kubuno_desktop_ui::FocusRing::new();
        let mut handlers = handlers;
        let mut events = Vec::new();
        let bounds = Rect::new(0.0, 0.0, 120.0, 20.0);

        {
            let frame = frame_at((10.0, 10.0), true);
            let mut ix = InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            node.interact(&mut ix, bounds);
        }
        assert!(vm.get("Opened").is_none(), "a press alone is not yet a click");
        {
            let frame = frame_at((10.0, 10.0), false);
            let mut ix = InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            node.interact(&mut ix, bounds);
        }

        assert_eq!(vm.get("Opened"), Some(Value::Bool(true)));
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, ViewEventKind::Clicked));
        assert_eq!(events[0].handler.as_deref(), Some("doc_clicked"));
    }

    #[test]
    fn link_label_click_outside_after_press_is_not_a_click() {
        let mut node = LinkLabelNode::new(
            PropSource::Literal("En savoir plus".to_string()),
            PropSource::Literal("Body".to_string()),
            PropSource::Literal(false),
            None,
            Some("doc_clicked".to_string()),
        );
        let mut vm = MapViewModel::new();
        let mut focus = kubuno_desktop_ui::FocusRing::new();
        let mut handlers = HandlerTable::new();
        let mut events = Vec::new();
        let bounds = Rect::new(0.0, 0.0, 120.0, 20.0);

        {
            let frame = frame_at((10.0, 10.0), true);
            let mut ix = InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            node.interact(&mut ix, bounds);
        }
        {
            let frame = frame_at((500.0, 500.0), false);
            let mut ix = InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            node.interact(&mut ix, bounds);
        }
        assert!(events.is_empty());
    }

    // ── Pure animation math (no live `Canvas` — see `compile::tests`' own
    // note on why every test here stays off one) ─────────────────────────

    #[test]
    fn advance_phase_wraps_a_full_turn() {
        let mut phase = 0.0_f32;
        for _ in 0..(1000 / 16 + 1) {
            phase = advance_phase(phase, 1000);
        }
        assert!((0.0..1.0).contains(&phase));
    }

    #[test]
    fn advance_phase_recovers_from_a_non_finite_input() {
        assert_eq!(advance_phase(f32::NAN, 1000), 0.0);
    }

    #[test]
    fn shared_icon_lookup_resolves_the_alias_and_falls_back_to_the_missing_glyph_sentinel() {
        assert_eq!(crate::icon::resolve_or("check", ""), "Check");
        assert_eq!(crate::icon::resolve_or("NotAnIcon", ""), "");
    }
}
