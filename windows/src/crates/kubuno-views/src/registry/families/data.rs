//! Component family `data` — declared with the `component!` table (see
//! `../macros.rs`) plus, in this same file, the view nodes those components
//! build. Compiled only with the `family-data` feature while the families are
//! being written in parallel; the feature is on by default once integrated.
//!
//! ## Components
//!
//! [`ListBox`](kubuno_ui::lists::ListBox), [`CheckedListBox`](kubuno_ui::lists::CheckedListBox),
//! [`ListView`](kubuno_ui::views::ListView), [`TreeView`](kubuno_ui::views::TreeView),
//! [`DataTable`](kubuno_ui::tables::DataTable) and [`MonthCalendar`](kubuno_ui::datetime::MonthCalendar)
//! — the six `kubuno-ui` primitives that hold a *collection* rather than a
//! single value.
//!
//! ## `ItemsSource`
//!
//! `XML_VIEWS.md` §3 describes a bound list (`Items="{Binding Users}"`) the
//! same shape `admin_users.rs::table()` already builds. `crate::binding::
//! Value::List` (rows of named [`crate::binding::Row`] fields) is that list
//! shape; every component below reads its `ItemsSource="{Binding Path}"`
//! every frame (`resolve_item_labels`/`resolve_item_rows`) and, when it
//! resolves to a `Value::List`, uses it — a `ListBox`/`CheckedListBox`/
//! `ListView`(single-column)/`TreeView`(flat, see below) row from each row's
//! `"Text"` field; a `ListView`/`DataTable` cell from the field its own
//! `<Column Binding="{Binding Field}">` names. The **static child markup**
//! this family reads directly off the parsed XML remains fully supported —
//! used whenever `ItemsSource` names no binding, or the binding is unset or
//! not currently a list (e.g. before the view model has loaded anything):
//!
//! - `<Item Text=".."/>` — one row of a `<ListBox>`/`<CheckedListBox>`/
//!   `<ListView>`; nested under itself, the same element also describes one
//!   node of a `<TreeView>`'s tree (`<Item Text=".."><Item Text=".."/></Item>`)
//!   — a hierarchy `ItemsSource` cannot express (`Value::List` is flat; a
//!   bound `<TreeView>` is always one level deep, see [`resolve_item_labels`]'s
//!   callers).
//! - `<Column Header=".." Binding=".." Width=".."/>` — one column of a
//!   `<ListView>`/`<DataTable>`. `Binding`'s bare field name (`column_id`'s
//!   own `binding_field`) is now also what a bound row is read by.
//!
//! `Item`/`Column` are registered here as ordinary (inert) components purely
//! so `crate::validate` accepts them as children — they never reach
//! `crate::compile::build_node` in practice (a parent reads them off
//! `crate::ast::Element` directly, see [`static_item_labels`] and
//! [`static_columns`]), so their own `build` never runs outside a test.
//! `crate::validate` now DOES restrict which parent each is valid under
//! (`ChildrenModel::List(&["Item"])`/`&["Column"]`, per component below) —
//! `<Item>` inside, say, `<Tabs>` is a validate-time diagnostic, not silently
//! accepted the way it was before that check existed.
//!
//! ## `DataTable`: column formats, sort and in-place editing
//!
//! A `<DataTable>`'s `<Column>` also takes `FormatString`, `Culture`, `NullValue` (WinForms
//! `DefaultCellStyle.Format`/`FormatProvider`/`NullValue`, the formats of [`crate::format`];
//! the same parts inside its `Binding` work too, the attributes win, the table's own `Culture`
//! is the default), `Alignment` (`Left` by default, as WinForms) and `ReadOnly`. A bound list is
//! sorted by the grid itself on the RAW values (numbers numerically, blanks first), never on
//! the formatted text; `SelectedIndex`, the events and the write paths keep naming rows by their
//! index in the bound list.
//!
//! Editing (WinForms `DataGridView`, `EditMode = EditOnKeystrokeOrF2`; `kubuno_ui::tables`'
//! `edit` module has the key table) needs a focusable table (`x:Name`) and `ReadOnly="false"`
//! (the default). A committed cell raises `OnCellValidating` (cancelable: the editor stays),
//! is written back, then `OnCellValueChanged` (when the text changed) and `OnCellEndEdit`:
//!
//! - a `BindingSource` behind the `ItemsSource` path `P` (it answers `P.Position`): the grid
//!   writes `P.Position` = the row, then `P.Current.<field>` with the column's format (parsed
//!   back: `1 234,50` → 1234.5); its `Position` follows the grid's current row, so moving to
//!   another row ends the row edit and a refusal (`RowValidating`, a value that did not convert)
//!   keeps the grid on the row; `P.Current.<field>.Error` is shown as the cell's error glyph;
//!   Escape on a cell not being edited writes `P.CancelEdit` (the row edit is cancelled);
//! - any other bound list: `P[<row>].<field>` through `ViewModel::set_bound` (parsed per the
//!   column's format), for the view model to apply;
//! - static rows: the cell's text itself.
//!
//! ## Events, reusing `node::ViewEventKind`
//!
//! [`crate::node::ViewEventKind`] is a closed, three-variant enum
//! (`Clicked`/`Toggled`/`Changed`), and `PaintCx::fire`/`InteractCx::fire`
//! are `pub(crate)`, shared by every family. This file's own `fire_*`
//! helpers stay as per-shape convenience wrappers around `PaintCx::fire`
//! (each adds the binding write-back, or lack of one, that shape needs): a
//! selection change is `Changed(index or path, as text)`; a checked toggle
//! is `Toggled(new_state)`; a row/day activation (double-click, or a header
//! sort toggle) is `Clicked` — the closest existing shape in each case,
//! distinguished by the `On*` handler name the XML attribute carried,
//! exactly like every other component in this crate.
//!
//! ## Wheel scrolling and scroll chaining
//!
//! Every scrollable node below claims the frame's wheel travel with
//! `kubuno_controls::host::claim_wheel()` — but ONLY once it has checked
//! that its own scroll position actually changed (before/after the real
//! `scroll`/`scroll_by`/`scroll_rows` call), never merely because the wheel
//! moved. A control already at its own scroll limit (or with nothing to
//! scroll at all — three rows in a tall box) must leave the wheel unclaimed
//! so it falls through to whatever `<ScrollArea>` hosts it — standard
//! browser/OS "scroll chaining", and the fix for a real bug this family
//! shipped with: hovering a short `ListView` inside a scrolling page used to
//! swallow every wheel notch regardless, so nothing past it (a `<MonthCalendar>`
//! two rows down, say) could ever be reached. `MonthCalendar` is the one
//! exception, by design: its wheel changes the shown MONTH, not a scroll
//! offset with a content-height limit, so it always legitimately "uses" the
//! wheel and keeps claiming it unconditionally, the same way a `NumericField`
//! spin control would.

use crate::ast::Element;
use crate::binding::{BindingFormat, BindingMode, BindingSpec, PropSource, Row, Value, ViewModel};
use crate::node::{PaintCx, ViewEventKind, ViewNode};
use crate::events::{CellCancelEventArgs, CellEventArgs, CellValidatingEventArgs, ChangeSource, ItemActivateEventArgs, ItemCheckEventArgs, SelectionChangedEventArgs, TextChangedEventArgs};
use crate::registry::macros::component;
use crate::registry::ComponentMeta;

use kubuno_controls::datetime::Date;
use kubuno_controls::enums::CheckState;
use kubuno_controls::host;
use kubuno_controls::host::{Frame, InputEvent};
use kubuno_controls::lists::SelectionMode;
use kubuno_controls::views::TreeView as ReplicaTreeView;
use kubuno_ui::datetime::{HeaderPart, MonthCalendar};
use kubuno_ui::lists::{CheckedListBox, ListBox};
use kubuno_ui::tables::{CellAction, CellError, CellMove, DataTable, Layout, SortOrder};
use kubuno_ui::text::EditInput;
use kubuno_ui::views::{ColumnHeader, ListView, ListViewItem, NodePath, TreeNode, TreeView, View};
use kubuno_ui::{Canvas, FocusId, Rect, Size, Widget};

// ─────────────────────────────────────────────────────────────────────────
// Static child markup — read directly off the parsed XML (see the module
// doc's "ItemsSource gap" section), not through `Props::build_children`:
// `<Item>`/`<Column>` are metadata for the PARENT to read, not paintable
// widgets of their own.
// ─────────────────────────────────────────────────────────────────────────

/// The `<Item Text=".."/>` children of `element`, flattened — what a
/// `<ListBox>`/`<CheckedListBox>`/`<ListView>` reads for its rows. A nested
/// `<Item>` (meaningful only to [`static_tree_nodes`]) is simply not
/// descended into here.
/// `Sorted`: alphabetical, ignoring case (then by the exact text, for a stable order).
fn sort_labels(labels: &mut [String]) {
    labels.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
}

/// A list node's items, sorted when the element writes `Sorted="true"`.
fn list_items(element: &Element) -> (Vec<String>, bool) {
    let sorted = element.attribute("Sorted").and_then(|a| a.value()).is_some_and(|v| v.trim() == "true");
    let mut items = static_item_labels(element);
    if sorted {
        sort_labels(&mut items);
    }
    (items, sorted)
}

fn static_item_labels(element: &Element) -> Vec<String> {
    element
        .children()
        .filter(|c| c.name().as_deref() == Some("Item"))
        .map(|c| c.attribute("Text").and_then(|a| a.value()).unwrap_or_default())
        .collect()
}

/// The `<Item Text=".." <FieldName>=".."/>` children of `element`, one row
/// per `<Item>`, one value per `field_names` entry, in column order — what a
/// `<ListView>` with declared `<Column Binding="{Binding Field}">` children
/// reads for a static (non-`ItemsSource`) row's per-column cells:
/// `<Item Text="Alice" Role="Admin"/>` against columns bound to `Name`
/// (index 0, read from the conventional `Text` attribute — the same one
/// `static_item_labels`/every other list control's `<Item>` already uses)
/// and `Role` (index ≥ 1, read from the attribute named exactly like the
/// column's own bare binding field). `field_names` empty (no `<Column>`
/// declared at all) falls back to a single implicit `Text` field, so a
/// `<ListView>` with no columns still gets its rows.
fn static_item_rows(element: &Element, field_names: &[String]) -> Vec<Vec<String>> {
    let implicit = ["Text".to_string()];
    let names: &[String] = if field_names.is_empty() { &implicit } else { field_names };
    element
        .children()
        .filter(|c| c.name().as_deref() == Some("Item"))
        .map(|c| {
            names
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let attr = if i == 0 { "Text" } else { field.as_str() };
                    c.attribute(attr).and_then(|a| a.value()).unwrap_or_default()
                })
                .collect()
        })
        .collect()
}

/// The `<Item Text="..">…</Item>` children of `element`, recursively — what
/// a `<TreeView>` reads for its node tree. A node with children starts
/// expanded, so a static tree is visible without any extra markup.
fn static_tree_nodes(element: &Element) -> Vec<TreeNode> {
    element
        .children()
        .filter(|c| c.name().as_deref() == Some("Item"))
        .map(|c| {
            let text = c.attribute("Text").and_then(|a| a.value()).unwrap_or_default();
            let mut node = TreeNode::new(text);
            node.children = static_tree_nodes(&c);
            node.expanded = !node.children.is_empty();
            node
        })
        .collect()
}

/// The `Height` of the `<Item>`s of a `<TreeView>` that set one, by node path (a row of its own
/// height; the others are `ItemHeight` high).
fn static_tree_heights(element: &Element, prefix: &[usize], out: &mut std::collections::HashMap<NodePath, f32>) {
    for (i, c) in element.children().filter(|c| c.name().as_deref() == Some("Item")).enumerate() {
        let mut path = prefix.to_vec();
        path.push(i);
        if let Some(h) = c.attribute("Height").and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok()).filter(|h| *h > 0.0) {
            out.insert(path.clone(), h);
        }
        static_tree_heights(&c, &path, out);
    }
}

/// The `<Column Header=".." Binding=".." Width=".."/>` children of
/// `element`, as raw `(header, binding, width)` triples — what a
/// `<ListView>`/`<DataTable>` turns into its own `ColumnHeader`s. `width`
/// falls back to a plain 160 DIP column, the same fallback
/// `kubuno_controls::views::ColumnHeader::default` uses for an unspecified
/// width.
fn static_columns(element: &Element) -> Vec<(String, String, i32)> {
    element
        .children()
        .filter(|c| c.name().as_deref() == Some("Column"))
        .map(|c| {
            let header = c.attribute("Header").and_then(|a| a.value()).unwrap_or_default();
            let binding = c.attribute("Binding").and_then(|a| a.value()).unwrap_or_default();
            let width = c
                .attribute("Width")
                .and_then(|a| a.value())
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(160);
            (header, binding, width)
        })
        .collect()
}

/// A stable column id (`ColumnHeader::name`, the replica's own identity key
/// — see `kubuno_ui::tables::column`) derived from a `Binding` expression
/// (`{Binding Name}` → `"Name"`) when there is one, else from the header
/// text, else a positional fallback so two same-titled columns still get
/// distinct identities (`DataTable::toggle_sort` keys the sort state on this).
/// The bare field name a `<Column Binding="{Binding Name}">` names, if any —
/// `"{Binding Name, Mode=TwoWay}"` → `"Name"`. `None` for a column with no
/// `Binding` (a static-label-only column), or a malformed one. Shared by
/// [`column_id`] (the id falls back to the header when there is none) and,
/// with the row-reading side of `ItemsSource`, by [`resolve_item_rows`]'s
/// callers directly, which need the bare field name — not `column_id`'s own
/// header/positional fallback — to read the matching [`crate::binding::Row`]
/// field.
fn binding_field(binding: &str) -> Option<&str> {
    binding
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .map(str::trim)
        .and_then(|s| s.strip_prefix("Binding"))
        .map(|s| s.split(',').next().unwrap_or(s).trim())
        .filter(|s| !s.is_empty())
}

fn column_id(binding: &str, header: &str, index: usize) -> String {
    let base = binding_field(binding).unwrap_or(header);
    if base.is_empty() {
        format!("col{index}")
    } else {
        base.to_string()
    }
}

/// `SelectionMode="One|None|MultiSimple|MultiExtended"` → the replica enum.
/// Unrecognised text (should not happen past `crate::validate`'s own enum
/// check) falls back to `One`, the replica's own default.
fn parse_selection_mode(s: &str) -> SelectionMode {
    match s {
        "None" => SelectionMode::None,
        "MultiSimple" => SelectionMode::MultiSimple,
        "MultiExtended" => SelectionMode::MultiExtended,
        _ => SelectionMode::One,
    }
}

/// `YYYY-MM-DD` → [`Date`]. `None` for anything else, including a partial or
/// malformed string — the caller simply leaves the calendar's own selection
/// alone in that case (see `MonthCalendarNode::paint`).
fn parse_iso_date(s: &str) -> Option<Date> {
    let mut parts = s.splitn(3, '-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Date::new(year, month, day))
}

/// The inverse of [`parse_iso_date`].
fn format_iso_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

/// A [`NodePath`] as the dot-joined text a `SelectedPath` binding carries
/// (`"0.2.1"`) — `NodePath` itself has no scalar `binding::Value` shape (see
/// the module doc), so this is the family's own stand-in encoding.
fn path_to_string(path: &NodePath) -> String {
    path.iter().map(usize::to_string).collect::<Vec<_>>().join(".")
}

/// The inverse of [`path_to_string`]. `None` for an empty or malformed text.
fn parse_path(s: &str) -> Option<NodePath> {
    if s.trim().is_empty() {
        return None;
    }
    s.split('.').map(|p| p.trim().parse::<usize>().ok()).collect()
}

// ─────────────────────────────────────────────────────────────────────────
// Event helpers — this family's per-shape convenience wrappers around the
// now-shared `crate::node::PaintCx::fire` (`pub(crate)`): each one adds the
// binding write-back (or lack of one) and the [`Value`]/[`ViewEventKind`]
// shape a data control's own event needs, then dispatches through the one
// implementation every family now shares instead of re-deriving "look up
// the handler, run it, always record the event".
// ─────────────────────────────────────────────────────────────────────────

/// A `SelectedIndex`-shaped change: writes the two-way binding back (when
/// there is one), dispatches the named handler with the new index, and
/// records a [`ViewEventKind::Changed`].
fn fire_index_changed(
    cx: &mut PaintCx<'_>,
    event: &'static str,
    focus_id: Option<FocusId>,
    handler: Option<&str>,
    binding: Option<&BindingSpec>,
    index: i32,
) {
    if let Some(spec) = binding {
        if spec.mode.writes_back() {
            spec.update_source(cx.vm, Value::F32(index as f32));
        }
    }
    let mut args = SelectionChangedEventArgs::new(None, usize::try_from(index).ok(), ChangeSource::User);
    cx.fire(event, focus_id, handler, ViewEventKind::Changed(index.to_string()), &mut args);
}

/// The text-valued twin of [`fire_index_changed`] — `SelectedPath` (a
/// [`NodePath`], see [`path_to_string`]), `SelectedDate` (an ISO date, see
/// [`format_iso_date`]), or a sort-state summary with no binding at all.
fn fire_string_changed(
    cx: &mut PaintCx<'_>,
    event: &'static str,
    focus_id: Option<FocusId>,
    handler: Option<&str>,
    binding: Option<&BindingSpec>,
    text: &str,
) {
    if let Some(spec) = binding {
        if spec.mode.writes_back() {
            spec.update_source(cx.vm, Value::Str(text.to_string()));
        }
    }
    let mut args = TextChangedEventArgs::new(String::new(), text.to_string(), ChangeSource::User);
    cx.fire(event, focus_id, handler, ViewEventKind::Changed(text.to_string()), &mut args);
}

/// A per-row check toggle (`CheckedListBox`): no binding (a whole-list check
/// set has no scalar `binding::Value` shape either), just the handler and a
/// [`ViewEventKind::Toggled`] carrying the row's new state.
fn fire_toggled(
    cx: &mut PaintCx<'_>,
    event: &'static str,
    focus_id: Option<FocusId>,
    handler: Option<&str>,
    index: usize,
    new_state: bool,
) {
    cx.fire(event, focus_id, handler, ViewEventKind::Toggled(new_state), &mut ItemCheckEventArgs { index, checked: new_state });
}

/// An activation (Enter / double-click on a row, a day, a node): the closest
/// existing [`ViewEventKind`] shape is `Clicked` — see the module doc.
fn fire_clicked(cx: &mut PaintCx<'_>, event: &'static str, focus_id: Option<FocusId>, handler: Option<&str>, item: Value) {
    cx.fire(event, focus_id, handler, ViewEventKind::Clicked, &mut ItemActivateEventArgs { item });
}

// ─────────────────────────────────────────────────────────────────────────
// `ItemsSource` — reading a bound row list every frame (`crate::binding::
// Value::List`/`Row`), the piece this file's own module doc used to call
// "the ItemsSource gap": a component below still declares the attribute as
// `PropKind::String` so `{Binding Path}` parses and validates (unchanged),
// and its `build` closure now also captures the parsed binding
// (`props.str("ItemsSource", "")?.binding().cloned()`) for its node to
// re-resolve every frame here, alongside — not instead of — the static
// `<Item>`/`<Column>` children this file already reads directly off the XML.
// A node with no `ItemsSource` binding, or one that resolves to anything
// other than `Value::List` (unset, wrong shape), simply keeps whatever the
// static children built at compile time; a bound list, once it resolves,
// takes over.
// ─────────────────────────────────────────────────────────────────────────

/// The flat, single-field shape `ListBox`/`CheckedListBox`/`ListView`
/// (single-column)/`TreeView` (flat — see the module doc's own hedge on tree
/// binding) read a row as: its `"Text"` field, rendered as display text
/// ([`crate::binding::Row::text`]). `None` when `items_source` is unbound or
/// does not currently resolve to a list — the caller's cue to leave its
/// existing (static-children-built) items exactly alone.
#[cfg(test)]
fn resolve_item_labels(vm: &dyn ViewModel, items_source: &Option<BindingSpec>) -> Option<Vec<String>> {
    let spec = items_source.as_ref()?;
    match vm.get(&spec.path) {
        Some(Value::List(rows)) => Some(labels_of(&rows)),
        _ => None,
    }
}

/// The multi-column shape `ListView`/`DataTable` read a row as: one text
/// value per declared `<Column Binding="{Binding Field}">`, in column order
/// — `field_names` is each column's bare field name (`column_id`'s own
/// `from_binding`, already derived once at build time; see each of those
/// components' `build`). `None` under the same conditions as
/// [`resolve_item_labels`].
#[cfg(test)]
fn resolve_item_rows(vm: &dyn ViewModel, items_source: &Option<BindingSpec>, field_names: &[String]) -> Option<Vec<Vec<String>>> {
    let spec = items_source.as_ref()?;
    match vm.get(&spec.path) {
        Some(Value::List(rows)) => Some(cells_of(&rows, field_names)),
        _ => None,
    }
}

/// Each row's `Text` field: the labels of a single-field list.
fn labels_of(rows: &[crate::binding::Row]) -> Vec<String> {
    rows.iter().map(|r| r.text("Text")).collect()
}

/// Each row's `field_names` fields, in column order: the cells of a multi-column list.
fn cells_of(rows: &[crate::binding::Row], field_names: &[String]) -> Vec<Vec<String>> {
    rows.iter().map(|r| field_names.iter().map(|f| r.text(f)).collect()).collect()
}

/// The shared "claim the wheel only if this actually scrolled" check every
/// nested scrollable below uses — see the module doc's "Wheel scrolling and
/// scroll chaining". `apply` receives the wheel's vertical DIP travel,
/// performs the real (already-clamped) scroll, and reports whether the
/// position actually changed; `host::claim_wheel()` runs exactly when it
/// did. Does nothing when the wheel already went to something else this
/// frame, the pointer is not over `bounds`, or there was no wheel travel —
/// `apply` is not even called in those cases, so a caller's closure never
/// has to guard against being asked to "scroll" a stationary wheel.
///
/// Takes no `Canvas`/`ViewModel`, only `&Frame`, so this — the actual
/// scroll-or-not decision — is unit-testable without the live `Canvas`
/// nothing in this crate's own test suite constructs either (see `node.rs`'s
/// module doc on why); `data::tests` below exercises it directly.
fn claim_wheel_if_scrolled(frame: &Frame, bounds: Rect, mut apply: impl FnMut(f32) -> bool) {
    let (mx, my) = frame.mouse;
    let (_, wy) = frame.wheel_dip();
    if wy != 0.0 && !host::wheel_claimed() && bounds.contains(mx, my) && apply(wy) {
        host::claim_wheel();
    }
}

// ─────────────────────────────────────────────────────────────────────────
// InertNode — the placeholder `<Item>`/`<Column>` build to (see the module
// doc): never actually reached outside a test, since their parent reads the
// raw XML instead of calling `Props::build_children` on them.
// ─────────────────────────────────────────────────────────────────────────

struct InertNode;

impl ViewNode for InertNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size::EMPTY
    }

    fn paint(&mut self, _cx: &mut PaintCx<'_>, _bounds: Rect) {}
}

// ─────────────────────────────────────────────────────────────────────────
// ListBox
// ─────────────────────────────────────────────────────────────────────────

struct ListBoxNode {
    items_source: Option<BindingSpec>,
    /// `ItemHeight` (0: the standard row).
    item_height: PropSource<f32>,
    /// The bound rows last shown (change detection of `ItemsSource`).
    watch: crate::binding::ListWatch,
    /// `Sorted`: the items are shown in alphabetical order.
    sorted: bool,
    selection_mode: PropSource<String>,
    selected_index: PropSource<f32>,
    focus_id: Option<FocusId>,
    on_selection_changed: Option<String>,
    /// `DrawMode` and the owner-draw handlers (EVT-8).
    pub(crate) draw_mode: PropSource<String>,
    pub(crate) owner: crate::owner_draw::OwnerDrawEvents,
    widget: ListBox,
    /// Whether the mouse was already down the last time this node acted —
    /// so a held button selects once per press, on the down edge, the way a
    /// native list box does (not on release, unlike a push button: see
    /// `node::press_release`'s own doc for why a button is different).
    pressed: bool,
}

impl ListBoxNode {
    fn new(
        items: Vec<String>,
        items_source: Option<BindingSpec>,
        selection_mode: PropSource<String>,
        selected_index: PropSource<f32>,
        focus_id: Option<FocusId>,
        on_selection_changed: Option<String>,
    ) -> Self {
        let mut widget = ListBox::new();
        widget.items = items;
        Self {
            items_source,
            item_height: PropSource::Literal(0.0),
            watch: Default::default(),
            sorted: false,
            selection_mode,
            selected_index,
            focus_id,
            on_selection_changed,
            draw_mode: PropSource::Literal("Normal".to_string()),
            owner: Default::default(),
            widget,
            pressed: false,
        }
    }
}

impl ViewNode for ListBoxNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        if let Some(mut labels) = self.watch.changed(cx.vm, self.items_source.as_ref()).map(|rows| labels_of(&rows)) {
            if self.sorted {
                sort_labels(&mut labels);
            }
            if self.widget.items != labels {
                self.widget.items = labels;
            }
        }
        let mode = parse_selection_mode(&self.selection_mode.resolve(cx.vm));
        if self.widget.selection_mode != mode {
            self.widget.set_selection_mode(mode);
        }
        let item_height = self.item_height.resolve(cx.vm).max(0.0).round() as i32;
        if self.widget.item_height != item_height {
            self.widget.item_height = item_height;
        }
        let wanted = self.selected_index.resolve(cx.vm);
        let wanted_idx = if wanted >= 0.0 { wanted.round() as i32 } else { -1 };
        if self.widget.selected_index() != wanted_idx {
            self.widget.set_selected_index(wanted_idx);
        }

        // Owner-draw (EVT-8): the draw mode, and the item heights of an OwnerDrawVariable list
        // (MeasureItem) before anything hit-tests them.
        let mode = crate::owner_draw::parse_draw_mode(&self.draw_mode.resolve(cx.vm));
        if self.widget.draw_mode != mode {
            self.widget.draw_mode = mode;
        }
        if mode == kubuno_ui::graphics::DrawMode::OwnerDrawVariable {
            let widget = &mut self.widget;
            crate::owner_draw::paint_with(cx, &self.owner, |c| widget.measure_items(c, bounds));
        } else {
            self.widget.item_heights = None;
        }
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();
        let (mx, my) = cx.frame.mouse;
        let hot_now = !cx.frame.pointer_outside() && bounds.contains(mx, my);
        self.widget.hot_index = if hot_now { self.widget.item_at(bounds, mx, my) } else { None };

        if hot_now && cx.frame.mouse_down && !self.pressed {
            self.pressed = true;
            if let Some(i) = self.widget.item_at(bounds, mx, my) {
                self.widget.pointer_select(i, cx.frame.mods.ctrl, cx.frame.mods.shift);
                fire_index_changed(
                    cx,
                    "OnSelectionChanged",
                    self.focus_id,
                    self.on_selection_changed.as_deref(),
                    self.selected_index.binding(),
                    self.widget.selected_index(),
                );
            }
        }
        if !cx.frame.mouse_down {
            self.pressed = false;
        }

        claim_wheel_if_scrolled(cx.frame, bounds, |wy| {
            let rh = self.widget.row_height();
            let rows = if rh > 0.0 { (wy / rh).round() as i32 } else { 0 };
            if rows == 0 {
                return false;
            }
            let before = self.widget.top_index;
            self.widget.scroll_rows(bounds, rows);
            self.widget.top_index != before
        });

        let state = focus_state.apply(crate::common::rest().hot(hot_now));
        let widget = &self.widget;
        crate::owner_draw::paint_with(cx, &self.owner, |c| widget.paint(c, bounds, state));
    }
}

// ─────────────────────────────────────────────────────────────────────────
// CheckedListBox
// ─────────────────────────────────────────────────────────────────────────

struct CheckedListBoxNode {
    items_source: Option<BindingSpec>,
    /// The bound rows last shown.
    watch: crate::binding::ListWatch,
    /// `Sorted`.
    sorted: bool,
    /// The item labels currently in [`Self::widget`] — kept alongside it
    /// (rather than re-derived from `widget.items`, which the composed
    /// replica does not expose directly the way `ListBox::items` does) so a
    /// bound `ItemsSource`'s new resolution can be compared against what is
    /// already shown, and the widget only rebuilt (see [`Self::paint`]) when
    /// it actually changed.
    items: Vec<String>,
    selected_index: PropSource<f32>,
    check_on_click: PropSource<bool>,
    focus_id: Option<FocusId>,
    on_selection_changed: Option<String>,
    on_checked_changed: Option<String>,
    widget: CheckedListBox,
    pressed: bool,
}

impl CheckedListBoxNode {
    #[allow(clippy::too_many_arguments)]
    fn new(
        items: Vec<String>,
        items_source: Option<BindingSpec>,
        selected_index: PropSource<f32>,
        check_on_click: PropSource<bool>,
        focus_id: Option<FocusId>,
        on_selection_changed: Option<String>,
        on_checked_changed: Option<String>,
    ) -> Self {
        let mut widget = CheckedListBox::new();
        for item in &items {
            widget.add_item(item.clone(), CheckState::Unchecked);
        }
        Self { items_source, watch: Default::default(), sorted: false, items, selected_index, check_on_click, focus_id, on_selection_changed, on_checked_changed, widget, pressed: false }
    }
}

impl ViewNode for CheckedListBoxNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        if let Some(mut labels) = self.watch.changed(cx.vm, self.items_source.as_ref()).map(|rows| labels_of(&rows)) {
            if self.sorted {
                sort_labels(&mut labels);
            }
            if self.items != labels {
                // No in-place `items` setter on the composed replica (unlike
                // `ListBox::items`) — rebuilt wholesale. Every row starts
                // unchecked, same as the widget's own initial construction;
                // a bound row's check state is not modelled (see this
                // component's `ItemsSource` doc).
                let mut widget = CheckedListBox::new();
                widget.check_on_click = self.widget.check_on_click;
                for item in &labels {
                    widget.add_item(item.clone(), CheckState::Unchecked);
                }
                self.widget = widget;
                self.items = labels;
            }
        }
        let want_check_on_click = self.check_on_click.resolve(cx.vm);
        if self.widget.check_on_click != want_check_on_click {
            self.widget.check_on_click = want_check_on_click;
        }
        let wanted = self.selected_index.resolve(cx.vm);
        let wanted_idx = if wanted >= 0.0 { wanted.round() as i32 } else { -1 };
        if self.widget.selected_index() != wanted_idx {
            self.widget.set_selected_index(wanted_idx);
        }

        let canvas: &dyn Canvas = cx.canvas;
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();
        let (mx, my) = cx.frame.mouse;
        let hot_now = !cx.frame.pointer_outside() && bounds.contains(mx, my);
        self.widget.hot_index = if hot_now { self.widget.item_at(bounds, mx, my) } else { None };

        if hot_now && cx.frame.mouse_down && !self.pressed {
            self.pressed = true;
            if let Some(i) = self.widget.item_at(bounds, mx, my) {
                let on_well = self.widget.check_at(bounds, mx, my) == Some(i);
                let before = self.widget.get_item_checked(i);
                self.widget.pointer_select(i, on_well);
                fire_index_changed(
                    cx,
                    "OnSelectionChanged",
                    self.focus_id,
                    self.on_selection_changed.as_deref(),
                    self.selected_index.binding(),
                    self.widget.selected_index(),
                );
                let after = self.widget.get_item_checked(i);
                if after != before {
                    fire_toggled(cx, "OnCheckedChanged", self.focus_id, self.on_checked_changed.as_deref(), i, after);
                }
            }
        }
        if !cx.frame.mouse_down {
            self.pressed = false;
        }

        claim_wheel_if_scrolled(cx.frame, bounds, |wy| {
            let rh = self.widget.row_height();
            let rows = if rh > 0.0 { (wy / rh).round() as i32 } else { 0 };
            if rows == 0 {
                return false;
            }
            let before = self.widget.list.top_index;
            self.widget.scroll_rows(bounds, rows);
            self.widget.list.top_index != before
        });

        let state = focus_state.apply(crate::common::rest().hot(hot_now));
        self.widget.paint(canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// ListView
// ─────────────────────────────────────────────────────────────────────────

/// One [`ListViewItem`] per row: `values[0]` is the primary column, every
/// further value a `with_sub` secondary column, in column order — the exact
/// shape `XML_VIEWS.md` §3's worked example describes
/// (`ListViewItem::new(…).with_sub(…)`).
fn build_list_view_item(values: &[String]) -> ListViewItem {
    let mut it = ListViewItem::new(values.first().cloned().unwrap_or_default());
    for v in values.iter().skip(1) {
        it = it.with_sub(v.clone());
    }
    it
}

struct ListViewNode {
    items_source: Option<BindingSpec>,
    /// The bound rows last shown.
    watch: crate::binding::ListWatch,
    /// This component's own `<Column Binding="..">` field names, in column
    /// order — read from a bound row every frame (see [`resolve_item_rows`]);
    /// empty for a column with no `Binding`.
    field_names: Vec<String>,
    multi_select: PropSource<bool>,
    selected_index: PropSource<f32>,
    focus_id: Option<FocusId>,
    on_selection_changed: Option<String>,
    on_activate: Option<String>,
    /// `OwnerDraw` and the owner-draw handlers (EVT-8).
    pub(crate) owner_draw: PropSource<bool>,
    pub(crate) owner: crate::owner_draw::OwnerDrawEvents,
    widget: ListView,
    pressed: bool,
}

impl ListViewNode {
    #[allow(clippy::too_many_arguments)]
    fn new(
        items: Vec<Vec<String>>,
        items_source: Option<BindingSpec>,
        columns: Vec<ColumnHeader>,
        field_names: Vec<String>,
        multi_select: PropSource<bool>,
        selected_index: PropSource<f32>,
        focus_id: Option<FocusId>,
        on_selection_changed: Option<String>,
        on_activate: Option<String>,
    ) -> Self {
        let mut widget = ListView::new();
        widget.view = View::Details;
        widget.items = items.iter().map(|r| build_list_view_item(r)).collect();
        widget.columns = columns;
        Self {
            items_source,
            field_names,
            watch: Default::default(),
            multi_select,
            selected_index,
            focus_id,
            on_selection_changed,
            on_activate,
            owner_draw: PropSource::Literal(false),
            owner: Default::default(),
            widget,
            pressed: false,
        }
    }
}

impl ViewNode for ListViewNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        if !self.field_names.is_empty() {
            if let Some(rows) = self.watch.changed(cx.vm, self.items_source.as_ref()).map(|rows| cells_of(&rows, &self.field_names)) {
                let items: Vec<ListViewItem> = rows.iter().map(|r| build_list_view_item(r)).collect();
                if self.widget.items != items {
                    self.widget.items = items;
                }
            }
        } else if let Some(labels) = self.watch.changed(cx.vm, self.items_source.as_ref()).map(|rows| labels_of(&rows)) {
            let items: Vec<ListViewItem> = labels.into_iter().map(ListViewItem::new).collect();
            if self.widget.items != items {
                self.widget.items = items;
            }
        }
        let want_multi = self.multi_select.resolve(cx.vm);
        if self.widget.multi_select != want_multi {
            self.widget.multi_select = want_multi;
        }
        let wanted = self.selected_index.resolve(cx.vm);
        let wanted_idx = wanted.round() as i32;
        let cur = self.widget.selected_indices().first().map(|&i| i as i32).unwrap_or(-1);
        if wanted_idx >= 0 && wanted_idx != cur {
            self.widget.click(wanted_idx as usize, false, false);
        }

        let canvas: &dyn Canvas = cx.canvas;
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();
        let (mx, my) = cx.frame.mouse;
        let hot_now = !cx.frame.pointer_outside() && bounds.contains(mx, my);
        self.widget.hot_index = if hot_now { self.widget.row_at(bounds, mx, my) } else { None };

        if hot_now && cx.frame.mouse_down && !self.pressed {
            self.pressed = true;
            if let Some(i) = self.widget.row_at(bounds, mx, my) {
                self.widget.click_mods(i, cx.frame.mods);
                let idx = self.widget.selected_indices().first().copied().unwrap_or(i) as i32;
                fire_index_changed(cx, "OnSelectionChanged", self.focus_id, self.on_selection_changed.as_deref(), self.selected_index.binding(), idx);
                if cx.frame.click_count >= 2 {
                    fire_clicked(cx, "OnItemActivate", self.focus_id, self.on_activate.as_deref(), Value::F32(i as f32));
                }
            }
        }
        if !cx.frame.mouse_down {
            self.pressed = false;
        }

        claim_wheel_if_scrolled(cx.frame, bounds, |wy| {
            // `ListView::scroll_by` already clamps, so comparing before/after
            // is enough to know whether it actually moved.
            let before = self.widget.scroll;
            self.widget.scroll_by(bounds, wy);
            self.widget.scroll != before
        });

        let state = focus_state.apply(crate::common::rest().hot(hot_now));
        let owner_draw = self.owner_draw.resolve(cx.vm);
        if self.widget.owner_draw != owner_draw {
            self.widget.owner_draw = owner_draw;
        }
        let _ = canvas;
        let widget = &self.widget;
        crate::owner_draw::paint_with(cx, &self.owner, |c| widget.paint(c, bounds, state));
    }
}

// ─────────────────────────────────────────────────────────────────────────
// TreeView
// ─────────────────────────────────────────────────────────────────────────

struct TreeViewNode {
    items_source: Option<BindingSpec>,
    /// `ItemHeight` (0: the standard row).
    item_height: PropSource<f32>,
    /// The bound rows last shown.
    watch: crate::binding::ListWatch,
    multi_select: PropSource<bool>,
    selected_path: PropSource<String>,
    focus_id: Option<FocusId>,
    on_selection_changed: Option<String>,
    on_activate: Option<String>,
    /// `DrawMode` (`Normal`, `OwnerDrawText`, `OwnerDrawAll`) and the owner-draw handlers (EVT-8).
    pub(crate) draw_mode: PropSource<String>,
    pub(crate) owner: crate::owner_draw::OwnerDrawEvents,
    widget: TreeView,
    pressed: bool,
}

impl TreeViewNode {
    #[allow(clippy::too_many_arguments)]
    fn new(
        nodes: Vec<TreeNode>,
        items_source: Option<BindingSpec>,
        multi_select: PropSource<bool>,
        selected_path: PropSource<String>,
        focus_id: Option<FocusId>,
        on_selection_changed: Option<String>,
        on_activate: Option<String>,
    ) -> Self {
        let mut widget = TreeView::new();
        widget.nodes = nodes;
        Self {
            items_source,
            item_height: PropSource::Literal(0.0),
            watch: Default::default(),
            multi_select,
            selected_path,
            focus_id,
            on_selection_changed,
            on_activate,
            draw_mode: PropSource::Literal("Normal".to_string()),
            owner: Default::default(),
            widget,
            pressed: false,
        }
    }
}

impl ViewNode for TreeViewNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        if let Some(rows) = self.watch.changed(cx.vm, self.items_source.as_ref()) {
            let nodes: Vec<TreeNode> = labels_of(&rows).into_iter().map(TreeNode::new).collect();
            if self.widget.nodes != nodes {
                self.widget.nodes = nodes;
            }
            // A row's `Height` field: that row's own height.
            self.widget.node_heights = rows
                .iter()
                .enumerate()
                .filter_map(|(i, r)| match r.get("Height") {
                    Some(Value::F32(h)) if *h > 0.0 => Some((vec![i], *h)),
                    Some(Value::Str(s)) => s.trim().parse::<f32>().ok().filter(|h| *h > 0.0).map(|h| (vec![i], h)),
                    _ => None,
                })
                .collect();
        }
        let item_height = self.item_height.resolve(cx.vm).round() as i32;
        if item_height > 0 && self.widget.item_height != item_height {
            self.widget.item_height = item_height;
        }
        let want_multi = self.multi_select.resolve(cx.vm);
        if self.widget.multi_select != want_multi {
            self.widget.multi_select = want_multi;
        }
        let wanted = self.selected_path.resolve(cx.vm);
        if let Some(path) = parse_path(&wanted) {
            if self.widget.selected_path.as_ref() != Some(&path) {
                self.widget.selected_path = Some(path);
            }
        }

        let canvas: &dyn Canvas = cx.canvas;
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();
        let (mx, my) = cx.frame.mouse;
        let hot_now = !cx.frame.pointer_outside() && bounds.contains(mx, my);
        self.widget.hot_row = if hot_now { self.widget.node_at(bounds, mx, my) } else { None };

        if hot_now && cx.frame.mouse_down && !self.pressed {
            self.pressed = true;
            if let Some(row) = self.widget.node_at(bounds, mx, my) {
                if self.widget.chevron_hit(bounds, mx, my) == Some(row) {
                    if let Some(path) = self.widget.path_at(row) {
                        if let Some(node) = ReplicaTreeView::node_at_mut(&mut self.widget.nodes, &path) {
                            node.expanded = !node.expanded;
                        }
                    }
                } else if let Some(path) = self.widget.path_at(row) {
                    self.widget.selected_path = Some(path.clone());
                    let text = path_to_string(&path);
                    fire_string_changed(cx, "OnSelectionChanged", self.focus_id, self.on_selection_changed.as_deref(), self.selected_path.binding(), &text);
                    if cx.frame.click_count >= 2 {
                        fire_clicked(cx, "OnItemActivate", self.focus_id, self.on_activate.as_deref(), Value::Str(text));
                    }
                }
            }
        }
        if !cx.frame.mouse_down {
            self.pressed = false;
        }

        claim_wheel_if_scrolled(cx.frame, bounds, |wy| {
            // `TreeView` has no `clamp_scroll`/`max_scroll` of its own (unlike
            // `ListView`) — derived here from its own public `rows()`/
            // `row_height()` so the upper bound is real content height, not
            // "grows forever downward" (without it, every downward notch
            // would count as "it moved" even once every row is on screen).
            let content_h = self.widget.rows().len() as f32 * self.widget.row_height();
            let max_scroll = (content_h - (bounds.bottom - bounds.top)).max(0.0);
            let before = self.widget.scroll;
            let after = (self.widget.scroll + wy).clamp(0.0, max_scroll);
            self.widget.scroll = after;
            after != before
        });

        let state = focus_state.apply(crate::common::rest().hot(hot_now));
        let mode = match self.draw_mode.resolve(cx.vm).as_str() {
            "OwnerDrawText" => kubuno_controls::views::TreeViewDrawMode::OwnerDrawText,
            "OwnerDrawAll" => kubuno_controls::views::TreeViewDrawMode::OwnerDrawAll,
            _ => kubuno_controls::views::TreeViewDrawMode::Normal,
        };
        if self.widget.draw_mode != mode {
            self.widget.draw_mode = mode;
        }
        let _ = canvas;
        let widget = &self.widget;
        crate::owner_draw::paint_with(cx, &self.owner, |c| widget.paint(c, bounds, state));
    }
}

// ─────────────────────────────────────────────────────────────────────────
// DataTable
// ─────────────────────────────────────────────────────────────────────────

/// One `<Column>` of a `<DataTable>` as its node reads it (see [`data_table_columns`]).
#[derive(Debug, Clone, Default, PartialEq)]
struct TableColumn {
    /// The column id, which is also the row field it shows (`column_id`).
    field: String,
    /// A `Binding` names the field: only then is an edited value written back.
    bound: bool,
    /// `FormatString` / `Culture` / `NullValue` — the column's attributes over the parts of its
    /// own `Binding` (`{Binding amount, FormatString=N2}`), over the table's `Culture`.
    format: BindingFormat,
    /// `ReadOnly="True"`: never edited in place.
    read_only: bool,
    /// `AutoSizeMode="Fill"` (WinForms `DataGridViewAutoSizeColumnMode.Fill`): the column takes the
    /// width the others leave, its `Width` as a minimum.
    fill: bool,
    /// The `Width` written (a fill column's minimum).
    min_width: i32,
}

/// The `<Column>` children of a `<DataTable>`: the widget's `ColumnHeader`s (sortable, aligned
/// per `Alignment`, flagged read-only) and what the node needs to format and edit each one.
/// `table_culture`: the `DataTable`'s own `Culture`, the default of its columns.
fn data_table_columns(element: &Element, table_culture: Option<&str>) -> (Vec<ColumnHeader>, Vec<TableColumn>) {
    let attr = |c: &Element, name: &str| c.attribute(name).and_then(|a| a.value()).filter(|v| !v.is_empty());
    let mut headers = Vec::new();
    let mut columns = Vec::new();
    for (i, c) in element.children().filter(|c| c.name().as_deref() == Some("Column")).enumerate() {
        // A header written `{Res key}` is the resource's text (in the culture the view is built in).
        let header = attr(&c, "Header")
            .map(|h| match crate::binding::is_binding_expr(&h).then(|| crate::binding::parse_binding(&h)).flatten() {
                Some(spec) if crate::resources::reference(&spec).is_some() => match crate::resources::get(&crate::binding::MapViewModel::default(), &spec) {
                    Some(Value::Str(s)) => s,
                    _ => h,
                },
                _ => h,
            })
            .unwrap_or_default();
        let binding = attr(&c, "Binding").unwrap_or_default();
        let width = attr(&c, "Width").and_then(|s| s.parse::<i32>().ok()).unwrap_or(160);
        let field = column_id(&binding, &header, i);
        let mut format = crate::binding::parse_binding(&binding).map(|b| b.format).unwrap_or_default();
        if let Some(f) = attr(&c, "FormatString") {
            format.format_string = Some(f);
        }
        if let Some(n) = c.attribute("NullValue").and_then(|a| a.value()) {
            format.null_value = Some(n);
        }
        if let Some(cu) = attr(&c, "Culture") {
            format.culture = Some(cu);
        } else if format.culture.is_none() {
            format.culture = table_culture.filter(|s| !s.is_empty()).map(str::to_string);
        }
        let read_only = attr(&c, "ReadOnly").is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let fill = attr(&c, "AutoSizeMode").is_some_and(|v| v.eq_ignore_ascii_case("fill"));
        let align = match attr(&c, "Alignment").as_deref() {
            Some("Right") => kubuno_controls::enums::HorizontalAlignment::Right,
            Some("Center") => kubuno_controls::enums::HorizontalAlignment::Center,
            _ => kubuno_controls::enums::HorizontalAlignment::Left,
        };
        let mut col = kubuno_ui::tables::with_flag(kubuno_ui::tables::column(&field, &header, width), kubuno_ui::tables::flags::SORTABLE);
        if read_only {
            col = kubuno_ui::tables::with_flag(col, kubuno_ui::tables::flags::READ_ONLY);
        }
        headers.push(kubuno_ui::tables::aligned(col, align));
        columns.push(TableColumn { field, bound: binding_field(&binding).is_some(), format, read_only, fill, min_width: width });
    }
    (headers, columns)
}

/// The text a cell shows for `row`'s field: as held without a format (DATA-1: `Row::text`),
/// else formatted per the column's `FormatString`/`Culture`/`NullValue`
/// ([`crate::format::to_target`]: a text number `"1234.5"` with `N2` → `1 234,50` in French, an
/// ISO date with `d` → `01/05/1952`, `""` (NULL) → `NullValue`; a text that does not parse —
/// a value being typed — is shown as it is).
fn cell_display(row: &Row, column: &TableColumn) -> String {
    if column.format.is_empty() {
        return row.text(&column.field);
    }
    let value = row.get(&column.field).cloned().unwrap_or(Value::Str(String::new()));
    match crate::format::to_target(value, crate::format::ValueKind::Text, &column.format) {
        Some(Value::Str(s)) => s,
        _ => row.text(&column.field),
    }
}

/// How two cells of a column compare for the grid's sort: numbers (held as numbers or as their
/// text, the way a `BindingSource` hands them) numerically, empty (NULL) cells first, anything
/// else as text ignoring case — never by the formatted text (`1 234,50` < `999,00` would be
/// wrong).
fn compare_cells(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn number(v: Option<&Value>) -> Option<f64> {
        match v? {
            Value::F32(f) => Some(f64::from(*f)),
            Value::Str(s) => s.trim().parse::<f64>().ok().filter(|n| n.is_finite()),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::List(_) | Value::Object(_) => None,
        }
    }
    fn text(v: Option<&Value>) -> String {
        match v {
            Some(Value::Str(s)) => s.clone(),
            Some(Value::F32(f)) => f.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            _ => String::new(),
        }
    }
    let (ta, tb) = (text(a), text(b));
    match (ta.is_empty(), tb.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    if let (Some(x), Some(y)) = (number(a), number(b)) {
        return x.partial_cmp(&y).unwrap_or(Ordering::Equal);
    }
    ta.to_lowercase().cmp(&tb.to_lowercase()).then_with(|| ta.cmp(&tb))
}

/// The grid's row order over a bound list: `order[view row] = index in the list`, stable, by the
/// raw values of the sort column (`field`), reversed for a descending sort.
/// Blank rows a bound `DataTable` shows under its headers in the designer (no data at design time).
const DESIGN_PLACEHOLDER_ROWS: usize = 3;

fn sort_order(rows: &[Row], field: Option<&str>, descending: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    if let Some(f) = field {
        order.sort_by(|&a, &b| {
            let o = compare_cells(rows[a].get(f), rows[b].get(f));
            if descending {
                o.reverse()
            } else {
                o
            }
        });
    }
    order
}

/// Sets the text of `model` column of `item` (column 0 is the item's own text).
fn set_item_cell(item: &mut ListViewItem, model: usize, text: String) {
    if model == 0 {
        item.text = text;
        return;
    }
    while item.sub_items.len() < model {
        item.sub_items.push(Default::default());
    }
    if let Some(sub) = item.sub_items.get_mut(model - 1) {
        sub.text = text;
    }
}

struct DataTableNode {
    items_source: Option<BindingSpec>,
    /// This component's own column ids, in column order — each one doubles
    /// as the row field it reads (see the component's `ItemsSource` doc and
    /// [`resolve_item_rows`]).
    field_names: Vec<String>,
    /// Format, binding and read-only state of each column, in column order.
    columns: Vec<TableColumn>,
    selected_index: PropSource<f32>,
    /// `ReadOnly` of the table (WinForms `DataGridView.ReadOnly`, default editable).
    read_only: PropSource<bool>,
    focus_id: Option<FocusId>,
    on_selection_changed: Option<String>,
    on_activate: Option<String>,
    on_sort_changed: Option<String>,
    on_cell_begin_edit: Option<String>,
    on_cell_validating: Option<String>,
    on_cell_value_changed: Option<String>,
    on_cell_end_edit: Option<String>,
    /// `OnCellClick` (WinForms `DataGridView.CellClick`): a click on a cell, its row and column.
    on_cell_click: Option<String>,
    /// `OwnerDraw` (owner-drawn cells) and the owner-draw handlers (EVT-8).
    pub(crate) owner_draw: PropSource<bool>,
    pub(crate) owner: crate::owner_draw::OwnerDrawEvents,
    widget: DataTable,
    pressed: bool,
    /// The bound rows the items were last built from, and the sort they were built with.
    shown_rows: Option<crate::binding::Rows>,
    shown_sort: (usize, SortOrder),
    /// `order[item index] = row index in the bound list` (identity without a sort or a binding).
    order: Vec<usize>,
    /// `PageSize` (0 = no pagination), `PageIndex` (two-way) and `TotalRows` (a value of 0
    /// or more switches to manual paging: the bound rows are the page).
    page_size: PropSource<f32>,
    page_index: PropSource<f32>,
    total_rows: PropSource<f32>,
    on_page_changed: Option<String>,
    /// The `PageIndex` last read from the view, so a click on the pager is not undone by
    /// the next frame while the view has not changed it.
    seen_page: Option<usize>,
    /// `Density` of the rows (`Compact`, `Normal`, `Comfortable`).
    density: PropSource<String>,
    /// `Loading` (skeleton rows), `EmptyTitle`/`EmptyText`, `ErrorText` (non-empty = error state).
    loading: PropSource<bool>,
    empty_title: PropSource<String>,
    empty_text: PropSource<String>,
    error_text: PropSource<String>,
    /// `SortColumn` (a column's field) and `SortOrder`, applied when the view changes them.
    sort_column: PropSource<String>,
    sort_order: PropSource<String>,
    seen_sort: Option<(String, String)>,
    /// `d:ItemsSource`: the rows the designer shows (read the first time it needs them), then those rows.
    design_source: Option<(String, Option<std::path::PathBuf>)>,
    design_rows: Option<crate::binding::Rows>,
}

/// The width of a `Fill` column in a table `width` wide whose other columns are `others` wide: what they
/// leave inside the one-pixel frame, `min` at least.
fn fill_width(width: f32, others: &[i32], min: i32) -> i32 {
    let room = (width - 2.0).floor() as i32;
    (room - others.iter().sum::<i32>()).max(min)
}

#[cfg(test)]
mod fill_width_tests {
    /// The other columns keep their widths; a table too narrow for them scrolls sideways (the fill
    /// column stays at its minimum) instead of squeezing every column.
    #[test]
    fn a_fill_column_takes_the_rest_and_never_less_than_its_minimum() {
        assert_eq!(super::fill_width(502.0, &[100, 100], 150), 300);
        assert_eq!(super::fill_width(302.0, &[100, 100], 150), 150);
    }
}

impl DataTableNode {
    /// The `d:ItemsSource` rows (designer only), read once.
    fn design_rows(&mut self) -> Option<crate::binding::Rows> {
        if self.design_rows.is_none() {
            self.design_rows = self.design_source.take().and_then(|s| crate::registry::families::items::design_items(&s));
        }
        self.design_rows.clone().filter(|r| !r.is_empty())
    }

    /// Gives a `Fill` column the width the other columns leave in a table `width` wide (its own
    /// `Width` at least), inside the table's one-pixel frame (WinForms' `DataGridViewAutoSizeColumnMode.Fill`
    /// with its `MinimumWidth`). The other columns keep their widths: when even the minimum does not fit,
    /// the table scrolls sideways, as the web's does.
    fn fill_columns(&mut self, width: f32) {
        let Some(fill) = self.columns.iter().position(|c| c.fill) else { return };
        let min = self.columns[fill].min_width;
        let others: Vec<i32> = self.widget.columns.iter().enumerate().filter(|(i, _)| *i != fill).map(|(_, c)| c.width).collect();
        if let Some(c) = self.widget.columns.get_mut(fill) {
            c.width = fill_width(width, &others, min);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        columns: Vec<ColumnHeader>,
        field_names: Vec<String>,
        items_source: Option<BindingSpec>,
        selected_index: PropSource<f32>,
        focus_id: Option<FocusId>,
        on_selection_changed: Option<String>,
        on_activate: Option<String>,
        on_sort_changed: Option<String>,
    ) -> Self {
        let mut widget = DataTable::new();
        widget.layout = Layout::Table;
        widget.columns = columns;
        Self {
            items_source,
            columns: field_names.iter().map(|f| TableColumn { field: f.clone(), bound: true, ..Default::default() }).collect(),
            field_names,
            selected_index,
            read_only: PropSource::Literal(false),
            focus_id,
            on_selection_changed,
            on_activate,
            on_sort_changed,
            on_cell_begin_edit: None,
            on_cell_validating: None,
            on_cell_value_changed: None,
            on_cell_end_edit: None,
            on_cell_click: None,
            owner_draw: PropSource::Literal(false),
            owner: Default::default(),
            widget,
            pressed: false,
            shown_rows: None,
            shown_sort: (0, SortOrder::None),
            order: Vec::new(),
            page_size: PropSource::Literal(0.0),
            page_index: PropSource::Literal(0.0),
            total_rows: PropSource::Literal(-1.0),
            on_page_changed: None,
            seen_page: None,
            density: PropSource::Literal("Normal".to_string()),
            loading: PropSource::Literal(false),
            empty_title: PropSource::Literal(String::new()),
            empty_text: PropSource::Literal(String::new()),
            error_text: PropSource::Literal(String::new()),
            sort_column: PropSource::Literal(String::new()),
            sort_order: PropSource::Literal("None".to_string()),
            seen_sort: None,
            design_source: None,
            design_rows: None,
        }
    }

    /// Applies the view's paging, density, state texts and declared sort to the widget.
    /// `PageIndex` and the sort are only applied when the view changes them, so the pager
    /// and the column headers keep what the user picked in between.
    fn apply_state(&mut self, vm: &dyn ViewModel) {
        let w = &mut self.widget;
        let size = self.page_size.resolve(vm).max(0.0).round() as usize;
        if w.page_size != size {
            w.set_page_size(size);
        }
        let total = self.total_rows.resolve(vm);
        let manual = total >= 0.0;
        w.manual_pagination = manual;
        w.total_rows = manual.then_some(total.round() as usize);
        // Paged by the server (`TotalRows`), a page holds what the server sends: no size to choose.
        let options = if manual { vec![size] } else { vec![10, 25, 50, 100] };
        if w.page_size_options != options {
            w.page_size_options = options;
        }
        let page = self.page_index.resolve(vm).max(0.0).round() as usize;
        if self.seen_page != Some(page) {
            self.seen_page = Some(page);
            w.set_page(page);
        }
        let density = match self.density.resolve(vm).as_str() {
            "Compact" => kubuno_ui::views::Density::Compact,
            "Comfortable" => kubuno_ui::views::Density::Comfortable,
            _ => kubuno_ui::views::Density::Normal,
        };
        if w.list().density != density {
            w.list_mut().density = density;
        }
        w.loading = self.loading.resolve(vm);
        let default = kubuno_ui::tables::Wording::default();
        let title = self.empty_title.resolve(vm);
        w.copy.empty.title = if title.is_empty() { default.empty.title } else { title };
        let text = self.empty_text.resolve(vm);
        w.copy.empty.description = if text.is_empty() { default.empty.description } else { text };
        let error = self.error_text.resolve(vm);
        let error = (!error.is_empty()).then_some(error);
        if w.error != error {
            w.error = error;
        }
        let sort = (self.sort_column.resolve(vm), self.sort_order.resolve(vm));
        if self.seen_sort.as_ref() != Some(&sort) {
            let column = self.columns.iter().position(|c| c.field == sort.0);
            let order = match (column, sort.1.as_str()) {
                (Some(_), "Ascending") => SortOrder::Ascending,
                (Some(_), "Descending") => SortOrder::Descending,
                _ => SortOrder::None,
            };
            w.sort_column = column.unwrap_or(0);
            w.sorting = order;
            self.seen_sort = Some(sort);
        }
    }

    /// The user moved to another page (the pager, a new page size): a two-way `PageIndex`
    /// follows and `OnPageChanged` fires with the new 0-based page.
    fn page_moved(&mut self, cx: &mut PaintCx<'_>, before: usize) {
        let now = self.widget.page_index();
        if now == before {
            return;
        }
        self.seen_page = Some(now);
        if let Some(spec) = self.page_index.binding() {
            if spec.mode.writes_back() {
                spec.update_source(cx.vm, Value::F32(now as f32));
            }
        }
        let mut args = crate::events::NumericValueChangedEventArgs::new(before as f32, now as f32, ChangeSource::User);
        cx.fire("OnPageChanged", self.focus_id, self.on_page_changed.as_deref(), ViewEventKind::Changed(now.to_string()), &mut args);
    }

    /// The user sorted with a column header: two-way `SortColumn`/`SortOrder` follow.
    fn sort_moved(&mut self, cx: &mut PaintCx<'_>) {
        let field = self.columns.get(self.widget.sort_column).map(|c| c.field.clone()).unwrap_or_default();
        let order = match self.widget.sorting {
            SortOrder::Ascending => "Ascending",
            SortOrder::Descending => "Descending",
            SortOrder::None => "None",
        }
        .to_string();
        let field = if order == "None" { String::new() } else { field };
        for (source, value) in [(&self.sort_column, field.clone()), (&self.sort_order, order.clone())] {
            if let Some(spec) = source.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::Str(value));
                }
            }
        }
        self.seen_sort = Some((self.sort_column.resolve(cx.vm), self.sort_order.resolve(cx.vm)));
    }

    /// The row of the bound list an item shows (the item index itself for static rows).
    fn source_row(&self, item: usize) -> usize {
        self.order.get(item).copied().unwrap_or(item)
    }

    /// The item showing row `source` of the bound list.
    fn item_of(&self, source: usize) -> Option<usize> {
        if self.order.is_empty() {
            return (source < self.widget.items.len()).then_some(source);
        }
        self.order.iter().position(|&s| s == source)
    }

    /// The `ItemsSource` path, when the rows come from a binding.
    fn source_path(&self) -> Option<&str> {
        self.items_source.as_ref().map(|s| s.path.as_str()).filter(|p| !p.is_empty())
    }

    /// The `ItemsSource` is a data source with a current row (a `BindingSource`: it answers
    /// `<path>.Position`), whose `Position` follows the grid's current row (WinForms'
    /// `CurrencyManager`).
    fn currency_path(&self, vm: &dyn ViewModel) -> Option<String> {
        let path = self.source_path()?;
        matches!(vm.get(&format!("{path}.Position")), Some(Value::F32(_))).then(|| path.to_string())
    }

    /// Rebuilds the items from the bound rows when they (or the sort) changed: formatted per
    /// column, in the grid's sort order, keeping the selection and the cursor on the same rows.
    fn refresh_rows(&mut self, vm: &dyn ViewModel) {
        if self.field_names.is_empty() {
            return;
        }
        let Some(path) = self.items_source.as_ref().map(|s| s.path.clone()) else { return };
        let rows = match vm.get(&path) {
            Some(Value::List(rows)) if !rows.is_empty() || !crate::common::design_frame() => rows,
            // In the designer without data: the `d:ItemsSource` rows when the view gives some.
            _ if crate::common::design_frame() && self.design_rows().is_some() => match self.design_rows() {
                Some(rows) => rows,
                None => return,
            },
            // Otherwise, like the Windows Forms designer's DataGridView, the column headers over a few
            // blank rows rather than the empty-state illustration.
            _ if crate::common::design_frame() => {
                let blank = build_list_view_item(&vec![String::new(); self.columns.len()]);
                if self.widget.items.len() != DESIGN_PLACEHOLDER_ROWS || self.widget.items.iter().any(|i| *i != blank) {
                    self.widget.items = vec![blank; DESIGN_PLACEHOLDER_ROWS];
                }
                self.shown_rows = None;
                return;
            }
            _ => return,
        };
        // The grid sorts a bound list itself, by the raw values (see `compare_cells`).
        self.widget.manual_sort = true;
        let sort = (self.widget.sort_column, self.widget.sorting);
        if self.shown_rows.as_ref() == Some(&rows) && self.shown_sort == sort {
            return;
        }
        let field = (sort.1 != SortOrder::None).then(|| self.columns.get(sort.0).map(|c| c.field.as_str())).flatten();
        let order = sort_order(&rows, field, sort.1 == SortOrder::Descending);
        let selected: Vec<usize> = self.widget.selected().iter().map(|&i| self.source_row(i)).collect();
        let cursor = self.widget.cursor.map(|i| self.source_row(i));
        let editing = self.widget.editor().map(|e| self.source_row(e.row));
        let mut items: Vec<ListViewItem> = order
            .iter()
            .map(|&r| build_list_view_item(&self.columns.iter().map(|c| cell_display(&rows[r], c)).collect::<Vec<_>>()))
            .collect();
        for (i, &r) in order.iter().enumerate() {
            if selected.contains(&r) {
                items[i].selected = true;
            }
        }
        if self.widget.items != items {
            self.widget.items = items;
        }
        self.widget.cursor = cursor.and_then(|r| order.iter().position(|&s| s == r));
        if let (Some(src), Some(e)) = (editing, self.widget.editor_mut()) {
            match order.iter().position(|&s| s == src) {
                Some(i) => e.row = i,
                None => {
                    self.widget.cancel_edit();
                }
            }
        }
        self.order = order;
        self.shown_rows = Some(rows);
        self.shown_sort = sort;
    }

    /// The user made item `item` the current row (click, arrows, a commit that moves): it is
    /// selected, `SelectedIndex` and `OnSelectionChanged` follow, and a `BindingSource` behind
    /// moves its `Position` there — which ends the pending row edit and can be refused by its
    /// `RowValidating`: the grid then goes back to the source's current row. Returns whether
    /// the move happened.
    fn user_moved_to(&mut self, cx: &mut PaintCx<'_>, item: usize, ctrl: bool, shift: bool) -> bool {
        let source = self.source_row(item);
        if let Some(path) = self.currency_path(&*cx.vm) {
            let current = match cx.vm.get(&format!("{path}.Position")) {
                Some(Value::F32(p)) => p.round() as i64,
                _ => -1,
            };
            if current != source as i64 {
                cx.vm.set(&format!("{path}.Position"), Value::F32(source as f32));
                let now = match cx.vm.get(&format!("{path}.Position")) {
                    Some(Value::F32(p)) => p.round() as i64,
                    _ => -1,
                };
                if now != source as i64 {
                    // Refused (RowValidating, a conversion error): stay on the source's row.
                    if let Some(back) = usize::try_from(now).ok().and_then(|p| self.item_of(p)) {
                        self.widget.click(back, false, false);
                        self.widget.cursor = Some(back);
                    }
                    return false;
                }
            }
        }
        self.widget.click(item, ctrl, shift);
        self.widget.cursor = Some(item);
        let idx = self.widget.selected().first().copied().unwrap_or(item);
        let src = self.source_row(idx) as i32;
        fire_index_changed(cx, "OnSelectionChanged", self.focus_id, self.on_selection_changed.as_deref(), self.selected_index.binding(), src);
        true
    }

    fn cell_args(&self, item: usize, column: usize) -> CellEventArgs {
        CellEventArgs { row_index: self.source_row(item), column_index: column, column: self.columns.get(column).map(|c| c.field.clone()).unwrap_or_default() }
    }

    /// Carries out what the table asked for (see `kubuno_ui::tables::CellAction`).
    fn run_action(&mut self, cx: &mut PaintCx<'_>, bounds: Rect, action: CellAction) {
        match action {
            CellAction::BeginEdit { row, column, initial, select_all } => {
                // The edited row becomes the current row first (a refused move edits nothing).
                let current = self.widget.selected().first().copied();
                if current != Some(row) && !self.user_moved_to(cx, row, false, false) {
                    return;
                }
                let base = self.cell_args(row, column);
                let mut args = CellCancelEventArgs { row_index: base.row_index, column_index: column, column: base.column, cancel: false };
                cx.fire("OnCellBeginEdit", self.focus_id, self.on_cell_begin_edit.as_deref(), ViewEventKind::Clicked, &mut args);
                if !args.cancel {
                    self.widget.begin_edit(row, column, initial.as_deref(), select_all);
                }
            }
            CellAction::Commit { row, column, text, then } => {
                let base = self.cell_args(row, column);
                let mut args = CellValidatingEventArgs {
                    row_index: base.row_index,
                    column_index: column,
                    column: base.column.clone(),
                    formatted_value: text.clone(),
                    cancel: false,
                };
                cx.fire("OnCellValidating", self.focus_id, self.on_cell_validating.as_deref(), ViewEventKind::Changed(text.clone()), &mut args);
                if args.cancel {
                    return; // WinForms: the editor stays open with the refused text.
                }
                let changed = self.widget.editor().is_some_and(|e| e.original != text);
                if changed {
                    self.write_back(cx, row, column, &text);
                }
                self.widget.end_edit();
                if changed {
                    let mut e = base.clone();
                    cx.fire("OnCellValueChanged", self.focus_id, self.on_cell_value_changed.as_deref(), ViewEventKind::Changed(text), &mut e);
                }
                let mut e = base;
                cx.fire("OnCellEndEdit", self.focus_id, self.on_cell_end_edit.as_deref(), ViewEventKind::Clicked, &mut e);
                self.refresh_rows(&*cx.vm);
                match self.widget.cell_after(row, column, then) {
                    Some((r, c)) if then != CellMove::Stay => {
                        if r == row || self.user_moved_to(cx, r, false, false) {
                            self.widget.set_current_cell(bounds, r, c);
                        }
                    }
                    Some(_) => {}
                    None => cx.focus.step(then != CellMove::Previous),
                }
            }
            CellAction::CancelEdit { row, column } => {
                self.widget.cancel_edit();
                let mut e = self.cell_args(row, column);
                cx.fire("OnCellEndEdit", self.focus_id, self.on_cell_end_edit.as_deref(), ViewEventKind::Clicked, &mut e);
            }
            CellAction::CancelRowEdit { .. } => {
                if let Some(path) = self.currency_path(&*cx.vm) {
                    if matches!(cx.vm.get(&format!("{path}.IsEditing")), Some(Value::Bool(true))) {
                        cx.vm.set(&format!("{path}.CancelEdit"), Value::Bool(true));
                    }
                }
            }
            CellAction::LeaveGrid { forward } => cx.focus.step(forward),
        }
    }

    /// Writes an edited cell back (see the component's doc): through the current row of a
    /// `BindingSource` (`<path>.Position` then `<path>.Current.<field>`), through
    /// `<path>[<row>].<field>` for any other bound list, into the item itself for static rows —
    /// parsed back per the column's format in the first two cases (`ViewModel::set_bound`).
    fn write_back(&mut self, cx: &mut PaintCx<'_>, item: usize, column: usize, text: &str) {
        let Some(col) = self.columns.get(column).cloned() else { return };
        let source = self.source_row(item);
        let path = self.source_path().map(str::to_string);
        match (path, col.bound && !col.read_only) {
            (Some(path), true) if self.shown_rows.is_some() => {
                let target = if self.currency_path(&*cx.vm).is_some() {
                    cx.vm.set(&format!("{path}.Position"), Value::F32(source as f32));
                    format!("{path}.Current.{}", col.field)
                } else {
                    format!("{path}[{source}].{}", col.field)
                };
                let spec = BindingSpec { path: target, mode: BindingMode::TwoWay, format: col.format.clone(), ..Default::default() };
                cx.vm.set_bound(&spec, Value::Str(text.to_string()));
            }
            _ => {
                let cols = self.widget.visible_columns();
                let model = cols.get(column).and_then(|c| self.widget.columns.iter().position(|m| m.name == c.name)).unwrap_or(column);
                if let Some(it) = self.widget.items.get_mut(item) {
                    set_item_cell(it, model, text.to_string());
                }
            }
        }
    }

    /// The error glyphs of the current row of a `BindingSource` behind: a column whose field is
    /// in error (`<path>.Current.<field>.Error`, a value that did not convert, a `RowValidating`
    /// error) shows the glyph in its cell (WinForms `ErrorText`).
    fn refresh_errors(&mut self, vm: &dyn ViewModel) {
        let mut errors = Vec::new();
        if let Some(path) = self.currency_path(vm) {
            let position = match vm.get(&format!("{path}.Position")) {
                Some(Value::F32(p)) if p >= 0.0 => Some(p.round() as usize),
                _ => None,
            };
            if let Some(item) = position.and_then(|p| self.item_of(p)) {
                for (i, col) in self.columns.iter().enumerate().filter(|(_, c)| c.bound) {
                    if let Some(Value::Str(message)) = vm.get(&format!("{path}.Current.{}.Error", col.field)) {
                        if !message.is_empty() {
                            errors.push(CellError { row: item, column: i, message });
                        }
                    }
                }
            }
        }
        if self.widget.cell_errors != errors {
            self.widget.cell_errors = errors;
        }
    }

    /// The keys of a focused table that is not editing a cell: the cell keys first (F2, ← →,
    /// Tab, Escape, typed text — `DataTable::cell_key`), then the rows' (`DataTable::on_key`:
    /// ↑ ↓ PgUp PgDn Home End, Ctrl+A, Enter → `OnRowActivated`). A move of the current row
    /// goes through [`Self::user_moved_to`].
    fn handle_keys(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        for e in host::events() {
            if self.widget.is_editing() {
                break; // The rest of the frame's input is the editor's.
            }
            match &e {
                InputEvent::Key { vk: k, down: true, mods, .. } => {
                    let (k, mods) = (*k, *mods);
                    // Escape without a pending row edit is not the grid's (a form's CancelButton).
                    if k == host::vk::ESCAPE && !self.row_edit_pending(&*cx.vm) {
                        continue;
                    }
                    let (used, action) = self.widget.cell_key(bounds, k, mods);
                    if used {
                        consume_event(&e);
                        if let Some(action) = action {
                            self.run_action(cx, bounds, action);
                        } else {
                            self.follow_cursor(cx);
                        }
                        continue;
                    }
                    // An editable table types a space into the cell (the `Text` that follows).
                    if self.widget.editable && k == host::vk::SPACE && mods.matches(host::Modifiers::NONE) {
                        continue;
                    }
                    let before = self.widget.cursor;
                    let (used, event) = self.widget.on_key(bounds, k, mods);
                    if used {
                        consume_event(&e);
                    }
                    if let Some(kubuno_ui::tables::TableEvent::RowActivated(row)) = event {
                        let src = self.source_row(row);
                        fire_clicked(cx, "OnRowActivated", self.focus_id, self.on_activate.as_deref(), Value::F32(src as f32));
                    }
                    if used && self.widget.cursor != before && !mods.shift {
                        self.follow_cursor(cx);
                    }
                }
                InputEvent::Text(s) => {
                    if let Some(action) = self.widget.cell_text_input(s) {
                        consume_event(&e);
                        self.run_action(cx, bounds, action);
                    }
                }
                _ => {}
            }
        }
    }

    /// A `BindingSource` behind has a row edit in progress (what a second Escape cancels).
    fn row_edit_pending(&self, vm: &dyn ViewModel) -> bool {
        self.currency_path(vm).is_some_and(|p| matches!(vm.get(&format!("{p}.IsEditing")), Some(Value::Bool(true))))
    }

    /// The keyboard moved the cursor: the row under it becomes the current (selected) row.
    fn follow_cursor(&mut self, cx: &mut PaintCx<'_>) {
        let Some(row) = self.widget.cursor else { return };
        if self.widget.selected().first().copied() != Some(row) || self.widget.selected().len() != 1 {
            let column = self.widget.current_column;
            if self.user_moved_to(cx, row, false, false) {
                self.widget.current_column = column;
            }
        }
    }
}

/// Consumes `e` (the first unconsumed event equal to it) from the host's frame input.
fn consume_event(e: &InputEvent) {
    let mut done = false;
    host::consume(|x| {
        let hit = !done && x == e;
        done |= hit;
        hit
    });
}

impl ViewNode for DataTableNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        self.apply_state(&*cx.vm);
        self.refresh_rows(&*cx.vm);
        self.fill_columns(bounds.right - bounds.left);
        if !self.widget.is_editing() {
            let wanted = self.selected_index.resolve(cx.vm);
            let wanted_idx = wanted.round() as i32;
            let cur = self.widget.selected().first().map(|&i| self.source_row(i) as i32).unwrap_or(-1);
            if wanted_idx >= 0 && wanted_idx != cur {
                if let Some(item) = self.item_of(wanted_idx as usize) {
                    self.widget.click(item, false, false);
                    if self.widget.cursor.is_some() {
                        self.widget.cursor = Some(item);
                    }
                }
            }
        }

        // Editing needs the keyboard: an editable table is a focus stop (it has an `x:Name`);
        // it keeps Tab for its cells while it holds the focus (WinForms `StandardTab = false`).
        let editable = !self.read_only.resolve(cx.vm) && self.focus_id.is_some();
        if self.widget.editable != editable {
            self.widget.editable = editable;
            if !editable {
                self.widget.cancel_edit();
            }
        }
        let opts = kubuno_ui::FocusOpts { wants_tab: editable, ..Default::default() };
        let focus_state = self.focus_id.map(|id| cx.focus.register_with(id, bounds, opts)).unwrap_or_default();
        self.widget.focus_part = focus_state.focused.then_some(kubuno_ui::tables::FocusPart::Rows);
        self.widget.focus_visible = focus_state.visible;

        let canvas: &dyn Canvas = cx.canvas;
        if self.widget.is_editing() {
            let input = EditInput::new(cx.frame, focus_state);
            if let Some(action) = self.widget.update_editor(canvas, bounds, &input) {
                self.run_action(cx, bounds, action);
            }
        } else if focus_state.focused {
            self.handle_keys(cx, bounds);
        }

        let (mx, my) = cx.frame.mouse;
        let hot_now = !cx.frame.pointer_outside() && bounds.contains(mx, my);
        self.widget.hot_index = if hot_now { self.widget.row_at(bounds, mx, my) } else { None };

        if hot_now && cx.frame.mouse_down && !self.pressed {
            self.pressed = true;
            let in_editor = self.widget.editor_rect(bounds).is_some_and(|r| r.contains(mx, my));
            // A press outside the editor commits it first (a refused value keeps it open).
            if !in_editor {
                if let Some(e) = self.widget.editor() {
                    let action = CellAction::Commit { row: e.row, column: e.column, text: e.text().to_string(), then: CellMove::Stay };
                    self.run_action(cx, bounds, action);
                }
            }
            if !in_editor && !self.widget.is_editing() {
                if let Some(chrome) = self.widget.chrome_at(canvas, bounds, mx, my) {
                    let before = (self.widget.sort_column, self.widget.sorting);
                    let page_before = self.widget.page_index();
                    self.widget.activate(chrome, cx.frame.mods.shift);
                    let after = (self.widget.sort_column, self.widget.sorting);
                    if after != before {
                        self.sort_moved(cx);
                        let text = format!("{}:{:?}", after.0, after.1);
                        fire_string_changed(cx, "OnSortChanged", self.focus_id, self.on_sort_changed.as_deref(), None, &text);
                    }
                    self.page_moved(cx, page_before);
                } else if let Some(row) = self.widget.row_at(bounds, mx, my) {
                    let column = self.widget.cell_column_at(bounds, mx);
                    if let Some(c) = column {
                        let mut e = self.cell_args(row, c);
                        cx.fire("OnCellClick", self.focus_id, self.on_cell_click.as_deref(), ViewEventKind::Clicked, &mut e);
                    }
                    if self.user_moved_to(cx, row, cx.frame.mods.ctrl, cx.frame.mods.shift) {
                        if let Some(c) = column {
                            self.widget.current_column = c;
                        }
                        if cx.frame.click_count >= 2 {
                            let src = self.source_row(row);
                            fire_clicked(cx, "OnRowActivated", self.focus_id, self.on_activate.as_deref(), Value::F32(src as f32));
                            if let (Some(c), true) = (column, self.widget.editable) {
                                if self.widget.column_editable(c) {
                                    let action = CellAction::BeginEdit { row, column: c, initial: None, select_all: true };
                                    self.run_action(cx, bounds, action);
                                }
                            }
                        }
                    }
                }
            }
        }
        if !cx.frame.mouse_down {
            self.pressed = false;
        }

        // `DataTable::scroll_by` already reports whether it actually moved
        // (unlike the other list controls' own `scroll_by`/`scroll_rows`),
        // so this was already scroll-chaining correctly — routed through the
        // shared helper anyway for one rule in one place.
        claim_wheel_if_scrolled(cx.frame, bounds, |wy| self.widget.scroll_by(bounds, 0.0, wy));

        self.refresh_rows(&*cx.vm);
        self.refresh_errors(&*cx.vm);
        let state = crate::common::rest().hot(hot_now);
        // The designer runs no handler of the view it shows: an owner-drawn table shows its default cells there,
        // as the Windows Forms designer does.
        let owner_draw = self.owner_draw.resolve(cx.vm) && !crate::common::design_frame();
        if self.widget.owner_draw_cells != owner_draw {
            self.widget.owner_draw_cells = owner_draw;
        }
        let widget = &self.widget;
        // The handler is told the bound list's row of each item, whatever the sort.
        crate::owner_draw::paint_with_rows(cx, &self.owner, &self.order, |c| widget.paint(c, bounds, state));
    }
}

// ─────────────────────────────────────────────────────────────────────────
// MonthCalendar
// ─────────────────────────────────────────────────────────────────────────

struct MonthCalendarNode {
    selected_date: PropSource<String>,
    today: PropSource<String>,
    focus_id: Option<FocusId>,
    on_date_selected: Option<String>,
    widget: MonthCalendar,
    pressed: bool,
}

impl MonthCalendarNode {
    fn new(selected_date: PropSource<String>, today: PropSource<String>, focus_id: Option<FocusId>, on_date_selected: Option<String>) -> Self {
        let mut widget = MonthCalendar::new();
        if let PropSource::Literal(s) = &selected_date {
            if let Some(date) = parse_iso_date(s) {
                widget.set_selection_start(date);
                widget.view_month = Some(date.first_of_month());
            }
        }
        Self { selected_date, today, focus_id, on_date_selected, widget, pressed: false }
    }
}

impl ViewNode for MonthCalendarNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        self.widget.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let wanted = self.selected_date.resolve(cx.vm);
        if let Some(date) = parse_iso_date(&wanted) {
            if self.widget.selection_start() != date {
                self.widget.set_selection_start(date);
                self.widget.view_month = Some(date.first_of_month());
            }
        }
        // Never left at the replica's `DEFAULT_TODAY` (`2000-01-01`)
        // sentinel — see `crate::clock`'s own doc.
        let today = parse_iso_date(&self.today.resolve(cx.vm)).unwrap_or_else(crate::clock::today);
        self.widget.set_today_date(today);

        let canvas: &dyn Canvas = cx.canvas;
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();
        let (mx, my) = cx.frame.mouse;
        let hot_now = !cx.frame.pointer_outside() && bounds.contains(mx, my);
        self.widget.hot_day = if hot_now { self.widget.day_at(bounds, mx, my) } else { None };
        self.widget.hot_header = if hot_now { self.widget.header_at(bounds, mx, my) } else { None };

        if hot_now && cx.frame.mouse_down && !self.pressed {
            self.pressed = true;
            if let Some(part) = self.widget.header_at(bounds, mx, my) {
                match part {
                    HeaderPart::Prev => self.widget.prev_month(),
                    HeaderPart::Next => self.widget.next_month(),
                    HeaderPart::Title => {}
                }
            } else if let Some(date) = self.widget.day_at(bounds, mx, my) {
                if self.widget.is_selectable(date) {
                    self.widget.click_day(date);
                    let text = format_iso_date(date);
                    fire_string_changed(cx, "OnDateSelected", self.focus_id, self.on_date_selected.as_deref(), self.selected_date.binding(), &text);
                }
            }
        }
        if !cx.frame.mouse_down {
            self.pressed = false;
        }

        let (_, wy) = cx.frame.wheel_dip();
        if wy != 0.0 && !host::wheel_claimed() && bounds.contains(mx, my) {
            self.widget.scroll(if wy > 0.0 { 1 } else { -1 });
            host::claim_wheel();
        }

        let state = focus_state.apply(crate::common::rest().hot(hot_now));
        self.widget.paint(canvas, bounds, state);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Registry declarations
//
// Every `ctor:`/`smoke:`/`build:` block below is spliced into a fresh
// module `component!` generates (`pub mod <mod_name> { .. }`, see
// `../macros.rs`), which is a CHILD of this module, not a copy of it: it
// does not inherit this file's own `use` statements. So every reference to
// a `kubuno_ui`/`kubuno_controls` item and to one of this family's own
// helpers/node types below is written as a full, absolute path — exactly
// how `registry/components.rs`'s own `ctor: kubuno_ui::buttons::Button::
// new(..)` already does it, and per this family's own brief ("use full
// paths for your node types in build blocks").
// ─────────────────────────────────────────────────────────────────────────

component! {
    mod_name: list_box,
    name: "ListBox",
    // Note: A scrolling list of strings (`kubuno_ui::lists::ListBox` replica).
    doc: "A list of items. Add the items as Item children or bind ItemsSource.",
    ctor: kubuno_ui::lists::ListBox::new(),
    children: ChildrenModel::List(&["Item"]),
    default_event: "OnSelectionChanged",
    props: [
        PropertyMeta::new("SelectionMode",
            PropKind::Enum(&["None", "One", "MultiSimple", "MultiExtended"]),
            "One",
            "How many items can be selected, and how Ctrl and Shift combine with a click.",
        ),
        // Note: The anchor of the selection (`-1` = none); bindable two-way.
        PropertyMeta::new("SelectedIndex", PropKind::F32, "-1",
            "Index of the selected item, starting at 0. -1 means none.",
        ),
        // Note: A `{Binding Path}` to a row list (`crate::binding::Value::List`); each row's `Text` field becomes one row's label, re-read every frame. Static `<Item Text=".."/>` children still work and are used as long as this either names no binding or the binding is unset/not a list.
        PropertyMeta::new("ItemsSource", PropKind::String, "",
            "Binding to the list of items to show, instead of Item children.",
        ),
        crate::owner_draw::DRAW_MODE,
        PropertyMeta::new("ItemHeight", PropKind::F32, "0", "Height of a row, in DIP (with DrawMode OwnerDrawFixed: the height the DrawItem handler draws in); 0 for the standard height.").category("Behavior"),
    ],
    events: [
        EventMeta::new("OnSelectionChanged", "Occurs when the selection changes.").category(crate::registry::EventCategory::Behavior).args::<crate::events::SelectionChangedEventArgs>(),
        crate::owner_draw::ON_DRAW_ITEM,
        crate::owner_draw::ON_MEASURE_ITEM,
    ],
    smoke: |mut b| {
        b.item_height = 32;
        b.items.push("A".to_string());
        b.set_selection_mode(kubuno_controls::lists::SelectionMode::MultiSimple);
        b.set_selected_index(0);
        b
    },
    build: |props, _cx| {
        let (items, sorted) = crate::registry::families::data::list_items(props.element());
        let selection_mode = props.enum_("SelectionMode", "One")?;
        let selected_index = props.f32("SelectedIndex", -1.0)?;
        let items_source = props.str("ItemsSource", "")?.binding().cloned();
        let focus_id = props.focus_id();
        let on_selection_changed = props.event("OnSelectionChanged");
        let mut node = crate::registry::families::data::ListBoxNode::new(
            items, items_source, selection_mode, selected_index, focus_id, on_selection_changed,
        );
        node.sorted = sorted;
        node.draw_mode = crate::owner_draw::draw_mode_prop(props)?;
        node.item_height = props.f32("ItemHeight", 0.0)?;
        node.owner = crate::owner_draw::OwnerDrawEvents::read(props);
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: checked_list_box,
    name: "CheckedListBox",
    // Note: A `ListBox` whose rows carry a check box (`kubuno_ui::lists::CheckedListBox` replica).
    doc: "A list of items, each with a check box.",
    ctor: kubuno_ui::lists::CheckedListBox::new(),
    children: ChildrenModel::List(&["Item"]),
    default_event: "OnSelectionChanged",
    props: [
        // Note: The single selected row (`-1` = none); bindable two-way.
        PropertyMeta::new("SelectedIndex", PropKind::F32, "-1",
            "Index of the selected item, starting at 0. -1 means none.",
        ),
        PropertyMeta::new("CheckOnClick", PropKind::Bool, "false",
            "Checks or unchecks an item with a single click anywhere on it.",
        ),
        // Note: A `{Binding Path}` to a row list (`crate::binding::Value::List`); each row's `Text` field becomes one row's label, re-read every frame (all unchecked — a bound row's check state is not modelled, see this family's report). Static `<Item Text=".."/>` children still work and are used as long as this either names no binding or the binding is unset/not a list.
        PropertyMeta::new("ItemsSource", PropKind::String, "",
            "Binding to the list of items to show, instead of Item children.",
        ),
    ],
    events: [
        EventMeta::new("OnSelectionChanged", "Occurs when the selected item changes.").category(crate::registry::EventCategory::Behavior).args::<crate::events::SelectionChangedEventArgs>(),
        EventMeta::new("OnCheckedChanged", "Occurs when an item is checked or unchecked.").category(crate::registry::EventCategory::Behavior).args::<crate::events::ItemCheckEventArgs>(),
    ],
    smoke: |mut b| {
        b.add_item("A", kubuno_controls::enums::CheckState::Unchecked);
        b.toggle_check(0);
        b
    },
    build: |props, _cx| {
        let (items, sorted) = crate::registry::families::data::list_items(props.element());
        let selected_index = props.f32("SelectedIndex", -1.0)?;
        let check_on_click = props.bool("CheckOnClick", false)?;
        let items_source = props.str("ItemsSource", "")?.binding().cloned();
        let focus_id = props.focus_id();
        let on_selection_changed = props.event("OnSelectionChanged");
        let on_checked_changed = props.event("OnCheckedChanged");
        let mut node = crate::registry::families::data::CheckedListBoxNode::new(
            items, items_source, selected_index, check_on_click, focus_id, on_selection_changed, on_checked_changed,
        );
        node.sorted = sorted;
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: list_view,
    name: "ListView",
    // Note: A columned, virtualised row list (`kubuno_ui::views::ListView` replica, Details mode).
    doc: "A list of items in columns. Declare the columns as Column children.",
    ctor: kubuno_ui::views::ListView::new(),
    children: ChildrenModel::List(&["Item", "Column"]),
    default_event: "OnSelectionChanged",
    props: [
        // Note: The anchor of the selection (`-1` = none); bindable two-way.
        PropertyMeta::new("SelectedIndex", PropKind::F32, "-1",
            "Index of the selected item, starting at 0. -1 means none.",
        ),
        PropertyMeta::new("MultiSelect", PropKind::Bool, "false", "Allows several items to be selected."),
        // Note: A `{Binding Path}` to a row list (`crate::binding::Value::List`); with `<Column>` children declared, each row's field named by a column's own `Binding` (its bare path, e.g. `Name` for `{Binding Name}`) becomes that column's cell text for that row, re-read every frame. A column with no `Binding` reads no field (an empty cell for every bound row). Static `<Item Text=".."/>` children still work and are used as long as this either names no binding or the binding is unset/not a list; `<Column>` children always declare the columns themselves either way.
        PropertyMeta::new("ItemsSource", PropKind::String, "",
            "Binding to the list of rows to show. Each column shows the field named by its Binding.",
        ),
        crate::owner_draw::OWNER_DRAW,
    ],
    events: [
        EventMeta::new("OnSelectionChanged", "Occurs when the selection changes.").category(crate::registry::EventCategory::Behavior).args::<crate::events::SelectionChangedEventArgs>(),
        EventMeta::new("OnItemActivate", "Occurs when an item is double-clicked.").category(crate::registry::EventCategory::Action).args::<crate::events::ItemActivateEventArgs>().aliases(&["OnActivate"]),
        crate::owner_draw::ON_DRAW_ITEM,
    ],
    smoke: |mut v| {
        v.columns.push(kubuno_ui::views::ColumnHeader::new("Nom", 100));
        v.items.push(kubuno_ui::views::ListViewItem::new("Ligne"));
        v.multi_select = true;
        v
    },
    build: |props, _cx| {
        let static_cols = crate::registry::families::data::static_columns(props.element());
        let columns = static_cols.iter().map(|(header, _binding, width)| kubuno_ui::views::ColumnHeader::new(header.clone(), *width)).collect();
        let field_names: Vec<String> = static_cols
            .iter()
            .map(|(_, binding, _)| crate::registry::families::data::binding_field(binding).unwrap_or_default().to_string())
            .collect();
        // One value per `field_names` entry, per `<Item>` — see
        // `static_item_rows`'s own doc for the `Text`/`<FieldName>`
        // attribute convention this reads.
        let items = crate::registry::families::data::static_item_rows(props.element(), &field_names);
        let selected_index = props.f32("SelectedIndex", -1.0)?;
        let multi_select = props.bool("MultiSelect", false)?;
        let items_source = props.str("ItemsSource", "")?.binding().cloned();
        let focus_id = props.focus_id();
        let on_selection_changed = props.event("OnSelectionChanged");
        let on_activate = props.event("OnActivate");
        let mut node = crate::registry::families::data::ListViewNode::new(
            items, items_source, columns, field_names, multi_select, selected_index, focus_id, on_selection_changed, on_activate,
        );
        node.owner_draw = props.bool("OwnerDraw", false)?;
        node.owner = crate::owner_draw::OwnerDrawEvents::read(props);
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: tree_view,
    name: "TreeView",
    // Note: An indented, expandable node tree (`kubuno_ui::views::TreeView` replica).
    doc: "A tree of items that can be expanded. Add the items as nested Item children.",
    ctor: kubuno_ui::views::TreeView::new(),
    children: ChildrenModel::List(&["Item"]),
    default_event: "OnSelectionChanged",
    props: [
        // Note: The selected node, as dot-joined child indices (`"0.2.1"`, `""` = none); bindable two-way.
        PropertyMeta::new("SelectedPath", PropKind::String, "",
            "Selected item, as indexes separated by dots, for example 0.2.1. Empty means none.",
        ),
        PropertyMeta::new("MultiSelect", PropKind::Bool, "false", "Allows several items to be selected."),
        // Note: A `{Binding Path}` to a row list (`crate::binding::Value::List`); each row's `Text` field becomes one FLAT, top-level, always-childless node — `Value::List` has no nested-row shape, so a bound tree is one level deep (see this family's report); nest with static `<Item Text="..">..</Item>` children for a real hierarchy. Static children still work and are used as long as this either names no binding or the binding is unset/not a list.
        PropertyMeta::new("ItemsSource", PropKind::String, "",
            "Binding to a flat list of items to show, instead of Item children.",
        ),
        PropertyMeta::new("DrawMode", PropKind::Enum(&["Normal", "OwnerDrawText", "OwnerDrawAll"]), "Normal",
            "Who draws the items: the control (Normal), or your DrawItem handler, for the text only (OwnerDrawText) or the whole row (OwnerDrawAll).",
        ).category("Behavior"),
        PropertyMeta::new("ItemHeight", PropKind::F32, "0", "Height of a row, in DIP; 0 for the standard height.").category("Appearance"),
    ],
    events: [
        EventMeta::new("OnSelectionChanged", "Occurs when the selected item changes.").category(crate::registry::EventCategory::Behavior).args::<crate::events::TextChangedEventArgs>(),
        EventMeta::new("OnItemActivate", "Occurs when an item is double-clicked.").category(crate::registry::EventCategory::Action).args::<crate::events::ItemActivateEventArgs>().aliases(&["OnActivate"]),
        crate::owner_draw::ON_DRAW_ITEM.aliases(&["OnDrawNode"]),
    ],
    smoke: |mut t| {
        t.nodes.push(
            kubuno_ui::views::TreeNode::new("Racine")
                .child(kubuno_ui::views::TreeNode::new("Enfant"))
                .expanded(),
        );
        t
    },
    build: |props, _cx| {
        let nodes = crate::registry::families::data::static_tree_nodes(props.element());
        let selected_path = props.str("SelectedPath", "")?;
        let multi_select = props.bool("MultiSelect", false)?;
        let items_source = props.str("ItemsSource", "")?.binding().cloned();
        let focus_id = props.focus_id();
        let on_selection_changed = props.event("OnSelectionChanged");
        let on_activate = props.event("OnActivate");
        let mut node = crate::registry::families::data::TreeViewNode::new(
            nodes, items_source, multi_select, selected_path, focus_id, on_selection_changed, on_activate,
        );
        node.draw_mode = props.enum_("DrawMode", "Normal")?;
        node.owner = crate::owner_draw::OwnerDrawEvents::read(props);
        crate::registry::families::data::static_tree_heights(props.element(), &[], &mut node.widget.node_heights);
        node.item_height = props.f32("ItemHeight", 0.0)?;
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: data_table,
    name: "DataTable",
    // Note: A full data grid — toolbar, sortable columns, pagination (`kubuno_ui::tables::DataTable` replica).
    doc: "A data table with sortable columns and pages. Declare the columns as Column children.",
    ctor: kubuno_ui::tables::DataTable::new(),
    children: ChildrenModel::List(&["Column"]),
    default_event: "OnSelectionChanged",
    props: [
        // Note: The anchor of the selection (`-1` = none); bindable two-way.
        PropertyMeta::new("SelectedIndex", PropKind::F32, "-1",
            "Index of the selected row, starting at 0. -1 means none.",
        ),
        // Note: A `{Binding Path}` to a row list (`crate::binding::Value::List`), the shape `admin_users.rs::table()` already hand-builds; each row's field named by a column's own `Binding` (its bare path, e.g. `Name` for `{Binding Name}` — the same name that column's id/sort-key already uses) becomes that column's cell text for that row, re-read every frame. Without this, the table declares its columns with static `<Column Header=".." Binding=".."/>` children and starts with zero rows.
        PropertyMeta::new("ItemsSource", PropKind::String, "",
            "Binding to the list of rows to show. Each column shows the field named by its Binding.",
        ),
        PropertyMeta::new("OwnerDraw", PropKind::Bool, "false", "Draws the cells with your DrawItem handler (e.sub_index is the column) instead of the table's own text.").category("Behavior"),
        // Note: WinForms `DataGridView.ReadOnly` (default editable). Editing needs the keyboard, so only a named (`x:Name`, focusable) table edits; F2, typing or a double-click begin an edit of the current cell, Enter/Tab commit, Escape cancels. An edited value goes to `<ItemsSource>.Current.<field>` of a BindingSource (after `Position`), to `<ItemsSource>[row].<field>` of a view-model list, into the row itself for static rows.
        PropertyMeta::new("ReadOnly", PropKind::Bool, "false",
            "Prevents the cells from being edited. When false, F2, typing or a double-click edits the current cell; Enter or Tab commits, Escape cancels.",
        ).category("Behavior"),
        // Note: The default `Culture` of the columns (`fr-FR`, `en-US`, `invariant`…): a column with a `FormatString` and no `Culture` of its own formats with it.
        PropertyMeta::new("Culture", PropKind::String, "",
            "Culture used to format and read the values of the columns that set no culture of their own, for example fr-FR. Empty uses the user's.",
        ).category("Behavior"),
        // Note: `pageSize` of the web table; 0 (the default) shows every row with no pager.
        PropertyMeta::new("PageSize", PropKind::F32, "0", "Number of rows per page. 0 shows every row without a pager.").category("Paging").bindable(),
        // Note: 0-based, bindable two-way: the pager writes the page the user moved to.
        PropertyMeta::new("PageIndex", PropKind::F32, "0", "Page shown, starting at 0. A two-way binding follows the pager.").category("Paging").bindable(),
        // Note: `manualPagination` + `totalRows`: 0 or more means the caller pages (its ItemsSource IS the page, fetched in OnPageChanged); -1 (the default) pages the bound rows locally.
        PropertyMeta::new("TotalRows", PropKind::F32, "-1", "Total number of rows when you fetch one page at a time: ItemsSource then holds only the page shown. -1 pages the rows itself.").category("Paging").bindable(),
        PropertyMeta::new("Density", PropKind::Enum(&["Compact", "Normal", "Comfortable"]), "Normal", "Height of the rows.").category("Appearance").bindable(),
        PropertyMeta::new("Loading", PropKind::Bool, "false", "Shows placeholder rows while the data is loading.").category("Behavior").bindable(),
        PropertyMeta::new("EmptyTitle", PropKind::String, "", "Title shown when there is no row. Empty uses the default text.").category("Appearance").bindable(),
        PropertyMeta::new("EmptyText", PropKind::String, "", "Description shown when there is no row. Empty uses the default text.").category("Appearance").bindable(),
        // Note: Non-empty switches the body to the error state (the web's `error` prop), this text as its description.
        PropertyMeta::new("ErrorText", PropKind::String, "", "When not empty, the table shows an error with this description instead of its rows.").category("Behavior").bindable(),
        // Note: The field (`Binding` name) of the sorted column; applied when the view changes it, written back two-way by a header click.
        PropertyMeta::new("SortColumn", PropKind::String, "", "Field of the column the rows are sorted by.").category("Behavior").bindable(),
        PropertyMeta::new("SortOrder", PropKind::Enum(&["None", "Ascending", "Descending"]), "None", "Direction of the sort.").category("Behavior").bindable(),
    ],
    events: [
        crate::owner_draw::ON_DRAW_ITEM.aliases(&["OnCellPainting"]),
        EventMeta::new("OnSelectionChanged", "Occurs when the selected row changes.").category(crate::registry::EventCategory::Behavior).args::<crate::events::SelectionChangedEventArgs>(),
        EventMeta::new("OnRowActivated", "Occurs when a row is double-clicked.").category(crate::registry::EventCategory::Action).args::<crate::events::ItemActivateEventArgs>(),
        EventMeta::new("OnSortChanged", "Occurs when a column header is clicked to sort the table.").category(crate::registry::EventCategory::Behavior).args::<crate::events::TextChangedEventArgs>(),
        EventMeta::new("OnCellBeginEdit", "Occurs when a cell is about to be edited. Set Cancel to keep it from being edited.").category(crate::registry::EventCategory::Behavior).args::<crate::events::CellCancelEventArgs>(),
        EventMeta::new("OnCellValidating", "Occurs when an edited cell is about to be committed, with the text typed. Set Cancel to refuse it and keep editing.").category(crate::registry::EventCategory::Focus).args::<crate::events::CellValidatingEventArgs>(),
        EventMeta::new("OnCellValueChanged", "Occurs when the value of a cell was changed by an edit.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::CellEventArgs>(),
        EventMeta::new("OnCellEndEdit", "Occurs when the edit of a cell ends, committed or cancelled.").category(crate::registry::EventCategory::Focus).args::<crate::events::CellEventArgs>(),
        EventMeta::new("OnCellClick", "Occurs when a cell is clicked: its row and its column.").category(crate::registry::EventCategory::Action).args::<crate::events::CellEventArgs>(),
        EventMeta::new("OnPageChanged", "Occurs when the user moves to another page. The value is the new page, starting at 0.").category(crate::registry::EventCategory::Behavior).args::<crate::events::NumericValueChangedEventArgs>(),
    ],
    smoke: |mut d| {
        d.columns.push(kubuno_ui::views::ColumnHeader::new("Nom", 100));
        d.layout = kubuno_ui::tables::Layout::Table;
        d
    },
    build: |props, cx| {
        let culture = props.element().attribute("Culture").and_then(|a| a.value());
        let (columns, table_columns) = crate::registry::families::data::data_table_columns(props.element(), culture.as_deref());
        let field_names: Vec<String> = table_columns.iter().map(|c| c.field.clone()).collect();
        let selected_index = props.f32("SelectedIndex", -1.0)?;
        let items_source = props.str("ItemsSource", "")?.binding().cloned();
        let focus_id = props.focus_id();
        let on_selection_changed = props.event("OnSelectionChanged");
        let on_activate = props.event("OnRowActivated");
        let on_sort_changed = props.event("OnSortChanged");
        let mut node = crate::registry::families::data::DataTableNode::new(
            columns, field_names, items_source, selected_index, focus_id, on_selection_changed, on_activate, on_sort_changed,
        );
        node.columns = table_columns;
        node.read_only = props.bool("ReadOnly", false)?;
        node.on_cell_begin_edit = props.event("OnCellBeginEdit");
        node.on_cell_validating = props.event("OnCellValidating");
        node.on_cell_value_changed = props.event("OnCellValueChanged");
        node.on_cell_end_edit = props.event("OnCellEndEdit");
        node.on_cell_click = props.event("OnCellClick");
        node.owner_draw = props.bool("OwnerDraw", false)?;
        node.owner = crate::owner_draw::OwnerDrawEvents::read(props);
        node.page_size = props.f32("PageSize", 0.0)?;
        node.page_index = props.f32("PageIndex", 0.0)?;
        node.total_rows = props.f32("TotalRows", -1.0)?;
        node.on_page_changed = props.event("OnPageChanged");
        node.density = props.enum_("Density", "Normal")?;
        node.loading = props.bool("Loading", false)?;
        node.empty_title = props.str("EmptyTitle", "")?;
        node.empty_text = props.str("EmptyText", "")?;
        node.error_text = props.str("ErrorText", "")?;
        node.sort_column = props.str("SortColumn", "")?;
        node.sort_order = props.enum_("SortOrder", "None")?;
        node.design_source = crate::registry::families::items::design_source(props, cx.base_dir.as_deref());
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: month_calendar,
    name: "MonthCalendar",
    // Note: A one-month date grid (`kubuno_ui::datetime::MonthCalendar` replica).
    doc: "A calendar showing one month.",
    ctor: kubuno_ui::datetime::MonthCalendar::new(),
    children: ChildrenModel::None,
    default_event: "OnDateSelected",
    props: [
        // Note: The picked day, as an ISO `YYYY-MM-DD` date (`""` = none); bindable two-way.
        PropertyMeta::new("SelectedDate", PropKind::String, "",
            "Selected date, in the form YYYY-MM-DD. Empty means none.",
        ),
        // Note: Overrides the « today » marker/footer (ISO `YYYY-MM-DD`) — for a test or a screenshot that needs a fixed date; empty (the default) reads the real local date every frame (`crate::clock::today`).
        PropertyMeta::new("Today", PropKind::String, "",
            "Date treated as today, in the form YYYY-MM-DD. Leave empty to use the current date.",
        ),
    ],
    events: [
        EventMeta::new("OnDateSelected", "Occurs when a day is clicked.").category(crate::registry::EventCategory::Action).args::<crate::events::TextChangedEventArgs>(),
    ],
    smoke: |c| {
        c.on(kubuno_controls::datetime::Date::new(2026, 9, 25))
    },
    build: |props, _cx| {
        let selected_date = props.str("SelectedDate", "")?;
        let today = props.str("Today", "")?;
        let focus_id = props.focus_id();
        let on_date_selected = props.event("OnDateSelected");
        Ok(Box::new(crate::registry::families::data::MonthCalendarNode::new(
            selected_date, today, focus_id, on_date_selected,
        )) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: item,
    name: "Item",
    // Note: A static list entry — a row of `<ListBox>`/`<CheckedListBox>`/`<ListView>`, or (nested under itself) one node of a `<TreeView>`'s tree. Read directly off the parsed XML by the parent's `build` (see this file's module doc); never reached through `Props::build_children`, so it builds an inert placeholder. Accepts any attribute name, not just `Text`: a `<ListView>`/`<DataTable>` row's other columns are read the same way, by an attribute named exactly like that column's own `Binding` field (see `static_item_rows`).
    doc: "An item of a ListBox, CheckedListBox, ListView or TreeView.",
    ctor: (),
    children: ChildrenModel::List(&["Item"]),
    open_attributes: true,
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text of the item."),
    ],
    events: [],
    smoke: |b| { b },
    build: |_props, _cx| {
        Ok(Box::new(crate::registry::families::data::InertNode) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: column,
    name: "Column",
    // Note: A static column declaration — a `<ListView>`/`<DataTable>` child. Read directly off the parsed XML by the parent's `build` (see this file's module doc); never reached through `Props::build_children`, so it builds an inert placeholder.
    doc: "A column of a ListView or DataTable.",
    ctor: (),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Header", PropKind::String, "", "Text of the column header."),
        // Note: A `{Binding Path}` naming the row field this column shows — accepted (and used to derive the column's stable id) but not resolved per row until `ItemsSource` is (see this file's module doc).
        PropertyMeta::new("Binding", PropKind::String, "",
            "Field of each row shown in this column, for example {Binding Name}.",
        ),
        PropertyMeta::new("Width", PropKind::F32, "160", "Width of the column, in pixels."),
        // Note: WinForms `DataGridViewColumn.DefaultCellStyle.Format` — the same formats as a binding's `FormatString` (`crate::format`): numbers `N2`, `C`, `P0`, `#,##0.00`…, dates `d`, `D`, `g`, `dd/MM/yyyy`…; an edited value is parsed back with it. May also be given inside `Binding` (`{Binding amount, FormatString=N2}`); this attribute wins.
        PropertyMeta::new("FormatString", PropKind::String, "",
            "Format of the values shown in this column, for example N2 for a number with two decimals or d for a short date. Empty shows the values as they are.",
        ).category("Appearance"),
        // Note: `DefaultCellStyle.FormatProvider`; empty takes the `DataTable`'s `Culture`, else the user's locale.
        PropertyMeta::new("Culture", PropKind::String, "",
            "Culture used to format and read the values of this column, for example fr-FR. Empty uses the table's culture.",
        ).category("Appearance"),
        // Note: `DefaultCellStyle.NullValue`: what an empty (NULL) value shows, and the text that writes NULL back.
        PropertyMeta::new("NullValue", PropKind::String, "",
            "Text shown for an empty value. Typing it writes an empty value back.",
        ).category("Appearance"),
        // Note: `DefaultCellStyle.Alignment` (horizontal part); WinForms' default is left (MiddleLeft), numbers included.
        PropertyMeta::new("Alignment", PropKind::Enum(&["Left", "Center", "Right"]), "Left",
            "Horizontal alignment of the values in this column.",
        ).category("Appearance"),
        // Note: `DataGridViewColumn.ReadOnly`.
        PropertyMeta::new("ReadOnly", PropKind::Bool, "false",
            "Prevents the cells of this column from being edited.",
        ).category("Behavior"),
        // Note: `DataGridViewColumn.AutoSizeMode` (`None`, `Fill`).
        PropertyMeta::new("AutoSizeMode", PropKind::Enum(&["None", "Fill"]), "None",
            "Fill gives the column the width the other columns leave, its Width at least.",
        ).category("Layout"),
    ],
    events: [],
    smoke: |b| { b },
    build: |_props, _cx| {
        Ok(Box::new(crate::registry::families::data::InertNode) as Box<dyn ViewNode>)
    },
}

/// Every component this family declares, in declaration order.
pub const ALL: &[ComponentMeta] = &[
    list_box::META,
    checked_list_box::META,
    list_view::META,
    tree_view::META,
    data_table::META,
    month_calendar::META,
    item::META,
    column::META,
];

#[cfg(test)]
mod tests {
    //! Compile-level coverage, in the same style as `crate::compile::tests`:
    //! `crate::compile::compile` runs the real parse → validate → build
    //! pipeline against `crate::registry::all()` (which includes this family
    //! under the `family-data` feature this crate is tested with), so these
    //! pin the static `<Item>`/`<Column>` markup this file's module doc
    //! describes, end to end, without needing a live `Canvas`.

    use super::*;
    use crate::binding::{BindingMode, MapViewModel, Row};
    use crate::compile::compile;

    // ── Wheel scrolling / scroll chaining ───────────────────────────────

    /// A frame with the mouse over `mouse` and `wy` DIP of vertical wheel
    /// travel — everything [`claim_wheel_if_scrolled`] reads, nothing else
    /// populated (no live `Canvas` needed at all: see this function's own
    /// doc on why it takes only `&Frame`).
    fn wheel_frame(mouse: (f32, f32), wy: f32) -> Frame {
        Frame {
            size: (400.0, 300.0),
            mouse,
            mouse_down: false,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 400.0, 300.0),
            chrome_top: 0.0,
            mods: kubuno_controls::host::Modifiers::NONE,
            wheel: (0.0, wy),
            click_count: 0,
            window_focused: true,
        }
    }

    #[test]
    fn a_list_that_fits_entirely_does_not_claim_the_wheel() {
        // The exact regression this fixes: a `<ListBox>` short enough that
        // all its rows are already on screen (a 3-item list in a box tall
        // enough for far more) used to swallow every wheel notch anyway —
        // this pins that it no longer does, which is the precondition for
        // an enclosing `<ScrollArea>` to receive it instead (standard scroll
        // chaining — kubuno_ui's own `ScrollArea` is the tested, trusted
        // consumer of an unclaimed wheel; this crate's own contract is
        // exactly "leave it unclaimed when there is nothing to scroll").
        let items = vec!["Alpha".to_string(), "Beta".to_string(), "Gamma".to_string()];
        let mut node = ListBoxNode::new(items, None, PropSource::Literal("One".to_string()), PropSource::Literal(-1.0), None, None);
        let bounds = Rect::new(0.0, 0.0, 200.0, 300.0); // Tall enough for every row.
        let before = node.widget.top_index;
        // Read/write only the DELTA `claim_wheel_if_scrolled` itself
        // produces, never the raw global flag: `host::WHEEL_CLAIMED` is a
        // `thread_local!` that Rust's test harness may run other tests on
        // the same OS thread before or after this one (there is no public
        // `begin_frame`-style reset reachable from this crate — see
        // `kubuno_controls::host::begin_frame`, `pub(crate)`), so only the
        // change THIS call makes is deterministic, not the ambient value.
        let claimed_before = host::wheel_claimed();

        claim_wheel_if_scrolled(&wheel_frame((100.0, 150.0), 40.0), bounds, |wy| {
            let rh = node.widget.row_height();
            let rows = if rh > 0.0 { (wy / rh).round() as i32 } else { 0 };
            if rows == 0 {
                return false;
            }
            let before = node.widget.top_index;
            node.widget.scroll_rows(bounds, rows);
            node.widget.top_index != before
        });

        assert_eq!(node.widget.top_index, before, "nothing to scroll: the list must not move");
        assert_eq!(host::wheel_claimed(), claimed_before, "and must leave the wheel exactly as it found it, for the enclosing ScrollArea");
    }

    #[test]
    fn a_list_that_overflows_scrolls_and_claims_the_wheel() {
        let items: Vec<String> = (0..50).map(|i| format!("Row {i}")).collect();
        let mut node = ListBoxNode::new(items, None, PropSource::Literal("One".to_string()), PropSource::Literal(-1.0), None, None);
        let bounds = Rect::new(0.0, 0.0, 200.0, 80.0); // Only a few rows fit.
        let before = node.widget.top_index;

        claim_wheel_if_scrolled(&wheel_frame((100.0, 40.0), 60.0), bounds, |wy| {
            let rh = node.widget.row_height();
            let rows = if rh > 0.0 { (wy / rh).round() as i32 } else { 0 };
            if rows == 0 {
                return false;
            }
            let before = node.widget.top_index;
            node.widget.scroll_rows(bounds, rows);
            node.widget.top_index != before
        });

        assert!(node.widget.top_index > before, "plenty to scroll: the list must move");
        assert!(host::wheel_claimed(), "and must claim the wheel so the page underneath does not also move");
    }

    #[test]
    fn claim_wheel_if_scrolled_never_calls_apply_when_the_pointer_is_elsewhere() {
        let mut called = false;
        claim_wheel_if_scrolled(&wheel_frame((999.0, 999.0), 40.0), Rect::new(0.0, 0.0, 200.0, 80.0), |_wy| {
            called = true;
            true
        });
        assert!(!called, "outside `bounds`: not this control's wheel to answer for");
    }

    #[test]
    fn claim_wheel_if_scrolled_never_calls_apply_with_no_wheel_travel() {
        let mut called = false;
        claim_wheel_if_scrolled(&wheel_frame((100.0, 40.0), 0.0), Rect::new(0.0, 0.0, 200.0, 80.0), |_wy| {
            called = true;
            true
        });
        assert!(!called);
    }

    #[test]
    fn resolve_item_labels_reads_a_bound_list_and_falls_back_when_unset() {
        let vm = MapViewModel::new().with(
            "Users",
            Value::from(vec![
                Row::new().with("Text", Value::Str("Alice".to_string())),
                Row::new().with("Text", Value::Str("Bob".to_string())),
            ]),
        );
        let spec = Some(BindingSpec { path: "Users".to_string(), mode: BindingMode::OneWay, ..Default::default() });
        assert_eq!(resolve_item_labels(&vm, &spec), Some(vec!["Alice".to_string(), "Bob".to_string()]));

        // No binding at all: the caller keeps its static items.
        assert_eq!(resolve_item_labels(&vm, &None), None);
        // A binding to a path that is not a list: same "leave it alone" signal.
        let scalar = Some(BindingSpec { path: "NotAList".to_string(), mode: BindingMode::OneWay, ..Default::default() });
        assert_eq!(resolve_item_labels(&vm.clone().with("NotAList", Value::Bool(true)), &scalar), None);
    }

    #[test]
    fn resolve_item_rows_reads_named_fields_in_column_order() {
        let vm = MapViewModel::new().with(
            "Users",
            Value::from(vec![Row::new().with("Name", Value::Str("Alice".to_string())).with("Role", Value::Str("Admin".to_string()))]),
        );
        let spec = Some(BindingSpec { path: "Users".to_string(), mode: BindingMode::OneWay, ..Default::default() });
        let field_names = vec!["Name".to_string(), "Role".to_string()];
        assert_eq!(resolve_item_rows(&vm, &spec, &field_names), Some(vec![vec!["Alice".to_string(), "Admin".to_string()]]));
    }

    #[test]
    fn list_box_node_starts_from_its_static_items_until_a_bound_list_resolves() {
        // `ListBoxNode::new`'s own construction — the same seed `paint`'s
        // first lines (`resolve_item_labels`, see the test above) leave
        // untouched until `items_source` actually resolves to a list; a
        // `ListBoxNode` never manufactures a live `Canvas`-driven frame in
        // this crate's own tests (see `node::tests`' own note on why).
        let node = ListBoxNode::new(
            vec!["Static".to_string()],
            Some(BindingSpec { path: "Users".to_string(), mode: BindingMode::OneWay, ..Default::default() }),
            PropSource::Literal("One".to_string()),
            PropSource::Literal(-1.0),
            None,
            None,
        );
        assert_eq!(node.widget.items, vec!["Static".to_string()]);
        // The exact same resync `ListBoxNode::paint` runs, checked directly
        // against a view model that now has a resolvable list.
        let vm = MapViewModel::new().with(
            "Users",
            Value::from(vec![Row::new().with("Text", Value::Str("Alice".to_string())), Row::new().with("Text", Value::Str("Bob".to_string()))]),
        );
        assert_eq!(resolve_item_labels(&vm, &node.items_source), Some(vec!["Alice".to_string(), "Bob".to_string()]));
    }

    #[test]
    fn list_box_with_items_source_binding_compiles() {
        let src = r#"<ListBox ItemsSource="{Binding Users}" SelectedIndex="0"/>"#;
        assert!(compile(src).is_ok());
    }

    #[test]
    fn data_table_with_items_source_binding_compiles() {
        let src = r#"
            <DataTable ItemsSource="{Binding Users}" OnSortChanged="users_sorted">
              <Column Header="Nom" Binding="{Binding Name}"/>
              <Column Header="Rôle" Binding="{Binding Role}"/>
            </DataTable>
        "#;
        assert!(compile(src).is_ok());
    }

    #[test]
    fn list_box_with_static_items_compiles() {
        let src = r#"
            <ListBox SelectionMode="MultiSimple" SelectedIndex="0">
              <Item Text="Alpha"/>
              <Item Text="Beta"/>
            </ListBox>
        "#;
        assert!(compile(src).is_ok());
    }

    #[test]
    fn checked_list_box_with_bound_selection_compiles() {
        let src = r#"
            <CheckedListBox CheckOnClick="true"
                             SelectedIndex="{Binding Row, Mode=TwoWay}"
                             OnCheckedChanged="row_checked">
              <Item Text="Alpha"/>
            </CheckedListBox>
        "#;
        assert!(compile(src).is_ok());
    }

    #[test]
    fn list_view_with_columns_and_items_compiles() {
        let src = r#"
            <ListView MultiSelect="true">
              <Column Header="Nom" Binding="{Binding Name}" Width="200"/>
              <Column Header="Rôle" Binding="{Binding Role}"/>
              <Item Text="Alice"/>
              <Item Text="Bob"/>
            </ListView>
        "#;
        assert!(compile(src).is_ok());
    }

    #[test]
    fn tree_view_with_nested_items_compiles() {
        let src = r#"
            <TreeView SelectedPath="0.0" OnActivate="node_opened">
              <Item Text="Racine">
                <Item Text="Enfant A"/>
                <Item Text="Enfant B"/>
              </Item>
            </TreeView>
        "#;
        assert!(compile(src).is_ok());
    }

    #[test]
    fn data_table_with_sortable_columns_compiles() {
        let src = r#"
            <DataTable ItemsSource="{Binding Users}" OnSortChanged="users_sorted">
              <Column Header="Utilisateur" Binding="{Binding Name}"/>
              <Column Header="Rôle" Binding="{Binding Role}" Width="140"/>
            </DataTable>
        "#;
        assert!(compile(src).is_ok());
    }

    #[test]
    fn month_calendar_with_bound_date_compiles() {
        let src = r#"<MonthCalendar SelectedDate="{Binding Day, Mode=TwoWay}" OnDateSelected="day_picked"/>"#;
        assert!(compile(src).is_ok());
    }

    #[test]
    fn column_id_prefers_the_binding_path_over_the_header() {
        assert_eq!(column_id("{Binding Name}", "Nom", 0), "Name");
        assert_eq!(column_id("{Binding Name, Mode=TwoWay}", "Nom", 0), "Name");
        assert_eq!(column_id("", "Nom", 0), "Nom");
        assert_eq!(column_id("", "", 3), "col3");
    }

    #[test]
    fn iso_date_round_trips() {
        let d = parse_iso_date("2026-09-25").expect("a well-formed ISO date parses");
        assert_eq!((d.year, d.month, d.day), (2026, 9, 25));
        assert_eq!(format_iso_date(d), "2026-09-25");
        // Un-padded numbers still parse (`u8`/`i32::parse` do not require
        // leading zeros); `format_iso_date` is what re-pads them.
        let unpadded = parse_iso_date("2026-9-5").expect("un-padded numbers still parse");
        assert_eq!(format_iso_date(unpadded), "2026-09-05");
        assert!(parse_iso_date("not-a-date").is_none());
        assert!(parse_iso_date("2026-09-25-extra").is_none());
    }

    #[test]
    fn node_path_round_trips_through_its_text_encoding() {
        let path: NodePath = vec![0, 2, 1];
        let text = path_to_string(&path);
        assert_eq!(text, "0.2.1");
        assert_eq!(parse_path(&text), Some(path));
        assert_eq!(parse_path(""), None);
        assert_eq!(parse_path("0.x.1"), None);
    }

    #[test]
    fn selection_mode_parses_the_four_declared_variants() {
        assert_eq!(parse_selection_mode("None"), SelectionMode::None);
        assert_eq!(parse_selection_mode("One"), SelectionMode::One);
        assert_eq!(parse_selection_mode("MultiSimple"), SelectionMode::MultiSimple);
        assert_eq!(parse_selection_mode("MultiExtended"), SelectionMode::MultiExtended);
        assert_eq!(parse_selection_mode("Nope"), SelectionMode::One);
    }
}

#[cfg(test)]
#[path = "data_edit_tests.rs"]
mod edit_tests;
