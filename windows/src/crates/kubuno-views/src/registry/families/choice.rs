//! Component family `choice` — declared with the `component!` table (see
//! `../macros.rs`) plus, in this same file, the view nodes those components
//! build. Compiled only with the `family-choice` feature while the families are
//! being written in parallel; the feature is on by default once integrated.
//!
//! Five components, read straight off the real builder API in
//! `kubuno-ui/src/buttons.rs` (`IconButton`, `CheckBox`, `RadioButton`) and
//! `kubuno-ui/src/range.rs` (`Slider`, `NumericField`) — the same "the tag
//! name IS the builder's name, an attribute IS a fluent setter" rule the five
//! phase-2a components (`../components.rs`) already follow. Property names
//! are `PascalCase`; enum values are the real Rust variant names.
//!
//! ## What each node does for real interaction
//!
//! * [`IconButtonNode`] — press/release like `ButtonNode`, but hit-tested as a
//!   disc (`IconButton::hit_test` is already circular; this node never
//!   re-derives that geometry, it just calls it).
//! * [`CheckBoxNode`] — a two-state bindable `Checked`, written back and
//!   `OnCheckedChanged`-dispatched on a completed click, exactly the shape
//!   `SwitchNode` already proves (same-frame write-back visibility included).
//!   The replica's three-state cycle (`Indeterminate`) is not modelled — see
//!   the module's "Deliberately deferred" note below.
//! * [`RadioButtonNode`] — mutual exclusion falls out of ordinary binding: a
//!   radio does not own a private `Checked` flag, it compares its own `Value`
//!   against a **shared** `SelectedValue` path. Two `<RadioButton Group="…"
//!   SelectedValue="{Binding Theme, Mode=TwoWay}">` elements bound to the same
//!   path exclude each other for free, with no group registry in the
//!   interpreter — `Group` itself is advisory (and doubles as a stable
//!   `FocusId` seed when no `x:Name` is given); see the property's own doc.
//! * [`SliderNode`] — drags with the real `Slider::drag_to` (itself
//!   "wherever the pointer is, clamped", not a grab-offset drag), live
//!   two-way write-back every frame the value actually moves, matching
//!   `TextFieldNode`'s "fire on every change, not just on release".
//! * [`NumericFieldNode`] — the two step buttons are real, hit-tested,
//!   pressed/released and two-way bound exactly like a click anywhere else in
//!   this crate. Free-text typing (`NumericField::begin_edit`/`commit`, the
//!   `NumericEdit` caret/selection machinery `TextFieldNode` has an analogue
//!   of for `TextField`) is **not** wired — see "Deliberately deferred".
//!
//! ## Deliberately deferred
//!
//! * `CheckBox`'s three-state cycle (`ThreeState`/`Indeterminate`) — every
//!   worked example this phase targets is a plain on/off check box; a
//!   dedicated `CheckState` enum property is mechanical to add later and would
//!   be speculative metadata today (the same posture the crate's own top-level
//!   doc takes toward Dock/Anchor layout).
//! * `NumericField`'s free-text typing, `DecimalPlaces`, `Hexadecimal` and
//!   `ThousandsSeparator` — the field always shows its non-editing
//!   `display_text()`; only the spin buttons and their two-way write-back are
//!   wired. Typing would need the same `EditInput`/focus-text plumbing
//!   `TextFieldNode` already has, applied to `NumericField::begin_edit`/
//!   `commit` — a real feature, just not one either worked example here needs,
//!   and out of scope for one family's slice of this crate.
//! * `RadioButton`'s keyboard roving tabindex (arrow-key navigation between
//!   options in a group, `kubuno_ui::buttons::radio_arrow_target`/
//!   `radio_tab_stop`) — those two free functions are exactly what a future
//!   pass would drive from a per-`Group` `FocusRing` extension; this phase
//!   gets mutual exclusion (the part every worked example needs) from the
//!   shared `SelectedValue` binding alone, with no keyboard grouping.
//! * `Slider`'s tick marks (`TickStyle`/`TickFrequency`) and value bubble
//!   (`Slider::paint_value_bubble`) — the bubble is a floating overlay
//!   (`host::overlay`) the interpreter has no concept of yet (`PaintCx` paints
//!   one rectangle, not a layered popup), and no worked example asks for
//!   ticks.

use kubuno_controls::enums::CheckState;
use kubuno_ui::buttons::{CheckBox as KCheckBox, IconButton as KIconButton, RadioButton as KRadioButton};
use kubuno_ui::range::{NumericField as KNumericField, Slider as KSlider, SpinPart};
use kubuno_ui::{Canvas, FocusId, Rect, Size, Widget, WidgetState};

use crate::binding::{PropSource, Value, ViewModel};
#[cfg(test)]
use crate::binding::BindingMode;
use crate::node::{press_release, InteractCx, PaintCx, ViewEventKind, ViewNode};
use crate::events::{ChangeSource, CheckedChangedEventArgs, NumericValueChangedEventArgs, TextChangedEventArgs};
#[allow(unused_imports)] // Used by the `component!` invocations below.
use crate::registry::macros::component;
use crate::registry::ComponentMeta;

// `press_release` (press-then-release-inside is a click) and event dispatch
// (`InteractCx::fire`) are shared with every other family now — see
// `crate::node`/`crate::icon`'s module docs — rather than each re-derived
// here.

/// Resolves an `Icon="…"` value to the `&'static str` glyph name
/// [`kubuno_controls::host`]'s `Canvas::vector_icon` requires, via the
/// shared [`crate::icon`] lookup. An unrecognised or empty name falls back
/// to a generic "more" glyph rather than leaking an owned `String` to
/// satisfy `'static`.
fn static_icon(name: &str) -> &'static str {
    crate::icon::resolve_or(name, "MoreHorizontal")
}

// ─────────────────────────────────────────────────────────────────────────
// IconButton
// ─────────────────────────────────────────────────────────────────────────

pub struct IconButtonNode {
    pub icon: PropSource<String>,
    pub diameter: PropSource<f32>,
    pub glyph: PropSource<f32>,
    pub filled: PropSource<bool>,
    pub focus_id: Option<FocusId>,
    pub on_click: Option<String>,
    /// `DropDownMenu`: the `<ContextMenu>` a click opens below the button.
    pub drop_down: Option<String>,
    pressed: bool,
}

impl IconButtonNode {
    fn build(&self, vm: &dyn ViewModel) -> KIconButton {
        let icon = static_icon(&self.icon.resolve(vm));
        let diameter = self.diameter.resolve(vm).max(1.0);
        let glyph = self.glyph.resolve(vm).max(1.0);
        let mut b = KIconButton::plain(icon, diameter, glyph);
        b.filled = self.filled.resolve(vm);
        b
    }

    /// See `ButtonNode::interact` (`crate::node`) — the same press/release
    /// shape, over a circular hit target instead of a rectangular one.
    pub(crate) fn interact(&mut self, ix: &mut InteractCx<'_>, bounds: Rect) -> WidgetState {
        let btn = self.build(ix.vm);
        let hot = !ix.frame.pointer_outside() && btn.hit_test(bounds, ix.frame.mouse.0, ix.frame.mouse.1);
        let (down_now, clicked) = press_release(&mut self.pressed, hot, ix.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| ix.focus.register(id, bounds)).unwrap_or_default();
        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        let clicked = clicked || ix.activate;
        if clicked {
            ix.fire("OnClick", self.focus_id, self.on_click.as_deref(), ViewEventKind::Clicked, &mut crate::node::click_args(ix.frame, bounds));
            if let Some(menu) = &self.drop_down {
                crate::window::show_context_menu(menu, crate::window::MenuAnchor::Below(crate::common::to_client(bounds)));
            }
        }
        state
    }
}

impl ViewNode for IconButtonNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm).measure(c)
    }

    /// A fixed-`Diameter` circle, not a `w-full` widget — see
    /// `display::BadgeNode::intrinsic_width`'s doc, the same reasoning here:
    /// without this a `<Stack>` column hands it a full-width block and the
    /// circle paints centred somewhere far from the row's left edge instead
    /// of sitting where the other children start.
    fn intrinsic_width(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Option<f32> {
        Some(self.build(vm).measure(c).width)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let state = {
            let mut ix = cx.interact_cx();
            self.interact(&mut ix, bounds)
        };
        let btn = self.build(cx.vm);
        btn.paint(cx.canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// CheckBox
// ─────────────────────────────────────────────────────────────────────────

pub struct CheckBoxNode {
    pub text: PropSource<String>,
    pub description: PropSource<String>,
    pub checked: PropSource<bool>,
    /// `CheckState` (Unchecked, Checked, Indeterminate), when written: wins over `Checked`.
    pub check_state: Option<PropSource<String>>,
    /// `AutoCheck`: a click changes the state (else only Click is raised).
    pub auto_check: bool,
    /// `ThreeState`: a click cycles Unchecked, Checked, Indeterminate.
    pub three_state: bool,
    /// `UseMnemonic`.
    pub use_mnemonic: bool,
    pub focus_id: Option<FocusId>,
    pub on_checked_changed: Option<String>,
    pressed: bool,
}

impl CheckBoxNode {
    /// The box's state this frame: `CheckState` when written, else `Checked`.
    fn state(&self, vm: &dyn ViewModel) -> CheckState {
        match self.check_state.as_ref().map(|s| s.resolve(vm)).as_deref() {
            Some("Checked") => CheckState::Checked,
            Some("Indeterminate") => CheckState::Indeterminate,
            Some(_) => CheckState::Unchecked,
            None if self.checked.resolve(vm) => CheckState::Checked,
            None => CheckState::Unchecked,
        }
    }

    fn build(&self, state: CheckState, vm: &dyn ViewModel) -> KCheckBox {
        let text = self.text.resolve(vm);
        let text = if self.use_mnemonic { crate::common::mnemonic(&text).0 } else { text };
        let b = KCheckBox::new(&text).description(&self.description.resolve(vm));
        let b = if self.three_state { b.tri_state() } else { b };
        b.check(state)
    }

    /// See `SwitchNode::interact` (`crate::node`) — the exact same two-way
    /// write-back / `OnToggled`-style dispatch shape, over `Checked` instead
    /// of `On`.
    pub(crate) fn interact(&mut self, ix: &mut InteractCx<'_>, bounds: Rect) -> WidgetState {
        let old = self.state(ix.vm);
        let checked = old == CheckState::Checked;
        let cb = self.build(old, ix.vm);
        let hot = !ix.frame.pointer_outside() && cb.hit_test(bounds, ix.frame.mouse.0, ix.frame.mouse.1);
        let (down_now, toggled) = press_release(&mut self.pressed, hot, ix.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| ix.focus.register(id, bounds)).unwrap_or_default();
        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        // A click (or its mnemonic) changes the state unless `AutoCheck` is off (WinForms).
        if (toggled || ix.activate) && self.auto_check {
            let next = match (old, self.three_state) {
                (CheckState::Unchecked, _) => CheckState::Checked,
                (CheckState::Checked, true) => CheckState::Indeterminate,
                _ => CheckState::Unchecked,
            };
            let new_checked = next == CheckState::Checked;
            if let Some(spec) = self.checked.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(ix.vm, Value::Bool(new_checked));
                }
            }
            if let Some(spec) = self.check_state.as_ref().and_then(|s| s.binding()) {
                if spec.mode.writes_back() {
                    let name = match next {
                        CheckState::Checked => "Checked",
                        CheckState::Indeterminate => "Indeterminate",
                        CheckState::Unchecked => "Unchecked",
                    };
                    spec.update_source(ix.vm, Value::Str(name.to_string()));
                }
            }
            let mut args = CheckedChangedEventArgs::new(checked, new_checked, ChangeSource::User);
            ix.fire("OnCheckedChanged", self.focus_id, self.on_checked_changed.as_deref(), ViewEventKind::Toggled(new_checked), &mut args);
        }
        state
    }
}

impl ViewNode for CheckBoxNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(self.state(vm), vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let state = {
            let mut ix = cx.interact_cx();
            self.interact(&mut ix, bounds)
        };
        let raw = self.text.resolve(cx.vm);
        let _ = cx.mnemonic_text(&raw, self.use_mnemonic, crate::common::MnemonicAction::Activate);
        // Re-resolved AFTER `interact`, for the same same-frame reason
        // `SwitchNode::paint` re-resolves `on_value` after its own `interact`.
        let checked = self.state(cx.vm);
        let cb = self.build(checked, cx.vm);
        cb.paint(cx.canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// RadioButton — mutual exclusion via a SHARED `SelectedValue` binding; see
// the module doc.
// ─────────────────────────────────────────────────────────────────────────

pub struct RadioButtonNode {
    pub text: PropSource<String>,
    pub description: PropSource<String>,
    pub value: PropSource<String>,
    pub selected_value: PropSource<String>,
    /// `AutoCheck`: a click selects the option (else only Click is raised).
    pub auto_check: bool,
    /// `UseMnemonic`.
    pub use_mnemonic: bool,
    pub focus_id: Option<FocusId>,
    pub on_checked_changed: Option<String>,
    pressed: bool,
}

impl RadioButtonNode {
    fn build(&self, selected: bool, vm: &dyn ViewModel) -> KRadioButton {
        let text = self.text.resolve(vm);
        let text = if self.use_mnemonic { crate::common::mnemonic(&text).0 } else { text };
        KRadioButton::new(&text).description(&self.description.resolve(vm)).selected(selected)
    }

    /// Selected is not a flag this node owns — it is `resolve(SelectedValue)
    /// == resolve(Value)`, so two radios bound to the same `SelectedValue`
    /// path are mutually exclusive without the interpreter ever tracking
    /// "siblings". A click on an ALREADY selected radio is a no-op — a radio
    /// does not toggle off, the same rule `kubuno_controls::buttons::
    /// select_radio` encodes for the replica.
    pub(crate) fn interact(&mut self, ix: &mut InteractCx<'_>, bounds: Rect) -> WidgetState {
        let value = self.value.resolve(ix.vm);
        let old_selected = self.selected_value.resolve(ix.vm);
        let selected_now = old_selected == value;
        let radio = self.build(selected_now, ix.vm);
        let hot = !ix.frame.pointer_outside() && radio.hit_test(bounds, ix.frame.mouse.0, ix.frame.mouse.1);
        let (down_now, clicked) = press_release(&mut self.pressed, hot, ix.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| ix.focus.register(id, bounds)).unwrap_or_default();
        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        let clicked = (clicked || ix.activate) && self.auto_check;
        if clicked && !selected_now {
            if let Some(spec) = self.selected_value.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(ix.vm, Value::Str(value.clone()));
                }
            }
            let mut args = TextChangedEventArgs::new(old_selected, value.clone(), ChangeSource::User);
            ix.fire("OnCheckedChanged", self.focus_id, self.on_checked_changed.as_deref(), ViewEventKind::Changed(value), &mut args);
        }
        state
    }
}

impl ViewNode for RadioButtonNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let value = self.value.resolve(vm);
        let selected = self.selected_value.resolve(vm) == value;
        self.build(selected, vm).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let state = {
            let mut ix = cx.interact_cx();
            self.interact(&mut ix, bounds)
        };
        let raw = self.text.resolve(cx.vm);
        let _ = cx.mnemonic_text(&raw, self.use_mnemonic, crate::common::MnemonicAction::Activate);
        let value = self.value.resolve(cx.vm);
        let selected = self.selected_value.resolve(cx.vm) == value;
        let radio = self.build(selected, cx.vm);
        radio.paint(cx.canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Slider
// ─────────────────────────────────────────────────────────────────────────

pub struct SliderNode {
    pub min: PropSource<f32>,
    pub max: PropSource<f32>,
    pub value: PropSource<f32>,
    pub step: PropSource<f32>,
    pub large_step: PropSource<f32>,
    pub focus_id: Option<FocusId>,
    pub on_value_changed: Option<String>,
    /// Whether a drag started on this slider is still in progress — kept
    /// across frames for the same reason `ButtonNode::pressed` is: the
    /// pointer may wander outside `bounds` mid-drag and the slider must keep
    /// following it, exactly like `Slider::drag_to`'s own contract (it does
    /// not require the point to be inside the rail).
    dragging: bool,
}

impl SliderNode {
    fn build(&self, vm: &dyn ViewModel, value: f32) -> KSlider {
        let mut s = KSlider::new();
        let a = self.min.resolve(vm).round() as i32;
        let b = self.max.resolve(vm).round() as i32;
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        s.set_minimum(lo);
        s.set_maximum(hi);
        let _ = s.set_small_change(self.step.resolve(vm).max(0.0).round() as i32);
        let _ = s.set_large_change(self.large_step.resolve(vm).max(0.0).round() as i32);
        let _ = s.set_value((value.round() as i32).clamp(lo, hi));
        s
    }

    /// Drags with the real `Slider::drag_to` every frame the pointer is down
    /// and either the drag already started or the press landed on the
    /// slider's hit target (`Slider::hit_test`, wider than the rail — see
    /// that type's own doc). Live write-back on every value change, the same
    /// "fire on every change, not just on release" `TextFieldNode` uses for
    /// typing.
    pub(crate) fn interact(&mut self, ix: &mut InteractCx<'_>, bounds: Rect) -> WidgetState {
        let value = self.value.resolve(ix.vm);
        let mut slider = self.build(ix.vm, value);
        let (mx, my) = ix.frame.mouse;
        let hit = !ix.frame.pointer_outside() && slider.hit_test(bounds, mx, my);
        if ix.frame.mouse_down && (self.dragging || hit) {
            self.dragging = true;
            if slider.drag_to(bounds, mx, my) {
                let new_value = slider.value() as f32;
                if let Some(spec) = self.value.binding() {
                    if spec.mode.writes_back() {
                        spec.update_source(ix.vm, Value::F32(new_value));
                    }
                }
                let mut args = NumericValueChangedEventArgs::new(value, new_value, ChangeSource::User);
                ix.fire("OnValueChanged", self.focus_id, self.on_value_changed.as_deref(), ViewEventKind::Changed(new_value.to_string()), &mut args);
            }
        } else {
            self.dragging = false;
        }
        let focus_state = self.focus_id.map(|id| ix.focus.register(id, bounds)).unwrap_or_default();
        focus_state.apply(crate::common::rest().hot(hit).pressed(self.dragging))
    }
}

impl ViewNode for SliderNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm, self.value.resolve(vm)).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let state = {
            let mut ix = cx.interact_cx();
            self.interact(&mut ix, bounds)
        };
        // Re-resolved AFTER `interact`, same-frame reason as every other
        // two-way node in this family.
        let value = self.value.resolve(cx.vm);
        let slider = self.build(cx.vm, value);
        slider.paint(cx.canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// NumericField — spin buttons only; see the module doc's "Deliberately
// deferred" for why free-text typing is not wired.
// ─────────────────────────────────────────────────────────────────────────

pub struct NumericFieldNode {
    pub min: PropSource<f32>,
    pub max: PropSource<f32>,
    pub value: PropSource<f32>,
    pub step: PropSource<f32>,
    pub invalid: PropSource<bool>,
    /// `DecimalPlaces`, `ThousandsSeparator`.
    pub decimal_places: i32,
    pub thousands: bool,
    pub focus_id: Option<FocusId>,
    pub on_value_changed: Option<String>,
    /// The step button held down, across frames — see `ButtonNode::pressed`.
    pressed_part: Option<SpinPart>,
    /// The step button the pointer is over — persisted so `paint`'s freshly
    /// rebuilt field (a SEPARATE instance from `interact`'s, per this crate's
    /// "rebuilt every frame" rule) still paints the right one lit.
    hover_part: Option<SpinPart>,
}

impl NumericFieldNode {
    fn build(&self, vm: &dyn ViewModel, value: f32) -> KNumericField {
        let lo = self.min.resolve(vm) as f64;
        let hi = self.max.resolve(vm) as f64;
        let mut f = KNumericField::ranged(lo, hi);
        let _ = f.set_increment(self.step.resolve(vm).max(0.0) as f64);
        let _ = f.set_decimal_places(self.decimal_places);
        f.set_thousands_separator(self.thousands);
        let _ = f.clamped(value as f64);
        f.invalid = self.invalid.resolve(vm);
        f
    }

    /// Press/release over whichever step button (`SpinPart::Up`/`Down`) the
    /// pointer is on, using the real `NumericField::spin_part_at`/`step` — a
    /// completed click steps the value by the real `Increment`, clamped by
    /// the replica itself (`NumericUpDown::up_button`/`down_button`), then
    /// writes back and dispatches exactly like every other node here.
    pub(crate) fn interact(&mut self, ix: &mut InteractCx<'_>, bounds: Rect) -> WidgetState {
        let value = self.value.resolve(ix.vm);
        let mut field = self.build(ix.vm, value);
        let (mx, my) = ix.frame.mouse;
        let hot = !ix.frame.pointer_outside() && field.hit_test(bounds, mx, my);
        let part_now = if hot { field.spin_part_at(bounds, mx, my) } else { None };
        self.hover_part = part_now;
        let down_now = part_now.is_some() && ix.frame.mouse_down;
        let clicked = self.pressed_part.is_some() && self.pressed_part == part_now && !ix.frame.mouse_down;
        self.pressed_part = if down_now { part_now } else { None };
        let focus_state = self.focus_id.map(|id| ix.focus.register(id, bounds)).unwrap_or_default();
        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        if clicked {
            if let Some(part) = part_now {
                let old_value = field.value() as f32;
                if field.step(part) {
                    let new_value = field.value() as f32;
                    if let Some(spec) = self.value.binding() {
                        if spec.mode.writes_back() {
                            spec.update_source(ix.vm, Value::F32(new_value));
                        }
                    }
                    let mut args = NumericValueChangedEventArgs::new(old_value, new_value, ChangeSource::User);
                    ix.fire("OnValueChanged", self.focus_id, self.on_value_changed.as_deref(), ViewEventKind::Changed(new_value.to_string()), &mut args);
                }
            }
        }
        state
    }
}

impl ViewNode for NumericFieldNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        self.build(vm, self.value.resolve(vm)).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let state = {
            let mut ix = cx.interact_cx();
            self.interact(&mut ix, bounds)
        };
        let value = self.value.resolve(cx.vm);
        let mut field = self.build(cx.vm, value);
        field.spin_hot = self.hover_part;
        field.paint(cx.canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Metadata
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: icon_button,
    name: "IconButton",
    // Note: A round icon-only button (`kubuno_ui::buttons::IconButton` replica).
    doc: "A round button showing only an icon.",
    ctor: kubuno_ui::buttons::IconButton::plain("Check", 36.0, 18.0),
    children: ChildrenModel::None,
    props: [
        // Note: A curated glyph key (`check`, `close`, `trash`, `search`, `plus`, `help`, `chevron-up`/`chevron-down`/`chevron-right`, `caret-down`, `more-vertical`); unrecognised or empty falls back to a generic "more" glyph.
        PropertyMeta::new("Icon", PropKind::String, "",
            "The icon: a name of the Kubuno icon set (Check, X, Trash2, Search, Plus, MoreVertical…), or an image file relative to the view.",
        ).editor("icon").category("Icon"),
        PropertyMeta::new("Diameter", PropKind::F32, "36",
            "Diameter of the button, in pixels.",
        ),
        PropertyMeta::new("Glyph", PropKind::F32, "18", "Size of the icon inside the button, in pixels."),
        // Note: A resting tint (`surface-2`) instead of no background at rest — the waffle pencil's look.
        PropertyMeta::new("Filled", PropKind::Bool, "false",
            "Gives the button a tinted background.",
        ),
        PropertyMeta::new("DropDownMenu", PropKind::String, "", "A ContextMenu of the view that a click opens below the button (a menu button).").category("Behavior").editor("reference:ContextMenu"),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the button is clicked or activated with Space or Enter.").args::<crate::events::MouseEventArgs>(),
    ],
    smoke: |mut b| {
        b.diameter = 28.0;
        b.glyph = 15.0;
        b.filled = true;
        b
    },
    build: |props, _cx| {
        let icon = props.str("Icon", "")?;
        let diameter = props.f32("Diameter", 36.0)?;
        let glyph = props.f32("Glyph", 18.0)?;
        let filled = props.bool("Filled", false)?;
        let focus_id = props.focus_id();
        let on_click = props.event("OnClick");
        let drop_down = match props.str("DropDownMenu", "")? {
            crate::binding::PropSource::Literal(m) => Some(m).filter(|m| !m.trim().is_empty()),
            crate::binding::PropSource::Bound { .. } => None,
        };
        Ok(Box::new(crate::registry::families::choice::IconButtonNode {
            icon,
            diameter,
            glyph,
            filled,
            focus_id,
            on_click,
            drop_down,
            pressed: false,
        }) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: check_box,
    name: "CheckBox",
    // Note: A two-state check box (`kubuno_ui::buttons::CheckBox` replica, `Appearance::Normal`).
    doc: "A check box.",
    ctor: kubuno_ui::buttons::CheckBox::new("Se souvenir de moi"),
    children: ChildrenModel::None,
    default_event: "OnCheckedChanged",
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text displayed next to the check box."),
        PropertyMeta::new("Description", PropKind::String, "", "Secondary text displayed under the label."),
        PropertyMeta::new("Checked", PropKind::Bool, "false", "Whether the box is checked."),
        PropertyMeta::new("CheckState", PropKind::Enum(&["Unchecked", "Checked", "Indeterminate"]), "Unchecked", "State of the box, including the indeterminate state of a three-state box.").category("Appearance").bindable(),
        PropertyMeta::new("AutoCheck", PropKind::Bool, "true", "Checks or unchecks the box when it is clicked.").category("Behavior"),
        PropertyMeta::new("ThreeState", PropKind::Bool, "false", "Lets a click also set the indeterminate state.").category("Behavior"),
    ],
    events: [
        EventMeta::new("OnCheckedChanged", "Occurs when the box is checked or unchecked.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::CheckedChangedEventArgs>(),
    ],
    smoke: |c| {
        let c = c.description("Reste connecté sur cet appareil");
        let c = c.tri_state();
        c.check(kubuno_controls::enums::CheckState::Indeterminate)
    },
    build: |props, _cx| {
        let text = props.str("Text", "")?;
        let description = props.str("Description", "")?;
        let checked = props.bool("Checked", false)?;
        let check_state = if props.has("CheckState") { Some(props.enum_("CheckState", "Unchecked")?) } else { None };
        let flag = |name: &str, default: bool| props.element().attribute(name).and_then(|a| a.value()).map(|v| v.trim() == "true").unwrap_or(default);
        let focus_id = props.focus_id();
        let on_checked_changed = props.event("OnCheckedChanged");
        Ok(Box::new(crate::registry::families::choice::CheckBoxNode {
            text,
            description,
            checked,
            check_state,
            auto_check: flag("AutoCheck", true),
            three_state: flag("ThreeState", false),
            use_mnemonic: flag("UseMnemonic", true),
            focus_id,
            on_checked_changed,
            pressed: false,
        }) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: radio_button,
    name: "RadioButton",
    // Note: A mutually-exclusive option (`kubuno_ui::buttons::RadioButton` replica). Selection is not a flag this element owns: it is `SelectedValue == Value`, so several `<RadioButton>` elements bound to the SAME `SelectedValue` path exclude each other with no group registry — see this family's module doc.
    doc: "An option button: only one option of a group can be selected.",
    ctor: kubuno_ui::buttons::RadioButton::new("Clair"),
    children: ChildrenModel::None,
    default_event: "OnCheckedChanged",
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text displayed next to the option."),
        PropertyMeta::new("Description", PropKind::String, "", "Secondary text displayed under the label."),
        PropertyMeta::new("Value", PropKind::String, "", "Value this option stands for."),
        // Note: Advisory grouping id: not enforced by the interpreter (exclusion comes from the shared `SelectedValue` binding alone), but used together with `Value` to seed a stable `FocusId` when the element carries no `x:Name` — read as a plain literal, never as a `{Binding …}`, since only stability across frames matters here.
        PropertyMeta::new("Group", PropKind::String, "",
            "Name of the group this option belongs to.",
        ),
        // Note: The group's current value, typically `{Binding …, Mode=TwoWay}`; this option paints selected when it resolves to the same text as `Value`.
        PropertyMeta::new("SelectedValue", PropKind::String, "",
            "Value of the selected option of the group, usually bound in both directions. The option is selected when it equals Value.",
        ),
        PropertyMeta::new("AutoCheck", PropKind::Bool, "true", "Selects the option when it is clicked.").category("Behavior"),
    ],
    events: [
        // Note: Raised with the new `SelectedValue` when this option is chosen.
        EventMeta::new("OnCheckedChanged", "Occurs when this option is selected.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>(),
    ],
    smoke: |r| {
        let r = r.description("Thème clair");
        r.selected(true)
    },
    build: |props, _cx| {
        let text = props.str("Text", "")?;
        let description = props.str("Description", "")?;
        let value = props.str("Value", "")?;
        let selected_value = props.str("SelectedValue", "")?;
        let group_literal = props.element().attribute("Group").and_then(|a| a.value()).unwrap_or_default();
        let value_literal = props.element().attribute("Value").and_then(|a| a.value()).unwrap_or_default();
        let focus_id = props.focus_id().or_else(|| {
            if group_literal.is_empty() && value_literal.is_empty() {
                None
            } else {
                Some(kubuno_ui::FocusId::of(&format!("{group_literal}\u{1}{value_literal}")))
            }
        });
        let on_checked_changed = props.event("OnCheckedChanged");
        let flag = |name: &str, default: bool| props.element().attribute(name).and_then(|a| a.value()).map(|v| v.trim() == "true").unwrap_or(default);
        Ok(Box::new(crate::registry::families::choice::RadioButtonNode {
            text,
            description,
            value,
            selected_value,
            auto_check: flag("AutoCheck", true),
            use_mnemonic: flag("UseMnemonic", true),
            focus_id,
            on_checked_changed,
            pressed: false,
        }) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: slider,
    name: "Slider",
    // Note: A drag-to-set range control (`kubuno_ui::range::Slider`, `@ui/RangeSlider` replica).
    doc: "A slider for choosing a value in a range.",
    ctor: kubuno_ui::range::Slider::new(),
    children: ChildrenModel::None,
    default_event: "OnValueChanged",
    props: [
        // Note: The lowest reachable value (`TrackBar::Minimum`).
        PropertyMeta::new("Minimum", PropKind::F32, "0", "Lowest value.").aliases(&["Min"]),
        // Note: The highest reachable value (`TrackBar::Maximum`).
        PropertyMeta::new("Maximum", PropKind::F32, "10", "Highest value.").aliases(&["Max"]),
        // Note: The current value, folded into `[Minimum, Maximum]`.
        PropertyMeta::new("Value", PropKind::F32, "0", "Current value, between Minimum and Maximum."),
        // Note: One keyboard/small step (`TrackBar::SmallChange`).
        PropertyMeta::new("SmallChange", PropKind::F32, "1", "Amount the value changes with an arrow key.").aliases(&["Step"]),
        // Note: One page step (`TrackBar::LargeChange`).
        PropertyMeta::new("LargeChange", PropKind::F32, "5", "Amount the value changes with Page Up or Page Down.").aliases(&["LargeStep"]),
    ],
    events: [
        EventMeta::new("OnValueChanged", "Occurs when the value changes.").category(crate::registry::EventCategory::Action).args::<crate::events::NumericValueChangedEventArgs>(),
    ],
    smoke: |mut s| {
        s.set_minimum(0);
        s.set_maximum(20);
        let _ = s.set_small_change(2);
        let _ = s.set_large_change(4);
        let _ = s.set_value(10);
        s
    },
    build: |props, _cx| {
        let min = props.f32("Minimum", 0.0)?;
        let max = props.f32("Maximum", 10.0)?;
        let value = props.f32("Value", 0.0)?;
        let step = props.f32("SmallChange", 1.0)?;
        let large_step = props.f32("LargeChange", 5.0)?;
        let focus_id = props.focus_id();
        let on_value_changed = props.event("OnValueChanged");
        Ok(Box::new(crate::registry::families::choice::SliderNode {
            min,
            max,
            value,
            step,
            large_step,
            focus_id,
            on_value_changed,
            dragging: false,
        }) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: numeric_field,
    name: "NumericField",
    // Note: A numeric spinner (`kubuno_ui::range::NumericField`, `@ui/NumberInput` replica). Only the step buttons are interactive today; see this family's module doc for why free-text typing is deferred.
    doc: "A number box with up and down buttons.",
    ctor: kubuno_ui::range::NumericField::ranged(0.0, 100.0),
    children: ChildrenModel::None,
    default_event: "OnValueChanged",
    props: [
        // Note: The lowest reachable value (`NumericUpDown::Minimum`).
        PropertyMeta::new("Minimum", PropKind::F32, "0", "Lowest value.").aliases(&["Min"]),
        // Note: The highest reachable value (`NumericUpDown::Maximum`).
        PropertyMeta::new("Maximum", PropKind::F32, "100", "Highest value.").aliases(&["Max"]),
        // Note: The current value, folded into `[Minimum, Maximum]`.
        PropertyMeta::new("Value", PropKind::F32, "0", "Current value, between Minimum and Maximum."),
        // Note: One spin-button step (`NumericUpDown::Increment`).
        PropertyMeta::new("Increment", PropKind::F32, "1", "Amount added or removed by the up and down buttons.").aliases(&["Step"]),
        PropertyMeta::new("DecimalPlaces", PropKind::F32, "0", "Number of decimal places shown.").category("Appearance"),
        PropertyMeta::new("ThousandsSeparator", PropKind::Bool, "false", "Shows a separator between groups of thousands.").category("Appearance"),
        PropertyMeta::new("Invalid", PropKind::Bool, "false",
            "Shows the field in the error colour.",
        ),
    ],
    events: [
        EventMeta::new("OnValueChanged", "Occurs when the value changes.").category(crate::registry::EventCategory::Action).args::<crate::events::NumericValueChangedEventArgs>(),
    ],
    smoke: |mut f| {
        let _ = f.set_increment(5.0);
        f.invalid = true;
        f.spin_hot = Some(kubuno_ui::range::SpinPart::Up);
        f
    },
    build: |props, _cx| {
        let min = props.f32("Minimum", 0.0)?;
        let max = props.f32("Maximum", 100.0)?;
        let value = props.f32("Value", 0.0)?;
        let step = props.f32("Increment", 1.0)?;
        let invalid = props.bool("Invalid", false)?;
        let decimal_places = props.element().attribute("DecimalPlaces").and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok()).map(|v| v.clamp(0.0, 15.0) as i32).unwrap_or(0);
        let thousands = props.element().attribute("ThousandsSeparator").and_then(|a| a.value()).is_some_and(|v| v.trim() == "true");
        let focus_id = props.focus_id();
        let on_value_changed = props.event("OnValueChanged");
        Ok(Box::new(crate::registry::families::choice::NumericFieldNode {
            min,
            max,
            value,
            step,
            invalid,
            decimal_places,
            thousands,
            focus_id,
            on_value_changed,
            pressed_part: None,
            hover_part: None,
        }) as Box<dyn ViewNode>)
    },
}

/// Every component this family declares, in declaration order.
pub const ALL: &[ComponentMeta] =
    &[icon_button::META, check_box::META, radio_button::META, slider::META, numeric_field::META];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::{BindingSpec, MapViewModel};
    use crate::compile::compile_with_registry;
    use crate::node::{ViewEvent, ViewEventKind};
    use kubuno_controls::host::{Frame, Modifiers};
    use kubuno_ui::focus::FocusRing;

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

    /// Everything an [`InteractCx`] borrows from, owned across several
    /// frames — see `crate::node::tests::Harness`, which this mirrors (that
    /// one is private to `crate::node`).
    struct Harness {
        vm: MapViewModel,
        focus: FocusRing,
        handlers: crate::binding::HandlerTable,
        events: Vec<ViewEvent>,
    }

    impl Harness {
        fn new(vm: MapViewModel) -> Self {
            Self { vm, focus: FocusRing::new(), handlers: crate::binding::HandlerTable::new(), events: Vec::new() }
        }

        fn frame(
            &mut self,
            bounds: Rect,
            mouse: (f32, f32),
            mouse_down: bool,
            mut act: impl FnMut(&mut InteractCx<'_>, Rect) -> WidgetState,
        ) -> WidgetState {
            let frame = frame_at(mouse, mouse_down);
            let mut ix = InteractCx::new(&frame, &mut self.vm, &mut self.focus, &mut self.handlers, &mut self.events);
            act(&mut ix, bounds)
        }
    }

    // ── Registry plumbing ───────────────────────────────────────────────

    #[test]
    fn every_component_is_registered_and_findable() {
        for c in ALL {
            assert!(crate::registry::lookup(c.name).is_some(), "{} not registered", c.name);
        }
        assert_eq!(ALL.len(), 5);
    }

    // ── Compiling from XML: defaults, diagnostics with line/column ─────

    /// `CompiledView`'s `Ok` side is not `Debug` (a compiled widget tree has
    /// no useful textual form — see `crate::compile::tests`' own note), so
    /// `Result::unwrap_err` is not reachable here; this extracts the `Err`
    /// side directly instead, mirroring that module's private helper.
    fn expect_err(src: &str) -> Vec<crate::syntax::Diagnostic> {
        match compile_with_registry(src, ALL) {
            Ok(_) => panic!("expected `{src}` to fail to compile"),
            Err(d) => d,
        }
    }

    #[test]
    fn icon_button_compiles_with_defaults() {
        let view = compile_with_registry(r#"<IconButton/>"#, ALL);
        assert!(view.is_ok());
    }

    #[test]
    fn check_box_unknown_attribute_is_a_diagnostic_with_line_and_column() {
        let err = expect_err(r#"<CheckBox Bogus="1"/>"#);
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("Bogus"), "{:?}", err[0]);
        assert_eq!(err[0].line, 1);
        assert!(err[0].column > 1);
    }

    #[test]
    fn check_box_bad_bool_is_a_diagnostic() {
        let err = expect_err(r#"<CheckBox Checked="yes"/>"#);
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("true"), "{:?}", err[0]);
    }

    #[test]
    fn slider_bad_number_is_a_diagnostic() {
        let err = expect_err(r#"<Slider Min="low"/>"#);
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("expected a number"), "{:?}", err[0]);
    }

    #[test]
    fn radio_button_compiles_with_a_two_way_selected_value_binding() {
        let src = r#"<RadioButton Group="theme" Value="Dark" SelectedValue="{Binding Theme, Mode=TwoWay}"/>"#;
        assert!(compile_with_registry(src, ALL).is_ok());
    }

    #[test]
    fn numeric_field_compiles_with_a_two_way_value_binding() {
        let src = r#"<NumericField Min="0" Max="10" Value="{Binding Count, Mode=TwoWay}"/>"#;
        assert!(compile_with_registry(src, ALL).is_ok());
    }

    // ── IconButton: click ───────────────────────────────────────────────

    #[test]
    fn icon_button_click_dispatches_its_handler_and_fires_an_event() {
        let mut node = IconButtonNode {
            icon: PropSource::Literal("check".to_string()),
            diameter: PropSource::Literal(36.0),
            glyph: PropSource::Literal(18.0),
            filled: PropSource::Literal(false),
            focus_id: None,
            on_click: Some("saved".to_string()),
            drop_down: None,
            pressed: false,
        };
        let handlers = crate::handlers! {
            "saved" => |vm, _v| { vm.set("Saved", Value::Bool(true)); },
        };
        let mut h = Harness::new(MapViewModel::new());
        h.handlers = handlers;
        // A 36 DIP circle at the origin — its centre (18, 18) is on it.
        let bounds = Rect::new(0.0, 0.0, 36.0, 36.0);

        h.frame(bounds, (18.0, 18.0), true, |ix, b| node.interact(ix, b));
        assert_eq!(h.vm.get("Saved"), None, "a press alone is not yet a click");
        h.frame(bounds, (18.0, 18.0), false, |ix, b| node.interact(ix, b));

        assert_eq!(h.vm.get("Saved"), Some(Value::Bool(true)));
        assert_eq!(h.events.len(), 1);
        assert!(matches!(h.events[0].kind, ViewEventKind::Clicked));
    }

    #[test]
    fn icon_button_hit_test_is_circular_not_square() {
        let mut node = IconButtonNode {
            icon: PropSource::Literal(String::new()),
            diameter: PropSource::Literal(36.0),
            glyph: PropSource::Literal(18.0),
            filled: PropSource::Literal(false),
            focus_id: None,
            on_click: Some("clicked".to_string()),
            drop_down: None,
            pressed: false,
        };
        let handlers = crate::handlers! { "clicked" => |vm, _v| { vm.set("Hit", Value::Bool(true)); }, };
        let mut h = Harness::new(MapViewModel::new());
        h.handlers = handlers;
        let bounds = Rect::new(0.0, 0.0, 36.0, 36.0);

        // (2, 2) is in the bounding SQUARE's corner but outside the inscribed
        // circle — a click there must not register.
        h.frame(bounds, (2.0, 2.0), true, |ix, b| node.interact(ix, b));
        h.frame(bounds, (2.0, 2.0), false, |ix, b| node.interact(ix, b));
        assert_eq!(h.vm.get("Hit"), None, "the corner of the square is outside the circle");
    }

    // ── CheckBox: two-way binding, same-frame visibility ────────────────

    #[test]
    fn check_box_click_writes_back_and_fires_on_checked_changed() {
        let mut node = CheckBoxNode {
            text: PropSource::Literal("Se souvenir".to_string()),
            description: PropSource::Literal(String::new()),
            checked: PropSource::Bound {
                spec: BindingSpec { path: "Remember".to_string(), mode: BindingMode::TwoWay, ..Default::default() },
                fallback: false,
            },
            check_state: None,
            auto_check: true,
            three_state: false,
            use_mnemonic: true,
            focus_id: None,
            on_checked_changed: None,
            pressed: false,
        };
        let mut h = Harness::new(MapViewModel::new().with("Remember", Value::Bool(false)));
        let bounds = Rect::new(0.0, 0.0, 200.0, 20.0);

        h.frame(bounds, (9.0, 9.0), true, |ix, b| node.interact(ix, b));
        assert_eq!(h.vm.get("Remember"), Some(Value::Bool(false)), "not toggled on press alone");
        h.frame(bounds, (9.0, 9.0), false, |ix, b| node.interact(ix, b));

        assert_eq!(h.vm.get("Remember"), Some(Value::Bool(true)));
        assert_eq!(h.events.len(), 1);
        assert!(matches!(h.events[0].kind, ViewEventKind::Toggled(true)));
        // Same-frame visibility: what the widget rebuilds with on the very
        // next resolve already reflects the write-back.
        assert!(node.checked.resolve(&h.vm));
    }

    #[test]
    fn check_box_click_outside_after_press_is_not_a_toggle() {
        let mut node = CheckBoxNode {
            text: PropSource::Literal(String::new()),
            description: PropSource::Literal(String::new()),
            checked: PropSource::Literal(false),
            check_state: None,
            auto_check: true,
            three_state: false,
            use_mnemonic: true,
            focus_id: None,
            on_checked_changed: None,
            pressed: false,
        };
        let mut h = Harness::new(MapViewModel::new());
        let bounds = Rect::new(0.0, 0.0, 200.0, 20.0);

        h.frame(bounds, (9.0, 9.0), true, |ix, b| node.interact(ix, b));
        h.frame(bounds, (500.0, 500.0), false, |ix, b| node.interact(ix, b));
        assert!(h.events.is_empty());
    }

    // ── RadioButton: group exclusion via a shared SelectedValue path ───

    #[test]
    fn radio_button_click_selects_via_the_shared_selected_value_path() {
        let selected_value = || PropSource::Bound {
            spec: BindingSpec { path: "Theme".to_string(), mode: BindingMode::TwoWay, ..Default::default() },
            fallback: String::new(),
        };
        let mut light = RadioButtonNode {
            text: PropSource::Literal("Clair".to_string()),
            description: PropSource::Literal(String::new()),
            value: PropSource::Literal("Light".to_string()),
            selected_value: selected_value(),
            auto_check: true,
            use_mnemonic: true,
            focus_id: None,
            on_checked_changed: Some("theme_changed".to_string()),
            pressed: false,
        };
        let dark = RadioButtonNode {
            text: PropSource::Literal("Sombre".to_string()),
            description: PropSource::Literal(String::new()),
            value: PropSource::Literal("Dark".to_string()),
            selected_value: selected_value(),
            auto_check: true,
            use_mnemonic: true,
            focus_id: None,
            on_checked_changed: Some("theme_changed".to_string()),
            pressed: false,
        };
        let handlers = crate::handlers! {
            "theme_changed" => |_vm, _v| {},
        };
        let mut h = Harness::new(MapViewModel::new().with("Theme", Value::Str("Dark".to_string())));
        h.handlers = handlers;
        let bounds = Rect::new(0.0, 0.0, 200.0, 20.0);

        // "Dark" starts selected; clicking "Light" flips the shared path.
        h.frame(bounds, (9.0, 9.0), true, |ix, b| light.interact(ix, b));
        h.frame(bounds, (9.0, 9.0), false, |ix, b| light.interact(ix, b));

        assert_eq!(h.vm.get("Theme"), Some(Value::Str("Light".to_string())));
        assert_eq!(h.events.len(), 1);
        assert!(matches!(h.events[0].kind, ViewEventKind::Changed(ref v) if v == "Light"));
        // "Dark" resolves unselected purely from the shared path — no group
        // registry involved.
        assert_ne!(dark.selected_value.resolve(&h.vm), dark.value.resolve(&h.vm));
    }

    #[test]
    fn radio_button_click_on_the_already_selected_option_is_a_no_op() {
        let mut node = RadioButtonNode {
            text: PropSource::Literal(String::new()),
            description: PropSource::Literal(String::new()),
            value: PropSource::Literal("Dark".to_string()),
            selected_value: PropSource::Bound {
                spec: BindingSpec { path: "Theme".to_string(), mode: BindingMode::TwoWay, ..Default::default() },
                fallback: String::new(),
            },
            auto_check: true,
            use_mnemonic: true,
            focus_id: None,
            on_checked_changed: None,
            pressed: false,
        };
        let mut h = Harness::new(MapViewModel::new().with("Theme", Value::Str("Dark".to_string())));
        let bounds = Rect::new(0.0, 0.0, 200.0, 20.0);

        h.frame(bounds, (9.0, 9.0), true, |ix, b| node.interact(ix, b));
        h.frame(bounds, (9.0, 9.0), false, |ix, b| node.interact(ix, b));
        assert!(h.events.is_empty(), "clicking the already-selected option raises nothing");
    }

    // ── Slider: drag, live write-back ───────────────────────────────────

    #[test]
    fn slider_drag_writes_back_live_and_fires_on_value_changed() {
        let mut node = SliderNode {
            min: PropSource::Literal(0.0),
            max: PropSource::Literal(20.0),
            value: PropSource::Bound {
                spec: BindingSpec { path: "Volume".to_string(), mode: BindingMode::TwoWay, ..Default::default() },
                fallback: 0.0,
            },
            step: PropSource::Literal(1.0),
            large_step: PropSource::Literal(5.0),
            focus_id: None,
            on_value_changed: None,
            dragging: false,
        };
        let mut h = Harness::new(MapViewModel::new().with("Volume", Value::F32(0.0)));
        let bounds = Rect::new(0.0, 0.0, 200.0, 24.0);

        // Press near the right edge (`Rect::contains` excludes the edge
        // itself): the value under the pointer is (close to) the maximum.
        h.frame(bounds, (195.0, 12.0), true, |ix, b| node.interact(ix, b));
        match h.vm.get("Volume") {
            Some(Value::F32(v)) => assert!(v > 15.0, "expected close to the max, got {v}"),
            other => panic!("expected a numeric write-back, got {other:?}"),
        }
        assert!(!h.events.is_empty(), "at least one OnValueChanged-worthy change while dragging");

        // Releasing outside the rail: dragging still tracks the pointer,
        // clamped — `Slider::drag_to` does not require the point to be
        // inside the rail.
        h.frame(bounds, (600.0, 12.0), false, |ix, b| node.interact(ix, b));
        match h.vm.get("Volume") {
            Some(Value::F32(v)) => assert!((v - 20.0).abs() < f32::EPSILON, "expected clamped to max, got {v}"),
            other => panic!("expected a numeric write-back, got {other:?}"),
        }
    }

    #[test]
    fn slider_measure_does_not_panic_without_a_live_canvas_dependent_path() {
        // `Slider::measure` only reads `TrackBar::preferred_size` and this
        // family's own tick/thumb-reach geometry, none of which needs a live
        // `Canvas` — mirrors `compile::tests`' own note on why this crate's
        // tests never construct one.
        let node = SliderNode {
            min: PropSource::Literal(0.0),
            max: PropSource::Literal(10.0),
            value: PropSource::Literal(5.0),
            step: PropSource::Literal(1.0),
            large_step: PropSource::Literal(5.0),
            focus_id: None,
            on_value_changed: None,
            dragging: false,
        };
        let vm = MapViewModel::new();
        let built = node.build(&vm, 5.0);
        assert_eq!(built.value(), 5);
    }

    // ── NumericField: spin buttons, live write-back ─────────────────────

    #[test]
    fn numeric_field_up_button_click_writes_back_and_fires_on_value_changed() {
        let mut node = NumericFieldNode {
            min: PropSource::Literal(0.0),
            max: PropSource::Literal(10.0),
            value: PropSource::Bound {
                spec: BindingSpec { path: "Count".to_string(), mode: BindingMode::TwoWay, ..Default::default() },
                fallback: 0.0,
            },
            step: PropSource::Literal(1.0),
            invalid: PropSource::Literal(false),
            decimal_places: 0,
            thousands: false,
            focus_id: None,
            on_value_changed: None,
            pressed_part: None,
            hover_part: None,
        };
        let mut h = Harness::new(MapViewModel::new().with("Count", Value::F32(0.0)));
        // `field_width`'s default box; the up button sits in the right-hand
        // spin column's TOP half, however wide the caller's box is (`w-6`
        // per the module's own `SPIN_COLUMN`), so the near-top-right corner
        // reliably lands on it.
        let bounds = Rect::new(0.0, 0.0, 96.0, 32.0);
        let up = (bounds.right - 4.0, bounds.top + 4.0);

        h.frame(bounds, up, true, |ix, b| node.interact(ix, b));
        assert_eq!(h.vm.get("Count"), Some(Value::F32(0.0)), "not stepped on press alone");
        h.frame(bounds, up, false, |ix, b| node.interact(ix, b));

        assert_eq!(h.vm.get("Count"), Some(Value::F32(1.0)));
        assert_eq!(h.events.len(), 1);
        assert!(matches!(h.events[0].kind, ViewEventKind::Changed(ref v) if v == "1"));
    }

    #[test]
    fn numeric_field_step_clamps_at_the_maximum() {
        let mut node = NumericFieldNode {
            min: PropSource::Literal(0.0),
            max: PropSource::Literal(1.0),
            value: PropSource::Bound {
                spec: BindingSpec { path: "Count".to_string(), mode: BindingMode::TwoWay, ..Default::default() },
                fallback: 0.0,
            },
            step: PropSource::Literal(1.0),
            invalid: PropSource::Literal(false),
            decimal_places: 0,
            thousands: false,
            focus_id: None,
            on_value_changed: None,
            pressed_part: None,
            hover_part: None,
        };
        let mut h = Harness::new(MapViewModel::new().with("Count", Value::F32(1.0)));
        let bounds = Rect::new(0.0, 0.0, 96.0, 32.0);
        let up = (bounds.right - 4.0, bounds.top + 4.0);

        h.frame(bounds, up, true, |ix, b| node.interact(ix, b));
        h.frame(bounds, up, false, |ix, b| node.interact(ix, b));

        // Already at `Max`: the click landed but `step` did not move the
        // value, so nothing is written back and nothing fires.
        assert_eq!(h.vm.get("Count"), Some(Value::F32(1.0)));
        assert!(h.events.is_empty());
    }
}
