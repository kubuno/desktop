//! Form fields — the data-entry layer the web calls its "Contacts style"
//! primitives.
//!
//! Every other UI family (buttons, lists, dialogs, editors…) was ported before
//! this one, which is why the desktop looked complete for a document app and
//! thin for a form. These are the fields a Contacts window, a settings page or a
//! share dialog is built from.
//!
//! They all sit on ONE base, [`outlined::OutlinedField`] — the Material outlined
//! text field whose label starts inside the box and floats up onto the border
//! when the field is focused or holds a value. The composites are thin layers
//! over it, exactly as on the web, so the notch, the focus colour and the
//! spacing are defined once:
//!
//! * [`FieldGroup`] — `@ui/FieldGroup`: several sub-fields stacked under ONE
//!   shared icon, with a Plus/Moins chevron revealing the « advanced » ones;
//! * [`LabelCombobox`] — `@ui/LabelCombobox`: a free-text « Libellé » field with
//!   a dropdown of presets that narrow as you type (hosted in a popup, so it
//!   escapes its container and the window);
//! * [`FloatCheckbox`] — `@ui/FloatCheckbox`: the round selection check floating
//!   over a media card.
//!
//! Still web-only: `AddressField`, `PhoneField`, `DateField` (they need the
//! country table and the date picker wired through), `LabelField` and the
//! `mention` popup.

pub mod outlined;

pub use outlined::{Clipboard, FieldResponse, HostClipboard, KeyOutcome, OutlinedField};

use drive_app_controls::themes::shape::SHADOW_WAFFLE;
use drive_app_controls::{Canvas, Rect};
use kubuno_controls::buttons::CheckBox;
use kubuno_controls::containers::Panel;
use kubuno_controls::enums::Size;
use kubuno_controls::host::{vk, Modifiers};
use kubuno_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::metrics::{pill, ShadowLayer};
use crate::widget::{Widget, WidgetState};

/// The composites' metrics, each with its web source.
mod m {
    /// `FieldGroup`/`AddressField`: `gap: 12` between the icon gutter, the
    /// fields and the chevron gutter.
    pub const GUTTER_GAP: f32 = 12.0;
    /// `gap: 10` between stacked sub-fields.
    pub const FIELD_GAP: f32 = 10.0;
    /// The shared icon (`MapPin size={24}` in `AddressField`; the Contacts
    /// groups pass the same size).
    pub const GROUP_ICON: f32 = 24.0;
    /// The Plus/Moins button: `width: 32, height: 32, borderRadius: 50%`, its
    /// chevron `size={20}`.
    pub const TOGGLE: f32 = 32.0;
    pub const TOGGLE_GLYPH: f32 = 20.0;
    /// `LabelCombobox`: `width: large ? 200 : 170`.
    pub const COMBO_W: f32 = 170.0;
    pub const COMBO_W_LARGE: f32 = 200.0;
    /// The list: `top: FIELD_H + 4`, `maxHeight: 260`, `borderRadius: 8`.
    pub const LIST_GAP: f32 = 4.0;
    pub const LIST_MAX_H: f32 = 260.0;
    pub const LIST_RADIUS: f32 = 8.0;
    /// A preset row: `padding: '10px 16px'` around a body line.
    pub const ITEM_PAD_X: f32 = 16.0;
    pub const ITEM_H: f32 = 36.0;
    /// Kept off the screen's edges when the list is placed.
    pub const EDGE: f32 = 8.0;
    /// How far `SHADOW_WAFFLE` reaches past the list (`0 4px 8px 3px`): 11 to
    /// the sides, 15 below — a popup hosting the list covers it.
    pub const SHADOW_SIDE: f32 = 12.0;
    pub const SHADOW_BELOW: f32 = 16.0;
    /// `FloatCheckbox`: `w-5 h-5 rounded-full border-2`, the ✓ at 10 px bold
    /// (a 12 DIP check glyph reads the same).
    pub const FLOAT_CHECK: f32 = 20.0;
    pub const FLOAT_BORDER: f32 = 2.0;
    pub const FLOAT_GLYPH: f32 = 12.0;
    /// `bg-black/30` behind an unselected check.
    pub const FLOAT_SCRIM_ALPHA: f32 = 0.3;
    /// The keyboard focus ring every desktop control draws.
    pub const FOCUS_RING: f32 = 2.0;
}

/// Tailwind's `shadow-sm`: `0 1px 2px 0 rgb(0 0 0 / .05)`.
const SHADOW_SM: [ShadowLayer; 1] = [ShadowLayer { dy: 1.0, blur: 2.0, spread: 0.0, opacity: 0.05 }];
const SHADOW_BLACK: (f32, f32, f32) = (0.0, 0.0, 0.0);

fn fade(c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * a, ..c }
}

// ═════════════════════════════════════════════════════════════════════════════
// FieldGroup
// ═════════════════════════════════════════════════════════════════════════════

/// One sub-field of a [`FieldGroup`].
pub struct GroupField {
    /// The value's key in the caller's record (`value[key]` on the web).
    pub key: String,
    /// Hidden until the group is expanded (Plus).
    pub advanced: bool,
    /// The field itself — a plain outlined field WITHOUT its own icon: the
    /// group owns the icon.
    pub field: OutlinedField,
}

/// Where a [`FieldGroup`]'s parts sit inside its bounds.
pub struct GroupLayout {
    /// The shared icon's square, centred on the first field's box.
    pub icon: Option<Rect>,
    /// `(index into fields, bounds)` of every SHOWN field, top to bottom.
    pub fields: Vec<(usize, Rect)>,
    /// The Plus/Moins button, when the group has advanced fields.
    pub toggle: Option<Rect>,
}

/// `@ui/FieldGroup` — several labelled sub-fields stacked under one shared
/// icon (a « Nom » block, an « Organisation » block…), with a chevron that
/// reveals the advanced ones.
pub struct FieldGroup {
    inner: Panel,
    /// The shared icon, by name; `None` keeps the gutter empty.
    pub icon: Option<&'static str>,
    pub fields: Vec<GroupField>,
    /// Whether the advanced sub-fields are shown (`expanded`).
    pub expanded: bool,
    /// The `large` variant, handed to every sub-field.
    pub large: bool,
}

impl FieldGroup {
    pub fn new(icon: Option<&'static str>) -> Self {
        Self { inner: Panel::new(), icon, fields: Vec::new(), expanded: false, large: false }
    }

    /// Builder: a sub-field shown at rest.
    pub fn field(mut self, key: &str, label: &str) -> Self {
        self.fields.push(GroupField { key: key.into(), advanced: false, field: OutlinedField::new(label).large(self.large) });
        self
    }

    /// Builder: a sub-field shown only once expanded.
    pub fn advanced(mut self, key: &str, label: &str) -> Self {
        self.fields.push(GroupField { key: key.into(), advanced: true, field: OutlinedField::new(label).large(self.large) });
        self
    }

    /// The field under `key`.
    pub fn get(&self, key: &str) -> Option<&OutlinedField> {
        self.fields.iter().find(|g| g.key == key).map(|g| &g.field)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut OutlinedField> {
        self.fields.iter_mut().find(|g| g.key == key).map(|g| &mut g.field)
    }

    pub fn has_advanced(&self) -> bool {
        self.fields.iter().any(|g| g.advanced)
    }

    /// Plus / Moins.
    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }

    /// The indices of the fields currently shown.
    pub fn shown(&self) -> Vec<usize> {
        (0..self.fields.len()).filter(|&i| self.expanded || !self.fields[i].advanced).collect()
    }

    fn field_height(&self, i: usize) -> f32 {
        self.fields[i].field.outer_height()
    }

    /// Builder: the `large` variant, handed down to every sub-field (present
    /// and future), as the web passes `large` to each `OutlinedField`.
    pub fn large(mut self, v: bool) -> Self {
        self.large = v;
        for g in &mut self.fields {
            g.field.large = v;
        }
        self
    }

    /// Lays the group out in `bounds`. Pure: the icon gutter, the stacked
    /// fields (each at its own true height), the chevron gutter.
    pub fn layout(&self, bounds: Rect) -> GroupLayout {
        let left = bounds.left + m::GROUP_ICON + m::GUTTER_GAP;
        let right = if self.has_advanced() { bounds.right - m::TOGGLE - m::GUTTER_GAP } else { bounds.right };
        let right = right.max(left);
        let mut y = bounds.top;
        let mut fields = Vec::new();
        for i in self.shown() {
            let h = self.field_height(i);
            fields.push((i, Rect::new(left, y, right, y + h)));
            y += h + m::FIELD_GAP;
        }
        // The icon and the chevron are centred on the FIRST field's box (the
        // web's `height: rowH` gutter), not on its headroom.
        let first_box = fields
            .first()
            .map(|&(i, r)| self.fields[i].field.box_rect(r))
            .unwrap_or(Rect::new(left, bounds.top, right, bounds.top + outlined::HEADROOM + outlined::HEIGHT));
        let cy = (first_box.top + first_box.bottom) / 2.0;
        let icon = self.icon.map(|_| {
            let h = m::GROUP_ICON / 2.0;
            Rect::new(bounds.left, cy - h, bounds.left + m::GROUP_ICON, cy + h)
        });
        let toggle = self.has_advanced().then(|| {
            let h = m::TOGGLE / 2.0;
            Rect::new(bounds.right - m::TOGGLE, cy - h, bounds.right, cy + h)
        });
        GroupLayout { icon, fields, toggle }
    }

    /// Paints the group with a state per sub-field (hover, focus…) and one for
    /// the chevron — what a live form passes; [`Widget::paint`] paints them
    /// all at rest.
    pub fn paint_parts(
        &self,
        c: &dyn Canvas,
        bounds: Rect,
        field_state: &dyn Fn(usize) -> WidgetState,
        toggle_state: WidgetState,
    ) {
        let t = c.theme();
        let l = self.layout(bounds);
        if let (Some(r), Some(icon)) = (l.icon, self.icon) {
            c.vector_icon(icon, &r, m::GROUP_ICON, &t.text_secondary);
        }
        for &(i, r) in &l.fields {
            self.fields[i].field.paint(c, r, field_state(i));
        }
        if let Some(r) = l.toggle {
            paint_toggle(c, r, self.expanded, toggle_state);
        }
    }
}

/// The round Plus/Moins chevron button: transparent, `#f1f3f4`
/// (`surface-2`) on hover, the chevron in the primary colour once expanded.
fn paint_toggle(c: &dyn Canvas, r: Rect, expanded: bool, state: WidgetState) {
    let t = c.theme();
    let round = pill(r.bottom - r.top);
    if state.hot && !state.disabled {
        c.fill_rounded(&r, round, &t.surface_2);
    }
    let glyph = if expanded { "ChevronUp" } else { "ChevronDown" };
    let colour = if expanded { t.accent } else { t.text_secondary };
    c.vector_icon(glyph, &r, m::TOGGLE_GLYPH, &colour);
    if state.show_focus_ring() {
        c.stroke_rounded_w(&r, round, &t.accent, m::FOCUS_RING);
    }
}

impl Widget for FieldGroup {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn type_name(&self) -> &'static str {
        "FieldGroup"
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        let shown = self.shown();
        let h: f32 = shown.iter().map(|&i| self.field_height(i)).sum::<f32>()
            + m::FIELD_GAP * shown.len().saturating_sub(1) as f32;
        let toggle = if self.has_advanced() { m::TOGGLE + m::GUTTER_GAP } else { 0.0 };
        Size::new(m::GROUP_ICON + m::GUTTER_GAP + 240.0 + toggle, h)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let disabled = state.disabled;
        self.paint_parts(canvas, bounds, &|_| WidgetState::REST.disabled(disabled), WidgetState::REST.disabled(disabled));
    }

    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        let l = self.layout(bounds);
        l.toggle.is_some_and(|r| r.contains(x, y)) || l.fields.iter().any(|&(i, r)| self.fields[i].field.hit_test(r, x, y))
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// LabelCombobox
// ═════════════════════════════════════════════════════════════════════════════

/// What a key did to a [`LabelCombobox`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComboKey {
    /// Not the combobox's key.
    Ignored,
    /// The list opened or its active row moved.
    Moved,
    /// A preset was picked into the field (Enter on the active row).
    Picked,
    /// The list closed (Escape) — the field keeps the focus.
    Closed,
}

/// `@ui/LabelCombobox` — an editable « Libellé »: an outlined field whose
/// value is FREE TEXT, plus a dropdown of presets that narrow as you type. A
/// combobox, not a closed select: a custom label is always kept.
///
/// The list opens when the field takes the focus (the web's `onFocus`) and
/// closes on a pick, a press outside, Escape or blur. The web offers no keys on
/// it; the desktop adds the ARIA combobox ones: Down/Up move the active row
/// (Alt+Down opens), Enter picks it, Escape closes the list and nothing else.
///
/// The list is a floating surface: [`LabelCombobox::place_list`] places it
/// against the SCREEN, and the caller hosts [`LabelCombobox::paint_list`] in an
/// interactive popup covering [`LabelCombobox::list_paint_bounds`].
pub struct LabelCombobox {
    pub field: OutlinedField,
    pub presets: Vec<String>,
    /// Whether the list is open.
    pub open: bool,
    /// The keyboard-active row, as an index into [`LabelCombobox::suggestions`].
    pub active: Option<usize>,
    /// The list's scroll offset, when the presets outgrow its 260 DIP.
    pub scroll: f32,
}

impl LabelCombobox {
    pub fn new(label: &str, presets: &[&str]) -> Self {
        Self {
            field: OutlinedField::new(label),
            presets: presets.iter().map(|s| s.to_string()).collect(),
            open: false,
            active: None,
            scroll: 0.0,
        }
    }

    /// The web's fixed width (`large ? 200 : 170`).
    pub fn width(&self) -> f32 {
        if self.field.large { m::COMBO_W_LARGE } else { m::COMBO_W }
    }

    /// The presets matching the typed text (case-insensitive `includes`),
    /// as indices into `presets` — all of them when the field is empty.
    pub fn suggestions(&self) -> Vec<usize> {
        let q = self.field.text.trim().to_lowercase();
        (0..self.presets.len()).filter(|&i| q.is_empty() || self.presets[i].to_lowercase().contains(&q)).collect()
    }

    /// Whether the list actually shows (open, and something matches).
    pub fn list_visible(&self) -> bool {
        self.open && !self.suggestions().is_empty()
    }

    /// The list's size for `field_width`.
    pub fn list_size(&self, field_width: f32) -> Size {
        let n = self.suggestions().len() as f32;
        Size::new(field_width, (n * m::ITEM_H).min(m::LIST_MAX_H))
    }

    /// Places a list of `size` under the field's box (`top: FIELD_H + 4`),
    /// flipping above it when the screen has no room below and more above,
    /// and kept inside `screen`. Pure.
    pub fn place_list(box_rect: Rect, size: Size, screen: Rect) -> Rect {
        let below = screen.bottom - box_rect.bottom - m::LIST_GAP - m::EDGE;
        let above = box_rect.top - screen.top - m::LIST_GAP - m::EDGE;
        let h = size.height;
        let top = if below < h && above > below {
            box_rect.top - m::LIST_GAP - h.min(above.max(0.0))
        } else {
            box_rect.bottom + m::LIST_GAP
        };
        let h = if below < h && above > below { h.min(above.max(0.0)) } else { h.min(below.max(0.0)).max(m::ITEM_H.min(h)) };
        let lo = screen.left + m::EDGE;
        let hi = (screen.right - size.width - m::EDGE).max(lo);
        let left = box_rect.left.max(lo).min(hi);
        Rect::new(left, top, left + size.width, top + h)
    }

    /// Everything the list paints, its shadow included — the popup's bounds.
    pub fn list_paint_bounds(list: Rect) -> Rect {
        Rect::new(list.left - m::SHADOW_SIDE, list.top - m::SHADOW_SIDE, list.right + m::SHADOW_SIDE, list.bottom + m::SHADOW_BELOW)
    }

    /// The row (index into [`LabelCombobox::suggestions`]) under a point.
    pub fn item_at(&self, list: Rect, x: f32, y: f32) -> Option<usize> {
        if !list.contains(x, y) {
            return None;
        }
        let k = ((y - list.top + self.scroll) / m::ITEM_H).floor();
        let n = self.suggestions().len();
        (k >= 0.0 && (k as usize) < n).then_some(k as usize)
    }

    /// Picks row `k` of the suggestions into the field and closes the list.
    pub fn pick(&mut self, k: usize) -> bool {
        let Some(&i) = self.suggestions().get(k) else { return false };
        let v = self.presets[i].clone();
        self.field.set_value(&v);
        self.open = false;
        self.active = None;
        true
    }

    /// Scrolls the list so row `k` is visible.
    fn reveal(&mut self, k: usize, list_h: f32) {
        let top = k as f32 * m::ITEM_H;
        if top < self.scroll {
            self.scroll = top;
        } else if top + m::ITEM_H > self.scroll + list_h {
            self.scroll = top + m::ITEM_H - list_h;
        }
    }

    /// One key-down, for a focused combobox. Pure; the caller takes the key
    /// from the host queue unless the answer is [`ComboKey::Ignored`] — so
    /// every other key goes on to the field.
    pub fn key(&mut self, key: u16, mods: Modifiers) -> ComboKey {
        let n = self.suggestions().len();
        let list_h = (n as f32 * m::ITEM_H).min(m::LIST_MAX_H);
        match key {
            vk::DOWN | vk::UP if mods.matches(Modifiers::NONE) || mods.matches(Modifiers::ALT) => {
                if n == 0 {
                    return ComboKey::Ignored;
                }
                if !self.open || mods.alt {
                    self.open = true;
                    return ComboKey::Moved;
                }
                let next = match (self.active, key == vk::DOWN) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(a), true) => (a + 1).min(n - 1),
                    (Some(a), false) => a.saturating_sub(1),
                };
                self.active = Some(next);
                self.reveal(next, list_h);
                ComboKey::Moved
            }
            vk::ENTER if mods.matches(Modifiers::NONE) => match self.active {
                Some(k) if self.open && k < n => {
                    self.pick(k);
                    ComboKey::Picked
                }
                _ => ComboKey::Ignored,
            },
            vk::ESCAPE if self.open => {
                self.open = false;
                self.active = None;
                ComboKey::Closed
            }
            _ => ComboKey::Ignored,
        }
    }

    /// Whether [`LabelCombobox::key`] would act on `key` — decided before the
    /// key is taken from the host queue, so the rest reaches the field.
    pub fn handles_key(&self, key: u16, mods: Modifiers) -> bool {
        let n = self.suggestions().len();
        match key {
            vk::DOWN | vk::UP => n > 0 && (mods.matches(Modifiers::NONE) || mods.matches(Modifiers::ALT)),
            vk::ENTER => mods.matches(Modifiers::NONE) && self.open && self.active.is_some_and(|a| a < n),
            vk::ESCAPE => self.open,
            _ => false,
        }
    }

    /// Keeps the active row valid after the text (and so the suggestions)
    /// changed — the web re-filters on every keystroke.
    pub fn refilter(&mut self) {
        let n = self.suggestions().len();
        if self.active.is_some_and(|a| a >= n) {
            self.active = None;
        }
        let max = (n as f32 * m::ITEM_H - m::LIST_MAX_H).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    /// Paints the open list in `rect` (the popup's local space): the
    /// `SHADOW_WAFFLE` card, one row per suggestion, `hot` (pointer) and the
    /// keyboard-active row washed in `surface-2`. Rows are clipped to the
    /// rounded card and each label is ellipsised.
    pub fn paint_list(&self, c: &dyn Canvas, rect: Rect, hot: Option<usize>) {
        self.list_view().paint(c, rect, hot);
    }

    /// An owned snapshot of the open list — what a popup's `'static` paint
    /// closure captures, since the combobox itself stays with its caller.
    pub fn list_view(&self) -> ComboList {
        ComboList {
            items: self.suggestions().iter().map(|&i| self.presets[i].clone()).collect(),
            active: self.active,
            scroll: self.scroll,
        }
    }

    /// Scrolls the list by `dy` DIP (the wheel over it), clamped to its rows.
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll += dy;
        self.refilter();
    }
}

/// The rows a [`LabelCombobox`] list shows, detached from the combobox
/// ([`LabelCombobox::list_view`]) so a popup can paint it after the frame.
#[derive(Clone, Debug, Default)]
pub struct ComboList {
    pub items: Vec<String>,
    pub active: Option<usize>,
    pub scroll: f32,
}

impl ComboList {
    /// Paints the list in `rect`: the `SHADOW_WAFFLE` card, one row per item,
    /// `hot` (pointer) and the keyboard-active row washed in `surface-2`. Rows
    /// are clipped to the rounded card and each label is ellipsised.
    pub fn paint(&self, c: &dyn Canvas, rect: Rect, hot: Option<usize>) {
        let t = c.theme();
        let f = c.formats();
        c.draw_shadow(&rect, m::LIST_RADIUS, &SHADOW_WAFFLE, SHADOW_BLACK);
        c.fill_rounded(&rect, m::LIST_RADIUS, &t.layer_background);
        c.push_clip_rounded(&rect, m::LIST_RADIUS);
        for (k, item) in self.items.iter().enumerate() {
            let top = rect.top + k as f32 * m::ITEM_H - self.scroll;
            if top + m::ITEM_H < rect.top || top > rect.bottom {
                continue;
            }
            let row = Rect::new(rect.left, top, rect.right, top + m::ITEM_H);
            if hot == Some(k) || self.active == Some(k) {
                c.fill_rounded(&row, 0.0, &t.surface_2);
            }
            let text = Rect::new(row.left + m::ITEM_PAD_X, row.top, row.right - m::ITEM_PAD_X, row.bottom);
            c.text_ellipsis(item, &text, &f.body, &t.text_primary);
        }
        c.pop_clip_rounded();
        c.stroke_rounded(&rect, m::LIST_RADIUS, &t.card_stroke);
    }
}

impl Widget for LabelCombobox {
    fn model(&self) -> &dyn Control {
        self.field.model()
    }

    fn type_name(&self) -> &'static str {
        "LabelCombobox"
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        Size::new(self.width(), self.field.measure(canvas).height)
    }

    /// The field alone; the list is a floating surface painted by the caller
    /// in a popup ([`LabelCombobox::paint_list`]).
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.field.paint(canvas, bounds, state);
    }

    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.field.hit_test(bounds, x, y)
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// FloatCheckbox
// ═════════════════════════════════════════════════════════════════════════════

/// `@ui/FloatCheckbox` — the round check floating over a media card (files,
/// photos) for multi-select. Invisible at rest; it appears when its card is
/// hovered (`reveal`, the web's `group-hover`) and stays while selected.
///
/// Selected: `bg-primary border-primary` with a white ✓; unselected:
/// `bg-black/30 border-white`. On the web it is a `role="checkbox"` div with
/// no tabindex; the desktop makes it reachable (Space toggles, the caller
/// wires it) and shows it with its ring whenever it holds the keyboard focus.
pub struct FloatCheckbox {
    inner: CheckBox,
    /// The card under it is hovered (`group-hover:opacity-100`).
    pub reveal: bool,
}

impl Default for FloatCheckbox {
    fn default() -> Self {
        Self::new(false)
    }
}

impl FloatCheckbox {
    pub fn new(selected: bool) -> Self {
        let mut inner = CheckBox::new();
        inner.set_checked(selected);
        Self { inner, reveal: false }
    }

    pub fn selected(&self) -> bool {
        self.inner.checked()
    }

    pub fn set_selected(&mut self, v: bool) {
        self.inner.set_checked(v);
    }

    /// `onToggle`.
    pub fn toggle(&mut self) {
        let v = !self.selected();
        self.inner.set_checked(v);
    }

    /// Builder: revealed by its card's hover.
    pub fn reveal(mut self, v: bool) -> Self {
        self.reveal = v;
        self
    }

    /// Whether it paints at all in `state` (`opacity-0` otherwise).
    pub fn is_shown(&self, state: WidgetState) -> bool {
        self.selected() || self.reveal || state.hot || state.show_focus_ring()
    }
}

impl Widget for FloatCheckbox {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn type_name(&self) -> &'static str {
        "FloatCheckbox"
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        Size::new(m::FLOAT_CHECK, m::FLOAT_CHECK)
    }

    fn paint(&self, c: &dyn Canvas, bounds: Rect, state: WidgetState) {
        if !self.is_shown(state) {
            return;
        }
        let t = c.theme();
        // Centred in its bounds at its own 20 DIP.
        let cx = (bounds.left + bounds.right) / 2.0;
        let cy = (bounds.top + bounds.bottom) / 2.0;
        let h = m::FLOAT_CHECK / 2.0;
        let r = Rect::new(cx - h, cy - h, cx + h, cy + h);
        let round = pill(m::FLOAT_CHECK);
        c.draw_shadow(&r, round, &SHADOW_SM, SHADOW_BLACK);
        // The white of the web's `border-white` and ✓ is the accent's own
        // foreground (white in the light palette); `black/30` is the scrim.
        let (fill, edge) = if self.selected() {
            (t.accent, t.accent)
        } else {
            (fade(t.dialog_scrim, m::FLOAT_SCRIM_ALPHA / t.dialog_scrim.a.max(0.01)), t.accent_foreground)
        };
        c.fill_rounded(&r, round, &fill);
        c.stroke_rounded_w(&r, round, &edge, m::FLOAT_BORDER);
        if self.selected() {
            c.vector_icon("Check", &r, m::FLOAT_GLYPH, &t.accent_foreground);
        }
        if state.show_focus_ring() {
            let o = m::FOCUS_RING;
            let ring = Rect::new(r.left - o, r.top - o, r.right + o, r.bottom + o);
            c.stroke_rounded_w(&ring, pill(m::FLOAT_CHECK + 2.0 * o), &t.accent, m::FOCUS_RING);
        }
    }

    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        crate::buttons::circular_hit(bounds, x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> FieldGroup {
        FieldGroup::new(Some("User")).field("first", "Prénom").field("last", "Nom").advanced("middle", "Deuxième prénom")
    }

    #[test]
    fn a_group_hides_its_advanced_fields_until_expanded() {
        let mut g = group();
        assert_eq!(g.shown(), vec![0, 1]);
        g.toggle();
        assert_eq!(g.shown(), vec![0, 1, 2]);
    }

    #[test]
    fn the_group_stacks_fields_at_their_true_height_between_its_gutters() {
        let g = group();
        let b = Rect::new(0.0, 0.0, 400.0, 400.0);
        let l = g.layout(b);
        assert_eq!(l.fields.len(), 2);
        let (_, a) = l.fields[0];
        let (_, c) = l.fields[1];
        assert_eq!(a.left, m::GROUP_ICON + m::GUTTER_GAP, "after the icon gutter");
        assert_eq!(a.right, 400.0 - m::TOGGLE - m::GUTTER_GAP, "before the chevron gutter");
        assert_eq!(a.bottom - a.top, outlined::HEADROOM + outlined::HEIGHT);
        assert_eq!(c.top, a.bottom + m::FIELD_GAP);
        // Icon and chevron centred on the first BOX, below the headroom.
        let icon = l.icon.expect("an icon");
        let mid = outlined::HEADROOM + outlined::HEIGHT / 2.0;
        assert_eq!((icon.top + icon.bottom) / 2.0, mid);
        let t = l.toggle.expect("a toggle");
        assert_eq!((t.top + t.bottom) / 2.0, mid);
    }

    #[test]
    fn a_group_without_advanced_fields_has_no_chevron_gutter() {
        let g = FieldGroup::new(None).field("a", "A");
        let l = g.layout(Rect::new(0.0, 0.0, 300.0, 100.0));
        assert!(l.toggle.is_none());
        assert!(l.icon.is_none());
        assert_eq!(l.fields[0].1.right, 300.0);
    }

    fn combo() -> LabelCombobox {
        LabelCombobox::new("Libellé", &["Domicile", "Professionnel", "Mobile", "Autre"])
    }

    #[test]
    fn the_presets_narrow_as_you_type() {
        let mut c = combo();
        assert_eq!(c.suggestions().len(), 4);
        c.field.set_value("MO");
        assert_eq!(c.suggestions(), vec![2], "case-insensitive includes");
        c.field.set_value("o");
        assert_eq!(c.suggestions(), vec![0, 1, 2]);
        c.field.set_value("zzz");
        assert!(c.suggestions().is_empty());
        c.open = true;
        assert!(!c.list_visible(), "nothing matches: no empty list");
    }

    #[test]
    fn arrows_open_then_walk_and_enter_picks() {
        let mut c = combo();
        assert_eq!(c.key(vk::DOWN, Modifiers::NONE), ComboKey::Moved);
        assert!(c.open, "Down opens");
        c.key(vk::DOWN, Modifiers::NONE);
        c.key(vk::DOWN, Modifiers::NONE);
        assert_eq!(c.active, Some(1));
        c.key(vk::UP, Modifiers::NONE);
        assert_eq!(c.active, Some(0));
        assert_eq!(c.key(vk::ENTER, Modifiers::NONE), ComboKey::Picked);
        assert_eq!(c.field.text, "Domicile");
        assert!(!c.open);
        assert_eq!(c.key(vk::ENTER, Modifiers::NONE), ComboKey::Ignored, "closed: Enter goes on");
    }

    #[test]
    fn escape_closes_only_an_open_list() {
        let mut c = combo();
        assert_eq!(c.key(vk::ESCAPE, Modifiers::NONE), ComboKey::Ignored);
        c.open = true;
        assert_eq!(c.key(vk::ESCAPE, Modifiers::NONE), ComboKey::Closed);
        assert!(!c.open);
    }

    #[test]
    fn the_list_drops_below_or_flips_above_inside_the_screen() {
        let screen = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let size = Size::new(170.0, 144.0);
        let below = LabelCombobox::place_list(Rect::new(100.0, 100.0, 270.0, 148.0), size, screen);
        assert_eq!(below.top, 148.0 + m::LIST_GAP);
        let above = LabelCombobox::place_list(Rect::new(100.0, 700.0, 270.0, 748.0), size, screen);
        assert_eq!(above.bottom, 700.0 - m::LIST_GAP);
        let pushed = LabelCombobox::place_list(Rect::new(900.0, 100.0, 1070.0, 148.0), size, screen);
        assert!(pushed.right <= screen.right - m::EDGE);
    }

    #[test]
    fn rows_are_found_under_the_pointer() {
        let c = combo();
        let list = Rect::new(0.0, 0.0, 170.0, 144.0);
        assert_eq!(c.item_at(list, 10.0, 5.0), Some(0));
        assert_eq!(c.item_at(list, 10.0, m::ITEM_H + 1.0), Some(1));
        assert_eq!(c.item_at(list, 10.0, 200.0), None);
    }

    #[test]
    fn handles_key_agrees_with_key() {
        let mut c = combo();
        assert!(!c.handles_key(vk::ESCAPE, Modifiers::NONE), "closed: Escape is the page's");
        assert!(!c.handles_key(vk::ENTER, Modifiers::NONE), "no active row: Enter goes to the field");
        assert!(c.handles_key(vk::DOWN, Modifiers::NONE));
        c.key(vk::DOWN, Modifiers::NONE);
        c.key(vk::DOWN, Modifiers::NONE);
        assert!(c.handles_key(vk::ENTER, Modifiers::NONE));
        assert!(c.handles_key(vk::ESCAPE, Modifiers::NONE));
        assert!(!c.handles_key(vk::LEFT, Modifiers::NONE), "caret keys stay with the field");
        c.field.set_value("zzz");
        assert!(!c.handles_key(vk::DOWN, Modifiers::NONE), "nothing to show");
    }

    #[test]
    fn the_list_view_snapshots_the_filtered_rows_and_scroll_clamps() {
        let mut c = combo();
        c.field.set_value("il");
        let v = c.list_view();
        assert_eq!(v.items, vec!["Domicile".to_string(), "Mobile".to_string()]);
        c.scroll_by(500.0);
        assert_eq!(c.scroll, 0.0, "two rows never scroll");
    }

    #[test]
    fn a_float_checkbox_is_invisible_until_revealed_or_selected() {
        let mut f = FloatCheckbox::new(false);
        assert!(!f.is_shown(WidgetState::REST));
        assert!(f.is_shown(WidgetState::REST.hot(true)), "its own hover shows it");
        f.toggle();
        assert!(f.is_shown(WidgetState::REST));
        assert!(FloatCheckbox::new(false).reveal(true).is_shown(WidgetState::REST));
    }
}
