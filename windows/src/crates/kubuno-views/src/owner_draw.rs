//! Owner-draw in views (EVT-8): an element whose `DrawMode` (or `OwnerDraw`) asks for it has its
//! items drawn by its `OnDrawItem` handler — and measured by its `OnMeasureItem` handler under
//! `OwnerDrawVariable` — with the surface lent to the handler ([`crate::events::DrawItemEventArgs`]).
//!
//! The widgets of `kubuno_ui` raise their items to the owner-draw handler lent for their paint
//! (`kubuno_ui::graphics::owner_draw::with_handler`); a node lends [`ElementOwnerDraw`], which turns
//! each item into the element's event: through its class's `on_…` method when it has one (a class
//! extending `ListBox` can override `on_event`), then to its `.kbview` handler.

use kubuno_ui::graphics::owner_draw::{self, DrawItemEventArgs as UiDrawItem, MeasureItemEventArgs as UiMeasureItem, OwnerDrawHandler};
use kubuno_ui::Canvas;

use crate::events::{DrawItemEventArgs, MeasureItemEventArgs};
use crate::node::PaintCx;
use crate::props::{BuildError, Props};

/// The owner-draw handlers an element names.
#[derive(Debug, Clone, Default)]
pub struct OwnerDrawEvents {
    pub draw_item: Option<String>,
    pub measure_item: Option<String>,
}

impl OwnerDrawEvents {
    /// Reads `OnDrawItem` / `OnMeasureItem` (and their aliases) from the element.
    pub fn read(props: &Props<'_>) -> Self {
        Self { draw_item: props.event("OnDrawItem"), measure_item: props.event("OnMeasureItem") }
    }

    /// Whether the element names a `DrawItem` handler (without one, items paint normally).
    pub fn active(&self) -> bool {
        self.draw_item.is_some()
    }
}

/// The `DrawMode` property of an element (`Normal`, `OwnerDrawFixed`, `OwnerDrawVariable`), read
/// each frame (it may be bound).
pub fn draw_mode_prop(props: &Props<'_>) -> Result<crate::binding::PropSource<String>, BuildError> {
    props.enum_("DrawMode", "Normal")
}

/// A `DrawMode` value.
pub fn parse_draw_mode(value: &str) -> kubuno_ui::graphics::DrawMode {
    use kubuno_ui::graphics::DrawMode;
    match value {
        "OwnerDrawFixed" => DrawMode::OwnerDrawFixed,
        "OwnerDrawVariable" => DrawMode::OwnerDrawVariable,
        _ => DrawMode::Normal,
    }
}

/// The metadata of `DrawMode` on a list (`Normal`, `OwnerDrawFixed`, `OwnerDrawVariable`).
pub const DRAW_MODE: crate::registry::PropertyMeta = crate::registry::PropertyMeta::new(
    "DrawMode",
    crate::registry::PropKind::Enum(&["Normal", "OwnerDrawFixed", "OwnerDrawVariable"]),
    "Normal",
    "Who draws the items: the control (Normal), or your DrawItem handler with one height for all items (OwnerDrawFixed) or a height per item from your MeasureItem handler (OwnerDrawVariable).",
)
.category("Behavior");

/// `DrawMode` where only fixed heights exist (tabs).
pub const DRAW_MODE_FIXED: crate::registry::PropertyMeta = crate::registry::PropertyMeta::new(
    "DrawMode",
    crate::registry::PropKind::Enum(&["Normal", "OwnerDrawFixed"]),
    "Normal",
    "Who draws the items: the control (Normal) or your DrawItem handler (OwnerDrawFixed).",
)
.category("Behavior");

/// `OwnerDraw` (list views, tables, menus): the items are drawn by the DrawItem handler.
pub const OWNER_DRAW: crate::registry::PropertyMeta =
    crate::registry::PropertyMeta::new("OwnerDraw", crate::registry::PropKind::Bool, "false", "Draws the items with your DrawItem handler instead of the control's own look.").category("Behavior");

/// The `DrawItem` event.
pub const ON_DRAW_ITEM: crate::registry::EventMeta = crate::registry::EventMeta::new("OnDrawItem", "Occurs when an owner-drawn item must be drawn: draw it with e.graphics() in e.bounds.")
    .category(crate::registry::EventCategory::Behavior)
    .args::<DrawItemEventArgs>();

/// The `MeasureItem` event.
pub const ON_MEASURE_ITEM: crate::registry::EventMeta =
    crate::registry::EventMeta::new("OnMeasureItem", "Occurs when the height of an item of an OwnerDrawVariable list is needed: set e.item_height.")
        .category(crate::registry::EventCategory::Behavior)
        .args::<MeasureItemEventArgs>();

/// The owner-draw handler a node lends while its widget paints.
pub struct ElementOwnerDraw<'x, 'a> {
    pub cx: &'x mut PaintCx<'a>,
    pub events: &'x OwnerDrawEvents,
    /// The bound list's row of each item shown (a sorted table): what `DrawItemEventArgs::index`
    /// reports, so the handler reads its own data at the right row whatever the sort.
    pub rows: Option<&'x [usize]>,
}

impl OwnerDrawHandler for ElementOwnerDraw<'_, '_> {
    fn measure_item(&mut self, e: &mut UiMeasureItem<'_>) {
        let Some(handler) = self.events.measure_item.as_deref() else { return };
        let cx = &mut *self.cx;
        MeasureItemEventArgs::lend(e, |args| cx.fire_quiet("OnMeasureItem", Some(handler), args));
    }

    fn draw_item(&mut self, e: &mut UiDrawItem<'_>) {
        let Some(handler) = self.events.draw_item.as_deref() else {
            e.draw_default = true;
            return;
        };
        let cx = &mut *self.cx;
        let rows = self.rows;
        DrawItemEventArgs::lend(e, |args| {
            if let (Some(rows), Some(i)) = (rows, args.index) {
                args.index = rows.get(i).copied().or(Some(i));
            }
            cx.fire_quiet("OnDrawItem", Some(handler), args)
        });
    }
}

/// Runs `paint` (the widget's paint on `cx.canvas`) with the element's owner-draw handler lent when
/// it names one; plainly otherwise.
pub fn paint_with<R>(cx: &mut PaintCx<'_>, events: &OwnerDrawEvents, paint: impl FnOnce(&dyn Canvas) -> R) -> R {
    let canvas: &dyn Canvas = cx.canvas;
    if !events.active() {
        return paint(canvas);
    }
    let mut handler = ElementOwnerDraw { cx, events, rows: None };
    owner_draw::with_handler(&mut handler, || paint(canvas))
}

/// [`paint_with`] for a list whose items are rows of a bound list in another order (a sorted
/// table): `rows[item]` is the list's row the handler is told about.
pub fn paint_with_rows<R>(cx: &mut PaintCx<'_>, events: &OwnerDrawEvents, rows: &[usize], paint: impl FnOnce(&dyn Canvas) -> R) -> R {
    let canvas: &dyn Canvas = cx.canvas;
    if !events.active() {
        return paint(canvas);
    }
    let mut handler = ElementOwnerDraw { cx, events, rows: Some(rows) };
    owner_draw::with_handler(&mut handler, || paint(canvas))
}

/// Paints a popup list whose owner-drawn rows were recorded during the page's paint
/// ([`kubuno_ui::graphics::owner_draw::RecordedItems`]): the recording is lent as the handler.
pub fn replay_in_popup(mut recorded: owner_draw::RecordedItems, paint: impl FnOnce()) {
    if recorded.is_empty() {
        paint();
    } else {
        owner_draw::with_handler(&mut recorded, paint);
    }
}
