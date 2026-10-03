//! `<BindingSource>` (`vskubuno/docs/DATA.md` §5): the currency manager between a [`Table`] and the
//! controls bound to it — a view of the table (filter, sort, deleted rows hidden, a detail list's
//! relation to its master), a position and its current row, an edit buffer (begin/end/cancel edit,
//! a new row being added), paging, and the WinForms events (`ListChanged`, `CurrentChanged`,
//! `PositionChanged`, `CurrentItemChanged`, `AddingNew`, `RowValidating`, `DataError`), raised on the
//! UI thread.
//!
//! It is a [`BindingProvider`] (DATA-2): in a view, `{Binding Source=customers, Path=Name}` reaches
//! it through the view runtime, with typed conversions (`FormatString`, `NullValue`, culture), and
//! the `.kbview` handlers of its cancelable events run synchronously.
//!
//! Bindings write through [`BindingSource::set_field`] on every keystroke: a value that converts to
//! the column's type is stored at once; one that does not (`"abc"` in an integer column) is kept as
//! the field's *proposed text* — the bound text box keeps showing it — and becomes a column error.
//! Validation of the row (`NOT NULL`, lengths, `RowValidating`) runs when the edit ends.
//!
//! **Master/detail** (DATA-3): `<BindingSource x:Name="orders" DataSource="customers"
//! DataMember="customer_id = id" TableAdapter="ordersAdapter"/>` shows the orders whose
//! `customer_id` is the current customer's `id`, follows the master's current row, and gives the
//! rows it adds that key.

use std::cell::RefCell;
use std::collections::BTreeMap;

use kubuno_views::binding::{BindingFormat, Row, Value};
use kubuno_views::format::ValueKind;
use kubuno_views::prelude::*;
use kubuno_views::scope::{BindingProvider, ComponentScope};

use crate::error::{DataError, Joined};
use crate::events::{emit, AddingNewEventArgs, DataErrorEventArgs, ListChangedEventArgs, ListChangedType, RowValidatingEventArgs};
use crate::filter::{Filter, SortSpec};
use crate::rt::Progress;
use crate::table::{RowState, Table};
use crate::value::{from_bound, to_bound, DbKind, DbValue};

/// The edit in progress on the current row.
#[derive(Debug, Clone)]
struct EditState {
    row_id: u64,
    /// The values before the edit (restored by `cancel_edit`).
    snapshot: Vec<DbValue>,
    /// Column → text that did not convert.
    proposed: BTreeMap<usize, String>,
    /// A row created by `add_new` (dropped by `cancel_edit`).
    is_new: bool,
}

/// A detail list's relation to its master (`DataMember="customer_id = id"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    /// The detail's column holding the master's key.
    pub child: String,
    /// The master's column (empty: its primary key).
    pub parent: String,
}

impl Relation {
    /// Parses `child = parent` (qualifiers such as `orders.customer_id = customers.id` are
    /// dropped); a single name is the child column, the parent being the master's key.
    pub fn parse(text: &str) -> Option<Relation> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let last = |s: &str| s.trim().rsplit('.').next().unwrap_or("").trim().to_string();
        let (child, parent) = match text.split_once('=') {
            Some((c, p)) => (last(c), last(p)),
            None => (last(text), String::new()),
        };
        (!child.is_empty()).then_some(Relation { child, parent })
    }
}

/// What a fill of a binding source asks its adapter for (see `crate::ops`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FillRequest {
    /// The page to read (paging), 0-based.
    pub page: u32,
    /// The parameters of a detail's parameterized select (`@customer_id` = the master's key).
    pub params: Vec<(String, DbValue)>,
}

/// `<BindingSource>` (see the module doc).
#[derive(Component)]
#[kubuno(extends = Component, overrides(Component))]
#[toolbox(icon = "list", category = "Data")]
#[default_event("CurrentChanged")]
#[default_property("DataSource")]
pub struct BindingSource {
    base: ComponentCore,
    /// The x:Name of the TableAdapter that fills this binding source and saves its changes — or, for a detail list, of its master BindingSource.
    #[property]
    #[category("Data")]
    pub data_source: String,
    /// For a detail list: the relation to the master, detail column = master column (for example customer_id = id).
    #[property]
    #[category("Data")]
    pub data_member: String,
    /// For a detail list: the x:Name of the TableAdapter that fills it and saves its changes.
    #[property]
    #[category("Data")]
    pub table_adapter: String,
    /// Shows only the rows that match, for example Name LIKE 'A%' AND Age >= 18.
    #[property]
    #[category("Data")]
    pub filter: String,
    /// The order of the rows, for example Name ASC, Age DESC.
    #[property]
    #[category("Data")]
    pub sort: String,
    /// Whether new rows can be added.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub allow_new: bool,
    /// Whether rows can be edited.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub allow_edit: bool,
    /// Whether rows can be deleted.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub allow_remove: bool,
    /// Fills the list when the view is shown (what the Fill call Windows Forms adds to Form_Load
    /// does), through the TableAdapter named by DataSource. A detail list follows its master instead.
    #[property]
    #[category("Data")]
    pub auto_fill: bool,
    /// Occurs when the list changes: rows added, deleted, changed, or everything reloaded.
    #[event]
    #[category("Data")]
    pub list_changed: Event<ListChangedEventArgs>,
    /// Occurs when the current row changes.
    #[event]
    #[category("Data")]
    pub current_changed: Event<EmptyEventArgs>,
    /// Occurs when the position changes.
    #[event]
    #[category("Data")]
    pub position_changed: Event<EmptyEventArgs>,
    /// Occurs when a value of the current row is committed.
    #[event]
    #[category("Data")]
    pub current_item_changed: Event<EmptyEventArgs>,
    /// Occurs before a new row is added; set Cancel to refuse it.
    #[event]
    #[category("Data")]
    pub adding_new: Event<AddingNewEventArgs>,
    /// Occurs when an edited row is committed; add errors or set Cancel to keep it in edit.
    #[event]
    #[category("Data")]
    pub row_validating: Event<RowValidatingEventArgs>,
    /// Occurs when an operation fails (a fill, a save, an edit that does not validate).
    #[event]
    #[category("Data")]
    pub data_error: Event<DataErrorEventArgs>,
    table: Table,
    /// Row ids in view order.
    view: Vec<u64>,
    position: i32,
    edit: Option<EditState>,
    applied_filter: Option<(String, Option<Filter>)>,
    applied_sort: Option<(String, SortSpec)>,
    last_error: Option<(String, String)>,
    version: u64,
    list_cache: RefCell<Option<(u64, Value)>>,
    /// A detail list: the master's current key (`None` inside: the master has no current row);
    /// `None`: not following a master yet.
    master_key: Option<Option<DbValue>>,
    /// Paging: the page shown (0-based), the rows in all pages (when counted), the page size.
    page: u32,
    total: Option<u64>,
    /// Keyset paging: the key each known page starts after (`[0]` is `None`, the first page).
    page_after: Vec<Option<DbValue>>,
    page_size: u32,
    /// A page or a refill asked for (a bound `PageIndex`, a parameterized detail's new master key).
    fill_request: Option<FillRequest>,
    /// A fill or a save is running; how many rows it read.
    busy: bool,
    progress: Progress,
    /// `AutoFill` already asked for its fill (once per instance; a hot reload keeps the rows).
    auto_filled: bool,
}

impl Default for BindingSource {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            data_source: String::new(),
            data_member: String::new(),
            table_adapter: String::new(),
            filter: String::new(),
            sort: String::new(),
            allow_new: true,
            allow_edit: true,
            allow_remove: true,
            auto_fill: false,
            auto_filled: false,
            list_changed: Event::default(),
            current_changed: Event::default(),
            position_changed: Event::default(),
            current_item_changed: Event::default(),
            adding_new: Event::default(),
            row_validating: Event::default(),
            data_error: Event::default(),
            table: Table::default(),
            view: Vec::new(),
            position: -1,
            edit: None,
            applied_filter: None,
            applied_sort: None,
            last_error: None,
            version: 0,
            list_cache: RefCell::new(None),
            master_key: None,
            page: 0,
            total: None,
            page_size: 0,
            page_after: vec![None],
            fill_request: None,
            busy: false,
            progress: Progress::new(),
        }
    }
}

const CLASS: &str = "BindingSource";

/// The next temporary key of a new row (negative, ADO.NET's `AutoIncrementSeed = -1`,
/// `AutoIncrementStep = -1`), unique in the process: a master's and its details' temporary keys
/// never collide in a hierarchical save.
static NEXT_TEMP_KEY: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);

impl Component for BindingSource {
    fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
        Some(self)
    }
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
        Some(self)
    }
}

impl BindingSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// The component's name (its `x:Name`).
    pub fn name(&self) -> &str {
        crate::events::name_of(&self.base)
    }

    /// Names the component outside a view.
    pub fn set_name(&mut self, name: impl Into<String>) -> &mut Self {
        crate::events::set_name_of(&mut self.base, name);
        self
    }

    /// The x:Name of the adapter that fills and saves this list: `TableAdapter`, else `DataSource`
    /// (unless this is a detail list).
    pub fn adapter_name(&self) -> &str {
        if !self.table_adapter.trim().is_empty() {
            self.table_adapter.trim()
        } else if self.relation().is_some() {
            ""
        } else {
            self.data_source.trim()
        }
    }

    /// The relation to the master of a detail list (`DataMember`).
    pub fn relation(&self) -> Option<Relation> {
        if self.data_source.trim().is_empty() {
            return None;
        }
        Relation::parse(&self.data_member)
    }

    // ── Raising ──────────────────────────────────────────────────────────────────────────────

    fn emit_list_changed(&mut self, kind: ListChangedType, new_index: i32, old_index: i32) {
        let e = self.list_changed.clone();
        emit(&mut self.base, CLASS, &e, "OnListChanged", ListChangedEventArgs { list_changed_type: kind, new_index, old_index });
    }

    fn emit_empty(&mut self, which: &'static str) {
        let e = match which {
            "OnCurrentChanged" => self.current_changed.clone(),
            "OnPositionChanged" => self.position_changed.clone(),
            _ => self.current_item_changed.clone(),
        };
        emit(&mut self.base, CLASS, &e, which, EmptyEventArgs);
    }

    /// Raises `DataError` and keeps the message for the ErrorProvider's summary.
    pub fn report_error(&mut self, column: &str, message: impl Into<String>) {
        let message = message.into();
        self.last_error = Some((column.to_string(), message.clone()));
        self.touch();
        let e = self.data_error.clone();
        emit(&mut self.base, CLASS, &e, "OnDataError", DataErrorEventArgs { message, column: column.to_string() });
    }

    /// The last reported error (`(column, message)`), cleared by a successful fill, save or edit.
    pub fn last_error(&self) -> Option<&(String, String)> {
        self.last_error.as_ref()
    }

    pub fn clear_last_error(&mut self) {
        if self.last_error.take().is_some() {
            self.touch();
        }
    }

    fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    /// A counter that changes with every change visible to bindings.
    pub fn version(&self) -> u64 {
        self.version
    }

    // ── Data ─────────────────────────────────────────────────────────────────────────────────

    pub fn table(&self) -> &Table {
        &self.table
    }

    /// The table, for direct changes; call [`Self::reset_bindings`] afterwards.
    pub fn table_mut(&mut self) -> &mut Table {
        self.touch();
        &mut self.table
    }

    /// Replaces the table (a fill): any edit is dropped, the position goes to the first row.
    pub fn load(&mut self, table: Table) {
        self.edit = None;
        self.table = table;
        self.last_error = None;
        self.busy = false;
        self.rebuild_view(None);
        let old = self.position;
        self.position = if self.view.is_empty() { -1 } else { 0 };
        self.touch();
        self.emit_list_changed(ListChangedType::Reset, -1, -1);
        if old != self.position {
            self.emit_empty("OnPositionChanged");
        }
        self.emit_empty("OnCurrentChanged");
    }

    /// Replaces the table with page `page` of a paged fill (`total`: the rows in all pages).
    pub fn load_page(&mut self, table: Table, page: u32, page_size: u32, total: Option<u64>) {
        self.page = page;
        self.page_size = page_size;
        self.total = total;
        self.load(table);
    }

    /// Re-applies the filter and sort and raises `ListChanged(Reset)`, keeping the current row when
    /// it is still visible (after direct changes to the table, or a changed `Filter`/`Sort`).
    pub fn reset_bindings(&mut self) {
        let current = self.current_id();
        self.rebuild_view(current);
        self.touch();
        self.emit_list_changed(ListChangedType::Reset, -1, -1);
        self.emit_empty("OnCurrentChanged");
    }

    /// Sets and applies the filter (validated: a malformed expression or an unknown column is an
    /// error, and the previous filter stays).
    pub fn set_filter(&mut self, filter: &str) -> Result<(), DataError> {
        let parsed = Filter::parse(filter)?;
        if let Some(f) = &parsed {
            self.check_columns(f.columns())?;
        }
        self.filter = filter.to_string();
        self.applied_filter = Some((filter.to_string(), parsed));
        self.reset_bindings();
        Ok(())
    }

    /// Sets and applies the sort (validated like the filter).
    pub fn set_sort(&mut self, sort: &str) -> Result<(), DataError> {
        let spec = SortSpec::parse(sort)?;
        self.check_columns(spec.0.iter().map(|(c, _)| c.as_str()).collect())?;
        self.sort = sort.to_string();
        self.applied_sort = Some((sort.to_string(), spec));
        self.reset_bindings();
        Ok(())
    }

    fn check_columns(&self, columns: Vec<&str>) -> Result<(), DataError> {
        if self.table.columns.is_empty() {
            return Ok(()); // Not filled yet: checked when the rows arrive.
        }
        match columns.into_iter().find(|c| self.table.column_index(c).is_none()) {
            Some(c) => Err(DataError::Validation(format!("there is no column `{c}`"))),
            None => Ok(()),
        }
    }

    /// Whether the filter or the sort text changed since it was applied (a bound `Filter`).
    fn view_settings_changed(&self) -> bool {
        self.applied_filter.as_ref().map(|(t, _)| t.as_str()) != Some(self.filter.as_str()) || self.applied_sort.as_ref().map(|(t, _)| t.as_str()) != Some(self.sort.as_str())
    }

    /// Rebuilds the view (relation, filter, sort) and puts the position on `keep` when it is visible.
    fn rebuild_view(&mut self, keep: Option<u64>) {
        // The properties may have been set directly (from the view's XML): parse what changed.
        if self.applied_filter.as_ref().map(|(t, _)| t.as_str()) != Some(self.filter.as_str()) {
            let text = self.filter.clone();
            let parsed = match Filter::parse(&text).and_then(|f| {
                if let Some(f) = &f {
                    self.check_columns(f.columns())?;
                }
                Ok(f)
            }) {
                Ok(f) => f,
                Err(e) => {
                    self.report_error("", format!("Filter ignored: {e}"));
                    None
                }
            };
            self.applied_filter = Some((text, parsed));
        }
        if self.applied_sort.as_ref().map(|(t, _)| t.as_str()) != Some(self.sort.as_str()) {
            let text = self.sort.clone();
            let spec = match SortSpec::parse(&text).and_then(|s| {
                self.check_columns(s.0.iter().map(|(c, _)| c.as_str()).collect())?;
                Ok(s)
            }) {
                Ok(s) => s,
                Err(e) => {
                    self.report_error("", format!("Sort ignored: {e}"));
                    SortSpec::default()
                }
            };
            self.applied_sort = Some((text, spec));
        }
        let filter = self.applied_filter.as_ref().and_then(|(_, f)| f.clone());
        let sort = self.applied_sort.as_ref().map(|(_, s)| s.clone()).unwrap_or_default();
        // A detail list shows the rows of its master's current row only.
        let relation = match (&self.master_key, self.relation()) {
            (Some(key), Some(rel)) => Some((self.table.column_index(&rel.child), key.clone())),
            // Not following its master yet: nothing to show.
            (None, Some(rel)) => Some((self.table.column_index(&rel.child), None)),
            _ => None,
        };
        let table = &self.table;
        let editing_new = self.edit.as_ref().filter(|e| e.is_new).map(|e| e.row_id);
        let mut ids: Vec<u64> = table
            .rows()
            .iter()
            .filter(|r| r.state != RowState::Deleted && Some(r.id) != editing_new)
            .filter(|r| match &relation {
                None => true,
                Some((Some(c), Some(key))) => r.values.get(*c).is_some_and(|v| !v.is_null() && v.sort_cmp(key) == std::cmp::Ordering::Equal),
                Some(_) => false,
            })
            .filter(|r| {
                filter.as_ref().is_none_or(|f| {
                    let get = |c: &str| table.column_index(c).and_then(|i| r.values.get(i));
                    f.matches(&get)
                })
            })
            .map(|r| r.id)
            .collect();
        if !sort.is_empty() {
            let keys: Vec<(usize, bool)> = sort.0.iter().filter_map(|(c, d)| table.column_index(c).map(|i| (i, *d))).collect();
            ids.sort_by(|a, b| {
                let (ra, rb) = (table.row_by_id(*a), table.row_by_id(*b));
                for &(i, desc) in &keys {
                    let va = ra.and_then(|r| r.values.get(i)).unwrap_or(&DbValue::Null);
                    let vb = rb.and_then(|r| r.values.get(i)).unwrap_or(&DbValue::Null);
                    let ord = va.sort_cmp(vb);
                    let ord = if desc { ord.reverse() } else { ord };
                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                }
                std::cmp::Ordering::Equal
            });
        }
        // A row being added is shown at the end, whatever the filter and sort (WinForms).
        if let Some(id) = editing_new {
            ids.push(id);
        }
        self.view = ids;
        self.position = match keep.and_then(|id| self.view.iter().position(|v| *v == id)) {
            Some(p) => p as i32,
            None if self.view.is_empty() => -1,
            None => self.position.clamp(0, self.view.len() as i32 - 1),
        };
    }

    // ── Master/detail ────────────────────────────────────────────────────────────────────────

    /// A detail list follows its master's current key (`None`: the master has no current row).
    /// Returns whether the key changed (the view was rebuilt, `ListChanged(Reset)` raised).
    pub fn set_master_key(&mut self, key: Option<DbValue>) -> bool {
        if self.master_key.as_ref() == Some(&key) {
            return false;
        }
        if self.edit.is_some() && self.end_edit().is_err() {
            // The master moved away: the detail row that does not validate goes back.
            self.cancel_edit();
        }
        self.master_key = Some(key);
        self.rebuild_view(None);
        self.position = if self.view.is_empty() { -1 } else { 0 };
        self.touch();
        self.emit_list_changed(ListChangedType::Reset, -1, -1);
        self.emit_empty("OnPositionChanged");
        self.emit_empty("OnCurrentChanged");
        true
    }

    /// The master key this detail list follows.
    pub fn master_key(&self) -> Option<&DbValue> {
        self.master_key.as_ref().and_then(Option::as_ref)
    }

    /// Replaces a temporary key by the key the database gave (a saved master): in the rows' column
    /// `column`, and in the master key this list follows.
    pub(crate) fn remap_keys(&mut self, column: Option<usize>, map: &[(DbValue, DbValue)]) {
        if map.is_empty() {
            return;
        }
        if let Some(c) = column {
            self.table.remap_column(c, map);
        }
        if let Some(Some(k)) = &self.master_key {
            if let Some((_, real)) = map.iter().find(|(t, _)| t == k) {
                self.master_key = Some(Some(real.clone()));
            }
        }
        let current = self.current_id();
        self.rebuild_view(current);
        self.touch();
    }

    // ── Paging and fills ─────────────────────────────────────────────────────────────────────

    /// The page shown (0-based) and the number of pages (`None`: not counted).
    pub fn page(&self) -> (u32, Option<u32>) {
        let count = match (self.total, self.page_size) {
            (Some(t), s) if s > 0 => Some(u32::try_from(t.div_ceil(u64::from(s))).unwrap_or(u32::MAX).max(1)),
            _ => None,
        };
        (self.page, count)
    }

    /// Asks for page `page` (a fill follows: the view runtime starts it, or the application with
    /// `fill`). Refused while rows have unsaved changes.
    pub fn request_page(&mut self, page: u32) -> Result<(), DataError> {
        if self.has_changes() {
            return Err(DataError::Validation("save or cancel the changes before changing the page".to_string()));
        }
        let page = match self.page().1 {
            Some(count) => page.min(count.saturating_sub(1)),
            None => page,
        };
        if page != self.page || self.table.columns.is_empty() {
            let params = self.fill_request.take().map(|r| r.params).unwrap_or_default();
            self.fill_request = Some(FillRequest { page, params });
            self.touch();
        }
        Ok(())
    }

    /// Keyset paging: the page that can be read for `page` (pages are read in order: at most one
    /// past the last one read) and the key it starts after.
    pub fn keyset_start(&self, page: u32) -> (u32, Option<DbValue>) {
        let last = self.page_after.len().saturating_sub(1);
        let p = (page as usize).min(last);
        (p as u32, self.page_after.get(p).cloned().flatten())
    }

    /// Keyset paging: page `page` was read; the next one starts after `next` (its last key).
    pub fn record_keyset(&mut self, page: u32, next: Option<DbValue>) {
        self.page_after.truncate(page as usize + 1);
        if let Some(n) = next {
            self.page_after.push(Some(n));
        }
    }

    /// Takes the fill this binding source asked for (a page, a detail's new master key).
    pub fn take_fill_request(&mut self) -> Option<FillRequest> {
        self.fill_request.take()
    }

    /// The page a fill should read now (the requested one, else the current one).
    pub fn current_request(&self) -> FillRequest {
        self.fill_request.clone().unwrap_or(FillRequest { page: self.page, params: Vec::new() })
    }

    /// Marks a fill or a save as running (`IsBusy`), resetting the row counter.
    pub fn set_busy(&mut self, busy: bool) {
        if busy {
            self.progress.reset();
        }
        self.busy = busy;
        self.touch();
    }

    /// The counter a running fill adds its rows to (`RowsRead`).
    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    // ── Currency ─────────────────────────────────────────────────────────────────────────────

    /// The number of rows in the view.
    pub fn count(&self) -> usize {
        self.view.len()
    }

    /// The position of the current row in the view (`-1` when the view is empty).
    pub fn position(&self) -> i32 {
        self.position
    }

    fn current_id(&self) -> Option<u64> {
        usize::try_from(self.position).ok().and_then(|p| self.view.get(p)).copied()
    }

    /// The row id at view position `index`.
    pub fn row_id_at(&self, index: usize) -> Option<u64> {
        self.view.get(index).copied()
    }

    /// The current row's value of `column`.
    pub fn current_value(&self, column: &str) -> Option<&DbValue> {
        let id = self.current_id()?;
        self.table.value(id, column)
    }

    /// The current row's key (its first key column, or `column` when given): what a detail list
    /// follows.
    pub fn current_key(&self, column: &str) -> Option<DbValue> {
        let id = self.current_id()?;
        let c = if column.is_empty() { *self.table.key_columns().first()? } else { self.table.column_index(column)? };
        self.table.row_by_id(id)?.values.get(c).cloned()
    }

    /// Moves to `position` (clamped). The current edit is ended first: when it does not validate,
    /// the position does not change and the error is returned (and raised as `DataError`).
    pub fn set_position(&mut self, position: i32) -> Result<(), DataError> {
        if self.view.is_empty() {
            return Ok(());
        }
        let target = position.clamp(0, self.view.len() as i32 - 1);
        if target == self.position {
            return Ok(());
        }
        self.end_edit()?;
        // Ending an edit may have moved rows (sort): clamp again.
        let target = target.clamp(0, (self.view.len() as i32 - 1).max(0));
        self.position = if self.view.is_empty() { -1 } else { target };
        self.touch();
        self.emit_empty("OnPositionChanged");
        self.emit_empty("OnCurrentChanged");
        Ok(())
    }

    pub fn move_first(&mut self) -> Result<(), DataError> {
        self.set_position(0)
    }
    pub fn move_previous(&mut self) -> Result<(), DataError> {
        self.set_position((self.position - 1).max(0))
    }
    pub fn move_next(&mut self) -> Result<(), DataError> {
        self.set_position(self.position + 1)
    }
    pub fn move_last(&mut self) -> Result<(), DataError> {
        self.set_position(self.view.len() as i32 - 1)
    }

    // ── Editing ──────────────────────────────────────────────────────────────────────────────

    /// Whether an edit is in progress.
    pub fn is_editing(&self) -> bool {
        self.edit.is_some()
    }

    /// Whether there is anything to save (an edit in progress counts).
    pub fn has_changes(&self) -> bool {
        self.table.has_changes() || self.edit.is_some()
    }

    /// Begins an edit of the current row (implicit on the first [`Self::set_field`]).
    pub fn begin_edit(&mut self) -> Result<(), DataError> {
        if self.edit.is_some() {
            return Ok(());
        }
        let id = self.current_id().ok_or_else(|| DataError::Validation("there is no current row".to_string()))?;
        if !self.allow_edit {
            return Err(DataError::Validation("this list cannot be edited (AllowEdit is false)".to_string()));
        }
        let snapshot = self.table.row_by_id(id).map(|r| r.values.clone()).unwrap_or_default();
        self.edit = Some(EditState { row_id: id, snapshot, proposed: BTreeMap::new(), is_new: false });
        Ok(())
    }

    /// Writes a binding's value into `column` of the current row (see the module doc). A value that
    /// does not convert is kept as proposed text and becomes a column error (`Ok`: the binding did
    /// its job; the error shows through the ErrorProvider).
    pub fn set_field(&mut self, column: &str, value: &Value) -> Result<(), DataError> {
        self.set_field_formatted(column, value, &BindingFormat::default())
    }

    /// [`Self::set_field`] for a typed binding: the text is parsed with the binding's format and
    /// culture (`1 234,50`, `10/12/1815`), `NullValue` writes NULL (DATA-2).
    pub fn set_field_formatted(&mut self, column: &str, value: &Value, format: &BindingFormat) -> Result<(), DataError> {
        let c = self.table.column_index(column).ok_or_else(|| DataError::Validation(format!("there is no column `{column}`")))?;
        if self.table.columns[c].read_only {
            return Err(DataError::Validation(format!("the column `{column}` is read-only")));
        }
        // Writing back what is already shown is not an edit (a two-way binding echoing its value).
        if self.edit.is_none() && self.display_value(c, ValueKind::Any, format) == Some(value.clone()) {
            return Ok(());
        }
        self.begin_edit()?;
        let Some(edit) = self.edit.as_mut() else { return Ok(()) };
        let kind = self.table.columns[c].ty.kind;
        let row_id = edit.row_id;
        match from_bound(value, kind, format) {
            Ok(v) => {
                edit.proposed.remove(&c);
                if let Some(row) = self.table.row_by_id_mut(row_id) {
                    row.column_errors.remove(&c);
                    if let Some(slot) = row.values.get_mut(c) {
                        *slot = v;
                    }
                }
            }
            Err(message) => {
                let text = match value {
                    Value::Str(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                edit.proposed.insert(c, text);
                if let Some(row) = self.table.row_by_id_mut(row_id) {
                    row.column_errors.insert(c, message);
                }
            }
        }
        self.touch();
        Ok(())
    }

    /// The value a binding reads for column `c` of the current row (the proposed text first).
    fn display_value(&self, c: usize, want: ValueKind, format: &BindingFormat) -> Option<Value> {
        let id = self.current_id()?;
        if let Some(edit) = &self.edit {
            if edit.row_id == id {
                if let Some(text) = edit.proposed.get(&c) {
                    return match want {
                        ValueKind::Number | ValueKind::Bool => None,
                        _ => Some(Value::Str(text.clone())),
                    };
                }
            }
        }
        let kind = self.table.columns.get(c)?.ty.kind;
        let v = self.table.row_by_id(id)?.values.get(c)?;
        if want == ValueKind::Any && format.is_empty() {
            return Some(v.to_view_value(kind));
        }
        to_bound(v, kind, want, format)
    }

    /// Ends the edit: validates the row (see the module doc) and commits it into the table. On
    /// failure the edit stays open, the errors are on the row's columns, `DataError` is raised.
    pub fn end_edit(&mut self) -> Result<(), DataError> {
        let Some(edit) = self.edit.clone() else { return Ok(()) };
        let Some(row) = self.table.row_by_id(edit.row_id).cloned() else {
            self.edit = None;
            return Ok(());
        };
        let mut errors: Vec<(String, String)> = row.column_errors.iter().filter_map(|(c, m)| self.table.columns.get(*c).map(|col| (col.name.clone(), m.clone()))).collect();
        for (c, col) in self.table.columns.iter().enumerate() {
            if row.column_errors.contains_key(&c) || col.read_only {
                continue;
            }
            let v = row.values.get(c).unwrap_or(&DbValue::Null);
            let required = !col.nullable && !(col.auto_increment && edit.is_new);
            if required && (v.is_null() || (col.ty.kind == DbKind::Text && matches!(v, DbValue::Text(s) if s.trim().is_empty()) && !col.primary_key)) {
                errors.push((col.name.clone(), "A value is required.".to_string()));
            } else if let (Some(max), DbValue::Text(s)) = (col.max_length, v) {
                if s.chars().count() > max {
                    errors.push((col.name.clone(), format!("At most {max} characters.")));
                }
            }
        }
        if errors.is_empty() {
            let args = RowValidatingEventArgs {
                row_index: self.position,
                values: self.table.columns.iter().zip(&row.values).map(|(c, v)| (c.name.clone(), v.clone())).collect(),
                errors: Vec::new(),
                cancel: false,
            };
            let e = self.row_validating.clone();
            let args = emit(&mut self.base, CLASS, &e, "OnRowValidating", args);
            errors.extend(args.errors);
            if args.cancel && errors.is_empty() {
                errors.push((String::new(), "The row was not accepted.".to_string()));
            }
        }
        if !errors.is_empty() {
            let resolved: Vec<(Option<usize>, String)> =
                errors.iter().map(|(col, m)| (if col.is_empty() { None } else { self.table.column_index(col) }, m.clone())).collect();
            if let Some(r) = self.table.row_by_id_mut(edit.row_id) {
                r.row_error = None;
                for (c, message) in resolved {
                    match c {
                        Some(c) => {
                            r.column_errors.insert(c, message);
                        }
                        None => r.row_error = Some(message),
                    }
                }
            }
            let summary = Joined(&errors).to_string();
            let first_column = errors.first().map(|(c, _)| c.clone()).unwrap_or_default();
            self.report_error(&first_column, summary.clone());
            return Err(DataError::Validation(summary));
        }
        // Commit.
        let old_position = self.position;
        if let Some(r) = self.table.row_by_id_mut(edit.row_id) {
            r.column_errors.clear();
            r.row_error = None;
            r.state = match r.state {
                RowState::Detached => RowState::Added,
                RowState::Unchanged if r.values != edit.snapshot => RowState::Modified,
                s => s,
            };
        }
        self.edit = None;
        self.last_error = None;
        // A committed new row takes its place in the filter/sort.
        self.rebuild_view(Some(edit.row_id));
        self.touch();
        if edit.is_new {
            self.emit_list_changed(ListChangedType::ItemAdded, self.position, -1);
        } else {
            self.emit_list_changed(ListChangedType::ItemChanged, self.position, old_position);
        }
        self.emit_empty("OnCurrentItemChanged");
        if old_position != self.position {
            self.emit_empty("OnPositionChanged");
        }
        Ok(())
    }

    /// Cancels the edit: the row gets its values back; a row being added disappears.
    pub fn cancel_edit(&mut self) {
        let Some(edit) = self.edit.take() else { return };
        if edit.is_new {
            self.table.remove_row(edit.row_id);
            let old = self.position;
            self.rebuild_view(None);
            self.position = if self.view.is_empty() { -1 } else { (old - 1).clamp(0, self.view.len() as i32 - 1) };
            self.touch();
            self.emit_list_changed(ListChangedType::ItemDeleted, -1, old);
            self.emit_empty("OnPositionChanged");
            self.emit_empty("OnCurrentChanged");
        } else {
            if let Some(r) = self.table.row_by_id_mut(edit.row_id) {
                r.values = edit.snapshot;
                r.column_errors.clear();
                r.row_error = None;
            }
            self.last_error = None;
            self.touch();
            self.emit_list_changed(ListChangedType::ItemChanged, self.position, self.position);
            self.emit_empty("OnCurrentItemChanged");
        }
    }

    /// Adds a new row in edit mode at the end of the view and moves to it (WinForms `AddNew`). A
    /// generated key gets a temporary negative value until the row is saved; a detail row gets its
    /// master's key.
    pub fn add_new(&mut self) -> Result<(), DataError> {
        if !self.allow_new {
            return Err(DataError::Validation("rows cannot be added (AllowNew is false)".to_string()));
        }
        if self.table.columns.is_empty() {
            return Err(DataError::Validation("the list has no columns yet (fill it first)".to_string()));
        }
        let relation = self.relation();
        if relation.is_some() && self.master_key().is_none() {
            return Err(DataError::Validation("the master list has no current row".to_string()));
        }
        self.end_edit()?;
        let e = self.adding_new.clone();
        let args = emit(&mut self.base, CLASS, &e, "OnAddingNew", AddingNewEventArgs::default());
        if args.cancel {
            return Err(DataError::Validation("adding a row was cancelled".to_string()));
        }
        let id = self.table.add_detached();
        let keys = self.table.key_columns();
        // Defaults: false for booleans, empty text for required text columns, a temporary key.
        let mut defaults: Vec<DbValue> = self
            .table
            .columns
            .iter()
            .enumerate()
            .map(|(i, col)| match col.ty.kind {
                DbKind::Int if keys == [i] && col.auto_increment => {
                    let k = NEXT_TEMP_KEY.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    DbValue::Int(k)
                }
                DbKind::Bool if !col.nullable => DbValue::Bool(false),
                DbKind::Text if !col.nullable && !col.primary_key => DbValue::Text(String::new()),
                _ => DbValue::Null,
            })
            .collect();
        if let (Some(rel), Some(key)) = (&relation, self.master_key().cloned()) {
            if let Some(c) = self.table.column_index(&rel.child) {
                defaults[c] = key;
            }
        }
        if let Some(row) = self.table.row_by_id_mut(id) {
            row.values = defaults;
        }
        let snapshot = self.table.row_by_id(id).map(|r| r.values.clone()).unwrap_or_default();
        self.edit = Some(EditState { row_id: id, snapshot, proposed: BTreeMap::new(), is_new: true });
        self.rebuild_view(Some(id));
        self.touch();
        self.emit_list_changed(ListChangedType::ItemAdded, self.position, -1);
        self.emit_empty("OnPositionChanged");
        self.emit_empty("OnCurrentChanged");
        Ok(())
    }

    /// Deletes the current row (a row being added is simply dropped).
    pub fn remove_current(&mut self) -> Result<(), DataError> {
        if !self.allow_remove {
            return Err(DataError::Validation("rows cannot be deleted (AllowRemove is false)".to_string()));
        }
        if self.edit.as_ref().is_some_and(|e| e.is_new) {
            self.cancel_edit();
            return Ok(());
        }
        let Some(id) = self.current_id() else { return Ok(()) };
        if let Some(edit) = self.edit.take() {
            if let Some(r) = self.table.row_by_id_mut(edit.row_id) {
                r.values = edit.snapshot;
                r.column_errors.clear();
            }
        }
        let old = self.position;
        self.table.delete_row(id);
        self.rebuild_view(None);
        self.position = if self.view.is_empty() { -1 } else { old.clamp(0, self.view.len() as i32 - 1) };
        self.last_error = None;
        self.touch();
        self.emit_list_changed(ListChangedType::ItemDeleted, -1, old);
        self.emit_empty("OnPositionChanged");
        self.emit_empty("OnCurrentChanged");
        Ok(())
    }

    /// After a save: keeps the current row, reapplies filter and sort, raises `ListChanged(Reset)`.
    pub(crate) fn after_save(&mut self) {
        self.last_error = None;
        self.busy = false;
        let current = self.current_id();
        self.rebuild_view(current);
        self.touch();
        self.emit_list_changed(ListChangedType::Reset, -1, -1);
        self.emit_empty("OnCurrentItemChanged");
    }

    // ── Errors ───────────────────────────────────────────────────────────────────────────────

    /// The error of `column` on the current row.
    pub fn column_error(&self, column: &str) -> Option<&str> {
        let c = self.table.column_index(column)?;
        let id = self.current_id()?;
        self.table.row_by_id(id)?.column_errors.get(&c).map(String::as_str)
    }

    /// Every error of the current row, `(column, message)` (an empty column for the row's own).
    pub fn current_errors(&self) -> Vec<(String, String)> {
        let Some(row) = self.current_id().and_then(|id| self.table.row_by_id(id)) else { return Vec::new() };
        let mut out: Vec<(String, String)> = row.column_errors.iter().filter_map(|(c, m)| self.table.columns.get(*c).map(|col| (col.name.clone(), m.clone()))).collect();
        if let Some(e) = &row.row_error {
            out.push((String::new(), e.clone()));
        }
        out
    }

    // ── Bindings ─────────────────────────────────────────────────────────────────────────────

    /// The rows of the view as the binding engine's list (cached until the next change).
    pub fn list_value(&self) -> Value {
        if let Some((v, value)) = self.list_cache.borrow().as_ref() {
            if *v == self.version {
                return value.clone();
            }
        }
        let rows: Vec<Row> = self
            .view
            .iter()
            .filter_map(|id| self.table.row_by_id(*id))
            .map(|r| {
                let mut row = Row::new();
                for (c, col) in self.table.columns.iter().enumerate() {
                    let v = match self.edit.as_ref().filter(|e| e.row_id == r.id).and_then(|e| e.proposed.get(&c)) {
                        Some(text) => Value::Str(text.clone()),
                        None => r.values.get(c).map_or(Value::Str(String::new()), |v| v.to_view_value(col.ty.kind)),
                    };
                    row = row.with(col.name.clone(), v);
                }
                row
            })
            .collect();
        let value = Value::from(rows);
        *self.list_cache.borrow_mut() = Some((self.version, value.clone()));
        value
    }

    /// What a binding path below this source reads (`vskubuno/docs/DATA.md` §7): `""` the list,
    /// `Position`, `Count`, `PositionText`, `HasChanges`, `IsEditing`, `CanMovePrevious`,
    /// `CanMoveNext`, `HasCurrent`, `PageIndex`, `PageCount`, `PageText`, `TotalCount`,
    /// `CanPreviousPage`, `CanNextPage`, `IsBusy`, `RowsRead`, `Current.<Column>` or `<Column>`, and
    /// `Current.<Column>.Error` (the column's error on the current row, `""` without one).
    pub fn get_path(&self, path: &str) -> Option<Value> {
        self.get_path_typed(path, ValueKind::Any, &BindingFormat::default())
    }

    /// [`Self::get_path`] for a property of shape `want`, formatted per `format` (DATA-2).
    pub fn get_path_typed(&self, path: &str, want: ValueKind, format: &BindingFormat) -> Option<Value> {
        let (page, pages) = self.page();
        let simple = match path {
            "" | "List" => return Some(self.list_value()),
            "Position" => Value::F32(self.position as f32),
            "Count" => Value::F32(self.count() as f32),
            "PositionText" => Value::Str(if self.position < 0 { "0 / 0".to_string() } else { format!("{} / {}", self.position + 1, self.count()) }),
            "HasChanges" => Value::Bool(self.has_changes()),
            "IsEditing" => Value::Bool(self.is_editing()),
            "CanMovePrevious" => Value::Bool(self.position > 0),
            "CanMoveNext" => Value::Bool(self.position >= 0 && (self.position as usize) + 1 < self.count()),
            "HasCurrent" => Value::Bool(self.position >= 0),
            "PageIndex" => Value::F32(page as f32),
            "PageCount" => Value::F32(pages.unwrap_or(1) as f32),
            "PageText" => Value::Str(match pages {
                Some(n) => format!("{} / {n}", page + 1),
                None => format!("{}", page + 1),
            }),
            "TotalCount" => Value::F32(self.total.unwrap_or(self.count() as u64) as f32),
            "CanPreviousPage" => Value::Bool(page > 0),
            "CanNextPage" => Value::Bool(match pages {
                Some(n) => page + 1 < n,
                None => self.page_size > 0 && self.table.rows().len() as u32 >= self.page_size,
            }),
            "IsBusy" => Value::Bool(self.busy),
            "RowsRead" => Value::F32(self.progress.rows() as f32),
            _ => {
                let column = path.strip_prefix("Current.").unwrap_or(path);
                // `<Column>.Error`: the column's error on the current row, `""` without one (what
                // a DataTable shows as the error glyph of the cell — WinForms `ErrorText`).
                if let Some(field) = column.strip_suffix(".Error").filter(|f| self.table.column_index(f).is_some()) {
                    return Some(Value::Str(self.column_error(field).unwrap_or_default().to_string()));
                }
                let c = self.table.column_index(column)?;
                return match self.display_value(c, want, format) {
                    Some(v) => Some(v),
                    None if self.current_id().is_none() => match (want, self.table.columns[c].ty.kind) {
                        (ValueKind::Bool, _) | (ValueKind::Any, DbKind::Bool) => Some(Value::Bool(false)),
                        (ValueKind::Number, _) => None,
                        _ => Some(Value::Str(String::new())),
                    },
                    None => None,
                };
            }
        };
        if want == ValueKind::Any && format.is_empty() {
            Some(simple)
        } else {
            kubuno_views::format::to_target(simple, want, format)
        }
    }

    /// What a two-way binding writes below this source: `Position`, `PageIndex`, a column of the
    /// current row, or the commands `CancelEdit` / `EndEdit` (any value). Errors are reported (`DataError`) and returned.
    pub fn set_path(&mut self, path: &str, value: &Value) -> Result<(), DataError> {
        self.set_path_formatted(path, value, &BindingFormat::default())
    }

    /// [`Self::set_path`] for a typed binding (DATA-2).
    pub fn set_path_formatted(&mut self, path: &str, value: &Value, format: &BindingFormat) -> Result<(), DataError> {
        let number = |v: &Value| -> Result<f32, DataError> {
            match v {
                Value::F32(f) => Ok(*f),
                Value::Str(s) => s.trim().parse::<f32>().map_err(|_| DataError::Validation("not a number".to_string())),
                _ => Err(DataError::Validation("not a number".to_string())),
            }
        };
        let result = match path {
            "Position" => match number(value) {
                Ok(f) if f >= 0.0 => self.set_position(f.round() as i32),
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
            "PageIndex" => match number(value) {
                Ok(f) if f >= 0.0 => self.request_page(f.round() as u32),
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
            // Commands a control writes (a DataTable's second Escape, a grid ending its row edit):
            // any value runs them.
            "CancelEdit" => {
                self.cancel_edit();
                return Ok(());
            }
            "EndEdit" => self.end_edit(),
            _ => {
                let column = path.strip_prefix("Current.").unwrap_or(path);
                if self.current_id().is_none() {
                    return Ok(()); // Nothing to edit (an empty list): the control's text is ignored.
                }
                self.set_field_formatted(column, value, format)
            }
        };
        // A refused move already reported its validation summary (end_edit).
        if let (Err(e), false) = (&result, path == "Position" || path == "EndEdit") {
            let column = path.strip_prefix("Current.").unwrap_or(path).to_string();
            self.report_error(&column, e.to_string());
        }
        result
    }
}

impl BindingProvider for BindingSource {
    fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, _scope: &ComponentScope) -> Option<Value> {
        self.get_path_typed(path, want, format)
    }

    fn binding_set(&mut self, path: &str, value: Value, format: &BindingFormat, scope: &ComponentScope) -> bool {
        let _ = self.set_path_formatted(path, &value, format);
        self.start_requested_fill(scope);
        true
    }

    fn binding_sync(&mut self, scope: &ComponentScope) -> bool {
        let mut changed = false;
        // A bound Filter/Sort changed.
        if self.view_settings_changed() && !self.table.columns.is_empty() {
            self.reset_bindings();
            changed = true;
        }
        // A detail list follows its master's current row.
        if let Some(rel) = self.relation() {
            let master = self.data_source.trim().to_string();
            if let Some(key) = scope.with_ref::<BindingSource, _>(&master, |m| m.current_key(&rel.parent)) {
                if self.set_master_key(key.clone()) {
                    changed = true;
                    // A parameterized detail select reads the new master's rows only.
                    let adapter = self.adapter_name().to_string();
                    let param = scope.with_ref::<crate::adapter::TableAdapter, _>(&adapter, |a| a.relation_parameter(&rel)).flatten();
                    if let (Some(param), Some(k)) = (param, key) {
                        self.fill_request = Some(FillRequest { page: 0, params: vec![(param, k)] });
                    }
                }
            }
        }
        // AutoFill: the first live frame of the view fills a list that has no master.
        if self.auto_fill && !self.auto_filled && self.relation().is_none() && kubuno_views::scope::current().is_some() && !self.name().is_empty() {
            self.auto_filled = true;
            if self.fill_request.is_none() && self.table.columns.is_empty() {
                self.fill_request = Some(FillRequest { page: 0, params: Vec::new() });
            }
        }
        self.start_requested_fill(scope);
        changed
    }
}

impl BindingSource {
    /// In a view (a frame is running), starts the fill this binding source asked for: a page, a
    /// parameterized detail's rows (the view runtime's executor, `crate::ops::fill_scope`).
    fn start_requested_fill(&mut self, scope: &ComponentScope) {
        if self.fill_request.is_none() || self.busy || kubuno_views::scope::current().is_none() {
            return;
        }
        let name = self.name().to_string();
        if name.is_empty() {
            return;
        }
        self.busy = true;
        let scope = scope.clone();
        drop(kubuno_views::events::spawn_local(async move {
            let _ = crate::ops::fill_scope(&scope, &name).await;
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::DataColumn;
    use crate::value::DbType;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn source() -> BindingSource {
        let mut id = DataColumn::new("id", DbType::new(DbKind::Int, "INTEGER"));
        id.primary_key = true;
        id.auto_increment = true;
        id.nullable = true;
        let mut name = DataColumn::new("name", DbType::new(DbKind::Text, "TEXT"));
        name.nullable = false;
        name.max_length = Some(10);
        let age = DataColumn::new("age", DbType::new(DbKind::Int, "INTEGER"));
        let mut t = Table::new("people", vec![id, name, age]);
        t.load_row(vec![DbValue::Int(1), "Charlie".into(), DbValue::Int(40)]);
        t.load_row(vec![DbValue::Int(2), "alice".into(), DbValue::Int(30)]);
        t.load_row(vec![DbValue::Int(3), "Bob".into(), DbValue::Null]);
        let mut bs = BindingSource::new();
        bs.set_name("people");
        bs.load(t);
        bs
    }

    fn log(bs: &BindingSource) -> Rc<RefCell<Vec<String>>> {
        let log = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        bs.list_changed.subscribe(move |_, e| l.borrow_mut().push(format!("List:{:?}", e.list_changed_type))).detach();
        let l = log.clone();
        bs.position_changed.subscribe(move |_, _| l.borrow_mut().push("Position".into())).detach();
        let l = log.clone();
        bs.current_changed.subscribe(move |s, _| l.borrow_mut().push(format!("Current({})", s.display_name()))).detach();
        let l = log.clone();
        bs.current_item_changed.subscribe(move |_, _| l.borrow_mut().push("Item".into())).detach();
        let l = log.clone();
        bs.data_error.subscribe(move |_, e| l.borrow_mut().push(format!("Error:{}", e.message))).detach();
        log
    }

    fn names(bs: &BindingSource) -> Vec<String> {
        match bs.list_value() {
            Value::List(rows) => rows.iter().map(|r| r.text("name")).collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn navigation_and_paths() {
        let mut bs = source();
        let log = log(&bs);
        assert_eq!(bs.get_path("Position"), Some(Value::F32(0.0)));
        assert_eq!(bs.get_path("Current.name"), Some(Value::Str("Charlie".into())));
        bs.move_last().expect("move");
        assert_eq!(bs.get_path("name"), Some(Value::Str("Bob".into())));
        assert_eq!(bs.get_path("PositionText"), Some(Value::Str("3 / 3".into())));
        assert_eq!(bs.get_path("CanMoveNext"), Some(Value::Bool(false)));
        bs.set_path("Position", &Value::F32(1.0)).expect("set");
        assert_eq!(bs.get_path("age"), Some(Value::Str("30".into())));
        assert_eq!(*log.borrow(), ["Position", "Current(people)", "Position", "Current(people)"]);
        assert_eq!(bs.get_path("nope"), None);
    }

    #[test]
    fn typed_reads_and_writes() {
        let mut bs = source();
        assert_eq!(bs.get_path_typed("age", ValueKind::Number, &BindingFormat::default()), Some(Value::F32(40.0)), "a number for a NumericField");
        let n0 = BindingFormat { format_string: Some("N1".into()), null_value: Some("-".into()), culture: Some("fr-FR".into()) };
        assert_eq!(bs.get_path_typed("age", ValueKind::Text, &n0), Some(Value::Str("40,0".into())));
        bs.set_path_formatted("age", &Value::Str("41,0".into()), &n0).expect("parsed");
        assert_eq!(bs.current_value("age"), Some(&DbValue::Int(41)));
        bs.set_path_formatted("age", &Value::Str("-".into()), &n0).expect("null");
        assert_eq!(bs.current_value("age"), Some(&DbValue::Null));
        assert_eq!(bs.get_path_typed("age", ValueKind::Text, &n0), Some(Value::Str("-".into())), "NullValue shown");
        assert_eq!(bs.get_path_typed("age", ValueKind::Number, &n0), None, "NULL: the numeric property falls back");
        bs.set_path_formatted("age", &Value::F32(42.0), &BindingFormat::default()).expect("number");
        assert_eq!(bs.current_value("age"), Some(&DbValue::Int(42)));
        assert_eq!(bs.get_path_typed("Count", ValueKind::Text, &BindingFormat::default()), Some(Value::Str("3".into())));
    }

    /// What a DataTable reads and writes below its binding source: the error of a column of the
    /// current row, and the `CancelEdit` / `EndEdit` commands.
    #[test]
    fn column_errors_and_edit_commands_are_paths() {
        let mut bs = source();
        assert_eq!(bs.get_path("Current.age.Error"), Some(Value::Str(String::new())));
        bs.set_path("Current.age", &Value::Str("abc".into())).expect("kept as proposed text");
        assert!(matches!(bs.get_path("Current.age.Error"), Some(Value::Str(m)) if !m.is_empty()));
        assert_eq!(bs.get_path("age.Error"), bs.get_path("Current.age.Error"));
        assert_eq!(bs.get_path("nope.Error"), None, "not a column");
        assert!(bs.set_path("EndEdit", &Value::Bool(true)).is_err(), "the conversion error refuses the row");
        assert!(bs.is_editing());
        bs.set_path("CancelEdit", &Value::Bool(true)).expect("cancel");
        assert!(!bs.is_editing());
        assert_eq!(bs.get_path("Current.age.Error"), Some(Value::Str(String::new())));
        assert_eq!(bs.get_path("age"), Some(Value::Str("40".into())), "the value is back");
        bs.set_path("age", &Value::Str("41".into())).expect("edit");
        bs.set_path("EndEdit", &Value::Bool(true)).expect("committed");
        assert!(!bs.is_editing() && bs.has_changes());
    }

    #[test]
    fn filter_and_sort_keep_the_current_row() {
        let mut bs = source();
        bs.set_position(1).expect("move"); // alice
        bs.set_sort("name ASC").expect("sort");
        assert_eq!(names(&bs), ["alice", "Bob", "Charlie"]);
        assert_eq!(bs.get_path("name"), Some(Value::Str("alice".into())));
        bs.set_filter("age >= 35 OR age IS NULL").expect("filter");
        assert_eq!(names(&bs), ["Bob", "Charlie"]);
        assert!(bs.set_filter("nope = 1").is_err());
        assert!(bs.set_filter("age >").is_err());
        assert_eq!(names(&bs), ["Bob", "Charlie"], "a refused filter keeps the previous one");
        bs.set_sort("age DESC").expect("sort");
        assert_eq!(names(&bs), ["Charlie", "Bob"]);
    }

    #[test]
    fn edits_commit_on_move_and_track_state() {
        let mut bs = source();
        let log = log(&bs);
        bs.set_path("Current.name", &Value::Str("Charles".into())).expect("edit");
        assert!(bs.is_editing());
        assert_eq!(names(&bs)[0], "Charles");
        bs.move_next().expect("move commits");
        assert!(!bs.is_editing());
        assert_eq!(bs.table().rows()[0].state, RowState::Modified);
        assert!(bs.has_changes());
        assert_eq!(*log.borrow(), ["List:ItemChanged", "Item", "Position", "Current(people)"]);
        // Echoing the shown value back is not an edit.
        bs.set_path("name", &Value::Str("alice".into())).expect("echo");
        assert!(!bs.is_editing());
    }

    #[test]
    fn conversion_errors_keep_the_proposed_text_and_block_the_move() {
        let mut bs = source();
        bs.set_path("age", &Value::Str("forty".into())).expect("kept");
        assert_eq!(bs.get_path("age"), Some(Value::Str("forty".into())), "the text box keeps what was typed");
        assert_eq!(bs.column_error("age"), Some("Enter a whole number."));
        let err = bs.move_next().expect_err("refused");
        assert!(err.to_string().contains("age: Enter a whole number."));
        assert_eq!(bs.position(), 0);
        assert_eq!(bs.last_error().map(|(c, _)| c.as_str()), Some("age"));
        bs.set_path("age", &Value::Str("41".into())).expect("fixed");
        assert_eq!(bs.column_error("age"), None);
        bs.move_next().expect("now it moves");
        assert_eq!(bs.table().rows()[0].values[2], DbValue::Int(41));
        assert!(bs.last_error().is_none());
    }

    #[test]
    fn required_length_and_row_validating() {
        let mut bs = source();
        bs.row_validating
            .subscribe(|_, e| {
                if e.text("name").starts_with('x') {
                    e.add_error("name", "No x.");
                }
            })
            .detach();
        bs.set_path("name", &Value::Str("".into())).expect("edit");
        assert!(bs.end_edit().is_err());
        assert_eq!(bs.column_error("name"), Some("A value is required."));
        bs.set_path("name", &Value::Str("way too long name".into())).expect("edit");
        assert!(bs.end_edit().is_err());
        assert_eq!(bs.column_error("name"), Some("At most 10 characters."));
        bs.set_path("name", &Value::Str("xavier".into())).expect("edit");
        assert!(bs.end_edit().is_err());
        assert_eq!(bs.current_errors(), [("name".to_string(), "No x.".to_string())]);
        bs.cancel_edit();
        assert_eq!(bs.get_path("name"), Some(Value::Str("Charlie".into())));
        assert!(bs.current_errors().is_empty());
        assert_eq!(bs.table().rows()[0].state, RowState::Unchanged);
    }

    #[test]
    fn add_new_cancel_and_commit() {
        let mut bs = source();
        bs.set_sort("name").expect("sort");
        let log = log(&bs);
        bs.add_new().expect("add");
        assert_eq!(bs.count(), 4);
        assert_eq!(bs.position(), 3, "the new row is at the end");
        assert!(bs.is_editing());
        let first = bs.current_value("id").cloned();
        assert!(matches!(first, Some(DbValue::Int(k)) if k < 0), "a temporary key");
        bs.cancel_edit();
        assert_eq!(bs.count(), 3);
        assert_eq!(log.borrow()[..3], ["List:ItemAdded", "Position", "Current(people)"]);
        bs.add_new().expect("add");
        assert!(matches!(bs.current_value("id"), Some(DbValue::Int(k)) if *k < 0) && bs.current_value("id") != first.as_ref(), "temporary keys are never reused");
        bs.set_path("name", &Value::Str("Aaron".into())).expect("edit");
        bs.set_path("age", &Value::Str("22".into())).expect("edit");
        bs.end_edit().expect("commit");
        assert_eq!(names(&bs), ["Aaron", "alice", "Bob", "Charlie"], "committed: sorted into place");
        assert_eq!(bs.position(), 0);
        let added: Vec<_> = bs.table().rows().iter().filter(|r| r.state == RowState::Added).collect();
        assert_eq!(added.len(), 1);
        let mut locked = source();
        locked.allow_new = false;
        assert!(locked.add_new().is_err());
    }

    #[test]
    fn remove_current_marks_deleted() {
        let mut bs = source();
        bs.move_last().expect("move");
        bs.remove_current().expect("remove");
        assert_eq!(bs.count(), 2);
        assert_eq!(bs.position(), 1);
        assert_eq!(bs.table().rows().iter().filter(|r| r.state == RowState::Deleted).count(), 1);
        assert!(bs.has_changes());
        let mut empty = BindingSource::new();
        assert!(empty.remove_current().is_ok());
        assert!(empty.set_position(3).is_ok());
        assert_eq!(empty.get_path("PositionText"), Some(Value::Str("0 / 0".into())));
    }

    #[test]
    fn a_detail_list_follows_its_master_key() {
        let cols = vec![DataColumn::new("id", DbType::new(DbKind::Int, "INTEGER")), DataColumn::new("customer_id", DbType::new(DbKind::Int, "INTEGER")), DataColumn::new("item", DbType::new(DbKind::Text, "TEXT"))];
        let mut t = Table::new("orders", cols);
        t.columns[0].primary_key = true;
        t.columns[0].auto_increment = true;
        for (id, c, item) in [(1, 1, "Tea"), (2, 2, "Coffee"), (3, 1, "Cake")] {
            t.load_row(vec![DbValue::Int(id), DbValue::Int(c), item.into()]);
        }
        let mut orders = BindingSource::new();
        orders.data_source = "customers".into();
        orders.data_member = "orders.customer_id = customers.id".into();
        assert_eq!(orders.relation(), Some(Relation { child: "customer_id".into(), parent: "id".into() }));
        orders.load(t);
        assert_eq!(orders.count(), 0, "no master key yet: nothing shown");
        assert!(orders.set_master_key(Some(DbValue::Int(1))));
        let items = |b: &BindingSource| match b.list_value() {
            Value::List(rows) => rows.iter().map(|r| r.text("item")).collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        assert_eq!(items(&orders), ["Tea", "Cake"]);
        assert!(!orders.set_master_key(Some(DbValue::Int(1))), "same key: nothing to do");
        orders.add_new().expect("add");
        assert_eq!(orders.current_value("customer_id"), Some(&DbValue::Int(1)), "a detail row gets its master's key");
        orders.set_path("item", &Value::Str("Scone".into())).expect("edit");
        orders.set_master_key(Some(DbValue::Int(2)));
        assert_eq!(items(&orders), ["Coffee"], "the new row was committed and belongs to master 1");
        orders.set_master_key(None);
        assert_eq!(orders.count(), 0);
        assert!(orders.add_new().is_err(), "no master row: no detail row");
        assert_eq!(Relation::parse("customer_id"), Some(Relation { child: "customer_id".into(), parent: String::new() }));
        assert_eq!(Relation::parse("  "), None);
    }

    #[test]
    fn paging_requests() {
        let mut bs = source();
        bs.load_page(bs.table().clone(), 0, 3, Some(8));
        assert_eq!(bs.page(), (0, Some(3)));
        assert_eq!(bs.get_path("PageText"), Some(Value::Str("1 / 3".into())));
        assert_eq!(bs.get_path("CanNextPage"), Some(Value::Bool(true)));
        bs.set_path("PageIndex", &Value::F32(9.0)).expect("page");
        assert_eq!(bs.take_fill_request().map(|r| r.page), Some(2), "clamped to the last page");
        bs.set_path("name", &Value::Str("Changed".into())).expect("edit");
        assert!(bs.request_page(1).is_err(), "unsaved changes keep the page");
    }

    #[test]
    fn events_of_a_named_component_wait_for_the_runtime() {
        // Named (sited, a view's component) and called from code: the events wait for the runtime.
        let mut bs = source();
        let _ = bs.base.take_queued();
        bs.move_next().expect("move");
        let queued: Vec<&str> = bs.base.take_queued().into_iter().map(|(a, _)| a).collect();
        assert_eq!(queued, ["OnPositionChanged", "OnCurrentChanged"]);
        // An anonymous one (outside any view) only raises to its Rust subscribers.
        let mut anonymous = BindingSource::new();
        anonymous.load(bs.table().clone());
        anonymous.move_next().expect("move");
        assert!(!anonymous.base.has_queued());
    }
}
