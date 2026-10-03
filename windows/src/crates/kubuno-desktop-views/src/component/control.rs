//! `Control` (WinForms `System.Windows.Forms.Control`): the level every visual component
//! derives from — bounds, visibility, focus, the control styles, and the overridable `on_…`
//! family, each of whose base behaviour raises the matching event.

use std::ops::{BitOr, Deref, DerefMut};

use kubuno_desktop_controls::host::{vk, Modifiers};
use kubuno_desktop_controls::ControlBase;
use kubuno_desktop_ui::{Canvas, FocusId, Rect, Size};

use super::cx::{EventCx, PaintEventCx};
use super::{Component, ComponentCore, HasComponentCore, Lineage};
use crate::events::{
    CancelEventArgs, CheckedChangedEventArgs, ChangeSource, DragEventArgs, EmptyEventArgs, Event, EventArgs, FormClosedEventArgs, FormClosingEventArgs, Key, KeyEventArgs, KeyPressEventArgs,
    LayoutEventArgs, MouseEventArgs, NumericValueChangedEventArgs, PaintEventArgs, ScrollEventArgs, SelectionChangedEventArgs, TextChangedEventArgs,
};

/// WinForms `ControlStyles`: behaviour switches a control class sets in its constructor
/// ([`Control::set_style`]). The hosts honour `SELECTABLE` (focusable by click and Tab),
/// `STANDARD_CLICK` (Click synthesized from a press and release inside), `STANDARD_DOUBLE_CLICK`
/// (DoubleClick on a second quick press, a second Click otherwise), `RESIZE_REDRAW` (a resize
/// invalidates), and the paint styles (EVT-8, [`super::paint`]): `USER_PAINT` (the control paints
/// itself: cleared, its paint methods are not called), `OPAQUE` (no `on_paint_background`),
/// `OPTIMIZED_DOUBLE_BUFFER` (`DoubleBuffered`: the paint is kept and replayed while the control is
/// valid), `SUPPORTS_TRANSPARENT_BACK_COLOR` (a `BackColor` with alpha lets the parent's
/// background show; without it the colour is made opaque). Every frame is composed off screen, so
/// `DOUBLE_BUFFER` and `ALL_PAINTING_IN_WM_PAINT` (the background painted in the paint pass) always
/// hold; the others are recorded for the control's own use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ControlStyles(pub u32);

impl ControlStyles {
    pub const NONE: Self = Self(0);
    pub const CONTAINER_CONTROL: Self = Self(0x0000_0001);
    pub const USER_PAINT: Self = Self(0x0000_0002);
    pub const OPAQUE: Self = Self(0x0000_0004);
    pub const RESIZE_REDRAW: Self = Self(0x0000_0010);
    pub const FIXED_WIDTH: Self = Self(0x0000_0020);
    pub const FIXED_HEIGHT: Self = Self(0x0000_0040);
    pub const STANDARD_CLICK: Self = Self(0x0000_0100);
    pub const SELECTABLE: Self = Self(0x0000_0200);
    pub const USER_MOUSE: Self = Self(0x0000_0400);
    pub const SUPPORTS_TRANSPARENT_BACK_COLOR: Self = Self(0x0000_0800);
    pub const STANDARD_DOUBLE_CLICK: Self = Self(0x0000_1000);
    pub const ALL_PAINTING_IN_WM_PAINT: Self = Self(0x0000_2000);
    pub const CACHE_TEXT: Self = Self(0x0000_4000);
    pub const ENABLE_NOTIFY_MESSAGE: Self = Self(0x0000_8000);
    pub const DOUBLE_BUFFER: Self = Self(0x0001_0000);
    pub const OPTIMIZED_DOUBLE_BUFFER: Self = Self(0x0002_0000);
    pub const USE_TEXT_FOR_ACCESSIBILITY: Self = Self(0x0004_0000);

    /// What `Control`'s constructor sets in WinForms, plus the double buffering Kubuno always
    /// has.
    pub const CONTROL_DEFAULT: Self = Self(
        Self::USER_PAINT.0
            | Self::STANDARD_CLICK.0
            | Self::STANDARD_DOUBLE_CLICK.0
            | Self::SELECTABLE.0
            | Self::ALL_PAINTING_IN_WM_PAINT.0
            | Self::USE_TEXT_FOR_ACCESSIBILITY.0
            | Self::OPTIMIZED_DOUBLE_BUFFER.0,
    );

    /// Whether every flag of `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Sets or clears the flags of `other`.
    pub fn set(&mut self, other: Self, on: bool) {
        if on {
            self.0 |= other.0;
        } else {
            self.0 &= !other.0;
        }
    }
}

impl BitOr for ControlStyles {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Which parts of the bounds a [`Control::set_bounds_core`] call changes (WinForms
/// `BoundsSpecified`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BoundsSpecified(pub u8);

impl BoundsSpecified {
    pub const NONE: Self = Self(0);
    pub const X: Self = Self(1);
    pub const Y: Self = Self(2);
    pub const WIDTH: Self = Self(4);
    pub const HEIGHT: Self = Self(8);
    pub const LOCATION: Self = Self(1 | 2);
    pub const SIZE: Self = Self(4 | 8);
    pub const ALL: Self = Self(15);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// A key with its modifiers (WinForms `Keys` key data, e.g. `Keys.Control | Keys.S`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Keys {
    pub key: Key,
    pub mods: Modifiers,
}

impl Keys {
    pub const fn new(key: Key, mods: Modifiers) -> Self {
        Self { key, mods }
    }

    /// The key without modifiers.
    pub const fn plain(key: Key) -> Self {
        Self { key, mods: Modifiers::NONE }
    }

    /// Ctrl + `key`.
    pub const fn ctrl(key: Key) -> Self {
        Self { key, mods: Modifiers::CTRL }
    }

    /// The keys WinForms routes to `ProcessDialogKey` when the control does not take them as
    /// input ([`Control::is_input_key`]): Tab, the arrows, Enter, Escape.
    pub fn is_dialog_key(&self) -> bool {
        matches!(self.key.0, vk::TAB | vk::LEFT | vk::RIGHT | vk::UP | vk::DOWN | vk::ENTER | vk::ESCAPE)
    }
}

/// A window message, as [`Control::wnd_proc`] sees it (WinForms `Message`). Kubuno controls are
/// drawn, not windowed: the host synthesizes the platform-neutral subset below (the Win32 ids and
/// packing, whatever the backend) for the control under the pointer or holding the focus, before
/// it turns them into events. A `wnd_proc` returning `true` consumes the message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Message {
    /// The message id (`Message::WM_KEYDOWN`…).
    pub msg: u32,
    pub wparam: usize,
    pub lparam: isize,
    /// What the control answers (WinForms `Message.Result`).
    pub result: isize,
}

impl Message {
    pub const WM_SETFOCUS: u32 = 0x0007;
    pub const WM_KILLFOCUS: u32 = 0x0008;
    pub const WM_KEYDOWN: u32 = 0x0100;
    pub const WM_KEYUP: u32 = 0x0101;
    pub const WM_CHAR: u32 = 0x0102;
    pub const WM_MOUSEMOVE: u32 = 0x0200;
    pub const WM_LBUTTONDOWN: u32 = 0x0201;
    pub const WM_LBUTTONUP: u32 = 0x0202;
    pub const WM_RBUTTONDOWN: u32 = 0x0204;
    pub const WM_RBUTTONUP: u32 = 0x0205;
    pub const WM_MBUTTONDOWN: u32 = 0x0207;
    pub const WM_MBUTTONUP: u32 = 0x0208;
    pub const WM_MOUSEWHEEL: u32 = 0x020A;

    /// A key message (`WM_KEYDOWN`/`WM_KEYUP`): `wparam` is the virtual key, `lparam` the
    /// modifiers (bit 0 Ctrl, 1 Shift, 2 Alt, 3 Win).
    pub fn key(msg: u32, keys: Keys) -> Self {
        let m = keys.mods;
        let mods = isize::from(m.ctrl) | isize::from(m.shift) << 1 | isize::from(m.alt) << 2 | isize::from(m.meta) << 3;
        Self { msg, wparam: usize::from(keys.key.0), lparam: mods, result: 0 }
    }

    /// `WM_CHAR`: `wparam` is the character.
    pub fn char(c: char) -> Self {
        Self { msg: Self::WM_CHAR, wparam: c as usize, lparam: 0, result: 0 }
    }

    /// A mouse message: `lparam` packs the point relative to the control, in DIP rounded to
    /// whole units, as two signed 16-bit halves (x low); a wheel's `wparam` high half is the
    /// travel in 1/120 notches, Win32's `WHEEL_DELTA`.
    pub fn mouse(msg: u32, x: f32, y: f32, wheel_notches: f32) -> Self {
        let lo = (x.round() as i32 as i16 as u16) as isize;
        let hi = (y.round() as i32 as i16 as u16) as isize;
        let delta = (-wheel_notches * 120.0).round() as i32 as i16 as u16 as usize;
        Self { msg, wparam: delta << 16, lparam: lo | hi << 16, result: 0 }
    }

    /// The key data of a key message.
    pub fn keys(&self) -> Option<Keys> {
        if !matches!(self.msg, Self::WM_KEYDOWN | Self::WM_KEYUP) {
            return None;
        }
        let l = self.lparam;
        let mods = Modifiers { ctrl: l & 1 != 0, shift: l & 2 != 0, alt: l & 4 != 0, meta: l & 8 != 0 };
        Some(Keys::new(Key(u16::try_from(self.wparam).unwrap_or_default()), mods))
    }

    /// The character of a `WM_CHAR`.
    pub fn char_code(&self) -> Option<char> {
        if self.msg != Self::WM_CHAR {
            return None;
        }
        u32::try_from(self.wparam).ok().and_then(char::from_u32)
    }

    /// The point of a mouse message, relative to the control.
    pub fn point(&self) -> (f32, f32) {
        let x = (self.lparam & 0xFFFF) as u16 as i16;
        let y = ((self.lparam >> 16) & 0xFFFF) as u16 as i16;
        (f32::from(x), f32::from(y))
    }
}

/// What a natively hosted control asks its window to be created with (WinForms `CreateParams`).
/// Kubuno's own controls are drawn, not windowed, so nothing consumes it yet; it is the hook a
/// control that wraps a native child window (a web view, a media player) overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateParams {
    pub class_name: String,
    pub caption: String,
    pub style: u32,
    pub ex_style: u32,
}

/// The state of the `Control` level: the WinForms property surface (the replica
/// [`kubuno_desktop_controls::ControlBase`]: name, text, bounds, dock/anchor, margin/padding,
/// visible/enabled, tab order…, reached through `Deref`), the control styles, and the run-time
/// state the host writes (hover, focus, creation, invalidation).
pub struct ControlCore {
    pub component: ComponentCore,
    /// The WinForms `Control` properties (also reachable directly: `core.text`, `core.bounds`).
    pub props: ControlBase,
    /// The focus identity the host registers the control under (from `x:Name`, or assigned).
    pub focus_id: Option<FocusId>,
    pub styles: ControlStyles,
    /// The pointer is over the control (set by the host each frame).
    pub hot: bool,
    /// A mouse button pressed on the control is still held.
    pub pressed: bool,
    /// The control holds the keyboard focus.
    pub focused: bool,
    pub(crate) created: bool,
    pub(crate) handle_created: bool,
    pub(crate) invalid: Option<Rect>,
    pub(crate) focus_requested: bool,
    pub(crate) update_requested: bool,
    pub(crate) layout_suspended: u32,
    /// The paint buffer (`OPTIMIZED_DOUBLE_BUFFER`, see [`super::paint`]).
    pub buffer: super::paint::PaintBuffer,
}

impl Default for ControlCore {
    fn default() -> Self {
        Self {
            component: ComponentCore::default(),
            props: ControlBase::default(),
            focus_id: None,
            styles: ControlStyles::CONTROL_DEFAULT,
            hot: false,
            pressed: false,
            focused: false,
            created: false,
            handle_created: false,
            invalid: None,
            focus_requested: false,
            update_requested: false,
            layout_suspended: 0,
            buffer: super::paint::PaintBuffer::default(),
        }
    }
}

impl ControlCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A core with `text` (a button's label, a label's text).
    pub fn with_text(text: &str) -> Self {
        let mut core = Self::default();
        core.props.text = text.to_string();
        core
    }
}

impl Deref for ControlCore {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.props
    }
}

impl DerefMut for ControlCore {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.props
    }
}

impl Lineage for ControlCore {
    const CHAIN: &'static [&'static str] = &["Control", "Component"];
}

impl HasComponentCore for ControlCore {
    fn component_core(&self) -> &ComponentCore {
        &self.component
    }
    fn component_core_mut(&mut self) -> &mut ComponentCore {
        &mut self.component
    }
}

/// Reaches the [`ControlCore`] (generated by `#[derive(Component)]`; the cores implement it).
#[doc(hidden)]
pub trait HasControlCore {
    fn control_core(&self) -> &ControlCore;
    fn control_core_mut(&mut self) -> &mut ControlCore;
}

impl HasControlCore for ControlCore {
    fn control_core(&self) -> &ControlCore {
        self
    }
    fn control_core_mut(&mut self) -> &mut ControlCore {
        self
    }
}

/// A part of a control that a screen reader lists under it ([`Control::accessible_parts`]).
#[derive(Debug, Clone, PartialEq)]
pub struct AccessiblePart {
    /// What it is called (a tile's app, a row's label).
    pub name: String,
    pub role: kubuno_desktop_controls::host::access::AccessRole,
    /// Where it is, in the control's own coordinates.
    pub bounds: Rect,
}

/// The base object a control delegates to (generated by `#[derive(Component)]`).
#[doc(hidden)]
pub trait ControlLink: HasControlCore {
    fn base_control(&self) -> Option<&dyn Control>;
    fn base_control_mut(&mut self) -> Option<&mut dyn Control>;
}

/// The root `on_paint_background`: `BackColor` (transparent over the parent's background only
/// with `SUPPORTS_TRANSPARENT_BACK_COLOR`), then `BackgroundImage`.
fn paint_background_default(core: &ControlCore, e: &mut PaintEventCx<'_>) {
    use kubuno_desktop_ui::graphics::{Color, Image, RectExt};
    use kubuno_desktop_ui::Canvas;
    let bounds = e.bounds();
    if let Some(back) = core.back_color {
        let mut color = Color::from(back);
        if color.a < 1.0 {
            if core.styles.contains(ControlStyles::SUPPORTS_TRANSPARENT_BACK_COLOR) {
                // `InvokePaintBackground` on the parent: the parent surface's fill shows through.
                e.graphics.fill_rectangle(Color::from(e.graphics.current_bg()), bounds);
            } else {
                color = color.with_alpha(1.0);
            }
        }
        e.graphics.fill_rectangle(color, bounds);
    }
    if let Some(path) = core.background_image.as_deref().filter(|p| !p.is_empty()) {
        let image = Image::from_file(path);
        if let Some(size) = e.graphics.image_size(&image) {
            e.graphics.with_saved(|g| {
                g.intersect_clip(bounds);
                for dest in kubuno_desktop_controls::styled::layout_image((size.width, size.height), bounds, core.background_image_layout) {
                    if dest.intersect(&bounds).is_some() {
                        g.draw_image(&image, dest);
                    }
                }
            });
        }
    }
}

/// Declares overridable `on_…` methods whose default delegates to the base object, else raises
/// the event.
macro_rules! overridable {
    ($($(#[$doc:meta])* fn $method:ident($args:ty) => $event:literal;)*) => {$(
        $(#[$doc])*
        fn $method(&mut self, e: &mut EventCx<'_, $args>) {
            if let Some(base) = self.base_control_mut() {
                return base.$method(e);
            }
            e.raise(&*self, $event);
        }
    )*};
}

/// Declares the event accessors (`button.click().subscribe(…)`).
macro_rules! accessors {
    ($($(#[$doc:meta])* fn $name:ident($args:ty) => $event:literal;)*) => {$(
        $(#[$doc])*
        fn $name(&self) -> Event<$args> {
            self.component_core().events.event($event)
        }
    )*};
}

/// A visual component (WinForms `Control`). Implemented by `#[derive(Component)]` (empty) unless
/// the class lists `Control` in `overrides(…)` — then `impl Control for MyControl { … }` holds the
/// methods it overrides.
///
/// Three kinds of methods:
/// - **properties and operations** (`bounds`, `set_text`, `focus`, `invalidate`,
///   `perform_layout`…) — provided, not meant to be overridden;
/// - **overridable behaviour** (`on_click`, `on_paint`, `process_cmd_key`, `is_input_key`,
///   `get_preferred_size`, `wnd_proc`…) — override them; the default delegates to the base
///   object, and at the root raises the event (`on_click` raises `Click`);
/// - **event accessors** (`click()`, `key_down()`…) — the handle Rust code subscribes to.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a control class",
    label = "not a `Control`",
    note = "derive it with a control base: `#[derive(Component)] #[kubuno(extends = Control)] struct {Self} {{ base: ControlCore, … }}`; when the class lists `Control` in `overrides(…)`, write `impl Control for {Self} {{ … }}`"
)]
pub trait Control: Component + ControlLink {
    // ── Properties and operations ───────────────────────────────────────────────────────

    /// `Control.Name`.
    fn name(&self) -> &str {
        &self.control_core().name
    }

    /// Sets `Control.Name`.
    fn set_name(&mut self, name: &str) {
        self.control_core_mut().name = name.to_string();
    }

    /// `Control.Text`.
    fn text(&self) -> &str {
        &self.control_core().text
    }

    /// Sets `Control.Text` and, when it changed, raises `TextChanged` (source: code).
    fn set_text(&mut self, text: &str) {
        if self.control_core().text == text {
            return;
        }
        let old = std::mem::replace(&mut self.control_core_mut().text, text.to_string());
        let mut args = TextChangedEventArgs::new(old, text.to_string(), ChangeSource::Code);
        self.on_text_changed(&mut EventCx::new(&mut args).from_class(self.class_name()));
    }

    /// `Control.Bounds`, in the coordinates of the canvas the control is painted on.
    fn bounds(&self) -> Rect {
        self.control_core().bounds
    }

    /// Sets the bounds through [`Control::set_bounds_core`]. Move/Resize are raised by the host
    /// when the control is next laid out at them (WinForms raises them from the window's own
    /// position change).
    fn set_bounds(&mut self, bounds: Rect) {
        self.set_bounds_core(bounds, BoundsSpecified::ALL);
    }

    /// `Control.Size`.
    fn size(&self) -> Size {
        // `ControlBase::size`, named: `ControlCore` is a `Control` too, whose `size` is this very method.
        ControlBase::size(&self.control_core().props)
    }

    /// `Control.Location`.
    fn location(&self) -> (f32, f32) {
        let b = self.control_core().bounds;
        (b.left, b.top)
    }

    /// `Control.Visible`.
    fn visible(&self) -> bool {
        self.control_core().visible
    }

    /// Sets `Visible`, raising `VisibleChanged` when it changes.
    fn set_visible(&mut self, visible: bool) {
        if self.control_core().visible != visible {
            self.control_core_mut().visible = visible;
            self.on_visible_changed(&mut EventCx::new(&mut EmptyEventArgs).from_class(self.class_name()));
        }
    }

    /// `Control.Enabled`.
    fn enabled(&self) -> bool {
        self.control_core().enabled
    }

    /// Sets `Enabled`, raising `EnabledChanged` when it changes.
    fn set_enabled(&mut self, enabled: bool) {
        if self.control_core().enabled != enabled {
            self.control_core_mut().enabled = enabled;
            self.on_enabled_changed(&mut EventCx::new(&mut EmptyEventArgs).from_class(self.class_name()));
        }
    }

    /// `Control.Focused`.
    fn focused(&self) -> bool {
        self.control_core().focused
    }

    /// `Control.CanFocus`: visible and enabled.
    fn can_focus(&self) -> bool {
        let c = self.control_core();
        c.visible && c.enabled
    }

    /// `Control.CanSelect`: [`Control::can_focus`] and the `SELECTABLE` style.
    fn can_select(&self) -> bool {
        self.can_focus() && self.control_core().styles.contains(ControlStyles::SELECTABLE)
    }

    /// `Control.Focus()`: asks the host to move the keyboard focus here at the next frame.
    /// `false` when the control cannot take it.
    fn focus(&mut self) -> bool {
        if !self.can_focus() {
            return false;
        }
        self.control_core_mut().focus_requested = true;
        true
    }

    /// `Control.GetStyle`.
    fn get_style(&self, flag: ControlStyles) -> bool {
        self.control_core().styles.contains(flag)
    }

    /// `Control.SetStyle`.
    fn set_style(&mut self, flags: ControlStyles, value: bool) {
        self.control_core_mut().styles.set(flags, value);
    }

    /// `Control.Invalidate()`: the whole control needs repainting.
    fn invalidate(&mut self) {
        let b = self.control_core().bounds;
        self.invalidate_rect(b);
    }

    /// `Control.Invalidate(Rectangle)`: `rect` needs repainting (accumulated until the host
    /// repaints): the paint buffer is dropped at the next paint and a frame is asked for, like
    /// WinForms posting `WM_PAINT`.
    fn invalidate_rect(&mut self, rect: Rect) {
        let core = self.control_core_mut();
        core.invalid = Some(match core.invalid {
            Some(r) => Rect::new(r.left.min(rect.left), r.top.min(rect.top), r.right.max(rect.right), r.bottom.max(rect.bottom)),
            None => rect,
        });
        kubuno_desktop_controls::host::request_repaint_after(0);
    }

    /// `Control.DoubleBuffered`: whether the paint is kept and replayed while the control is valid
    /// (the `OPTIMIZED_DOUBLE_BUFFER` style, on by default; see [`super::paint`]).
    fn double_buffered(&self) -> bool {
        self.get_style(ControlStyles::OPTIMIZED_DOUBLE_BUFFER)
    }

    /// Sets `DoubleBuffered`.
    fn set_double_buffered(&mut self, on: bool) {
        self.set_style(ControlStyles::OPTIMIZED_DOUBLE_BUFFER, on);
        self.control_core_mut().buffer.clear();
    }

    /// `Control.InvokePaint(c, e)`: runs `child`'s `on_paint` with this paint's args — what a
    /// composite control that draws its children itself calls.
    fn invoke_paint(&mut self, child: &mut dyn Control, e: &mut PaintEventCx<'_>) {
        child.on_paint(e);
    }

    /// `Control.InvokePaintBackground(c, e)`: runs `child`'s `on_paint_background`.
    fn invoke_paint_background(&mut self, child: &mut dyn Control, e: &mut PaintEventCx<'_>) {
        child.on_paint_background(e);
    }

    /// The paint's two layers on this object (WinForms `PaintWithErrorHandling` over the background
    /// and foreground layers): `on_paint_background` unless `OPAQUE`, then `on_paint`. What the
    /// default [`Control::on_print`] runs; an `on_print` override calls it for the normal rendering
    /// (it reaches this class's own overrides, which `self.base_mut().on_print(e)` would not).
    fn paint_layers(&mut self, e: &mut PaintEventCx<'_>) {
        if !self.get_style(ControlStyles::OPAQUE) {
            self.on_paint_background(&mut e.reborrow());
        }
        self.on_paint(e);
    }

    /// Renders the control off screen (WinForms `DrawToBitmap`, through `on_print`) as a display
    /// list whose origin is the control's top-left corner.
    fn draw_to_display_list(&mut self) -> kubuno_desktop_ui::graphics::DisplayList {
        let bounds = self.control_core().bounds;
        let canvas = kubuno_desktop_ui::graphics::testing::RecordingCanvas::new();
        let g = kubuno_desktop_ui::graphics::Graphics::new(&canvas).recording();
        g.push_offset(-bounds.left, -bounds.top);
        {
            let mut e = PaintEventCx::new(&g, &canvas, bounds, kubuno_desktop_ui::WidgetState::REST);
            self.on_print(&mut e);
        }
        g.pop_offset();
        g.take_recording().unwrap_or_default()
    }

    /// `Control.DrawToBitmap(bitmap, targetBounds)`: renders the control through `on_print` onto
    /// `target` with its top-left corner at `target_bounds`' (at its own size). To get pixels, give
    /// it a `Graphics` over an image surface.
    fn draw_to_bitmap(&mut self, target: &kubuno_desktop_ui::graphics::Graphics<'_>, target_bounds: Rect) {
        let list = self.draw_to_display_list();
        target.push_offset(target_bounds.left, target_bounds.top);
        list.replay(target);
        target.pop_offset();
    }

    /// `Control.DoDragDrop(data, allowedEffects)`: starts dragging `data` from this control, right
    /// after the current frame (a drag is a modal loop and never runs inside a paint). The returned
    /// operation completes with the effect the drop target chose (`NONE` when cancelled) — poll it
    /// or `.await` it in an async handler.
    fn do_drag_drop(&mut self, data: crate::events::DataObject, allowed: crate::events::DragDropEffects) -> crate::dnd::DragOperation {
        crate::dnd::do_drag_drop(data, allowed)
    }

    /// `Control.Update()`: paint the invalidated area now — in Kubuno, at the very next frame.
    fn update(&mut self) {
        self.control_core_mut().update_requested = true;
    }

    /// `Control.Refresh()`: [`Control::invalidate`] then [`Control::update`].
    fn refresh(&mut self) {
        self.invalidate();
        self.update();
    }

    /// The area invalidated since the host last repainted, if any.
    fn invalidated_rect(&self) -> Option<Rect> {
        self.control_core().invalid
    }

    /// `Control.CreateControl()`: runs [`Control::on_create_control`] then `HandleCreated` once,
    /// before the control is first painted (the hosts call it).
    fn create_control(&mut self) {
        if self.control_core().created {
            return;
        }
        self.control_core_mut().created = true;
        self.on_create_control();
        self.control_core_mut().handle_created = true;
        self.on_handle_created(&mut EventCx::new(&mut EmptyEventArgs).from_class(self.class_name()));
    }

    /// `Control.Created`.
    fn created(&self) -> bool {
        self.control_core().created
    }

    /// `Control.IsHandleCreated`.
    fn is_handle_created(&self) -> bool {
        self.control_core().handle_created
    }

    /// `Control.SuspendLayout()`.
    fn suspend_layout(&mut self) {
        self.control_core_mut().layout_suspended += 1;
    }

    /// `Control.ResumeLayout(performLayout)`.
    fn resume_layout(&mut self, perform_layout: bool) {
        let core = self.control_core_mut();
        core.layout_suspended = core.layout_suspended.saturating_sub(1);
        if perform_layout {
            self.perform_layout();
        }
    }

    /// `Control.PerformLayout()`: raises [`Control::on_layout`] unless layout is suspended.
    fn perform_layout(&mut self) {
        if self.control_core().layout_suspended > 0 {
            return;
        }
        let mut args = LayoutEventArgs::default();
        self.on_layout(&mut EventCx::new(&mut args).from_class(self.class_name()));
    }

    // ── Overridable behaviour ───────────────────────────────────────────────────────────

    /// Paints the control (WinForms `OnPaint`). The root behaviour paints nothing and raises
    /// `Paint`; a built-in class draws its Kubuno look here (`Button` draws the button). Call
    /// `self.base_mut().on_paint(e)` to draw the base look under or over your own.
    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        if let Some(base) = self.base_control_mut() {
            return base.on_paint(e);
        }
        e.raise(&*self, "OnPaint");
    }

    /// Paints the background before [`Control::on_paint`] (WinForms `OnPaintBackground`); not
    /// called for a control with the `OPAQUE` style. The root behaviour paints the control's
    /// `BackColor` (nothing when unset: the parent's surface shows through) — a colour with alpha
    /// over the parent's background when the class has `SUPPORTS_TRANSPARENT_BACK_COLOR`, made
    /// opaque otherwise — then its `BackgroundImage` laid out by `BackgroundImageLayout`. It raises
    /// no event (WinForms has none either).
    fn on_paint_background(&mut self, e: &mut PaintEventCx<'_>) {
        if let Some(base) = self.base_control_mut() {
            return base.on_paint_background(e);
        }
        paint_background_default(self.control_core(), e);
    }

    /// Renders the control for printing or an off-screen capture (WinForms `OnPrint`):
    /// [`Control::draw_to_bitmap`], `WM_PRINT`/`WM_PRINTCLIENT`, printing. The default paints both
    /// layers ([`Control::paint_layers`]: background unless `OPAQUE`, then `on_paint`, which raises
    /// `Paint`); it raises no event of its own. Override it to render differently on paper (no
    /// hover, a white ground…) and call `self.paint_layers(e)` for the normal rendering.
    fn on_print(&mut self, e: &mut PaintEventCx<'_>) {
        if self.get_style(ControlStyles::USER_PAINT) {
            self.paint_layers(e);
        }
    }

    /// Called once, before the control is first painted (WinForms `OnCreateControl`).
    fn on_create_control(&mut self) {
        if let Some(base) = self.base_control_mut() {
            base.on_create_control();
        }
    }

    /// The parts a screen reader lists under the control, in its own coordinates (WinForms'
    /// `AccessibleObject.GetChild`): a grid's tiles, a menu's rows — what it paints itself and no
    /// element of the view stands for. None by default.
    fn accessible_parts(&self) -> Vec<AccessiblePart> {
        match self.base_control() {
            Some(base) => base.accessible_parts(),
            None => Vec::new(),
        }
    }

    /// The pointer shape over the point `(x, y)` of the control (its own coordinates), for a control
    /// whose parts want different shapes (a ruler's markers: ↔ over a marker, an arrow elsewhere).
    /// `None` (the default): the element's `Cursor`. Asked every frame the pointer is over it.
    fn cursor_at(&self, x: f32, y: f32) -> Option<kubuno_desktop_controls::host::Cursor> {
        self.base_control().and_then(|base| base.cursor_at(x, y))
    }

    /// The tooltip over the point `(x, y)` of the control (its own coordinates), for a control whose
    /// parts are named (a ruler's « Retrait gauche »). `None` (the default): the element's `ToolTip`.
    /// Asked every frame the pointer is over it.
    fn tool_tip_at(&self, x: f32, y: f32) -> Option<String> {
        self.base_control().and_then(|base| base.tool_tip_at(x, y))
    }

    /// The size the control would like within `proposed` (WinForms `GetPreferredSize`): what a
    /// layout gives an auto-sized control. The root answer is the current size, at least the
    /// minimum size.
    fn get_preferred_size(&self, canvas: &dyn Canvas, proposed: Size) -> Size {
        if let Some(base) = self.base_control() {
            return base.get_preferred_size(canvas, proposed);
        }
        // `ControlBase`'s own `clamp`/`size`, named: through `ControlCore` they would resolve to
        // `Control::size`, this trait's, which calls back here.
        let base: &ControlBase = &self.control_core().props;
        base.clamp(base.size())
    }

    /// Stores new bounds (WinForms `SetBoundsCore`): only the parts `specified` names change. A
    /// custom container overrides it to constrain its size.
    fn set_bounds_core(&mut self, bounds: Rect, specified: BoundsSpecified) {
        if let Some(base) = self.base_control_mut() {
            return base.set_bounds_core(bounds, specified);
        }
        let core = self.control_core_mut();
        let old = core.bounds;
        let (mut x, mut y, mut w, mut h) = (old.left, old.top, old.right - old.left, old.bottom - old.top);
        if specified.contains(BoundsSpecified::X) {
            x = bounds.left;
        }
        if specified.contains(BoundsSpecified::Y) {
            y = bounds.top;
        }
        if specified.contains(BoundsSpecified::WIDTH) {
            w = bounds.right - bounds.left;
        }
        if specified.contains(BoundsSpecified::HEIGHT) {
            h = bounds.bottom - bounds.top;
        }
        core.bounds = Rect::new(x, y, x + w, y + h);
    }

    /// Whether the control takes `key` itself instead of letting it navigate (WinForms
    /// `IsInputKey`): a dialog key (Tab, arrows, Enter, Escape) the control wants (a text area
    /// takes the arrows, an editor takes Tab). The root answer is `false`.
    fn is_input_key(&self, key: Keys) -> bool {
        match self.base_control() {
            Some(base) => base.is_input_key(key),
            None => false,
        }
    }

    /// Whether the control takes the character `c` (WinForms `IsInputChar`). `true` by default.
    fn is_input_char(&self, c: char) -> bool {
        match self.base_control() {
            Some(base) => base.is_input_char(c),
            None => true,
        }
    }

    /// A command key (a shortcut) seen before anything else handles the key (WinForms
    /// `ProcessCmdKey`): return `true` to consume it — no KeyDown, KeyPress or navigation
    /// follows. Called on the focused control, then on each ancestor that is a control.
    fn process_cmd_key(&mut self, msg: &mut Message, key: Keys) -> bool {
        match self.base_control_mut() {
            Some(base) => base.process_cmd_key(msg, key),
            None => false,
        }
    }

    /// A dialog key the control did not take as input (WinForms `ProcessDialogKey`): return
    /// `true` to consume it (no KeyDown follows).
    fn process_dialog_key(&mut self, key: Keys) -> bool {
        match self.base_control_mut() {
            Some(base) => base.process_dialog_key(key),
            None => false,
        }
    }

    /// The message pre-filter (WinForms `WndProc`): sees the host's synthesized messages (see
    /// [`Message`]) before they become events; return `true` to consume one.
    fn wnd_proc(&mut self, msg: &mut Message) -> bool {
        match self.base_control_mut() {
            Some(base) => base.wnd_proc(msg),
            None => false,
        }
    }

    /// WinForms `CreateParams` (see [`CreateParams`]).
    fn create_params(&self) -> CreateParams {
        match self.base_control() {
            Some(base) => base.create_params(),
            None => CreateParams { class_name: self.class_name().to_string(), caption: self.control_core().text.clone(), style: 0, ex_style: 0 },
        }
    }

    /// Raises `Resize` (WinForms `OnResize`), then invalidates when the class has the
    /// `RESIZE_REDRAW` style.
    fn on_resize(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        if let Some(base) = self.base_control_mut() {
            return base.on_resize(e);
        }
        e.raise(&*self, "OnResize");
        if self.get_style(ControlStyles::RESIZE_REDRAW) {
            self.invalidate();
        }
    }

    overridable! {
        /// Raises `Click` (WinForms `OnClick`). Click carries the mouse args of the click (no
        /// button and no click count when it came from the keyboard).
        fn on_click(MouseEventArgs) => "OnClick";
        /// Raises `DoubleClick`.
        fn on_double_click(MouseEventArgs) => "OnDoubleClick";
        /// Raises `MouseClick`.
        fn on_mouse_click(MouseEventArgs) => "OnMouseClick";
        /// Raises `MouseDoubleClick`.
        fn on_mouse_double_click(MouseEventArgs) => "OnMouseDoubleClick";
        /// Raises `MouseDown`.
        fn on_mouse_down(MouseEventArgs) => "OnMouseDown";
        /// Raises `MouseUp`.
        fn on_mouse_up(MouseEventArgs) => "OnMouseUp";
        /// Raises `MouseMove`.
        fn on_mouse_move(MouseEventArgs) => "OnMouseMove";
        /// Raises `MouseEnter`.
        fn on_mouse_enter(EmptyEventArgs) => "OnMouseEnter";
        /// Raises `MouseLeave`.
        fn on_mouse_leave(EmptyEventArgs) => "OnMouseLeave";
        /// Raises `MouseHover`.
        fn on_mouse_hover(EmptyEventArgs) => "OnMouseHover";
        /// Raises `MouseWheel`.
        fn on_mouse_wheel(MouseEventArgs) => "OnMouseWheel";
        /// Raises `KeyDown`.
        fn on_key_down(KeyEventArgs) => "OnKeyDown";
        /// Raises `KeyUp`.
        fn on_key_up(KeyEventArgs) => "OnKeyUp";
        /// Raises `KeyPress`.
        fn on_key_press(KeyPressEventArgs) => "OnKeyPress";
        /// Raises `Enter` (the focus entered the control or one inside it).
        fn on_enter(EmptyEventArgs) => "OnEnter";
        /// Raises `Leave`.
        fn on_leave(EmptyEventArgs) => "OnLeave";
        /// Raises `GotFocus`.
        fn on_got_focus(EmptyEventArgs) => "OnGotFocus";
        /// Raises `LostFocus`.
        fn on_lost_focus(EmptyEventArgs) => "OnLostFocus";
        /// Raises `Validating`; a handler (or an override) that sets `cancel` keeps the focus.
        fn on_validating(CancelEventArgs) => "OnValidating";
        /// Raises `Validated`.
        fn on_validated(EmptyEventArgs) => "OnValidated";
        /// Raises `Move`.
        fn on_move(EmptyEventArgs) => "OnMove";
        /// Raises `SizeChanged`.
        fn on_size_changed(EmptyEventArgs) => "OnSizeChanged";
        /// Raises `LocationChanged`.
        fn on_location_changed(EmptyEventArgs) => "OnLocationChanged";
        /// Raises `Layout` (WinForms `OnLayout`): a custom container lays its children out here.
        fn on_layout(LayoutEventArgs) => "OnLayout";
        /// Raises `VisibleChanged`.
        fn on_visible_changed(EmptyEventArgs) => "OnVisibleChanged";
        /// Raises `EnabledChanged`.
        fn on_enabled_changed(EmptyEventArgs) => "OnEnabledChanged";
        /// Raises `TextChanged`.
        fn on_text_changed(TextChangedEventArgs) => "OnTextChanged";
        /// Raises `HandleCreated` (after [`Control::on_create_control`]).
        fn on_handle_created(EmptyEventArgs) => "OnHandleCreated";
        /// Raises `HandleDestroyed`.
        fn on_handle_destroyed(EmptyEventArgs) => "OnHandleDestroyed";
        /// Raises `DragEnter` when a drag comes over the control (its `AllowDrop` must be set): set
        /// `e.effect` to accept a drop.
        fn on_drag_enter(DragEventArgs) => "OnDragEnter";
        /// Raises `DragOver` while a drag moves over the control.
        fn on_drag_over(DragEventArgs) => "OnDragOver";
        /// Raises `DragDrop` when the data is dropped on the control.
        fn on_drag_drop(DragEventArgs) => "OnDragDrop";
        /// Raises `DragLeave` when a drag leaves the control or is cancelled.
        fn on_drag_leave(EmptyEventArgs) => "OnDragLeave";
    }

    /// An event this level has no method for (a control's own event: `ItemActivate`,
    /// `StepSelected`…). The root behaviour raises it.
    fn on_event(&mut self, event: &'static str, e: &mut EventCx<'_, dyn EventArgs>) {
        if let Some(base) = self.base_control_mut() {
            return base.on_event(event, e);
        }
        e.raise(&*self, event);
    }

    /// Delivers `event` (its attribute name, `"OnMouseDown"`) to the matching `on_…` method,
    /// virtually (the outermost override runs): the Control methods, the level methods of the
    /// class's chain (`on_checked_changed`, `on_selection_changed`, `on_form_closing`…), else
    /// [`Control::on_event`]. What the hosts call; not meant to be overridden.
    fn dispatch_event(&mut self, event: &'static str, e: &mut EventCx<'_, dyn EventArgs>) {
        e.set_origin(self.class_name());
        macro_rules! to {
            ($target:expr, $method:ident, $args:ty) => {
                if let Some(t) = e.typed::<$args>() {
                    return $target.$method(&mut t.named(event));
                }
            };
        }
        match event {
            "OnClick" => to!(self, on_click, MouseEventArgs),
            "OnDoubleClick" => to!(self, on_double_click, MouseEventArgs),
            "OnMouseClick" => to!(self, on_mouse_click, MouseEventArgs),
            "OnMouseDoubleClick" => to!(self, on_mouse_double_click, MouseEventArgs),
            "OnMouseDown" => to!(self, on_mouse_down, MouseEventArgs),
            "OnMouseUp" => to!(self, on_mouse_up, MouseEventArgs),
            "OnMouseMove" => to!(self, on_mouse_move, MouseEventArgs),
            "OnMouseEnter" => to!(self, on_mouse_enter, EmptyEventArgs),
            "OnMouseLeave" => to!(self, on_mouse_leave, EmptyEventArgs),
            "OnMouseHover" => to!(self, on_mouse_hover, EmptyEventArgs),
            "OnMouseWheel" => to!(self, on_mouse_wheel, MouseEventArgs),
            "OnKeyDown" => to!(self, on_key_down, KeyEventArgs),
            "OnKeyUp" => to!(self, on_key_up, KeyEventArgs),
            "OnKeyPress" => to!(self, on_key_press, KeyPressEventArgs),
            "OnEnter" => to!(self, on_enter, EmptyEventArgs),
            "OnLeave" => to!(self, on_leave, EmptyEventArgs),
            "OnGotFocus" => to!(self, on_got_focus, EmptyEventArgs),
            "OnLostFocus" => to!(self, on_lost_focus, EmptyEventArgs),
            "OnValidating" => to!(self, on_validating, CancelEventArgs),
            "OnValidated" => to!(self, on_validated, EmptyEventArgs),
            "OnResize" => to!(self, on_resize, EmptyEventArgs),
            "OnMove" => to!(self, on_move, EmptyEventArgs),
            "OnSizeChanged" => to!(self, on_size_changed, EmptyEventArgs),
            "OnLocationChanged" => to!(self, on_location_changed, EmptyEventArgs),
            "OnLayout" => to!(self, on_layout, LayoutEventArgs),
            "OnVisibleChanged" => to!(self, on_visible_changed, EmptyEventArgs),
            "OnEnabledChanged" => to!(self, on_enabled_changed, EmptyEventArgs),
            "OnTextChanged" => to!(self, on_text_changed, TextChangedEventArgs),
            "OnHandleCreated" => to!(self, on_handle_created, EmptyEventArgs),
            "OnHandleDestroyed" => to!(self, on_handle_destroyed, EmptyEventArgs),
            "OnDragEnter" => to!(self, on_drag_enter, DragEventArgs),
            "OnDragOver" => to!(self, on_drag_over, DragEventArgs),
            "OnDragDrop" => to!(self, on_drag_drop, DragEventArgs),
            "OnDragLeave" => to!(self, on_drag_leave, EmptyEventArgs),
            "OnCheckedChanged" => {
                if let Some(b) = self.as_button_base_mut() {
                    to!(b, on_checked_changed, CheckedChangedEventArgs);
                }
            }
            "OnSelectionChanged" | "OnSelectedIndexChanged" => {
                if let Some(l) = self.as_list_control_mut() {
                    to!(l, on_selection_changed, SelectionChangedEventArgs);
                }
            }
            "OnSelectedValueChanged" => {
                if let Some(l) = self.as_list_control_mut() {
                    to!(l, on_selected_value_changed, TextChangedEventArgs);
                }
            }
            "OnValueChanged" => {
                if let Some(r) = self.as_range_base_mut() {
                    to!(r, on_value_changed, NumericValueChangedEventArgs);
                }
            }
            "OnScroll" => {
                if let Some(s) = self.as_scrollable_control_mut() {
                    to!(s, on_scroll, ScrollEventArgs);
                } else if let Some(r) = self.as_range_base_mut() {
                    to!(r, on_scroll, ScrollEventArgs);
                }
            }
            "OnLoad" | "OnShown" | "OnActivated" | "OnDeactivate" | "OnFormClosing" | "OnFormClosed" => {
                if let Some(v) = self.as_view_mut() {
                    match event {
                        "OnLoad" => to!(v, on_load, EmptyEventArgs),
                        "OnShown" => to!(v, on_shown, EmptyEventArgs),
                        "OnActivated" => to!(v, on_activated, EmptyEventArgs),
                        "OnDeactivate" => to!(v, on_deactivate, EmptyEventArgs),
                        "OnFormClosing" => to!(v, on_form_closing, FormClosingEventArgs),
                        _ => to!(v, on_form_closed, FormClosedEventArgs),
                    }
                } else if event == "OnLoad" {
                    if let Some(u) = self.as_user_control_mut() {
                        to!(u, on_load, EmptyEventArgs);
                    }
                }
            }
            _ => {}
        }
        self.on_event(event, e);
    }

    // ── Event accessors ─────────────────────────────────────────────────────────────────

    accessors! {
        /// `Click` (`button.click().subscribe(|sender, e| …)`).
        fn click(MouseEventArgs) => "OnClick";
        /// `DoubleClick`.
        fn double_click(MouseEventArgs) => "OnDoubleClick";
        /// `MouseClick`.
        fn mouse_click(MouseEventArgs) => "OnMouseClick";
        /// `MouseDoubleClick`.
        fn mouse_double_click(MouseEventArgs) => "OnMouseDoubleClick";
        /// `MouseDown`.
        fn mouse_down(MouseEventArgs) => "OnMouseDown";
        /// `MouseUp`.
        fn mouse_up(MouseEventArgs) => "OnMouseUp";
        /// `MouseMove`.
        fn mouse_move(MouseEventArgs) => "OnMouseMove";
        /// `MouseEnter`.
        fn mouse_enter(EmptyEventArgs) => "OnMouseEnter";
        /// `MouseLeave`.
        fn mouse_leave(EmptyEventArgs) => "OnMouseLeave";
        /// `MouseHover`.
        fn mouse_hover(EmptyEventArgs) => "OnMouseHover";
        /// `MouseWheel`.
        fn mouse_wheel(MouseEventArgs) => "OnMouseWheel";
        /// `KeyDown`.
        fn key_down(KeyEventArgs) => "OnKeyDown";
        /// `KeyUp`.
        fn key_up(KeyEventArgs) => "OnKeyUp";
        /// `KeyPress`.
        fn key_press(KeyPressEventArgs) => "OnKeyPress";
        /// `Enter`.
        fn enter(EmptyEventArgs) => "OnEnter";
        /// `Leave`.
        fn leave(EmptyEventArgs) => "OnLeave";
        /// `GotFocus`.
        fn got_focus(EmptyEventArgs) => "OnGotFocus";
        /// `LostFocus`.
        fn lost_focus(EmptyEventArgs) => "OnLostFocus";
        /// `Validating`.
        fn validating(CancelEventArgs) => "OnValidating";
        /// `Validated`.
        fn validated(EmptyEventArgs) => "OnValidated";
        /// `Resize`.
        fn resize(EmptyEventArgs) => "OnResize";
        /// `Move`.
        fn moved(EmptyEventArgs) => "OnMove";
        /// `SizeChanged`.
        fn size_changed(EmptyEventArgs) => "OnSizeChanged";
        /// `LocationChanged`.
        fn location_changed(EmptyEventArgs) => "OnLocationChanged";
        /// `Layout`.
        fn layout(LayoutEventArgs) => "OnLayout";
        /// `VisibleChanged`.
        fn visible_changed(EmptyEventArgs) => "OnVisibleChanged";
        /// `EnabledChanged`.
        fn enabled_changed(EmptyEventArgs) => "OnEnabledChanged";
        /// `TextChanged`.
        fn text_changed(TextChangedEventArgs) => "OnTextChanged";
        /// `Paint`: its args lend the surface (`e.graphics()`) and the painted rectangle.
        fn paint(PaintEventArgs) => "OnPaint";
        /// `DragEnter`.
        fn drag_enter(DragEventArgs) => "OnDragEnter";
        /// `DragOver`.
        fn drag_over(DragEventArgs) => "OnDragOver";
        /// `DragDrop`.
        fn drag_drop(DragEventArgs) => "OnDragDrop";
        /// `DragLeave`.
        fn drag_leave(EmptyEventArgs) => "OnDragLeave";
        /// `HandleCreated`.
        fn handle_created(EmptyEventArgs) => "OnHandleCreated";
        /// `HandleDestroyed`.
        fn handle_destroyed(EmptyEventArgs) => "OnHandleDestroyed";
    }
}
