//! Component family `text` — declared with the `component!` table (see
//! `../macros.rs`) plus, in this same file, the view nodes those components
//! build. Compiled only with the `family-text` feature while the families are
//! being written in parallel; the feature is on by default once integrated.
//!
//! ## Components
//!
//! `TextArea`, `SearchField`, `MaskedField` (`kubuno_ui::text`), `Dropdown`
//! (`kubuno_ui::editors`), `ComboBox` (`kubuno_ui::lists`), `DatePicker`
//! (`kubuno_ui::datetime`), `ColorField` and `GradientField`
//! (`kubuno_ui::color`) — read off
//! those modules' real public builder API, the same way `registry::components`
//! reads `Button`/`Switch`/`TextField`/`Card`/`Stack` off `kubuno-ui`.
//!
//! ## Options: a child element, or an `ItemsSource` binding
//!
//! `Dropdown` and `ComboBox` both take their item list as `<Option Value=".."
//! Label=".."/>` children (`ChildrenModel::List`) — read straight off each
//! `<Option>` child through [`crate::ast::Element`] at build time
//! (`read_options`) rather than recursing through [`crate::props::Props::
//! build_children`]: an `Option` is data, never built into a live
//! [`crate::node::ViewNode`] of its own — **or** an `ItemsSource="{Binding
//! Path}"` to a [`crate::binding::Value::List`], re-read every frame
//! (`resolve_bound_options`), with `DisplayMember`/`ValueMember` naming which
//! row field is the label/value (defaults `"Label"`/`"Value"`, matching
//! `<Option>`'s own attribute names). A bound bare list takes over from the
//! static children only once it actually resolves to a list; unset or the
//! wrong shape leaves the static (or, absent any `<Option>` at all, empty)
//! list alone. `Option` is registered as its OWN [`crate::registry::
//! ComponentMeta`] too (`validate` rejects any child element name it cannot
//! look up — see `crate::validate`'s `walk`), but it has no real `kubuno_ui`
//! counterpart to drive a `component!` `ctor`/`smoke` pair against, so its
//! metadata is hand-built below instead of going through the macro (see
//! [`option`]'s doc).
//!
//! The selection itself is a two-way `SelectedValue="{Binding …}"` string —
//! an `<Option>`'s `Value`, or `""` for "nothing selected" — rather than an
//! index, so the view model never has to know the list's order.
//!
//! ## Floating parts
//!
//! `Dropdown`, `ComboBox` and `DatePicker` float their list/calendar in a
//! real `kubuno_controls::host::popup`, placed and painted the same way the
//! `kubuno-ui` gallery's own `editors`/`lists`/`datetime` pages do (their
//! `*_paint_bounds`/`paint_drop_down_at`/`popup_drop_down` helpers ARE the
//! popup wiring; this file only drives them from a [`crate::node::PaintCx`]
//! instead of a gallery `Frame`). `ColorField` and `GradientField` float the
//! full `ColorPicker` / `GradientPicker` the same way, driven by the pickers'
//! own interaction loop (`ColorPicker::pointer` / `ColorPicker::keyboard`)
//! and written back to a TwoWay binding (`Color` as `#rrggbb`, `Value` as the
//! web's `gradientToCss` string), with an `OnValueChanged` event.

use kubuno_controls::datetime::{Date, DateTime, DateTimePickerFormat};
use kubuno_controls::host;
use kubuno_controls::host::{vk, Modifiers};
use kubuno_ui::color;
use kubuno_ui::datetime::{DatePicker as UiDatePicker, FieldPart, HeaderPart};
use kubuno_ui::editors::{Dropdown as UiDropdown, DropdownVariant, ListKey};
use kubuno_ui::lists::{ComboBox as UiComboBox, ComboKey, ComboOutcome, FLOAT_SHADOW_MARGIN};
use kubuno_ui::text::{EditInput, MaskedField as UiMaskedField, SearchField as UiSearchField, TextArea as UiTextArea};
use kubuno_ui::{Canvas, FocusId, FocusOpts, Rect, Size, Widget};

use crate::ast::Element;
use crate::binding::{PropSource, Value, ViewModel};
use crate::node::{press_release, PaintCx, ViewEventKind, ViewNode};
use crate::events::{ChangeSource, CheckedChangedEventArgs, TextChangedEventArgs};
use crate::props::{BuildCx, BuildError, Props};
#[allow(unused_imports)] // Used by the `component!` invocations below.
use crate::registry::macros::component;
use crate::registry::{ChildrenModel, ComponentMeta, EventMeta, PropKind, PropertyMeta};

// ─────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────
//
// `press_release` and `PaintCx::fire` are shared with every other family now
// (`crate::node`, both `pub(crate)`) rather than reimplemented per file.

/// Grows `r` by `by` on every side — `crate::node`'s containers have no
/// equivalent helper `pub(crate)`, and `kubuno_ui`'s own `inflate`s (in
/// `editors`/`lists`) are private to their module.
fn inflate(r: Rect, by: f32) -> Rect {
    Rect::new(r.left - by, r.top - by, r.right + by, r.bottom + by)
}

/// `panel` in the local space of a popup painted at `pb` (`pb`'s top-left is
/// the popup canvas's origin) — the same translation the gallery's own
/// `rebase`/`local` helpers do for `host::popup`'s paint closure.
fn local_in(panel: Rect, pb: Rect) -> Rect {
    Rect::new(panel.left - pb.left, panel.top - pb.top, panel.right - pb.left, panel.bottom - pb.top)
}

/// `<Option Value=".." Label=".."/>` children of `el`, in document order —
/// what `Dropdown`/`ComboBox` populate their item list from (see this
/// module's doc, "Options: a child element, not a binding"). A child with no
/// `Label` falls back to its `Value` (so `<Option Value="fr"/>` alone is a
/// valid, self-labelling option); anything that is not literally named
/// `Option` is ignored rather than erroring — `crate::validate` already
/// refuses any OTHER unregistered element name, so this only guards a
/// defensive double-check.
fn read_options(el: &Element) -> Vec<(String, String)> {
    let mut options: Vec<(String, String)> = el
        .children()
        .filter(|c| c.name().as_deref() == Some("Option"))
        .map(|c| {
            let value = c.attribute("Value").and_then(|a| a.value()).unwrap_or_default();
            let label = c.attribute("Label").and_then(|a| a.value()).filter(|s| !s.is_empty()).unwrap_or_else(|| value.clone());
            (value, label)
        })
        .collect();
    if is_sorted(el) {
        sort_options(&mut options);
    }
    options
}

/// Whether a list writes `Sorted="true"`.
fn is_sorted(el: &Element) -> bool {
    el.attribute("Sorted").and_then(|a| a.value()).is_some_and(|v| v.trim() == "true")
}

/// `Sorted`: by label, alphabetical, ignoring case.
fn sort_options(options: &mut [(String, String)]) {
    options.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then_with(|| a.1.cmp(&b.1)));
}

/// The `ItemsSource`-bound twin of [`read_options`]: `(Value, Label)` per row
/// of a bound `crate::binding::Value::List`, read through `DisplayMember`/
/// `ValueMember` (each row's named field — see [`DropdownNode`]/
/// [`ComboBoxNode`]'s own docs and their `ItemsSource` property). `None`
/// under the same conditions `registry::families::data::resolve_item_labels`
/// documents (no binding, or it does not currently resolve to a list) — the
/// caller's cue to keep its existing, static-`<Option>`-built option list.
/// A label that resolves to empty text falls back to the value, mirroring
/// [`read_options`]'s own `<Option>` fallback.
#[cfg(test)]
fn resolve_bound_options(
    vm: &dyn ViewModel,
    items_source: &Option<crate::binding::BindingSpec>,
    display_member: &str,
    value_member: &str,
) -> Option<Vec<(String, String)>> {
    let spec = items_source.as_ref()?;
    let Value::List(rows) = vm.get(&spec.path)? else { return None };
    Some(options_of(&rows, display_member, value_member))
}

/// `(value, label)` per row, read through `value_member` / `display_member` (the label falls
/// back to the value).
fn options_of(rows: &[crate::binding::Row], display_member: &str, value_member: &str) -> Vec<(String, String)> {
    rows.iter()
        .map(|r| {
            let value = r.text(value_member);
            let label = r.text(display_member);
            let label = if label.is_empty() { value.clone() } else { label };
            (value, label)
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────
// Option — a data-only element, not a paintable widget (see the module doc)
// ─────────────────────────────────────────────────────────────────────────

/// `<Option>`'s metadata, hand-built rather than through [`component`]:
/// that macro's `ctor`/`smoke` pair exists specifically to guard against
/// drift from a REAL `kubuno_ui` constructor (`registry/mod.rs`'s module
/// doc), and there is no `kubuno_ui` widget an `<Option>` corresponds to —
/// it is pure data `Dropdown`/`ComboBox` read directly off the parsed
/// element (`read_options`), never built into a widget of its own. Giving it
/// a fabricated `ctor` just to fit the macro would be exactly the kind of
/// invented primitive `kubuno-ui`'s own rule 1 ("restate, don't own") warns
/// against.
mod option {
    use super::{BuildCx, BuildError, ChildrenModel, ComponentMeta, EventMeta, OptionNode, PropKind, Props, PropertyMeta, ViewNode};

    pub const PROPERTIES: &[PropertyMeta] = &[
        PropertyMeta::new("Value", PropKind::String, "", "Value of the item."),
        PropertyMeta::new("Label", PropKind::String, "", "Text shown for the item. Defaults to Value."),
    ];
    pub const EVENTS: &[EventMeta] = &[];

    /// Never reached in normal use (see the module doc): `Dropdown`/`ComboBox`
    /// read `<Option>` children directly, without calling
    /// [`crate::compile::build_node`] on them. Kept correct anyway, as a
    /// harmless no-op node, for the defensive case of an `<Option>` compiled
    /// standalone (e.g. by a future caller, or a malformed view that nests one
    /// somewhere else `crate::validate` still accepts).
    fn build(_props: &Props<'_>, _cx: &mut BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
        Ok(Box::new(OptionNode) as Box<dyn ViewNode>)
    }

    pub const META: ComponentMeta = ComponentMeta {
        name: "Option",
        doc: "An item of a Dropdown or ComboBox list.",
        properties: PROPERTIES,
        events: EVENTS,
        children: ChildrenModel::None,
        layout: crate::registry::LayoutKind::None,
        open_attributes: false,
        default_event: None,
        build,
    };
}

/// The no-op [`ViewNode`] a standalone `<Option>` would build into — see
/// [`option`]'s doc.
struct OptionNode;

impl ViewNode for OptionNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size::EMPTY
    }

    fn paint(&mut self, _cx: &mut PaintCx<'_>, _bounds: Rect) {}
}

// ─────────────────────────────────────────────────────────────────────────
// TextArea
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: text_area,
    name: "TextArea",
    // Note: A multiline text input (`kubuno_ui::text::TextArea`, the multiline case of TextField).
    doc: "A multi-line text box.",
    ctor: kubuno_ui::text::TextArea::new(),
    children: ChildrenModel::None,
    default_event: "OnTextChanged",
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text in the field."),
        PropertyMeta::new("Placeholder", PropKind::String, "", "Hint shown while the field is empty."),
        PropertyMeta::new("Invalid", PropKind::Bool, "false",
            "Shows the field in the error colour.",
        ),
        PropertyMeta::new("AcceptsReturn", PropKind::Bool, "true", "Types a new line when Enter is pressed. When false, Enter clicks the view's accept button.").category("Behavior"),
        PropertyMeta::new("WordWrap", PropKind::Bool, "true", "Wraps long lines at the edge of the field.").category("Behavior"),
        PropertyMeta::new("MinLines", PropKind::F32, "0", "Grows with its text from this many lines (in a layout that sizes it, a Stack or AutoSize); 0 for the standard height.").category("Layout"),
        PropertyMeta::new("MaxLines", PropKind::F32, "0", "Grows with its text up to this many lines, then scrolls; 0 for no limit.").category("Layout"),
    ],
    events: [
        EventMeta::new("OnTextChanged", "Occurs when the text changes.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>().aliases(&["OnChanged"]),
    ],
    smoke: |mut a| {
        a.set_text("hello\nworld");
        a.placeholder_text = "Notes".into();
        a.invalid = true;
        a
    },
    build: |props, _cx| {
        let text = props.str("Text", "")?;
        let placeholder = props.str("Placeholder", "")?;
        let invalid = props.bool("Invalid", false)?;
        let focus_id = props.focus_id();
        let on_changed = props.event("OnChanged");
        let text_box = crate::common::TextBoxProps::read(props)?;
        let mut node = crate::registry::families::text::TextAreaNode::new(text, placeholder, invalid, focus_id, on_changed).with_text_box(text_box);
        node.lines = (props.f32("MinLines", 0.0)?, props.f32("MaxLines", 0.0)?);
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

/// A [`UiTextArea`] replica held live, its caret/selection/undo history real
/// state a rebuild-from-nothing would erase every frame — the same shape as
/// `crate::node::TextFieldNode`, whose `Deref` target (`TextField`) this
/// widget itself derefs to, so `update`/`paint`/`display`/`reset_text` are
/// literally the same calls.
pub struct TextAreaNode {
    pub text: PropSource<String>,
    pub placeholder: PropSource<String>,
    pub invalid: PropSource<bool>,
    pub focus_id: Option<FocusId>,
    pub on_changed: Option<String>,
    field: UiTextArea,
    /// See `crate::node::TextFieldNode::synced` — the same reconciliation
    /// rule: only overwrite the live buffer from the binding before the
    /// field has taken its first edit, or while it is not focused.
    synced: bool,
    /// The bound text at the last frame: a change of it made by code (a composer cleared after a send)
    /// replaces the text even while the field has the focus, as setting `TextBox.Text` does.
    last_bound: Option<String>,
    /// The `TextBoxBase` properties, and `AcceptsReturn`/`WordWrap`.
    text_box: Option<crate::common::TextBoxProps>,
    /// `MinLines` / `MaxLines`: the height follows the text between them (0 / 0: the fixed height).
    pub lines: (PropSource<f32>, PropSource<f32>),
}

impl TextAreaNode {
    pub fn new(
        text: PropSource<String>,
        placeholder: PropSource<String>,
        invalid: PropSource<bool>,
        focus_id: Option<FocusId>,
        on_changed: Option<String>,
    ) -> Self {
        Self { text, placeholder, invalid, focus_id, on_changed, field: UiTextArea::new(), synced: false, last_bound: None, text_box: None, lines: (PropSource::Literal(0.0), PropSource::Literal(0.0)) }
    }

    /// Builder: the `TextBoxBase` properties.
    pub fn with_text_box(mut self, props: crate::common::TextBoxProps) -> Self {
        self.text_box = Some(props);
        self
    }
}

/// The height of one line of a text area, and of its top and bottom insets (with the border), in DIP.
const AREA_LINE: f32 = 20.0;
const AREA_INSETS: f32 = 18.0;

impl TextAreaNode {
    /// The auto-grown height for a content `width`, or `None` for the standard height.
    fn grown_height(&self, c: &dyn Canvas, vm: &dyn ViewModel, width: Option<f32>) -> Option<f32> {
        let (min, max) = (self.lines.0.resolve(vm).max(0.0), self.lines.1.resolve(vm).max(0.0));
        if min <= 0.0 && max <= 0.0 {
            return None;
        }
        let text = self.field.display();
        let wrap = self.text_box.as_ref().is_none_or(|tb| tb.word_wrap);
        let content = width.map(|w| (w - 2.0 * 12.0).max(1.0));
        let format = &c.formats().body;
        let lines: f32 = text
            .split('\n')
            .map(|l| match (wrap, content) {
                (true, Some(w)) if !l.is_empty() => (c.measure(l, format) / w).ceil().max(1.0),
                _ => 1.0,
            })
            .sum();
        let lines = lines.max(min.max(1.0));
        let lines = if max > 0.0 { lines.min(max) } else { lines };
        Some(lines * AREA_LINE + AREA_INSETS)
    }
}

impl ViewNode for TextAreaNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let size = self.field.measure(c);
        match self.grown_height(c, vm, None) {
            Some(h) => Size::new(size.width, h),
            None => size,
        }
    }

    fn measure_for_width(&self, c: &dyn Canvas, vm: &dyn ViewModel, width: f32) -> Size {
        let size = self.field.measure(c);
        match self.grown_height(c, vm, Some(width)) {
            Some(h) => Size::new(size.width, h),
            None => size,
        }
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let bound_text = self.text.resolve(cx.vm);
        if let Some(tb) = &self.text_box {
            tb.apply(&mut self.field, &*cx.vm);
            self.field.word_wrap = tb.word_wrap;
            self.field.enter_submits = !tb.accepts_return;
        }
        let opts = self.text_box.as_ref().map(|tb| tb.focus_opts(true)).unwrap_or(FocusOpts::TEXT);
        let focus_state = self.focus_id.map(|id| cx.focus.register_with(id, bounds, opts)).unwrap_or_default();

        // The model's real text, never `display()` (which masks a password).
        let changed_by_code = self.last_bound.as_ref().is_some_and(|last| *last != bound_text) && self.field.text() != bound_text;
        if !self.synced || changed_by_code || (!focus_state.focused && self.field.text() != bound_text) {
            self.field.reset_text(&bound_text);
            self.synced = true;
        }
        self.field.placeholder_text = self.placeholder.resolve(cx.vm);
        self.field.invalid = self.invalid.resolve(cx.vm);

        let canvas: &dyn Canvas = cx.canvas;
        let input = EditInput::new(cx.frame, focus_state);
        let outcome = self.field.update(canvas, bounds, &input);
        let state = focus_state.apply(crate::common::rest());
        self.field.paint(canvas, bounds, state);
        if outcome.submitted {
            crate::common::submit(cx.services.as_deref_mut());
        }

        if outcome.changed {
            let new_text = self.field.text().to_string();
            if let Some(spec) = self.text.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::Str(new_text.clone()));
                }
            }
            let mut args = TextChangedEventArgs::new(bound_text.clone(), new_text.clone(), ChangeSource::User);
            self.last_bound = Some(new_text.clone());
            cx.fire("OnTextChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(new_text), &mut args);
        } else {
            self.last_bound = Some(bound_text);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// SearchField
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: search_field,
    name: "SearchField",
    // Note: A search pill (`kubuno_ui::text::SearchField`) — a leading glyph and a clear "x" shown while it holds text.
    doc: "A search box with a clear button.",
    ctor: kubuno_ui::text::SearchField::new(),
    children: ChildrenModel::None,
    default_event: "OnTextChanged",
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text being searched for."),
        PropertyMeta::new("Placeholder", PropKind::String, "", "Hint shown while the field is empty."),
    ],
    events: [
        EventMeta::new("OnTextChanged", "Occurs when the search text changes.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>().aliases(&["OnChanged"]),
    ],
    smoke: |mut s| {
        s.set_text("kubuno");
        s.placeholder_text = "Rechercher".into();
        s
    },
    build: |props, _cx| {
        let text = props.str("Text", "")?;
        let placeholder = props.str("Placeholder", "")?;
        let focus_id = props.focus_id();
        let on_changed = props.event("OnChanged");
        let text_box = crate::common::TextBoxProps::read(props)?;
        Ok(Box::new(crate::registry::families::text::SearchFieldNode::new(text, placeholder, focus_id, on_changed).with_text_box(text_box)) as Box<dyn ViewNode>)
    },
}

pub struct SearchFieldNode {
    pub text: PropSource<String>,
    pub placeholder: PropSource<String>,
    pub focus_id: Option<FocusId>,
    pub on_changed: Option<String>,
    field: UiSearchField,
    synced: bool,
    /// The `TextBoxBase` properties.
    text_box: Option<crate::common::TextBoxProps>,
}

impl SearchFieldNode {
    pub fn new(text: PropSource<String>, placeholder: PropSource<String>, focus_id: Option<FocusId>, on_changed: Option<String>) -> Self {
        Self { text, placeholder, focus_id, on_changed, field: UiSearchField::new(), synced: false, text_box: None }
    }

    /// Builder: the `TextBoxBase` properties.
    pub fn with_text_box(mut self, props: crate::common::TextBoxProps) -> Self {
        self.text_box = Some(props);
        self
    }
}

impl ViewNode for SearchFieldNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.field.measure(c)
    }

    /// `SearchField::measure` (`kubuno_ui::text`) is content-sized (glyph +
    /// text/placeholder width + the reserved ✕ column) — not `w-full` — but
    /// `SearchField::paint` fills its pill across whatever `bounds` it is
    /// given, same as `Badge`: without this a `<Stack>` column stretches
    /// that pill's subtle fill across the FULL row, reading as barely
    /// visible at that width instead of the small search pill it should be.
    fn intrinsic_width(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Option<f32> {
        Some(self.field.measure(c).width)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let bound_text = self.text.resolve(cx.vm);
        if let Some(tb) = &self.text_box {
            tb.apply(&mut self.field, &*cx.vm);
        }
        let focus_state = self.focus_id.map(|id| cx.focus.register_with(id, bounds, FocusOpts::TEXT)).unwrap_or_default();

        if !self.synced || (!focus_state.focused && self.field.text() != bound_text) {
            self.field.set_text(&bound_text);
            self.synced = true;
        }
        self.field.placeholder_text = self.placeholder.resolve(cx.vm);

        let canvas: &dyn Canvas = cx.canvas;
        let input = EditInput::new(cx.frame, focus_state);
        let outcome = self.field.update(canvas, bounds, &input);
        let state = focus_state.apply(crate::common::rest());
        self.field.paint(canvas, bounds, state);

        if outcome.changed {
            let new_text = self.field.text().to_string();
            if let Some(spec) = self.text.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::Str(new_text.clone()));
                }
            }
            let mut args = TextChangedEventArgs::new(bound_text.clone(), new_text.clone(), ChangeSource::User);
            cx.fire("OnTextChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(new_text), &mut args);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// MaskedField
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: masked_field,
    name: "MaskedField",
    // Note: A field governed by a WinForms mask (`kubuno_ui::text::MaskedField`, e.g. "00/00/0000").
    doc: "A text box that follows an input mask, for example 00/00/0000.",
    ctor: kubuno_ui::text::MaskedField::new(),
    children: ChildrenModel::None,
    default_event: "OnTextChanged",
    props: [
        // Note: The WinForms mask pattern (e.g. `00/00/0000`); applied once and re-applied whenever this value changes.
        PropertyMeta::new("Mask", PropKind::String, "",
            "Input mask, for example 00/00/0000 for a date.",
        ),
        PropertyMeta::new("Text", PropKind::String, "", "Text in the field."),
        // Note: Turns the border and the focus outline to the danger colour — a natural driver is `!mask_completed()`, left to the caller.
        PropertyMeta::new("Invalid", PropKind::Bool, "false",
            "Shows the field in the error colour.",
        ),
    ],
    events: [
        EventMeta::new("OnTextChanged", "Occurs when the text changes.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>().aliases(&["OnChanged"]),
    ],
    smoke: |mut m| {
        m.set_mask("00/00/0000");
        m.invalid = true;
        m
    },
    build: |props, _cx| {
        let mask = props.str("Mask", "")?;
        let text = props.str("Text", "")?;
        let invalid = props.bool("Invalid", false)?;
        let focus_id = props.focus_id();
        let on_changed = props.event("OnChanged");
        let text_box = crate::common::TextBoxProps::read(props)?;
        Ok(Box::new(crate::registry::families::text::MaskedFieldNode::new(mask, text, invalid, focus_id, on_changed).with_text_box(text_box)) as Box<dyn ViewNode>)
    },
}

pub struct MaskedFieldNode {
    pub mask: PropSource<String>,
    pub text: PropSource<String>,
    pub invalid: PropSource<bool>,
    pub focus_id: Option<FocusId>,
    pub on_changed: Option<String>,
    field: UiMaskedField,
    /// The last `Mask` value applied to [`Self::field`] — `set_mask` resets
    /// the whole buffer, so it must only run when the mask text actually
    /// changed, never every frame.
    applied_mask: String,
    synced: bool,
    /// The `TextBoxBase` properties.
    text_box: Option<crate::common::TextBoxProps>,
}

impl MaskedFieldNode {
    pub fn new(
        mask: PropSource<String>,
        text: PropSource<String>,
        invalid: PropSource<bool>,
        focus_id: Option<FocusId>,
        on_changed: Option<String>,
    ) -> Self {
        Self { mask, text, invalid, focus_id, on_changed, field: UiMaskedField::new(), applied_mask: String::new(), synced: false, text_box: None }
    }

    /// Builder: the `TextBoxBase` properties.
    pub fn with_text_box(mut self, props: crate::common::TextBoxProps) -> Self {
        self.text_box = Some(props);
        self
    }
}

impl ViewNode for MaskedFieldNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.field.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let mask = self.mask.resolve(cx.vm);
        if mask != self.applied_mask {
            self.field.set_mask(&mask);
            self.applied_mask = mask;
            self.synced = false; // A new mask invalidates whatever text was fed through the old one.
        }

        let bound_text = self.text.resolve(cx.vm);
        if let Some(tb) = &self.text_box {
            tb.apply_base(&mut self.field, &*cx.vm);
        }
        let focus_state = self.focus_id.map(|id| cx.focus.register_with(id, bounds, FocusOpts::TEXT)).unwrap_or_default();
        // `value()`, not `display()`: the same string, but with the typed characters where a
        // password mask displays its glyphs.
        if !self.synced || (!focus_state.focused && self.field.value() != bound_text) {
            self.field.set_text(&bound_text);
            self.synced = true;
        }
        self.field.invalid = self.invalid.resolve(cx.vm);

        let canvas: &dyn Canvas = cx.canvas;
        let input = EditInput::new(cx.frame, focus_state);
        let outcome = self.field.update(canvas, bounds, &input);
        let state = focus_state.apply(crate::common::rest());
        self.field.paint(canvas, bounds, state);

        if outcome.changed {
            let new_text = self.field.value();
            if let Some(spec) = self.text.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::Str(new_text.clone()));
                }
            }
            let mut args = TextChangedEventArgs::new(bound_text.clone(), new_text.clone(), ChangeSource::User);
            cx.fire("OnTextChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(new_text), &mut args);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Dropdown — a real `host::popup` list, exactly as
// `kubuno-ui/examples/gallery/pages/editors.rs`'s own interactive column
// drives one (`Dropdown::place_drop_down` / `drop_down_paint_bounds` /
// `paint_drop_down_at`, its list in a popup of its own).
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: dropdown,
    name: "Dropdown",
    // Note: A selector with a floating list (`kubuno_ui::editors::Dropdown`) — items are <Option Value=".." Label=".."/> children.
    doc: "A drop-down list for choosing one option. Add the options as Option children.",
    ctor: kubuno_ui::editors::Dropdown::new(),
    children: ChildrenModel::List(&["Option"]),
    default_event: "OnSelectedValueChanged",
    props: [
        // Note: The selected <Option>'s Value, or empty for no selection — see the family's module doc.
        PropertyMeta::new("SelectedValue", PropKind::String, "",
            "Value of the selected option. Empty when nothing is selected.",
        ),
        PropertyMeta::new("Placeholder", PropKind::String, "", "Text shown when nothing is selected."),
        PropertyMeta::new("Variant", PropKind::Enum(&["Default", "Ghost"]), "Default",
            "Look of the list: with a border, or borderless for a toolbar.",
        ),
        // Note: A `{Binding Path}` to a row list (`crate::binding::Value::List`); each row becomes one `<Option>`, `DisplayMember`/`ValueMember` naming which row field is the label/value, re-read every frame. Static `<Option Value=".." Label=".."/>` children still work and are used as long as this either names no binding or the binding is unset/not a list.
        PropertyMeta::new("ItemsSource", PropKind::String, "",
            "Binding to the list of items to show, instead of Option children.",
        ),
        PropertyMeta::new("DisplayMember", PropKind::String, "Label", "Field of each bound item shown as its text."),
        PropertyMeta::new("ValueMember", PropKind::String, "Value", "Field of each bound item used as its value."),
        crate::owner_draw::DRAW_MODE,
    ],
    events: [
        EventMeta::new("OnSelectedValueChanged", "Occurs when the selected option changes.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>().aliases(&["OnChanged"]),
        crate::owner_draw::ON_DRAW_ITEM,
        crate::owner_draw::ON_MEASURE_ITEM,
    ],
    smoke: |mut d| {
        d.add_option("Trier par nom", None);
        d.set_selected_index(0);
        d.height = 28.0;
        d.variant = kubuno_ui::editors::DropdownVariant::Ghost;
        d
    },
    build: |props, _cx| {
        let selected_value = props.str("SelectedValue", "")?;
        let placeholder = props.str("Placeholder", "")?;
        let variant = props.enum_("Variant", "Default")?;
        let items_source = props.str("ItemsSource", "")?.binding().cloned();
        let display_member = props.str("DisplayMember", "Label")?;
        let value_member = props.str("ValueMember", "Value")?;
        let focus_id = props.focus_id();
        let on_changed = props.event("OnChanged");
        let options = crate::registry::families::text::read_options(props.element());
        let mut widget = kubuno_ui::editors::Dropdown::new();
        for (_, label) in &options {
            widget.add_option(label.clone(), None);
        }
        let mut node = crate::registry::families::text::DropdownNode::new(
            selected_value, placeholder, variant, items_source, display_member, value_member, focus_id, on_changed, options, widget,
        );
        node.sorted = crate::registry::families::text::is_sorted(props.element());
        node.draw_mode = crate::owner_draw::draw_mode_prop(props)?;
        node.owner = crate::owner_draw::OwnerDrawEvents::read(props);
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

fn parse_dropdown_variant(s: &str) -> DropdownVariant {
    match s {
        "Ghost" => DropdownVariant::Ghost,
        _ => DropdownVariant::Default,
    }
}

pub struct DropdownNode {
    /// `Sorted`: the options are shown in alphabetical order.
    pub sorted: bool,
    pub selected_value: PropSource<String>,
    pub placeholder: PropSource<String>,
    pub variant: PropSource<String>,
    items_source: Option<crate::binding::BindingSpec>,
    /// The bound rows last shown, and the members they were read with.
    watch: crate::binding::ListWatch,
    bound_members: (String, String),
    display_member: PropSource<String>,
    value_member: PropSource<String>,
    pub focus_id: Option<FocusId>,
    pub on_changed: Option<String>,
    /// `(Value, Label)` per `<Option>` — literal (from static children) until
    /// `items_source` resolves at least once, after which it follows the
    /// bound row list every frame it changes (see [`ViewNode::paint`]).
    options: Vec<(String, String)>,
    /// `DrawMode` and the owner-draw handlers (EVT-8).
    pub draw_mode: PropSource<String>,
    pub owner: crate::owner_draw::OwnerDrawEvents,
    widget: UiDropdown,
    pressed: bool,
    panel_pressed: bool,
}

impl DropdownNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        selected_value: PropSource<String>,
        placeholder: PropSource<String>,
        variant: PropSource<String>,
        items_source: Option<crate::binding::BindingSpec>,
        display_member: PropSource<String>,
        value_member: PropSource<String>,
        focus_id: Option<FocusId>,
        on_changed: Option<String>,
        options: Vec<(String, String)>,
        widget: UiDropdown,
    ) -> Self {
        Self {
            sorted: false,
            selected_value,
            placeholder,
            variant,
            items_source,
            watch: Default::default(),
            bound_members: Default::default(),
            display_member,
            value_member,
            focus_id,
            on_changed,
            options,
            draw_mode: PropSource::Literal("Normal".to_string()),
            owner: Default::default(),
            widget,
            pressed: false,
            panel_pressed: false,
        }
    }

    /// Re-reads `items_source` (if bound) and, when the resolved options
    /// differ from what is already shown, resyncs both [`Self::options`] and
    /// the live widget's item list — see [`resolve_bound_options`].
    fn resync_bound_options(&mut self, vm: &dyn ViewModel) {
        let display_member = self.display_member.resolve(vm);
        let value_member = self.value_member.resolve(vm);
        let members = (display_member, value_member);
        if self.bound_members != members {
            self.watch.reset();
            self.bound_members = members;
        }
        let (display_member, value_member) = &self.bound_members;
        if let Some(mut resolved) = self.watch.changed(vm, self.items_source.as_ref()).map(|rows| options_of(&rows, display_member, value_member)) {
            if self.sorted {
                sort_options(&mut resolved);
            }
            if resolved != self.options {
                self.widget.items = resolved.iter().map(|(_, label)| label.clone()).collect();
                self.widget.icons = vec![None; resolved.len()];
                self.options = resolved;
            }
        }
    }

    fn index_of(&self, value: &str) -> Option<usize> {
        self.options.iter().position(|(v, _)| v == value)
    }

    /// Applies `value` to the widget's selection — `-1` (none) when it does
    /// not match any `<Option>`, including an empty binding.
    fn apply_selection(&mut self, value: &str) {
        let index = self.index_of(value).map(|i| i as i32).unwrap_or(-1);
        self.widget.set_selected_index(index);
    }
}

impl ViewNode for DropdownNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        self.resync_bound_options(cx.vm);
        let canvas: &dyn Canvas = cx.canvas;
        self.widget.placeholder = self.placeholder.resolve(cx.vm);
        self.widget.variant = parse_dropdown_variant(&self.variant.resolve(cx.vm));
        let bound_value = self.selected_value.resolve(cx.vm);
        self.apply_selection(&bound_value);

        if cx.frame.dismiss {
            self.widget.close();
            self.widget.clear_placement();
        }

        let trigger = self.widget.trigger_rect(bounds);
        let (mx, my) = cx.frame.mouse;
        let hot = !cx.frame.pointer_outside() && trigger.contains(mx, my);
        let (down_now, clicked) = press_release(&mut self.pressed, hot, cx.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, trigger)).unwrap_or_default();

        if clicked {
            self.widget.toggle();
            if !self.widget.open {
                self.widget.clear_placement();
            }
        }

        let mut committed_value: Option<String> = None;

        if self.widget.open {
            let area = cx.frame.screen_area();
            self.widget.place_drop_down(canvas, bounds, area);
            let panel = self.widget.drop_down_rect(bounds);
            cx.focus.keep_focus_in(panel);

            if let Some(i) = self.widget.item_at(bounds, mx, my) {
                self.widget.hot_index = Some(i);
            }
            let over_panel = panel.contains(mx, my);
            let (_, row_clicked) = press_release(&mut self.panel_pressed, over_panel, cx.frame.mouse_down);
            if row_clicked {
                if let Some(i) = self.widget.item_at(bounds, mx, my) {
                    self.widget.commit(i);
                    committed_value = self.options.get(i).map(|(v, _)| v.clone());
                }
            }

            if !focus_state.focused {
                self.widget.close();
                self.widget.clear_placement();
            }
        } else {
            self.panel_pressed = false;
        }

        if focus_state.focused {
            if let ListKey::Committed(i) = self.widget.take_input(host::now_ms()) {
                committed_value = self.options.get(i).map(|(v, _)| v.clone());
            }
        }

        if let Some(value) = committed_value {
            let old_value = self.selected_value.resolve(cx.vm);
            if let Some(spec) = self.selected_value.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::Str(value.clone()));
                }
            }
            let mut args = TextChangedEventArgs::new(old_value, value.clone(), ChangeSource::User);
            cx.fire("OnSelectedValueChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(value), &mut args);
        }

        // Re-resolved AFTER interaction, for same-frame feedback — the same
        // technique `crate::node::SwitchNode`/`TextFieldNode` use.
        let bound_value = self.selected_value.resolve(cx.vm);
        self.apply_selection(&bound_value);

        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        let mode = crate::owner_draw::parse_draw_mode(&self.draw_mode.resolve(cx.vm));
        if self.widget.draw_mode != mode {
            self.widget.draw_mode = mode;
        }
        let _ = canvas;
        let widget = &self.widget;
        crate::owner_draw::paint_with(cx, &self.owner, |c| widget.paint_field(c, bounds, state));

        if self.widget.open {
            let panel = self.widget.drop_down_rect(bounds);
            let pb = self.widget.drop_down_paint_bounds(bounds);
            let local = local_in(panel, pb);
            // Owner-drawn rows are drawn now, by the element's handler, and replayed in the popup.
            let recorded = if self.owner.active() {
                let mut handler = crate::owner_draw::ElementOwnerDraw { cx: &mut *cx, events: &self.owner, rows: None };
                self.widget.record_drop_down_items(local, &mut handler)
            } else {
                kubuno_ui::graphics::owner_draw::RecordedItems::new()
            };
            let snapshot = self.widget.clone();
            host::popup(pb, move |cv| crate::owner_draw::replay_in_popup(recorded, || snapshot.paint_drop_down_at(cv, local)));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// ComboBox — the same real popup treatment as Dropdown, over
// `kubuno_ui::lists::ComboBox` instead (a select-only ARIA combobox; see
// `kubuno-ui/examples/gallery/pages/lists.rs`'s own interactive column).
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: combo_box,
    name: "ComboBox",
    // Note: A select-only combo box with a floating list (`kubuno_ui::lists::ComboBox`) — items are <Option Value=".." Label=".."/> children.
    doc: "A list box that drops down, for choosing one option. Add the options as Option children.",
    ctor: kubuno_ui::lists::ComboBox::new(),
    children: ChildrenModel::List(&["Option"]),
    default_event: "OnSelectedValueChanged",
    props: [
        // Note: The selected <Option>'s Value, or empty for no selection — see the family's module doc.
        PropertyMeta::new("SelectedValue", PropKind::String, "",
            "Value of the selected option. Empty when nothing is selected.",
        ),
        // Note: A `{Binding Path}` to a row list (`crate::binding::Value::List`); each row becomes one `<Option>`, `DisplayMember`/`ValueMember` naming which row field is the label/value, re-read every frame. Static `<Option Value=".." Label=".."/>` children still work and are used as long as this either names no binding or the binding is unset/not a list.
        PropertyMeta::new("ItemsSource", PropKind::String, "",
            "Binding to the list of items to show, instead of Option children.",
        ),
        PropertyMeta::new("DisplayMember", PropKind::String, "Label", "Field of each bound item shown as its text."),
        PropertyMeta::new("ValueMember", PropKind::String, "Value", "Field of each bound item used as its value."),
        crate::owner_draw::DRAW_MODE,
    ],
    events: [
        EventMeta::new("OnSelectedValueChanged", "Occurs when the selected option changes.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>().aliases(&["OnChanged"]),
        crate::owner_draw::ON_DRAW_ITEM,
        crate::owner_draw::ON_MEASURE_ITEM,
    ],
    smoke: |mut c| {
        c.add_item("Français");
        c.set_selected_index(0);
        c
    },
    build: |props, _cx| {
        let selected_value = props.str("SelectedValue", "")?;
        let items_source = props.str("ItemsSource", "")?.binding().cloned();
        let display_member = props.str("DisplayMember", "Label")?;
        let value_member = props.str("ValueMember", "Value")?;
        let focus_id = props.focus_id();
        let on_changed = props.event("OnChanged");
        let options = crate::registry::families::text::read_options(props.element());
        let mut widget = kubuno_ui::lists::ComboBox::new();
        for (_, label) in &options {
            widget.add_item(label.clone());
        }
        let mut node = crate::registry::families::text::ComboBoxNode::new(
            selected_value, items_source, display_member, value_member, focus_id, on_changed, options, widget,
        );
        node.sorted = crate::registry::families::text::is_sorted(props.element());
        node.draw_mode = crate::owner_draw::draw_mode_prop(props)?;
        node.owner = crate::owner_draw::OwnerDrawEvents::read(props);
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

/// This frame's combo-navigation keys, read from the host queue — the same
/// mapping `kubuno-ui`'s own gallery (`lists.rs`' `take_combo_keys`) applies,
/// rebuilt against the public [`host::take_key`] one key at a time instead of
/// that example's `host::consume` closure (not reachable from here: examples
/// are not part of the crate's public API).
fn take_combo_keys() -> Vec<ComboKey> {
    let mut out = Vec::new();
    for _ in 0..host::take_key(vk::DOWN, Modifiers::ALT) {
        out.push(ComboKey::AltDown);
    }
    for _ in 0..host::take_key(vk::UP, Modifiers::ALT) {
        out.push(ComboKey::AltUp);
    }
    for _ in 0..host::take_key(vk::DOWN, Modifiers::NONE) {
        out.push(ComboKey::Down);
    }
    for _ in 0..host::take_key(vk::UP, Modifiers::NONE) {
        out.push(ComboKey::Up);
    }
    for _ in 0..host::take_key(vk::HOME, Modifiers::NONE) {
        out.push(ComboKey::Home);
    }
    for _ in 0..host::take_key(vk::END, Modifiers::NONE) {
        out.push(ComboKey::End);
    }
    for _ in 0..host::take_key(vk::PAGE_UP, Modifiers::NONE) {
        out.push(ComboKey::PageUp);
    }
    for _ in 0..host::take_key(vk::PAGE_DOWN, Modifiers::NONE) {
        out.push(ComboKey::PageDown);
    }
    for _ in 0..host::take_key(vk::ENTER, Modifiers::NONE) {
        out.push(ComboKey::Enter);
    }
    for _ in 0..host::take_key(vk::SPACE, Modifiers::NONE) {
        out.push(ComboKey::Space);
    }
    for _ in 0..host::take_key(vk::F4, Modifiers::NONE) {
        out.push(ComboKey::F4);
    }
    out
}

pub struct ComboBoxNode {
    /// `Sorted`.
    pub sorted: bool,
    pub selected_value: PropSource<String>,
    items_source: Option<crate::binding::BindingSpec>,
    /// The bound rows last shown, and the members they were read with.
    watch: crate::binding::ListWatch,
    bound_members: (String, String),
    display_member: PropSource<String>,
    value_member: PropSource<String>,
    pub focus_id: Option<FocusId>,
    pub on_changed: Option<String>,
    options: Vec<(String, String)>,
    /// `DrawMode` and the owner-draw handlers (EVT-8).
    pub draw_mode: PropSource<String>,
    pub owner: crate::owner_draw::OwnerDrawEvents,
    widget: UiComboBox,
    pressed: bool,
    panel_pressed: bool,
}

impl ComboBoxNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        selected_value: PropSource<String>,
        items_source: Option<crate::binding::BindingSpec>,
        display_member: PropSource<String>,
        value_member: PropSource<String>,
        focus_id: Option<FocusId>,
        on_changed: Option<String>,
        options: Vec<(String, String)>,
        widget: UiComboBox,
    ) -> Self {
        Self {
            sorted: false,
            selected_value,
            items_source,
            watch: Default::default(),
            bound_members: Default::default(),
            display_member,
            value_member,
            focus_id,
            on_changed,
            options,
            draw_mode: PropSource::Literal("Normal".to_string()),
            owner: Default::default(),
            widget,
            pressed: false,
            panel_pressed: false,
        }
    }

    fn index_of(&self, value: &str) -> Option<usize> {
        self.options.iter().position(|(v, _)| v == value)
    }

    fn apply_selection(&mut self, value: &str) {
        let index = self.index_of(value).map(|i| i as i32).unwrap_or(-1);
        self.widget.set_selected_index(index);
    }

    /// See [`DropdownNode::resync_bound_options`] — the same resync, over
    /// `UiComboBox::items` instead of `UiDropdown::items`/`icons` (a select-
    /// only combo box has no per-option icon to keep aligned).
    fn resync_bound_options(&mut self, vm: &dyn ViewModel) {
        let display_member = self.display_member.resolve(vm);
        let value_member = self.value_member.resolve(vm);
        let members = (display_member, value_member);
        if self.bound_members != members {
            self.watch.reset();
            self.bound_members = members;
        }
        let (display_member, value_member) = &self.bound_members;
        if let Some(mut resolved) = self.watch.changed(vm, self.items_source.as_ref()).map(|rows| options_of(&rows, display_member, value_member)) {
            if self.sorted {
                sort_options(&mut resolved);
            }
            if resolved != self.options {
                self.widget.items = resolved.iter().map(|(_, label)| label.clone()).collect();
                self.options = resolved;
            }
        }
    }
}

impl ViewNode for ComboBoxNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        self.resync_bound_options(cx.vm);
        let canvas: &dyn Canvas = cx.canvas;
        let bound_value = self.selected_value.resolve(cx.vm);
        self.apply_selection(&bound_value);

        if cx.frame.dismiss {
            self.widget.close();
        }

        let field = self.widget.field_rect(bounds);
        let (mx, my) = cx.frame.mouse;
        let hot = !cx.frame.pointer_outside() && field.contains(mx, my);
        let (down_now, clicked) = press_release(&mut self.pressed, hot, cx.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, field)).unwrap_or_default();

        if clicked {
            self.widget.toggle();
        }

        let area = cx.frame.screen_area();
        let mut committed_value: Option<String> = None;

        if self.widget.is_open() {
            let panel = self.widget.drop_down_rect_in(bounds, area);
            cx.focus.keep_focus_in(panel);

            if let Some(i) = self.widget.item_at_panel(panel, mx, my) {
                self.widget.hot_index = Some(i);
            }
            let over_panel = panel.contains(mx, my);
            let (_, row_clicked) = press_release(&mut self.panel_pressed, over_panel, cx.frame.mouse_down);
            if row_clicked {
                if let Some(i) = self.widget.item_at_panel(panel, mx, my) {
                    self.widget.commit(i);
                    committed_value = self.options.get(i).map(|(v, _)| v.clone());
                }
            }

            if !focus_state.focused {
                self.widget.close();
            }
        } else {
            self.panel_pressed = false;
        }

        if focus_state.focused {
            let mut keys = take_combo_keys();
            if self.widget.is_open() && cx.focus.take_escape() {
                keys.push(ComboKey::Escape);
            }
            for key in keys {
                if let ComboOutcome::Committed(i) = self.widget.handle_key(key) {
                    committed_value = self.options.get(i).map(|(v, _)| v.clone());
                }
            }
            let typed: String = host::take_text().chars().filter(|c| !c.is_whitespace()).collect();
            if !typed.is_empty() {
                self.widget.type_to_select(&typed, host::now_ms());
            }
        }

        if let Some(value) = committed_value {
            let old_value = self.selected_value.resolve(cx.vm);
            if let Some(spec) = self.selected_value.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::Str(value.clone()));
                }
            }
            let mut args = TextChangedEventArgs::new(old_value, value.clone(), ChangeSource::User);
            cx.fire("OnSelectedValueChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(value), &mut args);
        }

        let bound_value = self.selected_value.resolve(cx.vm);
        self.apply_selection(&bound_value);

        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        let mode = crate::owner_draw::parse_draw_mode(&self.draw_mode.resolve(cx.vm));
        if self.widget.draw_mode != mode {
            self.widget.draw_mode = mode;
        }
        let _ = canvas;
        let widget = &self.widget;
        crate::owner_draw::paint_with(cx, &self.owner, |c| widget.paint_trigger(c, bounds, state));

        if self.widget.is_open() {
            let panel = self.widget.drop_down_rect_in(bounds, area);
            let pb = inflate(panel, FLOAT_SHADOW_MARGIN);
            let local = local_in(panel, pb);
            let recorded = if self.owner.active() {
                let mut handler = crate::owner_draw::ElementOwnerDraw { cx: &mut *cx, events: &self.owner, rows: None };
                self.widget.record_drop_down_items(local, &mut handler)
            } else {
                kubuno_ui::graphics::owner_draw::RecordedItems::new()
            };
            let snapshot = self.widget.clone();
            host::popup(pb, move |cv| crate::owner_draw::replay_in_popup(recorded, || snapshot.paint_drop_down_at(cv, local)));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// DatePicker — the calendar panel in a real `host::popup`, via
// `DatePicker::popup_drop_down` itself (it does its own `.clone()` +
// `host::popup`, exactly as `kubuno-ui/examples/gallery/pages/datetime.rs`'s
// interactive column relies on).
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: date_picker,
    name: "DatePicker",
    // Note: A date field with a floating calendar (`kubuno_ui::datetime::DatePicker`).
    doc: "A date field with a drop-down calendar.",
    ctor: kubuno_ui::datetime::DatePicker::new(),
    children: ChildrenModel::None,
    default_event: "OnValueChanged",
    props: [
        // Note: The selected date, ISO 8601 (`YYYY-MM-DD`).
        PropertyMeta::new("Date", PropKind::String, "", "Selected date, in the form YYYY-MM-DD."),
        PropertyMeta::new("Format", PropKind::Enum(&["Long", "Short", "Time"]), "Long",
            "How the date is shown: long date, short date, or a time.",
        ),
        PropertyMeta::new("Invalid", PropKind::Bool, "false", "Shows the field in the error colour."),
        // Note: Overrides the dropped panel's « today » marker (ISO `YYYY-MM-DD`) — for a test or a screenshot that needs a fixed date; empty (the default) reads the real local date every frame (`crate::clock::today`).
        PropertyMeta::new("Today", PropKind::String, "",
            "Date treated as today, in the form YYYY-MM-DD. Leave empty to use the current date.",
        ),
    ],
    events: [
        EventMeta::new("OnValueChanged", "Occurs when the date changes.").category(crate::registry::EventCategory::Action).args::<crate::events::TextChangedEventArgs>().aliases(&["OnChanged"]),
    ],
    smoke: |mut p| {
        p.invalid = true;
        p.open_panel();
        p.close_panel();
        p
    },
    build: |props, _cx| {
        let date = props.str("Date", "")?;
        let format = props.enum_("Format", "Long")?;
        let invalid = props.bool("Invalid", false)?;
        let today = props.str("Today", "")?;
        let focus_id = props.focus_id();
        let on_changed = props.event("OnChanged");
        Ok(Box::new(crate::registry::families::text::DatePickerNode::new(date, format, invalid, today, focus_id, on_changed)) as Box<dyn ViewNode>)
    },
}

fn parse_date_format(s: &str) -> DateTimePickerFormat {
    match s {
        "Short" => DateTimePickerFormat::Short,
        "Time" => DateTimePickerFormat::Time,
        _ => DateTimePickerFormat::Long,
    }
}

fn format_iso_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

fn parse_iso_date(s: &str) -> Option<Date> {
    let mut parts = s.splitn(4, '-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || day < 1 || day > Date::days_in_month(year, month) {
        return None;
    }
    Some(Date::new(year, month, day))
}

/// Handles a click on the field: the check box toggles, a spin button steps
/// the selected segment, the value selects the segment under the pointer and
/// — when the field has a panel — toggles it. The exact behaviour
/// `kubuno-ui`'s own gallery (`datetime.rs`'s `click_field`) drives the real
/// widget with; reproduced here rather than reached through (examples are
/// not part of the crate's public API).
fn click_date_field(c: &dyn Canvas, picker: &mut UiDatePicker, bounds: Rect, x: f32, y: f32) {
    match picker.field_at(bounds, x, y) {
        Some(FieldPart::CheckBox) => picker.checked = !picker.checked,
        Some(FieldPart::SpinUp) => picker.step(1),
        Some(FieldPart::SpinDown) => picker.step(-1),
        Some(FieldPart::Value) => {
            if let Some(seg) = picker.segment_at(c, bounds, x, y) {
                picker.select_segment(seg);
            }
            if picker.has_panel() {
                picker.toggle_panel();
            }
        }
        None => {}
    }
}

pub struct DatePickerNode {
    pub date: PropSource<String>,
    pub format: PropSource<String>,
    pub invalid: PropSource<bool>,
    pub today: PropSource<String>,
    pub focus_id: Option<FocusId>,
    pub on_changed: Option<String>,
    widget: UiDatePicker,
    pressed: bool,
    panel_pressed: bool,
}

impl DatePickerNode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        date: PropSource<String>,
        format: PropSource<String>,
        invalid: PropSource<bool>,
        today: PropSource<String>,
        focus_id: Option<FocusId>,
        on_changed: Option<String>,
    ) -> Self {
        Self { date, format, invalid, today, focus_id, on_changed, widget: UiDatePicker::new(), pressed: false, panel_pressed: false }
    }
}

impl ViewNode for DatePickerNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let canvas: &dyn Canvas = cx.canvas;
        self.widget.viewport = Some(cx.frame.screen_area());
        self.widget.invalid = self.invalid.resolve(cx.vm);
        // Never left at the replica's `DEFAULT_TODAY` (`2000-01-01`)
        // sentinel — see `crate::clock`'s own doc.
        let today = parse_iso_date(&self.today.resolve(cx.vm)).unwrap_or_else(crate::clock::today);
        self.widget.calendar.set_today_date(today);
        let format = parse_date_format(&self.format.resolve(cx.vm));
        if self.widget.format != format {
            self.widget.format = format;
            self.widget.show_up_down = format == DateTimePickerFormat::Time;
        }

        let field = self.widget.field_rect(bounds);
        let (mx, my) = cx.frame.mouse;
        let hot = !cx.frame.pointer_outside() && field.contains(mx, my);
        let (down_now, clicked) = press_release(&mut self.pressed, hot, cx.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, field)).unwrap_or_default();

        // Only pull the bound value in while the field is not being edited —
        // the same reconciliation `crate::node::TextFieldNode` applies to its
        // own live buffer.
        if !focus_state.focused {
            let bound = self.date.resolve(cx.vm);
            if let Some(d) = parse_iso_date(&bound) {
                if d != self.widget.value().date {
                    self.widget.set_value(DateTime::at_midnight(d));
                    self.widget.sync_calendar();
                }
            }
        }

        let before = self.widget.value();

        if cx.frame.dismiss {
            self.widget.close_panel();
        }
        if clicked {
            click_date_field(canvas, &mut self.widget, bounds, mx, my);
        }

        if self.widget.open {
            let panel = self.widget.drop_down_rect(bounds);
            cx.focus.keep_focus_in(panel);
            let over_panel = panel.contains(mx, my);
            let (_, panel_clicked) = press_release(&mut self.panel_pressed, over_panel, cx.frame.mouse_down);
            if panel_clicked {
                match self.widget.calendar.header_at(panel, mx, my) {
                    Some(HeaderPart::Prev) => self.widget.calendar.prev_month(),
                    Some(HeaderPart::Next) => self.widget.calendar.next_month(),
                    Some(HeaderPart::Title) => {}
                    None => {
                        if let Some(date) = self.widget.day_at(bounds, mx, my) {
                            self.widget.pick(date);
                        }
                    }
                }
            }
            self.widget.calendar.hot_day = self.widget.day_at(bounds, mx, my).filter(|d| self.widget.calendar.is_selectable(*d));
            self.widget.calendar.hot_header = self.widget.calendar.header_at(panel, mx, my);

            if !focus_state.focused {
                self.widget.close_panel();
            }
        } else {
            self.panel_pressed = false;
        }

        if focus_state.focused {
            self.widget.take_input();
        }

        let after = self.widget.value();
        if after != before {
            let new_date = format_iso_date(after.date);
            if let Some(spec) = self.date.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::Str(new_date.clone()));
                }
            }
            let mut args = TextChangedEventArgs::new(format_iso_date(before.date), new_date.clone(), ChangeSource::User);
            cx.fire("OnValueChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(new_date), &mut args);
        }

        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        self.widget.paint_field_only(canvas, bounds, state);

        if let Some(pb) = self.widget.popup_drop_down(bounds) {
            cx.focus.keep_focus_in(pb);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// ColorField — the swatch and its floating ColorPicker, as `ColorField.tsx`:
// a click toggles a popover (placed by `ColorField::popover_rect`, to the
// LEFT of the swatch when there is room, clamped to the monitor) holding the
// full `ColorPicker`, driven by the picker's own interaction loop
// (`ColorPicker::pointer` / `ColorPicker::keyboard`). A press outside closes
// it, as the web's backdrop does; Escape closes it too. Every change is
// written back to a TwoWay `Color` binding and raises `OnValueChanged`.
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: color_field,
    name: "ColorField",
    // Note: A colour swatch (`kubuno_ui::color::ColorField`) that opens the full `ColorPicker` in a floating popover — hue ring, SV area in three shapes, harmonies, hex, RGB/HSV/HSL/CMYK/GRAY channels, chips, recent colours, eyedropper.
    doc: "A colour swatch that opens a colour picker.",
    ctor: kubuno_ui::color::ColorField::new(kubuno_ui::color::Color::default()),
    children: ChildrenModel::None,
    default_event: "OnValueChanged",
    props: [
        // Note: The swatch's colour, as a `#rrggbb` hex literal; TwoWay-bindable (the picker writes back `#rrggbb`).
        PropertyMeta::new("Color", PropKind::String, "#000000", "Colour shown, as #rrggbb."),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the swatch is clicked, which opens or closes its colour picker.").category(crate::registry::EventCategory::Action).args::<crate::events::CheckedChangedEventArgs>(),
        EventMeta::new("OnValueChanged", "Occurs when the colour is changed in the picker.").category(crate::registry::EventCategory::Action).args::<crate::events::TextChangedEventArgs>().aliases(&["OnChanged"]),
    ],
    smoke: |f| {
        f.open(true).size(32.0, 24.0)
    },
    build: |props, _cx| {
        let color = props.str("Color", "#000000")?;
        let focus_id = props.focus_id();
        let on_click = props.event("OnClick");
        let on_changed = props.event("OnValueChanged");
        Ok(Box::new(crate::registry::families::text::ColorFieldNode::new(color, focus_id, on_click, on_changed)) as Box<dyn ViewNode>)
    },
}

/// A focus id for part `slot` of the popover owned by `owner`.
fn popover_part_id(owner: Option<FocusId>, salt: u64, slot: usize) -> FocusId {
    let base = owner.map(|f| f.0).unwrap_or(0x9e37_79b9_7f4a_7c15);
    FocusId(base.rotate_left(17) ^ salt ^ (slot as u64).wrapping_mul(0x0100_0000_01b3))
}

/// The popup bounds for a panel painted with `shadow-2xl`: the panel grown
/// by the shadow's reach, and the panel's rectangle in the popup's own space.
fn popover_bounds(panel: Rect) -> (Rect, Rect) {
    let (l, t, r, b) = kubuno_ui::datetime::shadow_outset(&kubuno_ui::datetime::SHADOW_2XL);
    let bounds = Rect::new(panel.left - l, panel.top - t, panel.right + r, panel.bottom + b);
    let local = Rect::new(l, t, l + (panel.right - panel.left), t + (panel.bottom - panel.top));
    (bounds, local)
}

pub struct ColorFieldNode {
    pub color: PropSource<String>,
    pub focus_id: Option<FocusId>,
    pub on_click: Option<String>,
    pub on_changed: Option<String>,
    widget: color::ColorField,
    picker: color::ColorPicker,
    pressed: bool,
    prev_down: bool,
}

impl ColorFieldNode {
    pub fn new(color: PropSource<String>, focus_id: Option<FocusId>, on_click: Option<String>, on_changed: Option<String>) -> Self {
        Self {
            color,
            focus_id,
            on_click,
            on_changed,
            widget: color::ColorField::new(color::Color::default()),
            picker: color::ColorPicker::default(),
            pressed: false,
            prev_down: false,
        }
    }

    fn set_open(&mut self, cx: &mut PaintCx<'_>, open: bool) {
        if self.widget.open == open {
            return;
        }
        self.widget.open = open;
        if open {
            self.picker.set_color(self.widget.color);
        } else {
            self.picker.end_edit();
            self.picker.drag = None;
        }
        cx.fire("OnClick", self.focus_id, self.on_click.as_deref(), ViewEventKind::Clicked, &mut CheckedChangedEventArgs::new(!open, open, ChangeSource::User));
    }
}

impl ViewNode for ColorFieldNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    /// `ColorField::measure` is a fixed swatch size (`32×24` by default), not
    /// `w-full`: without this a `<Stack>` column would stretch the swatch
    /// into a full-width colour bar instead of the small trigger it is.
    fn intrinsic_width(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Option<f32> {
        Some(self.widget.measure(c).width)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let canvas: &dyn Canvas = cx.canvas;
        // The bound colour wins while the picker is closed; while it is open
        // the picker owns the value (and writes it back).
        if !self.widget.open {
            if let Some(c) = color::parse(&self.color.resolve(cx.vm)) {
                self.widget.color = c;
            }
        }

        let (mx, my) = cx.frame.mouse;
        let away = cx.frame.pointer_outside();
        let hot = !away && bounds.contains(mx, my);
        let pressed_now = cx.frame.mouse_down && !self.prev_down;
        self.prev_down = cx.frame.mouse_down;
        let (down_now, clicked) = press_release(&mut self.pressed, hot, cx.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();
        if hot {
            host::set_cursor(host::Cursor::Hand);
        }

        // The eyedropper runs whatever the pointer does; while it is armed
        // (and until its click is released) presses go nowhere else.
        if self.widget.open {
            self.picker.poll_eyedropper();
        }
        let swallow = self.picker.swallows_pointer();

        if clicked && !swallow {
            let open = !self.widget.open;
            self.set_open(cx, open);
        } else if focus_state.focused
            && !self.widget.open
            && (host::take_key(vk::ENTER, Modifiers::NONE) > 0 || host::take_key(vk::SPACE, Modifiers::NONE) > 0)
        {
            self.set_open(cx, true);
        }

        if self.widget.open {
            let size = (self.picker.measure(canvas).width, self.picker.height_for_width(color::pm::WIDTH));
            let panel = color::ColorField::popover_rect(bounds, size, cx.frame.screen_area());
            cx.focus.keep_focus_in(panel);
            let inside = panel.contains(mx, my);
            let mut close = false;
            if !swallow {
                if (pressed_now && !inside && !bounds.contains(mx, my)) || cx.frame.dismiss {
                    close = true;
                }
                if !self.picker.is_picking() && host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 {
                    close = true;
                }
            }
            let before = self.picker.color();
            let pointer = color::PickerPointer { x: mx, y: my, down: cx.frame.mouse_down, pressed: pressed_now && inside, away };
            let mut ev = self.picker.pointer(canvas, panel, pointer);
            self.picker.focus = None;
            self.picker.focus_visible = false;
            for (part, rect) in self.picker.tab_stops(panel) {
                let opts = if part.is_text() { FocusOpts::TEXT } else { FocusOpts::default() };
                let st = cx.focus.register_with(popover_part_id(self.focus_id, 0xC010, part.focus_slot()), rect, opts);
                if st.focused {
                    self.picker.focus = Some(part);
                    self.picker.focus_visible = st.visible;
                }
            }
            let kev = self.picker.keyboard(panel, cx.frame.window_focused);
            if ev == color::PickerEvent::None {
                ev = kev;
            }
            let after = self.picker.color();
            if !after.same_swatch(before) || !after.same_swatch(self.widget.color) {
                let old = self.widget.color.rgb.to_hex();
                let new = after.rgb.to_hex();
                self.widget.color = after;
                if old != new {
                    if let Some(spec) = self.color.binding() {
                        if spec.mode.writes_back() {
                            spec.update_source(cx.vm, Value::Str(new.clone()));
                        }
                    }
                    let mut args = TextChangedEventArgs::new(old, new.clone(), ChangeSource::User);
                    cx.fire("OnValueChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(new), &mut args);
                }
            }
            if ev == color::PickerEvent::Close {
                close = true;
            }
            if close {
                self.set_open(cx, false);
                if let Some(id) = self.focus_id {
                    cx.focus.focus_visibly(id);
                }
            } else {
                let (pb, local) = popover_bounds(panel);
                let p = self.picker.clone();
                host::popup(pb, move |canvas| p.paint(canvas, local, kubuno_ui::WidgetState::REST));
            }
        }

        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        self.widget.paint(canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// GradientField — `GradientField` in `GradientPicker.tsx`: a swatch showing
// the gradient that opens a `GradientPicker` (with its ✕) in the same kind
// of popover. The value is the web's own serialisation, `gradientToCss`
// (`linear-gradient(90deg, rgba(…) 0%, …)` / `radial-gradient(circle, …)`),
// TwoWay-bindable.
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: gradient_field,
    name: "GradientField",
    // Note: A gradient swatch (`kubuno_ui::color::GradientField`) that opens a `GradientPicker` — linear/radial, draggable stops, angle, per-stop colour/position/opacity — in a floating popover.
    doc: "A gradient swatch that opens a gradient picker.",
    ctor: kubuno_ui::color::GradientField::new(kubuno_ui::color::Gradient::default()),
    children: ChildrenModel::None,
    default_event: "OnValueChanged",
    props: [
        // Note: The gradient as CSS (`gradientToCss`): `linear-gradient(<angle>deg, rgba(r, g, b, a) <p>%, …)` or `radial-gradient(circle, …)`. Empty = the web's `DEFAULT_GRADIENT`.
        PropertyMeta::new("Value", PropKind::String, "", "Gradient shown, as a CSS linear-gradient(...) or radial-gradient(circle, ...)."),
    ],
    events: [
        EventMeta::new("OnValueChanged", "Occurs when the gradient is changed in the picker.").category(crate::registry::EventCategory::Action).args::<crate::events::TextChangedEventArgs>().aliases(&["OnChanged"]),
    ],
    smoke: |f| {
        f.open(true).size(32.0, 24.0)
    },
    build: |props, _cx| {
        let value = props.str("Value", "")?;
        let focus_id = props.focus_id();
        let on_changed = props.event("OnValueChanged");
        Ok(Box::new(crate::registry::families::text::GradientFieldNode::new(value, focus_id, on_changed)) as Box<dyn ViewNode>)
    },
}

/// What the gradient popover is being dragged by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GradientDrag {
    Stop(usize),
    Angle,
    Opacity,
}

pub struct GradientFieldNode {
    pub value: PropSource<String>,
    pub focus_id: Option<FocusId>,
    pub on_changed: Option<String>,
    widget: color::GradientField,
    picker: color::GradientPicker,
    /// The selected stop's own `ColorPicker`, when its field is open.
    stop_picker: Option<color::ColorPicker>,
    drag: Option<GradientDrag>,
    pressed: bool,
    prev_down: bool,
}

impl GradientFieldNode {
    pub fn new(value: PropSource<String>, focus_id: Option<FocusId>, on_changed: Option<String>) -> Self {
        Self {
            value,
            focus_id,
            on_changed,
            widget: color::GradientField::new(color::Gradient::default()),
            picker: color::GradientPicker::default(),
            stop_picker: None,
            drag: None,
            pressed: false,
            prev_down: false,
        }
    }

    fn set_open(&mut self, open: bool) {
        self.widget.open = open;
        if open {
            self.picker = self.widget.picker();
        } else {
            self.picker.end_edit();
            self.stop_picker = None;
            self.drag = None;
        }
    }

    /// One frame of the popover's `GradientPicker`: the pointer (stops,
    /// bar, type buttons, sliders, boxes, ✕) and the keyboard. Returns
    /// whether it asked to close.
    fn run_picker(&mut self, cx: &mut PaintCx<'_>, panel: Rect, pressed: bool) -> bool {
        use color::{GradientKind, GradientPart, GradientPicker};
        let (mx, my) = cx.frame.mouse;
        let gp = &mut self.picker;
        let g = gp.layout(panel);
        let mut close = false;
        let hovered = if self.drag.is_some() { None } else { gp.part_at(panel, mx, my) };
        gp.hot = hovered;
        match hovered {
            Some(GradientPart::Stop(_)) => host::set_cursor(host::Cursor::ResizeEW),
            Some(GradientPart::Field | GradientPart::Angle | GradientPart::Opacity) => host::set_cursor(host::Cursor::Hand),
            Some(part) if part.is_text() => host::set_cursor(host::Cursor::IBeam),
            _ => {}
        }
        let mut open_stop = false;
        if pressed {
            match hovered {
                Some(GradientPart::Stop(i)) => {
                    gp.selected = i;
                    self.drag = Some(GradientDrag::Stop(i));
                }
                Some(GradientPart::Bar) => {
                    let p = gp.position_at(panel, mx);
                    gp.selected = gp.gradient.add_stop(p);
                }
                Some(GradientPart::Add) => gp.selected = gp.gradient.add_stop(0.5),
                Some(GradientPart::Linear) => gp.gradient.kind = GradientKind::Linear,
                Some(GradientPart::Radial) => gp.gradient.kind = GradientKind::Radial,
                Some(GradientPart::Close) => close = true,
                Some(GradientPart::Bin) => {
                    gp.remove_selected();
                }
                Some(GradientPart::Field) => open_stop = true,
                Some(GradientPart::Angle) => {
                    if g.angle.is_some_and(|a| GradientPicker::slider_track(a).contains(mx, my)) {
                        self.drag = Some(GradientDrag::Angle);
                    }
                }
                Some(GradientPart::Opacity) => {
                    if GradientPicker::slider_track(g.opacity).contains(mx, my) {
                        self.drag = Some(GradientDrag::Opacity);
                    }
                }
                Some(part) if part.is_text() => gp.begin_edit(part),
                _ => {}
            }
        }
        if !cx.frame.mouse_down {
            self.drag = None;
        }
        if !cx.frame.pointer_outside() {
            match self.drag {
                Some(GradientDrag::Stop(i)) => {
                    let p = gp.position_at(panel, mx);
                    if let Some(st) = gp.gradient.stops.get_mut(i) {
                        st.position = p;
                    }
                }
                Some(GradientDrag::Angle) => {
                    if let Some(a) = gp.angle_at(panel, mx) {
                        gp.gradient.angle = a;
                    }
                }
                Some(GradientDrag::Opacity) => {
                    let v = gp.opacity_at(panel, mx);
                    let sel = gp.selected_index();
                    if let Some(st) = gp.gradient.stops.get_mut(sel) {
                        st.opacity = v;
                    }
                }
                None => {}
            }
        }
        gp.hot_stop = match self.drag {
            Some(GradientDrag::Stop(i)) => Some(i),
            _ => gp.stop_at(panel, mx, my),
        };
        gp.focus = None;
        gp.focus_visible = false;
        for (part, rect) in gp.tab_stops(panel) {
            let opts = if part.is_text() { FocusOpts::TEXT } else { FocusOpts::default() };
            let st = cx.focus.register_with(popover_part_id(self.focus_id, 0x6AD1, part.focus_slot()), rect, opts);
            if st.focused {
                gp.focus = Some(part);
                gp.focus_visible = st.visible;
            }
        }
        match gp.focus {
            Some(part) if part.is_text() => {
                if gp.draft(part).is_none() {
                    gp.begin_edit(part);
                }
                let outcome = gp.edit.as_mut().map(|(_, d)| d.take_input()).unwrap_or(color::DraftOutcome::Idle);
                match outcome {
                    color::DraftOutcome::Edited => {
                        gp.apply_edit();
                    }
                    color::DraftOutcome::Commit | color::DraftOutcome::Cancel => {
                        if outcome == color::DraftOutcome::Commit {
                            gp.apply_edit();
                        }
                        gp.begin_edit(part);
                    }
                    _ => {}
                }
                for (k, shift) in color::take_color_keys() {
                    gp.key(part, k, shift);
                }
                let phase = gp.draft(part).map(|d| d.last_input_ms).unwrap_or(0);
                gp.caret_on = cx.frame.window_focused && kubuno_ui::focus::caret_visible(phase);
            }
            Some(part) => {
                gp.end_edit();
                for (k, shift) in color::take_color_keys() {
                    gp.key(part, k, shift);
                }
                if host::take_key(vk::ENTER, Modifiers::NONE) > 0 || host::take_key(vk::SPACE, Modifiers::NONE) > 0 {
                    match part {
                        GradientPart::Linear => gp.gradient.kind = GradientKind::Linear,
                        GradientPart::Radial => gp.gradient.kind = GradientKind::Radial,
                        GradientPart::Close => close = true,
                        GradientPart::Bin => {
                            gp.remove_selected();
                        }
                        GradientPart::Add => gp.selected = gp.gradient.add_stop(0.5),
                        GradientPart::Field => open_stop = true,
                        _ => {}
                    }
                }
            }
            None => gp.end_edit(),
        }
        if open_stop {
            self.stop_picker = match self.stop_picker {
                Some(_) => None,
                None => gp.selected_stop().map(|s| color::ColorPicker::new(color::Color::opaque(s.color))),
            };
        }
        gp.field_open = self.stop_picker.is_some();
        close
    }
}

impl ViewNode for GradientFieldNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn intrinsic_width(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Option<f32> {
        Some(self.widget.measure(c).width)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let canvas: &dyn Canvas = cx.canvas;
        if !self.widget.open {
            let css = self.value.resolve(cx.vm);
            self.widget.gradient = if css.trim().is_empty() {
                color::Gradient::default()
            } else {
                color::Gradient::from_css(&css).unwrap_or_default()
            };
        }
        let (mx, my) = cx.frame.mouse;
        let away = cx.frame.pointer_outside();
        let hot = !away && bounds.contains(mx, my);
        let pressed_now = cx.frame.mouse_down && !self.prev_down;
        self.prev_down = cx.frame.mouse_down;
        let (down_now, clicked) = press_release(&mut self.pressed, hot, cx.frame.mouse_down);
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();
        if hot {
            host::set_cursor(host::Cursor::Hand);
        }
        if let Some(sp) = self.stop_picker.as_mut() {
            sp.poll_eyedropper();
        }
        let swallow = self.stop_picker.as_ref().is_some_and(|p| p.swallows_pointer());

        if clicked && !swallow {
            let open = !self.widget.open;
            self.set_open(open);
        } else if focus_state.focused
            && !self.widget.open
            && (host::take_key(vk::ENTER, Modifiers::NONE) > 0 || host::take_key(vk::SPACE, Modifiers::NONE) > 0)
        {
            self.set_open(true);
        }

        if self.widget.open {
            let w = self.picker.measure(canvas).width;
            let size = (w, self.picker.height_for_width(w));
            let panel = color::ColorField::popover_rect(bounds, size, cx.frame.screen_area());
            // The stop's own picker, opened from the panel's colour field.
            let stop_panel = self.stop_picker.as_ref().map(|sp| {
                let anchor = self.picker.layout(panel).field;
                let s = (sp.measure(canvas).width, sp.height_for_width(color::pm::WIDTH));
                color::ColorField::popover_rect(anchor, s, cx.frame.screen_area())
            });
            cx.focus.keep_focus_in(panel);
            if let Some(sp) = stop_panel {
                cx.focus.keep_focus_in(sp);
            }
            let in_stop = stop_panel.is_some_and(|r| r.contains(mx, my));
            let inside = panel.contains(mx, my);
            let mut close = false;
            if !swallow && ((pressed_now && !inside && !in_stop && !bounds.contains(mx, my)) || cx.frame.dismiss) {
                close = true;
            }
            let before = self.picker.gradient.to_css();
            if let (Some(sp), Some(spanel)) = (self.stop_picker.as_mut(), stop_panel) {
                if !sp.is_picking() && host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 {
                    self.stop_picker = None;
                } else {
                    let pointer = color::PickerPointer { x: mx, y: my, down: cx.frame.mouse_down, pressed: pressed_now && in_stop, away };
                    let ev = sp.pointer(canvas, spanel, pointer);
                    sp.focus = None;
                    sp.focus_visible = false;
                    for (part, rect) in sp.tab_stops(spanel) {
                        let opts = if part.is_text() { FocusOpts::TEXT } else { FocusOpts::default() };
                        let st = cx.focus.register_with(popover_part_id(self.focus_id, 0x5709, part.focus_slot()), rect, opts);
                        if st.focused {
                            sp.focus = Some(part);
                            sp.focus_visible = st.visible;
                        }
                    }
                    let kev = sp.keyboard(spanel, cx.frame.window_focused);
                    let chosen = sp.color().rgb;
                    let sel = self.picker.selected_index();
                    if let Some(stop) = self.picker.gradient.stops.get_mut(sel) {
                        stop.color = chosen;
                    }
                    if ev == color::PickerEvent::Close || kev == color::PickerEvent::Close {
                        self.stop_picker = None;
                    }
                }
            } else if host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 {
                close = true;
            }
            if self.stop_picker.is_some() && pressed_now && inside && !in_stop {
                // A press on the gradient panel closes the stop's picker, as
                // the web's backdrop does.
                self.stop_picker = None;
            }
            close |= self.run_picker(cx, panel, pressed_now && inside && !in_stop);
            let after = self.picker.gradient.to_css();
            if after != before {
                self.widget.gradient = self.picker.gradient.clone();
                if let Some(spec) = self.value.binding() {
                    if spec.mode.writes_back() {
                        spec.update_source(cx.vm, Value::Str(after.clone()));
                    }
                }
                let mut args = TextChangedEventArgs::new(before, after.clone(), ChangeSource::User);
                cx.fire("OnValueChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(after), &mut args);
            }
            if close {
                self.set_open(false);
                if let Some(id) = self.focus_id {
                    cx.focus.focus_visibly(id);
                }
            } else {
                let (pb, local) = popover_bounds(panel);
                let p = self.picker.clone();
                host::popup(pb, move |canvas| p.paint(canvas, local, kubuno_ui::WidgetState::REST));
                if let (Some(sp), Some(spanel)) = (self.stop_picker.clone(), stop_panel) {
                    let (sb, slocal) = popover_bounds(spanel);
                    host::popup(sb, move |canvas| sp.paint(canvas, slocal, kubuno_ui::WidgetState::REST));
                }
            }
        }

        let state = focus_state.apply(crate::common::rest().hot(hot).pressed(down_now));
        self.widget.paint(canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────

/// Every component this family declares, in declaration order.
pub const ALL: &[ComponentMeta] = &[
    option::META,
    text_area::META,
    search_field::META,
    masked_field::META,
    dropdown::META,
    combo_box::META,
    date_picker::META,
    color_field::META,
    gradient_field::META,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_area_grows_with_its_text_between_min_and_max_lines() {
        let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
        let vm = crate::binding::MapViewModel::new();
        let mut node = TextAreaNode::new(PropSource::Literal(String::new()), PropSource::Literal(String::new()), PropSource::Literal(false), None, None);
        let fixed = node.measure_for_width(&canvas, &vm, 300.0).height;
        node.lines = (PropSource::Literal(2.0), PropSource::Literal(4.0));
        let empty = node.measure_for_width(&canvas, &vm, 300.0).height;
        node.field.set_text("un
deux
trois");
        let three = node.measure_for_width(&canvas, &vm, 300.0).height;
        node.field.set_text("1
2
3
4
5
6");
        let six = node.measure_for_width(&canvas, &vm, 300.0).height;
        assert!(empty < three && three < six, "{empty} {three} {six}");
        assert_eq!(six, 4.0 * AREA_LINE + AREA_INSETS, "capped at MaxLines");
        assert!(fixed > 0.0);
    }

    #[test]
    fn every_component_is_registered_under_its_own_name() {
        let names: Vec<&str> = ALL.iter().map(|m| m.name).collect();
        assert_eq!(names, ["Option", "TextArea", "SearchField", "MaskedField", "Dropdown", "ComboBox", "DatePicker", "ColorField", "GradientField"]);
    }

    #[test]
    fn read_options_reads_value_and_label_and_falls_back_to_value() {
        use crate::ast::{AstNode, Document};
        use crate::syntax::parse;

        let src = r#"<Dropdown><Option Value="fr" Label="Français"/><Option Value="en"/></Dropdown>"#;
        let p = parse(src);
        let doc = Document::cast(p.syntax()).unwrap();
        let root = doc.root_element().unwrap();
        let options = read_options(&root);
        assert_eq!(options, vec![("fr".to_string(), "Français".to_string()), ("en".to_string(), "en".to_string())]);
    }

    #[test]
    fn iso_date_round_trips() {
        let d = Date::new(2026, 9, 25);
        assert_eq!(parse_iso_date(&format_iso_date(d)), Some(d));
        assert_eq!(parse_iso_date("not-a-date"), None);
        assert_eq!(parse_iso_date("2026-13-01"), None);
        assert_eq!(parse_iso_date("2026-02-30"), None);
    }

    #[test]
    fn resolve_bound_options_reads_display_and_value_members_and_falls_back_to_value() {
        let vm = crate::binding::MapViewModel::new().with(
            "Langs",
            Value::from(vec![
                crate::binding::Row::new().with("Code", Value::Str("fr".to_string())).with("Name", Value::Str("Français".to_string())),
                // No `Name` field: the label falls back to the value, same as a static `<Option>` with no `Label`.
                crate::binding::Row::new().with("Code", Value::Str("en".to_string())),
            ]),
        );
        let spec = Some(crate::binding::BindingSpec { path: "Langs".to_string(), mode: crate::binding::BindingMode::OneWay, ..Default::default() });
        let resolved = resolve_bound_options(&vm, &spec, "Name", "Code");
        assert_eq!(resolved, Some(vec![("fr".to_string(), "Français".to_string()), ("en".to_string(), "en".to_string())]));

        // Unbound, or bound to something that is not currently a list: the
        // caller's cue to keep its static `<Option>`-built list.
        assert_eq!(resolve_bound_options(&vm, &None, "Name", "Code"), None);
        let missing = Some(crate::binding::BindingSpec { path: "NotThere".to_string(), mode: crate::binding::BindingMode::OneWay, ..Default::default() });
        assert_eq!(resolve_bound_options(&vm, &missing, "Name", "Code"), None);
    }

    #[test]
    fn dropdown_and_combo_box_compile_with_items_source_binding() {
        let reg: Vec<ComponentMeta> = ALL.to_vec();
        let src = r#"<Dropdown SelectedValue="{Binding Lang}" ItemsSource="{Binding Langs}" DisplayMember="Name" ValueMember="Code"/>"#;
        assert!(crate::compile::compile_with_registry(src, &reg).is_ok());
        let src = r#"<ComboBox SelectedValue="{Binding Lang}" ItemsSource="{Binding Langs}"/>"#;
        assert!(crate::compile::compile_with_registry(src, &reg).is_ok());
    }

    #[test]
    fn dropdown_and_combo_box_compile_with_option_children() {
        let reg: Vec<ComponentMeta> = ALL.to_vec();
        let src = r#"<Dropdown SelectedValue="{Binding Lang, Mode=TwoWay}"><Option Value="fr" Label="Français"/><Option Value="en" Label="English"/></Dropdown>"#;
        assert!(crate::compile::compile_with_registry(src, &reg).is_ok());
        let src = r#"<ComboBox SelectedValue="{Binding Lang}"><Option Value="fr" Label="Français"/></ComboBox>"#;
        assert!(crate::compile::compile_with_registry(src, &reg).is_ok());
    }

    #[test]
    fn date_picker_compiles_and_rejects_a_bad_format() {
        let reg: Vec<ComponentMeta> = ALL.to_vec();
        assert!(crate::compile::compile_with_registry(r#"<DatePicker Date="2026-09-25" Format="Short"/>"#, &reg).is_ok());
        let err = crate::compile::compile_with_registry(r#"<DatePicker Format="Weird"/>"#, &reg);
        assert!(err.is_err());
    }

    #[test]
    fn text_area_search_field_and_masked_field_compile() {
        let reg: Vec<ComponentMeta> = ALL.to_vec();
        assert!(crate::compile::compile_with_registry(r#"<TextArea Text="{Binding Notes, Mode=TwoWay}" Placeholder="Notes"/>"#, &reg).is_ok());
        assert!(crate::compile::compile_with_registry(r#"<SearchField Text="{Binding Query, Mode=TwoWay}"/>"#, &reg).is_ok());
        assert!(crate::compile::compile_with_registry(r#"<MaskedField Mask="00/00/0000" Text="{Binding Proxy, Mode=TwoWay}"/>"#, &reg).is_ok());
    }

    #[test]
    fn color_field_compiles() {
        let reg: Vec<ComponentMeta> = ALL.to_vec();
        assert!(crate::compile::compile_with_registry(r#"<ColorField Color="{Binding Accent}"/>"#, &reg).is_ok());
        assert!(crate::compile::compile_with_registry(r#"<ColorField Color="{Binding Accent, Mode=TwoWay}" OnValueChanged="Changed"/>"#, &reg).is_ok());
        assert!(crate::compile::compile_with_registry(r#"<GradientField Value="{Binding Fill, Mode=TwoWay}"/>"#, &reg).is_ok());
    }
}
