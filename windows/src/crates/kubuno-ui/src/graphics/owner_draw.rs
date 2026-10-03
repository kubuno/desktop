//! Owner-draw: a list-like control (`ListBox`, `ComboBox`/`Dropdown`, `ListView`, `TreeView`,
//! `DataTable` cells, `Tabs`, menus) hands the painting of its items to its owner — WinForms'
//! `DrawMode.OwnerDrawFixed` / `OwnerDrawVariable` with the `DrawItem` and `MeasureItem` events.
//!
//! The widgets of this crate are painted with `&self` inside a frame, so the owner's handler is
//! not stored on them: it is **lent for a paint** with [`with_handler`] (a scoped, re-entrancy-safe
//! thread-local), and a widget whose draw mode asks for it calls [`draw_item`] / [`measure_item`]
//! for each item. With no handler lent, the widget paints its items normally.
//!
//! ```ignore
//! list.draw_mode = DrawMode::OwnerDrawFixed;
//! owner_draw::with_handler(&mut |e: &mut DrawItemEventArgs<'_>| {
//!     e.draw_background();
//!     e.graphics.draw_icon("folder", icon_rect(e.bounds), 16.0, e.fore_color);
//!     e.graphics.draw_string(&e.text, &e.font, e.fore_color, text_rect(e.bounds), &StringFormat::single_line_ellipsis());
//!     e.draw_focus_rectangle();
//! }, || list.paint(canvas, bounds, state));
//! ```

use std::cell::RefCell;
use std::ptr::NonNull;

use drive_app_controls::Rect;

use super::paint::{DashStyle, Pen};
use super::text::{Font, StringFormat};
use super::types::{Color, RectExt};
use super::Graphics;

/// Who paints a list's items (`DrawMode`): the list, or its owner with fixed or measured heights.
pub use kubuno_controls::lists::DrawMode;

/// What an item looks like when it is drawn (`DrawItemState`, WinForms' values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DrawItemState(pub u32);

impl DrawItemState {
    pub const NONE: Self = Self(0);
    pub const SELECTED: Self = Self(1);
    pub const GRAYED: Self = Self(2);
    pub const DISABLED: Self = Self(4);
    pub const CHECKED: Self = Self(8);
    pub const FOCUS: Self = Self(16);
    pub const DEFAULT: Self = Self(32);
    pub const HOT_LIGHT: Self = Self(64);
    pub const INACTIVE: Self = Self(128);
    pub const NO_ACCELERATOR: Self = Self(256);
    pub const NO_FOCUS_RECT: Self = Self(512);
    /// The item is drawn in a combo box's edit field, not its list.
    pub const COMBO_BOX_EDIT: Self = Self(4096);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Sets or clears the bits of `other`.
    pub fn set(&mut self, other: Self, on: bool) {
        if on {
            self.0 |= other.0;
        } else {
            self.0 &= !other.0;
        }
    }

    pub fn with(mut self, other: Self, on: bool) -> Self {
        self.set(other, on);
        self
    }
}

impl std::ops::BitOr for DrawItemState {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// The args of `DrawItem` (`DrawItemEventArgs`, with ListView's/TreeView's `DrawDefault`).
pub struct DrawItemEventArgs<'g> {
    /// Where to draw (`e.Graphics`).
    pub graphics: &'g Graphics<'g>,
    /// The control raising it: `"ListBox"`, `"ComboBox"`, `"ListView"`, `"TreeView"`, `"DataTable"`,
    /// `"Tabs"`, `"Menu"`.
    pub control: &'static str,
    /// The item (`Index`); `None` for a combo box's empty edit field (WinForms' `-1`).
    pub index: Option<usize>,
    /// The column of a cell or sub-item (`DataTable`, `ListView` details), `None` for a whole item.
    pub sub_index: Option<usize>,
    /// The item's rectangle, in the surface's coordinates (`Bounds`).
    pub bounds: Rect,
    pub state: DrawItemState,
    /// The item's text as the control would show it (Kubuno: WinForms reads `Items[e.Index]`).
    pub text: String,
    pub font: Font,
    /// The colour the control would draw the text in for this state (`ForeColor`).
    pub fore_color: Color,
    /// The colour the control would fill the item with for this state (`BackColor`).
    pub back_color: Color,
    /// Set it to let the control draw the item itself (`DrawDefault`); a handler that sets it and
    /// draws nothing gets the normal look.
    pub draw_default: bool,
}

impl<'g> DrawItemEventArgs<'g> {
    /// Args for `index` in `bounds`, coloured from the theme for `state`.
    pub fn new(graphics: &'g Graphics<'g>, control: &'static str, index: Option<usize>, bounds: Rect, state: DrawItemState, text: impl Into<String>) -> Self {
        let theme = graphics.theme_colors();
        let selected = state.contains(DrawItemState::SELECTED);
        let dead = state.contains(DrawItemState::DISABLED) || state.contains(DrawItemState::GRAYED);
        let back_color: Color = if selected {
            theme.list_selected.into()
        } else if state.contains(DrawItemState::HOT_LIGHT) {
            theme.row_hover.into()
        } else {
            Color::TRANSPARENT
        };
        let mut fore_color: Color = theme.text_primary.into();
        if dead {
            fore_color = fore_color.with_alpha(0.45);
        }
        Self { graphics, control, index, sub_index: None, bounds, state, text: text.into(), font: Font::default(), fore_color, back_color, draw_default: false }
    }

    /// Fills the item with its [`DrawItemEventArgs::back_color`] (`DrawBackground`).
    pub fn draw_background(&self) {
        if !self.back_color.is_transparent() {
            self.graphics.fill_rounded_rectangle(self.back_color, self.bounds, 4.0);
        }
    }

    /// The focus cue, when the item has the focus and wants one (`DrawFocusRectangle`): a dotted
    /// rectangle inside the bounds.
    pub fn draw_focus_rectangle(&self) {
        if self.state.contains(DrawItemState::FOCUS) && !self.state.contains(DrawItemState::NO_FOCUS_RECT) {
            let pen = Pen::new(self.fore_color, 1.0).with_dash(DashStyle::Dot);
            self.graphics.draw_rectangle(&pen, self.bounds.inflated(-1.5, -1.5));
        }
    }

    /// The item's text as the control lays a label out: one line, left, vertically centred, with an
    /// ellipsis (a Kubuno convenience).
    pub fn draw_text(&self) {
        let r = Rect::new(self.bounds.left + 8.0, self.bounds.top, self.bounds.right - 8.0, self.bounds.bottom);
        self.graphics.draw_string(&self.text, &self.font, self.fore_color, r, &StringFormat::single_line_ellipsis());
    }
}

/// The args of `MeasureItem` (`MeasureItemEventArgs`): set [`MeasureItemEventArgs::item_height`]
/// (and `item_width`) for a variable-height item.
pub struct MeasureItemEventArgs<'g> {
    /// For measuring text (`e.Graphics.MeasureString`).
    pub graphics: &'g Graphics<'g>,
    pub control: &'static str,
    pub index: usize,
    pub text: String,
    /// In: the control's own row height. Out: this item's.
    pub item_height: f32,
    /// In: the control's width. Out: the item's width (used by a combo box's drop-down width).
    pub item_width: f32,
}

/// The owner of owner-drawn items: `DrawItem` (required) and `MeasureItem` (for
/// `OwnerDrawVariable`). A closure `FnMut(&mut DrawItemEventArgs)` is a handler.
pub trait OwnerDrawHandler {
    /// `MeasureItem`: the default keeps the control's row height.
    fn measure_item(&mut self, _e: &mut MeasureItemEventArgs<'_>) {}
    /// `DrawItem`.
    fn draw_item(&mut self, e: &mut DrawItemEventArgs<'_>);
}

impl<F: FnMut(&mut DrawItemEventArgs<'_>)> OwnerDrawHandler for F {
    fn draw_item(&mut self, e: &mut DrawItemEventArgs<'_>) {
        self(e)
    }
}

type Erased = NonNull<dyn OwnerDrawHandler + 'static>;

thread_local! {
    /// The handlers lent by [`with_handler`], innermost last.
    static HANDLERS: RefCell<Vec<Option<Erased>>> = const { RefCell::new(Vec::new()) };
}

struct Pop;

impl Drop for Pop {
    fn drop(&mut self) {
        HANDLERS.with(|h| {
            h.borrow_mut().pop();
        });
    }
}

/// Lends `handler` to the owner-drawn widgets painted by `f` (see the module doc). Nests: an inner
/// call shadows an outer one until it returns.
pub fn with_handler<R>(handler: &mut dyn OwnerDrawHandler, f: impl FnOnce() -> R) -> R {
    let ptr: NonNull<dyn OwnerDrawHandler + '_> = NonNull::from(handler);
    // SAFETY: only the trait object's lifetime bound is erased (same layout). The pointer is used
    // while `f` runs — during which `handler` stays mutably borrowed by this call — and removed by
    // `Pop` before this function returns or unwinds.
    let erased: Erased = unsafe { std::mem::transmute::<NonNull<dyn OwnerDrawHandler + '_>, Erased>(ptr) };
    HANDLERS.with(|h| h.borrow_mut().push(Some(erased)));
    let _pop = Pop;
    f()
}

/// Whether a handler is lent (the widgets paint normally when not).
pub fn has_handler() -> bool {
    HANDLERS.with(|h| h.borrow().last().is_some_and(Option::is_some))
}

/// Runs `call` with the innermost lent handler taken out of the stack (a widget painted by the
/// handler itself does not reach it again: no second `&mut`), `false` when none is lent.
fn call_top(call: impl FnOnce(&mut dyn OwnerDrawHandler)) -> bool {
    let taken = HANDLERS.with(|h| h.borrow_mut().last_mut().and_then(Option::take));
    let Some(mut ptr) = taken else { return false };
    struct PutBack(Erased);
    impl Drop for PutBack {
        fn drop(&mut self) {
            HANDLERS.with(|h| {
                if let Some(slot) = h.borrow_mut().last_mut() {
                    *slot = Some(self.0);
                }
            });
        }
    }
    let _back = PutBack(ptr);
    // SAFETY: the pointer came from a `&mut` that `with_handler` keeps borrowed for as long as it
    // is in the stack; taking it out of the stack while it runs makes this the only access.
    let handler = unsafe { ptr.as_mut() };
    call(handler);
    true
}

/// Raises `DrawItem` to the lent handler. Returns `false` when no handler is lent, or when the
/// handler asked for the default drawing ([`DrawItemEventArgs::draw_default`]): the widget then
/// draws the item itself.
pub fn draw_item(e: &mut DrawItemEventArgs<'_>) -> bool {
    call_top(|h| h.draw_item(e)) && !e.draw_default
}

/// Raises `MeasureItem` to the lent handler; `false` when none is lent (the height is unchanged).
pub fn measure_item(e: &mut MeasureItemEventArgs<'_>) -> bool {
    call_top(|h| h.measure_item(e))
}

/// Owner-drawn items **recorded ahead of time** and replayed later: what a list painted in its own
/// popup window needs (a combo box's drop-down is painted after the page, when the owner's handler
/// — its view model — is no longer at hand). The page's paint records each visible item through the
/// real handler ([`RecordedItems::record`]); the popup lends the recording as its handler.
#[derive(Clone, Default)]
pub struct RecordedItems {
    items: Vec<(usize, super::DisplayList, bool)>,
}

impl RecordedItems {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records item `index` drawn by `handler` in `bounds` (the popup's coordinates).
    pub fn record(&mut self, handler: &mut dyn OwnerDrawHandler, control: &'static str, index: usize, bounds: Rect, state: DrawItemState, text: &str) {
        let g = Graphics::recorder();
        let mut e = DrawItemEventArgs::new(&g, control, Some(index), bounds, state, text);
        handler.draw_item(&mut e);
        let default = e.draw_default;
        let list = g.take_recording().unwrap_or_default();
        self.items.push((index, list, default));
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl OwnerDrawHandler for RecordedItems {
    fn draw_item(&mut self, e: &mut DrawItemEventArgs<'_>) {
        match self.items.iter().find(|(i, ..)| Some(*i) == e.index) {
            Some((_, list, false)) => list.replay(e.graphics),
            _ => e.draw_default = true,
        }
    }
}

/// The heights of `count` items of an `OwnerDrawVariable` list: `MeasureItem` for each, starting
/// from `row_height` (with `text(i)` and `width`). `None` when no handler is lent.
pub fn measure_items(graphics: &Graphics<'_>, control: &'static str, count: usize, row_height: f32, width: f32, text: impl Fn(usize) -> String) -> Option<Vec<f32>> {
    if !has_handler() {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let mut e = MeasureItemEventArgs { graphics, control, index: i, text: text(i), item_height: row_height, item_width: width };
        measure_item(&mut e);
        out.push(e.item_height.max(1.0));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::RectExt;

    #[test]
    fn a_lent_handler_draws_items_and_can_ask_for_the_default() {
        let g = Graphics::recorder();
        let mut seen = Vec::new();
        let mut handler = |e: &mut DrawItemEventArgs<'_>| {
            seen.push((e.index, e.state.contains(DrawItemState::SELECTED)));
            if e.index == Some(1) {
                e.draw_default = true;
            } else {
                e.draw_background();
                e.draw_text();
            }
        };
        let drawn: Vec<bool> = with_handler(&mut handler, || {
            (0..3)
                .map(|i| {
                    let state = DrawItemState::NONE.with(DrawItemState::SELECTED, i == 0);
                    let mut e = DrawItemEventArgs::new(&g, "ListBox", Some(i), Rect::from_xywh(0.0, i as f32 * 20.0, 100.0, 20.0), state, format!("item {i}"));
                    draw_item(&mut e)
                })
                .collect()
        });
        assert_eq!(drawn, vec![true, false, true], "item 1 asked for the default look");
        assert_eq!(seen, vec![(Some(0), true), (Some(1), false), (Some(2), false)]);
        let ops = g.recorded().map(|l| l.describe()).unwrap_or_default();
        // Selected item 0: background + text; item 2 (not selected, not hot): text only.
        assert_eq!(ops, vec!["FillRoundedRect", "Text(\"item 0\")", "Text(\"item 2\")"]);
    }

    #[test]
    fn without_a_handler_nothing_is_raised() {
        let g = Graphics::recorder();
        let mut e = DrawItemEventArgs::new(&g, "ListBox", Some(0), Rect::default(), DrawItemState::NONE, "x");
        assert!(!has_handler());
        assert!(!draw_item(&mut e));
        assert!(measure_items(&g, "ListBox", 3, 20.0, 100.0, |_| String::new()).is_none());
    }

    struct Tall;
    impl OwnerDrawHandler for Tall {
        fn measure_item(&mut self, e: &mut MeasureItemEventArgs<'_>) {
            e.item_height = if e.index.is_multiple_of(2) { 40.0 } else { e.item_height };
        }
        fn draw_item(&mut self, _e: &mut DrawItemEventArgs<'_>) {}
    }

    #[test]
    fn measure_runs_per_item_and_handlers_nest_without_aliasing() {
        let g = Graphics::recorder();
        let heights = with_handler(&mut Tall, || measure_items(&g, "ListBox", 3, 20.0, 100.0, |i| i.to_string()));
        assert_eq!(heights, Some(vec![40.0, 20.0, 40.0]));

        // A handler that paints another owner-drawn item: the nested raise finds no handler (the
        // outer one is running), unless an inner one is lent.
        let mut inner_calls = 0;
        let mut outer = |e: &mut DrawItemEventArgs<'_>| {
            let g2 = e.graphics;
            let mut nested = DrawItemEventArgs::new(g2, "Menu", Some(9), e.bounds, DrawItemState::NONE, "n");
            assert!(!draw_item(&mut nested), "the running handler is not reachable again");
            let mut inner = |_: &mut DrawItemEventArgs<'_>| inner_calls += 1;
            with_handler(&mut inner, || {
                let mut nested = DrawItemEventArgs::new(g2, "Menu", Some(9), e.bounds, DrawItemState::NONE, "n");
                assert!(draw_item(&mut nested));
            });
        };
        with_handler(&mut outer, || {
            let mut e = DrawItemEventArgs::new(&g, "ListBox", Some(0), Rect::default(), DrawItemState::NONE, "x");
            assert!(draw_item(&mut e));
            // After the raise the outer handler is back.
            let mut e = DrawItemEventArgs::new(&g, "ListBox", Some(1), Rect::default(), DrawItemState::NONE, "y");
            assert!(draw_item(&mut e));
        });
        assert_eq!(inner_calls, 2);
        assert!(!has_handler());
    }

    #[test]
    fn focus_rectangles_are_dotted_and_optional() {
        let g = Graphics::recorder();
        let e = DrawItemEventArgs::new(&g, "ListBox", Some(0), Rect::from_xywh(0.0, 0.0, 50.0, 20.0), DrawItemState::FOCUS, "x");
        e.draw_focus_rectangle();
        let e = DrawItemEventArgs::new(&g, "ListBox", Some(0), Rect::from_xywh(0.0, 0.0, 50.0, 20.0), DrawItemState::FOCUS | DrawItemState::NO_FOCUS_RECT, "x");
        e.draw_focus_rectangle();
        let list = g.take_recording().unwrap_or_default();
        assert_eq!(list.describe(), vec!["StrokeRect"]);
        match &list.ops[0] {
            crate::graphics::Op::Stroke { pen, .. } => assert_eq!(pen.dash_style, DashStyle::Dot),
            other => panic!("unexpected {other:?}"),
        }
    }
}
