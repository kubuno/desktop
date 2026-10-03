//! `<BindingNavigator>` (`vskubuno/docs/DATA.md` §8, lot DATA-2, WinForms `BindingNavigator`): the
//! tool strip that moves through a binding source and edits it —
//! first / previous | position box / count | next / last | add / delete | save — with the standard
//! Kubuno icons (Lucide), disabled where the action is not possible.
//!
//! ```xml
//! <BindingNavigator BindingSource="customers" X="16" Y="16" Width="330" Height="32"/>
//! ```
//!
//! - The position box takes a row number (type it, Enter moves there, Escape gives up).
//! - Add and Delete call the binding source's `add_new` / `remove_current`; a refusal (a row that
//!   does not validate, `AllowNew="false"`) shows through the ErrorProvider like any edit.
//! - Save (`ShowSaveItem`) saves the binding source's changes through its TableAdapter in one
//!   transaction (`AutoSave="true"`, the default), and raises `SaveItemClick`: with
//!   `AutoSave="false"` the view model saves in its handler (`kubuno_data::save_all` for a master and
//!   its details).
//! - Every item raises `ItemClicked` with its name (`MoveFirstItem`, `MovePreviousItem`,
//!   `MoveNextItem`, `MoveLastItem`, `AddNewItem`, `DeleteItem`, `SaveItem`, `PositionItem`).

use kubuno_views::events::EventArgs;
use kubuno_views::prelude::*;

use crate::binding_source::BindingSource;

/// `ItemClicked` args: which item of the navigator was clicked.
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
pub struct NavigatorItemClickedEventArgs {
    /// `MoveFirstItem`, `MovePreviousItem`, `MoveNextItem`, `MoveLastItem`, `AddNewItem`,
    /// `DeleteItem`, `SaveItem`, `PositionItem`.
    pub item: String,
}

/// The items of the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigatorItem {
    MoveFirst,
    MovePrevious,
    Position,
    MoveNext,
    MoveLast,
    AddNew,
    Delete,
    Save,
}

impl NavigatorItem {
    pub fn name(self) -> &'static str {
        match self {
            NavigatorItem::MoveFirst => "MoveFirstItem",
            NavigatorItem::MovePrevious => "MovePreviousItem",
            NavigatorItem::Position => "PositionItem",
            NavigatorItem::MoveNext => "MoveNextItem",
            NavigatorItem::MoveLast => "MoveLastItem",
            NavigatorItem::AddNew => "AddNewItem",
            NavigatorItem::Delete => "DeleteItem",
            NavigatorItem::Save => "SaveItem",
        }
    }

    /// The Lucide icon of a button item.
    fn icon(self) -> &'static str {
        match self {
            NavigatorItem::MoveFirst => "ChevronsLeft",
            NavigatorItem::MovePrevious => "ChevronLeft",
            NavigatorItem::MoveNext => "ChevronRight",
            NavigatorItem::MoveLast => "ChevronsRight",
            NavigatorItem::AddNew => "Plus",
            NavigatorItem::Delete => "Trash2",
            NavigatorItem::Save => "Save",
            NavigatorItem::Position => "",
        }
    }

    /// The tooltip-like description (accessibility, logs).
    pub fn text(self) -> &'static str {
        match self {
            NavigatorItem::MoveFirst => "Move first",
            NavigatorItem::MovePrevious => "Move previous",
            NavigatorItem::Position => "Current position",
            NavigatorItem::MoveNext => "Move next",
            NavigatorItem::MoveLast => "Move last",
            NavigatorItem::AddNew => "Add new",
            NavigatorItem::Delete => "Delete",
            NavigatorItem::Save => "Save",
        }
    }
}

/// One laid-out part of the strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Part {
    Item(NavigatorItem, Rect),
    /// The `/ N` count after the position box.
    Count(Rect),
    Separator(Rect),
}

/// What the strip shows of its binding source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NavigatorState {
    pub position: i32,
    pub count: usize,
    pub can_add: bool,
    pub can_delete: bool,
    pub has_changes: bool,
    pub busy: bool,
}

impl NavigatorState {
    fn of(bs: &BindingSource) -> Self {
        Self {
            position: bs.position(),
            count: bs.count(),
            can_add: bs.allow_new && !bs.table().columns.is_empty() && (bs.relation().is_none() || bs.master_key().is_some()),
            can_delete: bs.allow_remove && bs.position() >= 0,
            has_changes: bs.has_changes(),
            busy: matches!(bs.get_path("IsBusy"), Some(Value::Bool(true))),
        }
    }

    /// Whether `item` can be used now.
    pub fn enabled(&self, item: NavigatorItem) -> bool {
        let last = self.count as i32 - 1;
        !self.busy
            && match item {
                NavigatorItem::MoveFirst | NavigatorItem::MovePrevious => self.position > 0,
                NavigatorItem::MoveNext | NavigatorItem::MoveLast => self.position >= 0 && self.position < last,
                NavigatorItem::Position => self.count > 0,
                NavigatorItem::AddNew => self.can_add,
                NavigatorItem::Delete => self.can_delete,
                NavigatorItem::Save => self.has_changes,
            }
    }
}

/// The height of the strip and of its buttons, DIP.
const HEIGHT: f32 = 32.0;
const BUTTON: f32 = 28.0;
const POSITION_BOX: f32 = 48.0;
const SEPARATOR: f32 = 9.0;

/// `<BindingNavigator>` (see the module doc).
#[derive(Component)]
#[kubuno(extends = Control, overrides(Control))]
#[toolbox(icon = "navigator", category = "Data")]
#[default_event("ItemClicked")]
#[default_property("BindingSource")]
pub struct BindingNavigator {
    base: ControlCore,
    /// The x:Name of the BindingSource the navigator moves through and edits.
    #[property]
    #[category("Data")]
    pub binding_source: String,
    /// Whether the Save item saves the binding source's changes through its TableAdapter (else only SaveItemClick is raised).
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub auto_save: bool,
    /// Whether the Add item is shown.
    #[property]
    #[category("Appearance")]
    #[default_value(true)]
    pub show_add_item: bool,
    /// Whether the Delete item is shown.
    #[property]
    #[category("Appearance")]
    #[default_value(true)]
    pub show_delete_item: bool,
    /// Whether the Save item is shown.
    #[property]
    #[category("Appearance")]
    #[default_value(true)]
    pub show_save_item: bool,
    /// Occurs when an item of the navigator is clicked.
    #[event]
    #[category("Action")]
    pub item_clicked: Event<NavigatorItemClickedEventArgs>,
    /// Occurs when the Save item is clicked (after the automatic save started, when AutoSave is true).
    #[event]
    #[category("Action")]
    pub save_item_click: Event<EmptyEventArgs>,
    /// The part under the pointer, the part pressed, the text typed in the position box.
    hot: Option<NavigatorItem>,
    pressed: Option<NavigatorItem>,
    typed: Option<String>,
    /// The last state painted (what a click acts on), and the width of its count text.
    state: NavigatorState,
    count_width: f32,
}

impl Default for BindingNavigator {
    fn default() -> Self {
        // Painted every frame: what it shows is another component's state.
        let mut base = ControlCore::default();
        base.styles.set(ControlStyles::OPTIMIZED_DOUBLE_BUFFER, false);
        Self {
            base,
            binding_source: String::new(),
            auto_save: true,
            show_add_item: true,
            show_delete_item: true,
            show_save_item: true,
            item_clicked: Event::default(),
            save_item_click: Event::default(),
            hot: None,
            pressed: None,
            typed: None,
            state: NavigatorState::default(),
            count_width: 24.0,
        }
    }
}

impl BindingNavigator {
    pub fn new(binding_source: impl Into<String>) -> Self {
        Self { binding_source: binding_source.into(), ..Self::default() }
    }

    /// The text of the count (`/ 12`).
    fn count_text(&self) -> String {
        format!("/ {}", self.state.count)
    }

    /// The parts of the strip in `bounds` (left to right); `count_width`: the measured count text.
    pub fn layout(&self, bounds: Rect, count_width: f32) -> Vec<Part> {
        let top = bounds.top + ((bounds.bottom - bounds.top) - BUTTON).max(0.0) / 2.0;
        let mut x = bounds.left + 2.0;
        let mut parts = Vec::new();
        let button = |parts: &mut Vec<Part>, x: &mut f32, item: NavigatorItem| {
            parts.push(Part::Item(item, Rect::new(*x, top, *x + BUTTON, top + BUTTON)));
            *x += BUTTON + 2.0;
        };
        let separator = |parts: &mut Vec<Part>, x: &mut f32| {
            parts.push(Part::Separator(Rect::new(*x, top + 4.0, *x + SEPARATOR, top + BUTTON - 4.0)));
            *x += SEPARATOR;
        };
        button(&mut parts, &mut x, NavigatorItem::MoveFirst);
        button(&mut parts, &mut x, NavigatorItem::MovePrevious);
        separator(&mut parts, &mut x);
        parts.push(Part::Item(NavigatorItem::Position, Rect::new(x, top, x + POSITION_BOX, top + BUTTON)));
        x += POSITION_BOX + 6.0;
        parts.push(Part::Count(Rect::new(x, top, x + count_width, top + BUTTON)));
        x += count_width + 4.0;
        separator(&mut parts, &mut x);
        button(&mut parts, &mut x, NavigatorItem::MoveNext);
        button(&mut parts, &mut x, NavigatorItem::MoveLast);
        if self.show_add_item || self.show_delete_item {
            separator(&mut parts, &mut x);
            if self.show_add_item {
                button(&mut parts, &mut x, NavigatorItem::AddNew);
            }
            if self.show_delete_item {
                button(&mut parts, &mut x, NavigatorItem::Delete);
            }
        }
        if self.show_save_item {
            separator(&mut parts, &mut x);
            button(&mut parts, &mut x, NavigatorItem::Save);
        }
        parts
    }

    /// The item at `(x, y)` (relative to the control), in the last layout.
    pub fn item_at(&self, x: f32, y: f32) -> Option<NavigatorItem> {
        let size = self.base.bounds;
        let local = Rect::new(0.0, 0.0, size.right - size.left, size.bottom - size.top);
        self.layout(local, self.count_width).into_iter().find_map(|p| match p {
            Part::Item(item, r) if r.contains(x, y) => Some(item),
            _ => None,
        })
    }

    /// Performs `item` on the binding source (what a click does). Returns whether it ran.
    pub fn perform(&mut self, item: NavigatorItem) -> bool {
        let name = self.binding_source.trim().to_string();
        let Some(scope) = kubuno_views::scope::current() else {
            tracing::warn!(target: "kubuno_data", "BindingNavigator used outside a view: nothing to navigate");
            return false;
        };
        let ran = scope
            .with::<BindingSource, _>(&name, |bs| {
                // The binding source as it is now (not as last painted).
                if !NavigatorState::of(bs).enabled(item) && item != NavigatorItem::Position {
                    return false;
                }
                let r = match item {
                    NavigatorItem::MoveFirst => bs.move_first(),
                    NavigatorItem::MovePrevious => bs.move_previous(),
                    NavigatorItem::MoveNext => bs.move_next(),
                    NavigatorItem::MoveLast => bs.move_last(),
                    NavigatorItem::AddNew => bs.add_new(),
                    NavigatorItem::Delete => bs.remove_current(),
                    NavigatorItem::Position => match self.typed.as_deref().map(str::trim).and_then(|t| t.parse::<i32>().ok()) {
                        Some(n) => bs.set_position(n - 1),
                        None => Ok(()),
                    },
                    NavigatorItem::Save => Ok(()),
                };
                if let Err(e) = r {
                    // A refused move already reported its summary; the others report here.
                    if !matches!(item, NavigatorItem::MoveFirst | NavigatorItem::MovePrevious | NavigatorItem::MoveNext | NavigatorItem::MoveLast | NavigatorItem::Position) {
                        bs.report_error("", e.to_string());
                    }
                }
                self.state = NavigatorState::of(bs);
                true
            })
            .unwrap_or(false);
        if !ran {
            return false;
        }
        if item == NavigatorItem::Save && self.auto_save {
            drop(kubuno_views::events::spawn_local(async move {
                if let Err(e) = crate::ops::save_scope(&scope, &[name.as_str()]).await {
                    tracing::info!(target: "kubuno_data", error = %e, "the navigator's save failed (reported by the binding source)");
                }
            }));
        }
        self.typed = None;
        self.raise_item_clicked(NavigatorItemClickedEventArgs { item: item.name().to_string() });
        if item == NavigatorItem::Save {
            self.raise_save_item_click(EmptyEventArgs);
        }
        self.invalidate();
        ran
    }
}

impl Control for BindingNavigator {
    fn get_preferred_size(&self, canvas: &dyn Canvas, _proposed: Size) -> Size {
        let count = canvas.measure(&self.count_text(), &canvas.formats().body);
        let parts = self.layout(Rect::new(0.0, 0.0, 10_000.0, HEIGHT), count);
        let right = parts
            .iter()
            .map(|p| match p {
                Part::Item(_, r) | Part::Count(r) | Part::Separator(r) => r.right,
            })
            .fold(0.0, f32::max);
        Size { width: right + 2.0, height: HEIGHT }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        // The binding source as it is now (the view's component, found by name).
        if let Some(scope) = kubuno_views::scope::current() {
            if let Some(s) = scope.with_ref::<BindingSource, _>(self.binding_source.trim(), NavigatorState::of) {
                self.state = s;
            }
        }
        let g = e.graphics;
        let theme = g.theme();
        let bounds = e.clip_rectangle;
        let enabled_control = !e.state.disabled;
        g.fill_rounded(&bounds, 6.0, &theme.layer_background);
        g.stroke_rounded(&bounds, 6.0, &theme.card_stroke);
        let count_text = self.count_text();
        let count_width = g.measure(&count_text, &g.formats().body);
        self.count_width = count_width;
        for part in self.layout(bounds, count_width) {
            match part {
                Part::Separator(r) => {
                    let mid = ((r.left + r.right) / 2.0).round();
                    g.fill_rounded(&Rect::new(mid, r.top, mid + 1.0, r.bottom), 0.0, &theme.divider);
                }
                Part::Count(r) => {
                    g.text(&count_text, &r, &g.formats().body, &theme.text_secondary, false);
                }
                Part::Item(NavigatorItem::Position, r) => {
                    let editing = self.typed.is_some();
                    g.fill_rounded(&r, 4.0, &theme.window_background);
                    g.stroke_rounded(&r, 4.0, if editing { &theme.accent } else { &theme.border_strong });
                    let text = match &self.typed {
                        Some(t) => t.clone(),
                        None if self.state.position >= 0 => (self.state.position + 1).to_string(),
                        None => "0".to_string(),
                    };
                    let color = if enabled_control && self.state.count > 0 { &theme.text_primary } else { &theme.text_tertiary };
                    g.text(&text, &r, &g.formats().body, color, true);
                }
                Part::Item(item, r) => {
                    let enabled = enabled_control && self.state.enabled(item);
                    if enabled && self.pressed == Some(item) {
                        g.fill_rounded(&r, 4.0, &theme.control_fill_pressed);
                    } else if enabled && self.hot == Some(item) {
                        g.fill_rounded(&r, 4.0, &theme.control_fill_hover);
                    }
                    let color = if enabled { &theme.text_primary } else { &theme.text_tertiary };
                    g.vector_icon(item.icon(), &r, 16.0, color);
                }
            }
        }
        e.raise(self, "OnPaint");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (x, y) = (e.args().x, e.args().y);
        let hot = self.item_at(x, y);
        if hot != self.hot {
            self.hot = hot;
            self.invalidate();
        }
        e.raise(self, "OnMouseMove");
    }

    fn on_mouse_leave(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        self.hot = None;
        self.pressed = None;
        self.invalidate();
        e.raise(self, "OnMouseLeave");
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        self.pressed = self.item_at(e.args().x, e.args().y);
        if self.pressed == Some(NavigatorItem::Position) && self.state.count > 0 {
            self.typed = Some(String::new());
        } else if self.typed.is_some() {
            self.typed = None;
        }
        self.invalidate();
        e.raise(self, "OnMouseDown");
    }

    fn on_mouse_click(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let item = self.item_at(e.args().x, e.args().y);
        let pressed = self.pressed.take();
        if let Some(item) = item.filter(|i| Some(*i) == pressed && *i != NavigatorItem::Position) {
            self.perform(item);
        }
        e.raise(self, "OnMouseClick");
    }

    fn on_key_press(&mut self, e: &mut EventCx<'_, KeyPressEventArgs>) {
        let c = e.args().key_char;
        if c.is_ascii_digit() && self.state.count > 0 {
            let typed = self.typed.get_or_insert_with(String::new);
            if typed.len() < 9 {
                typed.push(c);
            }
            e.args_mut().handled = true;
            self.invalidate();
        }
        e.raise(self, "OnKeyPress");
    }

    fn on_key_down(&mut self, e: &mut EventCx<'_, KeyEventArgs>) {
        const ENTER: u16 = 0x0D;
        const ESCAPE: u16 = 0x1B;
        const BACK: u16 = 0x08;
        if self.typed.is_some() {
            match e.args().key.vk() {
                ENTER => {
                    self.perform(NavigatorItem::Position);
                    e.args_mut().handled = true;
                }
                ESCAPE => {
                    self.typed = None;
                    e.args_mut().handled = true;
                    self.invalidate();
                }
                BACK => {
                    if let Some(t) = self.typed.as_mut() {
                        t.pop();
                    }
                    e.args_mut().handled = true;
                    self.invalidate();
                }
                _ => {}
            }
        }
        e.raise(self, "OnKeyDown");
    }

    fn on_lost_focus(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        if self.typed.take().is_some() {
            self.invalidate();
        }
        e.raise(self, "OnLostFocus");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_enabled_where_they_make_sense() {
        let s = NavigatorState { position: 0, count: 3, can_add: true, can_delete: true, has_changes: false, busy: false };
        assert!(!s.enabled(NavigatorItem::MoveFirst) && !s.enabled(NavigatorItem::MovePrevious));
        assert!(s.enabled(NavigatorItem::MoveNext) && s.enabled(NavigatorItem::MoveLast));
        assert!(!s.enabled(NavigatorItem::Save));
        let end = NavigatorState { position: 2, has_changes: true, ..s };
        assert!(end.enabled(NavigatorItem::MovePrevious) && !end.enabled(NavigatorItem::MoveNext) && end.enabled(NavigatorItem::Save));
        let busy = NavigatorState { busy: true, ..end };
        assert!(!busy.enabled(NavigatorItem::Save), "nothing while a fill or a save runs");
        let empty = NavigatorState::default();
        assert!(!empty.enabled(NavigatorItem::Delete) && !empty.enabled(NavigatorItem::Position));
    }

    #[test]
    fn layout_places_every_item_and_hit_tests_them() {
        let mut n = BindingNavigator::new("customers");
        let parts = n.layout(Rect::new(0.0, 0.0, 400.0, 32.0), 30.0);
        let items: Vec<NavigatorItem> = parts.iter().filter_map(|p| if let Part::Item(i, _) = p { Some(*i) } else { None }).collect();
        use NavigatorItem::*;
        assert_eq!(items, [MoveFirst, MovePrevious, Position, MoveNext, MoveLast, AddNew, Delete, Save]);
        n.show_save_item = false;
        n.show_add_item = false;
        let fewer = n.layout(Rect::new(0.0, 0.0, 400.0, 32.0), 30.0);
        assert!(!fewer.iter().any(|p| matches!(p, Part::Item(Save | AddNew, _))));
        n.base.bounds = Rect::new(100.0, 50.0, 500.0, 82.0);
        assert_eq!(n.item_at(4.0, 16.0), Some(MoveFirst));
        assert_eq!(n.item_at(34.0, 16.0), Some(MovePrevious));
        assert_eq!(n.item_at(0.5, 0.5), None);
    }
}
