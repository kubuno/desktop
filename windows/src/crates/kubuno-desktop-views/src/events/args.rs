//! The standard event-args catalogue (`vskubuno/docs/EVENTS.md` §1).
//!
//! One plain struct per kind of args, WinForms names and fields in Rust case.
//! Coordinates are DIP, relative to the sender's bounds (WinForms: client
//! coordinates of the control). Writable outputs (`handled`, `cancel`,
//! `effect`, `suppress_key_press`) are plain public fields: a handler takes
//! `&mut Args`.
//!
//! | Args | Used by |
//! |---|---|
//! | [`EmptyEventArgs`] (the root, `EventArgs` in tooling) | Click, Enter/Leave, GotFocus/LostFocus, Load, Shown, Activated… |
//! | [`HandledEventArgs`] | events a handler can mark handled |
//! | [`MouseEventArgs`] | MouseDown/Up/Move/Click/DoubleClick/Wheel |
//! | [`KeyEventArgs`] | KeyDown, KeyUp |
//! | [`KeyPressEventArgs`] | KeyPress |
//! | [`PaintEventArgs`] | Paint (the surface lent for the paint) |
//! | [`DrawItemEventArgs`] / [`MeasureItemEventArgs`] | DrawItem / MeasureItem (owner-draw) |
//! | [`CancelEventArgs`] | Validating, TabSelecting… |
//! | [`FormClosingEventArgs`] / [`FormClosedEventArgs`] | FormClosing / FormClosed |
//! | [`DragEventArgs`] | DragEnter/Over/Drop |
//! | [`ScrollEventArgs`] | Scroll |
//! | [`LayoutEventArgs`] | Layout |
//! | [`ValueChangedEventArgs<T>`] (+ [`TextChangedEventArgs`], [`CheckedChangedEventArgs`], [`SelectionChangedEventArgs`]) | TextChanged, CheckedChanged, ValueChanged, SelectedIndexChanged |
//! | [`CellEventArgs`] / [`CellCancelEventArgs`] / [`CellValidatingEventArgs`] | DataTable CellEndEdit, CellValueChanged / CellBeginEdit / CellValidating |
//! | [`PropertyChangedEventArgs`] | view-model `PropertyChanged` |
//! | [`HotReloadedEventArgs`] | Kubuno-specific |

use std::any::Any;
use std::fmt;

use kubuno_desktop_controls::host::Modifiers;
use kubuno_desktop_ui::graphics::{Color, DrawItemState, Font, Graphics, GraphicsSlot};
use kubuno_desktop_ui::Rect;

use super::{ArgsChain, EventArgs};
use crate::binding::Value;

// ─────────────────────────────────────────────────────────────────────────
// Root
// ─────────────────────────────────────────────────────────────────────────

/// The root args, carrying no data (WinForms `EventArgs` / `EventArgs.Empty`).
///
/// Its tooling name is `"EventArgs"` (the trait takes the Rust name): its
/// chain is just `["EventArgs"]`, and every other args type's chain ends
/// with it, so a handler declared with the root args is compatible with
/// every event.
///
/// ```
/// use kubuno_desktop_views::events::{ArgsChain, EmptyEventArgs, EventArgs};
/// use kubuno_desktop_views::binding::Value;
///
/// assert_eq!(EmptyEventArgs::CHAIN, ["EventArgs"]);
/// assert_eq!(EmptyEventArgs.legacy_value(), Value::Bool(true));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmptyEventArgs;

impl ArgsChain for EmptyEventArgs {
    const NAME: &'static str = "EventArgs";
    const CHAIN: &'static [&'static str] = &["EventArgs"];
    const RUST_TYPE: &'static str = "EmptyEventArgs";
}

impl super::ReadOnlyArgs for EmptyEventArgs {}

impl EventArgs for EmptyEventArgs {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn type_chain(&self) -> &'static [&'static str] {
        Self::CHAIN
    }
}

/// Args of an event a handler can mark handled (WinForms `HandledEventArgs`).
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[args(handled)]
pub struct HandledEventArgs {
    pub handled: bool,
}

// ─────────────────────────────────────────────────────────────────────────
// Mouse and keyboard
// ─────────────────────────────────────────────────────────────────────────

/// The mouse button an event is about (WinForms `MouseButtons`, one at a time).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// No button: MouseMove, MouseWheel, MouseEnter/Leave.
    #[default]
    None,
    Left,
    Right,
    Middle,
}

/// Mouse event args (WinForms `MouseEventArgs`).
///
/// `x`/`y` are DIP relative to the sender's bounds; `delta` is wheel travel in
/// notches (web sign convention, as `kubuno_desktop_controls::host::Frame::wheel`).
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq)]
pub struct MouseEventArgs {
    pub button: MouseButton,
    /// 1 for a click, 2 for a double click (`Frame::click_count`).
    pub clicks: u8,
    pub x: f32,
    pub y: f32,
    pub delta: f32,
    pub mods: Modifiers,
}

/// A key, as its Win32 virtual-key code (`kubuno_desktop_controls::host::vk`).
///
/// ```
/// use kubuno_desktop_views::events::Key;
/// use kubuno_desktop_controls::host::vk;
///
/// assert_eq!(Key::letter('s'), Key(vk::letter('S')));
/// assert_eq!(Key(vk::ENTER).vk(), 0x0D);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Key(pub u16);

impl Key {
    /// The virtual-key code.
    pub const fn vk(self) -> u16 {
        self.0
    }

    /// The key of an ASCII letter, either case (`vk::letter`).
    pub const fn letter(c: char) -> Self {
        Key(kubuno_desktop_controls::host::vk::letter(c))
    }
}

/// KeyDown / KeyUp args (WinForms `KeyEventArgs`).
///
/// As in WinForms, suppressing the key press also marks the key handled:
/// use [`KeyEventArgs::suppress`] rather than the field alone.
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[args(handled)]
pub struct KeyEventArgs {
    pub key: Key,
    pub mods: Modifiers,
    pub handled: bool,
    /// When set by a KeyDown handler, the matching KeyPress is not raised.
    pub suppress_key_press: bool,
}

impl KeyEventArgs {
    /// Fresh args: not handled, key press not suppressed.
    pub fn new(key: Key, mods: Modifiers) -> Self {
        Self { key, mods, handled: false, suppress_key_press: false }
    }

    /// Sets `suppress_key_press` and `handled` together (WinForms'
    /// `SuppressKeyPress` setter does the same).
    pub fn suppress(&mut self) {
        self.suppress_key_press = true;
        self.handled = true;
    }
}

fn key_press_legacy(e: &KeyPressEventArgs) -> Value {
    Value::Str(e.key_char.to_string())
}

/// KeyPress args (WinForms `KeyPressEventArgs`): the character typed.
/// Its legacy value is the character as `Value::Str`.
#[derive(EventArgs, Debug, Clone, Copy, PartialEq, Eq)]
#[args(handled, legacy = key_press_legacy)]
pub struct KeyPressEventArgs {
    pub key_char: char,
    pub handled: bool,
}

impl KeyPressEventArgs {
    /// Fresh, unhandled args for `key_char`.
    pub fn new(key_char: char) -> Self {
        Self { key_char, handled: false }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Paint and owner-draw (EVT-8)
// ─────────────────────────────────────────────────────────────────────────

/// Paint args (WinForms `PaintEventArgs`): the drawing surface lent for the paint
/// ([`PaintEventArgs::graphics`], a WinForms-like [`Graphics`]) and the area to repaint.
///
/// [`EventArgs`] are `'static` values; the surface only lives for the paint. It is lent through a
/// [`GraphicsSlot`], which is emptied when the raise returns: a copy kept afterwards draws nothing.
///
/// ```
/// use kubuno_desktop_views::events::PaintEventArgs;
/// use kubuno_desktop_ui::graphics::{Color, Graphics, RectExt};
/// use kubuno_desktop_ui::Rect;
///
/// let g = Graphics::recorder();
/// let r = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
/// PaintEventArgs::lend(&g, r, |e| e.graphics().fill_rectangle(Color::RED, e.clip_rectangle));
/// assert_eq!(g.recorded().map(|l| l.len()), Some(1));
/// ```
#[derive(EventArgs, Default)]
pub struct PaintEventArgs {
    /// The area to repaint (`ClipRectangle`), in the surface's coordinates: the control's bounds.
    pub clip_rectangle: Rect,
    graphics: GraphicsSlot,
}

impl PaintEventArgs {
    /// Args with no surface lent (its [`PaintEventArgs::graphics`] draws nothing).
    pub fn new(clip_rectangle: Rect) -> Self {
        Self { clip_rectangle, graphics: GraphicsSlot::empty() }
    }

    /// Runs `f` with args lending `graphics` (emptied when `f` returns).
    pub fn lend<R>(graphics: &Graphics<'_>, clip_rectangle: Rect, f: impl FnOnce(&mut PaintEventArgs) -> R) -> R {
        GraphicsSlot::lend(graphics, |slot| {
            let mut args = PaintEventArgs { clip_rectangle, graphics: slot };
            f(&mut args)
        })
    }

    /// The surface to draw on (`e.Graphics`); outside the paint that raised the event, a surface
    /// that draws nothing.
    pub fn graphics(&self) -> &Graphics<'_> {
        self.graphics.get()
    }

    /// Whether a surface is lent (the paint is running).
    pub fn is_painting(&self) -> bool {
        self.graphics.is_lent()
    }
}

impl fmt::Debug for PaintEventArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let r = &self.clip_rectangle;
        f.debug_struct("PaintEventArgs").field("clip_rectangle", &(r.left, r.top, r.right, r.bottom)).field("painting", &self.is_painting()).finish()
    }
}

/// `DrawItem` args of an owner-drawn list, combo box, list view, tree view, table, tab strip or
/// menu (WinForms `DrawItemEventArgs`, with ListView's/TreeView's `DrawDefault`). Draw the item with
/// [`DrawItemEventArgs::graphics`] in [`DrawItemEventArgs::bounds`]; set `draw_default` to let the
/// control draw it.
#[derive(EventArgs, Default)]
#[args(legacy = draw_item_legacy)]
pub struct DrawItemEventArgs {
    /// The item (`Index`); `None` for a combo box's empty edit field (WinForms' `-1`).
    pub index: Option<usize>,
    /// The column of a table cell or list-view sub-item, `None` for a whole item.
    pub sub_index: Option<usize>,
    /// The item's rectangle (`Bounds`), in the surface's coordinates.
    pub bounds: Rect,
    pub state: DrawItemState,
    /// The item's text as the control would show it.
    pub text: String,
    pub font: Font,
    /// The colour the control would use for the text in this state (`ForeColor`).
    pub fore_color: Color,
    /// The colour the control would fill the item with in this state (`BackColor`).
    pub back_color: Color,
    /// Set it to let the control draw the item itself (`DrawDefault`).
    pub draw_default: bool,
    /// The control raising it (`"ListBox"`, `"ComboBox"`, `"ListView"`, `"TreeView"`, `"DataTable"`,
    /// `"Tabs"`, `"Menu"`, `"Dropdown"`).
    pub control: &'static str,
    graphics: GraphicsSlot,
}

fn draw_item_legacy(e: &DrawItemEventArgs) -> Value {
    Value::F32(e.index.map_or(-1.0, |i| i as f32))
}

impl DrawItemEventArgs {
    /// Runs `f` with the views' args for the widget-level args `e` (its surface lent), then copies
    /// back what a handler may change (`draw_default`, the colours, the font).
    pub fn lend<R>(e: &mut kubuno_desktop_ui::graphics::DrawItemEventArgs<'_>, f: impl FnOnce(&mut DrawItemEventArgs) -> R) -> R {
        GraphicsSlot::lend(e.graphics, |slot| {
            let mut args = DrawItemEventArgs {
                index: e.index,
                sub_index: e.sub_index,
                bounds: e.bounds,
                state: e.state,
                text: e.text.clone(),
                font: e.font.clone(),
                fore_color: e.fore_color,
                back_color: e.back_color,
                draw_default: e.draw_default,
                control: e.control,
                graphics: slot,
            };
            let r = f(&mut args);
            e.draw_default = args.draw_default;
            e.fore_color = args.fore_color;
            e.back_color = args.back_color;
            e.font = args.font;
            r
        })
    }

    /// The surface to draw the item on (`e.Graphics`).
    pub fn graphics(&self) -> &Graphics<'_> {
        self.graphics.get()
    }

    fn with_ui<R>(&self, f: impl FnOnce(&kubuno_desktop_ui::graphics::DrawItemEventArgs<'_>) -> R) -> R {
        let g = self.graphics();
        let mut ui = kubuno_desktop_ui::graphics::DrawItemEventArgs::new(g, self.control, self.index, self.bounds, self.state, self.text.clone());
        ui.sub_index = self.sub_index;
        ui.font = self.font.clone();
        ui.fore_color = self.fore_color;
        ui.back_color = self.back_color;
        f(&ui)
    }

    /// Fills the item with `back_color` (`DrawBackground`).
    pub fn draw_background(&self) {
        self.with_ui(|e| e.draw_background());
    }

    /// The focus cue when the item has the focus (`DrawFocusRectangle`).
    pub fn draw_focus_rectangle(&self) {
        self.with_ui(|e| e.draw_focus_rectangle());
    }

    /// The item's text as the control lays a label out (one line, ellipsis).
    pub fn draw_text(&self) {
        self.with_ui(|e| e.draw_text());
    }
}

impl fmt::Debug for DrawItemEventArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DrawItemEventArgs").field("index", &self.index).field("sub_index", &self.sub_index).field("state", &self.state).field("text", &self.text).finish()
    }
}

/// `MeasureItem` args of an `OwnerDrawVariable` list (WinForms `MeasureItemEventArgs`): set
/// `item_height` (and `item_width`).
#[derive(EventArgs, Default)]
#[args(legacy = measure_item_legacy)]
pub struct MeasureItemEventArgs {
    pub index: usize,
    pub text: String,
    /// In: the control's row height. Out: this item's.
    pub item_height: f32,
    /// In: the control's width. Out: the item's.
    pub item_width: f32,
    pub control: &'static str,
    graphics: GraphicsSlot,
}

fn measure_item_legacy(e: &MeasureItemEventArgs) -> Value {
    Value::F32(e.index as f32)
}

impl MeasureItemEventArgs {
    /// Runs `f` with the views' args for `e`, then copies the measured size back.
    pub fn lend<R>(e: &mut kubuno_desktop_ui::graphics::MeasureItemEventArgs<'_>, f: impl FnOnce(&mut MeasureItemEventArgs) -> R) -> R {
        GraphicsSlot::lend(e.graphics, |slot| {
            let mut args = MeasureItemEventArgs { index: e.index, text: e.text.clone(), item_height: e.item_height, item_width: e.item_width, control: e.control, graphics: slot };
            let r = f(&mut args);
            e.item_height = args.item_height;
            e.item_width = args.item_width;
            r
        })
    }

    /// For measuring text (`e.Graphics.MeasureString`).
    pub fn graphics(&self) -> &Graphics<'_> {
        self.graphics.get()
    }
}

impl fmt::Debug for MeasureItemEventArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeasureItemEventArgs").field("index", &self.index).field("item_height", &self.item_height).finish()
    }
}
// ─────────────────────────────────────────────────────────────────────────
// Cancel, closing
// ─────────────────────────────────────────────────────────────────────────

/// Args of a cancelable operation (WinForms `CancelEventArgs`).
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[args(cancel)]
pub struct CancelEventArgs {
    pub cancel: bool,
}

/// Why a view (window) is closing (WinForms `CloseReason`, the cases a
/// Kubuno host can tell apart).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum CloseReason {
    /// Unknown or not given.
    #[default]
    None,
    /// The user closed it: the close button, Alt+F4, the system menu.
    UserClosing,
    /// The application asked (`close()` from code).
    ApplicationExitCall,
    /// The window that owns this one is closing.
    OwnerClosing,
    /// Windows is shutting down or the session is ending.
    WindowsShutDown,
    /// The Task Manager closed the application.
    TaskManagerClosing,
}

/// FormClosing args (WinForms `FormClosingEventArgs`, a `CancelEventArgs`):
/// `cancel = true` keeps the window open.
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[args(extends = CancelEventArgs, cancel)]
pub struct FormClosingEventArgs {
    pub reason: CloseReason,
    pub cancel: bool,
}

/// FormClosed args (WinForms `FormClosedEventArgs`).
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FormClosedEventArgs {
    pub reason: CloseReason,
}

/// DpiChanged args (WinForms `DpiChangedEventArgs`): the window's DPI before and after it moved
/// to a display of another scale.
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DpiChangedEventArgs {
    pub old_dpi: u32,
    pub new_dpi: u32,
}

/// CaptionButtonClick args: which of the window's own title-bar buttons (`CaptionButtons`) was
/// clicked, by its id.
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptionButtonEventArgs {
    pub id: String,
}

// ─────────────────────────────────────────────────────────────────────────
// Drag and drop
// ─────────────────────────────────────────────────────────────────────────

/// The operations a drag allows or a drop target accepts (WinForms `DragDropEffects`, a bit set);
/// and the data being dragged (WinForms `DataObject`: text, files, custom formats). Shared with the
/// host, which moves them through OLE.
pub use kubuno_desktop_controls::host::dnd::{DataObject, DragDropEffects};

/// DragEnter / DragOver / DragDrop args (WinForms `DragEventArgs`). A handler sets `effect` (within
/// `allowed`) to accept the drop there; the cursor shows it while the pointer moves.
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
pub struct DragEventArgs {
    pub data: DataObject,
    /// What the source allows (`AllowedEffect`).
    pub allowed: DragDropEffects,
    /// What a drop here does (`Effect`): the handler's answer, kept from one DragOver to the next.
    pub effect: DragDropEffects,
    /// The pointer, relative to the control (WinForms gives screen coordinates).
    pub x: f32,
    pub y: f32,
    pub mods: Modifiers,
    /// WinForms' `KeyState` bits: 1 left button, 2 right, 4 Shift, 8 Ctrl, 16 middle, 32 Alt.
    pub key_state: u32,
}

impl DragEventArgs {
    /// The effect a drop performs for the keys held, among `allowed` (Ctrl copies, Shift moves…).
    pub fn suggested_effect(&self) -> DragDropEffects {
        self.allowed.pick(self.mods)
    }
}
// ─────────────────────────────────────────────────────────────────────────
// Scroll, layout
// ─────────────────────────────────────────────────────────────────────────

/// What moved the scroll position (WinForms `ScrollEventType`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ScrollEventType {
    SmallDecrement,
    SmallIncrement,
    LargeDecrement,
    LargeIncrement,
    ThumbPosition,
    ThumbTrack,
    First,
    Last,
    #[default]
    EndScroll,
}

/// Which scroll bar (WinForms `ScrollOrientation`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ScrollOrientation {
    HorizontalScroll,
    #[default]
    VerticalScroll,
}

fn scroll_legacy(e: &ScrollEventArgs) -> Value {
    Value::F32(e.new)
}

/// Scroll args (WinForms `ScrollEventArgs`). Its legacy value is the new
/// position as `Value::F32`.
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq)]
#[args(legacy = scroll_legacy)]
pub struct ScrollEventArgs {
    pub kind: ScrollEventType,
    pub old: f32,
    pub new: f32,
    pub orientation: ScrollOrientation,
}

/// Layout args (WinForms `LayoutEventArgs`): what triggered the layout pass.
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutEventArgs {
    /// The element whose change caused the layout (its stable id), if known.
    pub affected_element: Option<String>,
    /// The property that changed (`"Width"`, `"Visible"`…), if known.
    pub affected_property: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────
// Value changes
// ─────────────────────────────────────────────────────────────────────────

/// Where a value change came from.
///
/// In an immediate-mode runtime a bound value can change "behind the
/// control's back": `*Changed` events fire for every source (like WinForms),
/// and a handler that only cares about the user's edits ignores the rest —
/// which also avoids WinForms' "handler sets `Text`, which raises
/// `TextChanged`" re-entrancy trap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ChangeSource {
    /// The user changed it through the control.
    #[default]
    User,
    /// The bound view-model value changed.
    Binding,
    /// Code changed it (a deferred property override).
    Code,
}

/// A value converted to the untyped [`Value`] the legacy `handlers!` table
/// receives (`vskubuno/docs/EVENTS.md` §5.4).
pub trait IntoLegacyValue {
    /// The Rust name of `ValueChangedEventArgs<Self>` a handler declares
    /// ([`ArgsChain::RUST_TYPE`]): the alias when there is one
    /// (`TextChangedEventArgs` for `String`).
    const ARGS_TYPE: &'static str = "ValueChangedEventArgs";
    /// The legacy value, exactly as today's runtime produces it.
    fn to_legacy_value(&self) -> Value;
}

impl IntoLegacyValue for bool {
    const ARGS_TYPE: &'static str = "CheckedChangedEventArgs";
    fn to_legacy_value(&self) -> Value {
        Value::Bool(*self)
    }
}

impl IntoLegacyValue for String {
    const ARGS_TYPE: &'static str = "TextChangedEventArgs";
    fn to_legacy_value(&self) -> Value {
        Value::Str(self.clone())
    }
}

impl IntoLegacyValue for f32 {
    const ARGS_TYPE: &'static str = "NumericValueChangedEventArgs";
    fn to_legacy_value(&self) -> Value {
        Value::F32(*self)
    }
}

impl IntoLegacyValue for f64 {
    const ARGS_TYPE: &'static str = "ValueChangedEventArgs<f64>";
    fn to_legacy_value(&self) -> Value {
        Value::F32(*self as f32)
    }
}

impl IntoLegacyValue for i32 {
    const ARGS_TYPE: &'static str = "ValueChangedEventArgs<i32>";
    fn to_legacy_value(&self) -> Value {
        Value::F32(*self as f32)
    }
}

impl IntoLegacyValue for u32 {
    const ARGS_TYPE: &'static str = "ValueChangedEventArgs<u32>";
    fn to_legacy_value(&self) -> Value {
        Value::F32(*self as f32)
    }
}

/// An index, as today's selection events send it (`Value::F32(index)`).
impl IntoLegacyValue for usize {
    const ARGS_TYPE: &'static str = "ValueChangedEventArgs<usize>";
    fn to_legacy_value(&self) -> Value {
        Value::F32(*self as f32)
    }
}

/// An optional index: `Value::F32(index)`, or `Value::F32(-1.0)` for "no
/// selection" (WinForms' `SelectedIndex == -1`).
impl IntoLegacyValue for Option<usize> {
    const ARGS_TYPE: &'static str = "SelectionChangedEventArgs";
    fn to_legacy_value(&self) -> Value {
        match self {
            Some(i) => Value::F32(*i as f32),
            None => Value::F32(-1.0),
        }
    }
}

impl IntoLegacyValue for Value {
    const ARGS_TYPE: &'static str = "ValueChangedEventArgs<Value>";
    fn to_legacy_value(&self) -> Value {
        self.clone()
    }
}

/// A value changed: `old` → `new`, and from where (`vskubuno/docs/EVENTS.md`
/// §1 — deliberately richer than WinForms' bare `EventArgs` on `TextChanged`).
/// Its legacy value is `new`'s ([`IntoLegacyValue`]).
///
/// ```
/// use kubuno_desktop_views::events::{ChangeSource, CheckedChangedEventArgs, EventArgs};
/// use kubuno_desktop_views::binding::Value;
///
/// let e = CheckedChangedEventArgs::new(false, true, ChangeSource::User);
/// assert_eq!(e.legacy_value(), Value::Bool(true));
/// assert_eq!(e.type_chain(), ["ValueChangedEventArgs", "EventArgs"]);
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValueChangedEventArgs<T> {
    pub old: T,
    pub new: T,
    pub source: ChangeSource,
}

impl<T> ValueChangedEventArgs<T> {
    pub fn new(old: T, new: T, source: ChangeSource) -> Self {
        Self { old, new, source }
    }
}

impl<T: IntoLegacyValue> ArgsChain for ValueChangedEventArgs<T> {
    const NAME: &'static str = "ValueChangedEventArgs";
    const CHAIN: &'static [&'static str] = &["ValueChangedEventArgs", "EventArgs"];
    const RUST_TYPE: &'static str = T::ARGS_TYPE;
}

impl<T: IntoLegacyValue + 'static> super::ReadOnlyArgs for ValueChangedEventArgs<T> {}

impl<T: IntoLegacyValue + 'static> EventArgs for ValueChangedEventArgs<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn type_chain(&self) -> &'static [&'static str] {
        Self::CHAIN
    }
    fn legacy_value(&self) -> Value {
        self.new.to_legacy_value()
    }
}

/// TextChanged (legacy value: the text).
pub type TextChangedEventArgs = ValueChangedEventArgs<String>;
/// CheckedChanged / Toggled (legacy value: the new state).
pub type CheckedChangedEventArgs = ValueChangedEventArgs<bool>;
/// SelectedIndexChanged / SelectionChanged (legacy value: the index as
/// `F32`, `-1` for none).
pub type SelectionChangedEventArgs = ValueChangedEventArgs<Option<usize>>;
/// ValueChanged of numeric controls (Slider, NumericUpDown).
pub type NumericValueChangedEventArgs = ValueChangedEventArgs<f32>;

fn item_legacy(e: &ItemEventArgs) -> Value {
    Value::F32(e.index as f32)
}

/// An item of a control was clicked or picked (a toolbar command, a breadcrumb segment,
/// a stepper step): its index among its siblings. Its legacy value is the index as
/// `Value::F32`, what those controls always sent.
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[args(legacy = item_legacy)]
pub struct ItemEventArgs {
    pub index: usize,
}

fn item_activate_legacy(e: &ItemActivateEventArgs) -> Value {
    e.item.clone()
}

/// An item of a list, table, calendar or tree was activated (double-click or Enter).
/// `item` identifies it as the control always reported it: the row index as
/// `Value::F32`, or a tree node's path as `Value::Str`; that is also its legacy value.
#[derive(EventArgs, Debug, Clone, PartialEq)]
#[args(legacy = item_activate_legacy)]
pub struct ItemActivateEventArgs {
    pub item: Value,
}

fn item_check_legacy(e: &ItemCheckEventArgs) -> Value {
    Value::F32(e.index as f32)
}

/// An item of a checked list was checked or unchecked (WinForms `ItemCheckEventArgs`).
/// Its legacy value is the item's index as `Value::F32`.
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[args(legacy = item_check_legacy)]
pub struct ItemCheckEventArgs {
    pub index: usize,
    pub checked: bool,
}

fn cell_legacy(e: &CellEventArgs) -> Value {
    Value::F32(e.row_index as f32)
}

/// A cell of a `DataTable` (WinForms `DataGridViewCellEventArgs`): `CellEndEdit`,
/// `CellValueChanged`. `row_index` is the row's index in the bound list (its `ItemsSource`
/// order, whatever the grid's own sort), `column_index` the column's among the `<Column>`s,
/// `column` its field (`Binding`). Its legacy value is the row index as `Value::F32`.
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
#[args(legacy = cell_legacy)]
pub struct CellEventArgs {
    pub row_index: usize,
    pub column_index: usize,
    pub column: String,
}

/// `CellBeginEdit` of a `DataTable` (WinForms `DataGridViewCellCancelEventArgs`): `cancel = true`
/// keeps the cell out of edit mode.
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
#[args(extends = CancelEventArgs, cancel)]
pub struct CellCancelEventArgs {
    pub row_index: usize,
    pub column_index: usize,
    pub column: String,
    pub cancel: bool,
}

/// `CellValidating` of a `DataTable` (WinForms `DataGridViewCellValidatingEventArgs`): the text
/// the user typed (`formatted_value`, before it is parsed back per the column's format);
/// `cancel = true` refuses it — the editor stays open with the text.
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
#[args(extends = CancelEventArgs, cancel)]
pub struct CellValidatingEventArgs {
    pub row_index: usize,
    pub column_index: usize,
    pub column: String,
    pub formatted_value: String,
    pub cancel: bool,
}

fn property_changed_legacy(e: &PropertyChangedEventArgs) -> Value {
    Value::Str(e.property.clone())
}

/// A view-model property changed (`INotifyPropertyChanged.PropertyChanged`).
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
#[args(legacy = property_changed_legacy)]
pub struct PropertyChangedEventArgs {
    /// The property's name, as bindings spell it.
    pub property: String,
}

/// The view was hot-reloaded (Kubuno-specific).
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HotReloadedEventArgs {
    /// How many diagnostics the new file has (0: it compiled cleanly).
    pub diagnostics: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Cancelable, Handled};

    #[test]
    fn chains_follow_extends() {
        assert_eq!(EmptyEventArgs::CHAIN, ["EventArgs"]);
        assert_eq!(MouseEventArgs::CHAIN, ["MouseEventArgs", "EventArgs"]);
        assert_eq!(KeyEventArgs::CHAIN, ["KeyEventArgs", "EventArgs"]);
        assert_eq!(CancelEventArgs::CHAIN, ["CancelEventArgs", "EventArgs"]);
        assert_eq!(FormClosingEventArgs::CHAIN, ["FormClosingEventArgs", "CancelEventArgs", "EventArgs"]);
        assert_eq!(FormClosingEventArgs::NAME, "FormClosingEventArgs");
        assert_eq!(TextChangedEventArgs::CHAIN, ["ValueChangedEventArgs", "EventArgs"]);
    }

    #[test]
    fn three_level_chain_via_string_extends() {
        #[derive(EventArgs, Default)]
        #[args(extends = "FormClosingEventArgs", cancel = "veto")]
        struct AppClosing {
            veto: bool,
        }
        assert_eq!(AppClosing::CHAIN, ["AppClosing", "FormClosingEventArgs", "CancelEventArgs", "EventArgs"]);
        let mut e = AppClosing::default();
        e.set_cancel(true);
        assert!(e.veto);
        let dynamic: &dyn EventArgs = &e;
        assert!(dynamic.is_a("CancelEventArgs"));
        assert!(!dynamic.is_a("MouseEventArgs"));
    }

    #[test]
    fn generic_derive_gets_static_bound() {
        #[derive(EventArgs)]
        struct Payload<T> {
            item: T,
        }
        let e = Payload { item: 3_u8 };
        assert_eq!(e.type_chain(), ["Payload", "EventArgs"]);
        assert_eq!(e.item, 3);

        #[derive(EventArgs)]
        struct Ping;
        assert_eq!(Ping.type_chain(), ["Ping", "EventArgs"]);
    }

    #[test]
    fn capability_hooks() {
        let mut k = KeyEventArgs::default();
        assert!(k.as_handled().is_some());
        assert!(k.as_cancelable().is_none());
        if let Some(h) = k.as_handled_mut() {
            h.set_handled(true);
        }
        assert!(k.handled());

        let mut c = FormClosingEventArgs::default();
        assert!(c.as_handled().is_none());
        if let Some(x) = c.as_cancelable_mut() {
            x.set_cancel(true);
        }
        assert!(c.cancel);

        let m = MouseEventArgs::default();
        assert!(m.as_handled().is_none() && m.as_cancelable().is_none());
    }

    #[test]
    fn key_suppress_sets_handled() {
        let mut k = KeyEventArgs::new(Key::letter('a'), Modifiers::CTRL);
        k.suppress();
        assert!(k.suppress_key_press && k.handled);
    }

    #[test]
    fn legacy_values_match_todays_runtime() {
        assert_eq!(MouseEventArgs::default().legacy_value(), Value::Bool(true));
        assert_eq!(KeyPressEventArgs::new('x').legacy_value(), Value::Str("x".into()));
        assert_eq!(
            TextChangedEventArgs::new(String::new(), "hi".into(), ChangeSource::User).legacy_value(),
            Value::Str("hi".into())
        );
        assert_eq!(CheckedChangedEventArgs::new(true, false, ChangeSource::Binding).legacy_value(), Value::Bool(false));
        assert_eq!(SelectionChangedEventArgs::new(None, Some(2), ChangeSource::User).legacy_value(), Value::F32(2.0));
        assert_eq!(SelectionChangedEventArgs::new(Some(2), None, ChangeSource::User).legacy_value(), Value::F32(-1.0));
        assert_eq!(
            ScrollEventArgs { new: 12.0, ..Default::default() }.legacy_value(),
            Value::F32(12.0)
        );
        assert_eq!(
            PropertyChangedEventArgs { property: "Name".into() }.legacy_value(),
            Value::Str("Name".into())
        );
    }

    #[test]
    fn downcast_through_dyn() {
        let mut e = DragEventArgs { allowed: DragDropEffects::ALL, ..Default::default() };
        let dynamic: &mut dyn EventArgs = &mut e;
        assert!(dynamic.is::<DragEventArgs>());
        assert!(dynamic.downcast_ref::<MouseEventArgs>().is_none());
        if let Some(d) = dynamic.downcast_mut::<DragEventArgs>() {
            d.effect = DragDropEffects::COPY;
        }
        assert_eq!(e.effect, DragDropEffects::COPY);
        assert!(e.data.is_empty());
    }

    #[test]
    fn drag_effects_and_data() {
        let mut fx = DragDropEffects::NONE;
        assert!(fx.is_none());
        fx |= DragDropEffects::LINK;
        assert!(fx.contains(DragDropEffects::LINK) && !fx.contains(DragDropEffects::COPY));
        assert!(DragDropEffects::ALL.contains(DragDropEffects::SCROLL | DragDropEffects::MOVE));
        let data = DataObject { custom: vec![("kubuno/element".into(), vec![1, 2])], ..Default::default() };
        assert_eq!(data.get("kubuno/element"), Some(&[1_u8, 2][..]));
        assert_eq!(data.get("other"), None);
    }

    #[test]
    fn paint_args_lend_their_surface_for_the_raise_only() {
        use kubuno_desktop_ui::graphics::{Color, Graphics};
        let p = PaintEventArgs::new(Rect::new(0.0, 0.0, 10.0, 5.0));
        assert!(format!("{p:?}").contains("10.0") && !p.is_painting());
        assert_eq!(p.type_chain(), ["PaintEventArgs", "EventArgs"]);
        let g = Graphics::recorder();
        let dynamic_ok = PaintEventArgs::lend(&g, Rect::new(0.0, 0.0, 4.0, 4.0), |e| {
            assert!(e.is_painting());
            e.graphics().fill_rectangle(Color::RED, e.clip_rectangle);
            let dynamic: &mut dyn EventArgs = e;
            dynamic.is::<PaintEventArgs>()
        });
        assert!(dynamic_ok);
        assert_eq!(g.recorded().map(|l| l.describe()), Some(vec!["FillRect".to_string()]));
    }

    #[test]
    fn draw_item_args_copy_back_what_the_handler_changed() {
        use kubuno_desktop_ui::graphics::{owner_draw, Graphics, RectExt};
        let g = Graphics::recorder();
        let mut ui = owner_draw::DrawItemEventArgs::new(&g, "ListBox", Some(3), Rect::from_xywh(0.0, 0.0, 50.0, 20.0), DrawItemState::SELECTED, "x");
        DrawItemEventArgs::lend(&mut ui, |e| {
            assert_eq!((e.index, e.control, e.legacy_value()), (Some(3), "ListBox", Value::F32(3.0)));
            e.draw_background();
            e.draw_default = true;
        });
        assert!(ui.draw_default);
        assert_eq!(g.recorded().map(|l| l.describe()), Some(vec!["FillRoundedRect".to_string()]));
        let mut m = owner_draw::MeasureItemEventArgs { graphics: &g, control: "ListBox", index: 1, text: "y".into(), item_height: 20.0, item_width: 100.0 };
        MeasureItemEventArgs::lend(&mut m, |e| e.item_height = 44.0);
        assert_eq!(m.item_height, 44.0);
    }
}
