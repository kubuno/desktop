//! [`DataContext`]: data components **outside a view runtime** (tests, tools, a view model that paints
//! without `Runtime`) — the same components, paths, events and operations as in a view, over a
//! [`ComponentScope`] this context owns (`vskubuno/docs/DATA.md` §7).
//!
//! In a view, the runtime owns the components (DATA-2): nothing to write in the view model, bindings
//! reach them by name, their `.kbview` handlers run, and the async helpers are
//! [`crate::fill`]/[`crate::save`]/[`crate::save_all`]. A `DataContext` is for the other cases:
//!
//! ```ignore
//! let mut ctx = DataContext::from_view(&view_text)?;
//! kubuno_desktop_data::fill_blocking(&mut ctx, "customers")?;
//! ctx.set("customers.Name", Value::Str("Ada".into()));
//! kubuno_desktop_data::save_blocking(&mut ctx, "customers")?;
//! ```
//!
//! Its components' events wait on them until [`DataContext::take_events`] (or [`pump`], which
//! delivers them through a view model's typed handlers).

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use kubuno_desktop_views::binding::{MapViewModel, Value};
use kubuno_desktop_views::component::{Component, ComponentLink, Site};
use kubuno_desktop_views::events::router::SlotEvents;
use kubuno_desktop_views::events::{dispatch_typed, ElementRef, EventArgs, EventSink, UiHandle};
use kubuno_desktop_views::prelude::ViewModel;
use kubuno_desktop_views::scope::ComponentScope;

use crate::adapter::TableAdapter;
use crate::binding_source::BindingSource;
use crate::command::DbCommand;
use crate::connection::{ConnectionHandle, DbConnection};
use crate::error::{logged, DataError};
use crate::error_provider::ErrorProvider;
use crate::navigator::BindingNavigator;
use crate::ops;
use crate::rt::DataTask;

/// Implemented by a view model that owns a [`DataContext`] (what [`fill`], [`save`] and [`pump`] need).
pub trait HasDataContext {
    fn data_context(&mut self) -> &mut DataContext;
}

/// An event raised by a data component for the `.kbview` handler its element names.
pub struct PendingEvent {
    /// The component's `x:Name`.
    pub component: String,
    /// Its class (`"BindingSource"`).
    pub class: &'static str,
    /// The handler's name (the attribute's value).
    pub handler: String,
    pub args: Box<dyn EventArgs>,
}

/// A component of a context: its name, its instance, its element.
type Owned = (String, Rc<RefCell<dyn Component>>, Rc<SlotEvents>);

/// The data components of a view, outside a view runtime (see the module doc).
#[derive(Default)]
pub struct DataContext {
    scope: ComponentScope,
    /// The instances (the scope holds them weakly), with their elements.
    owned: Vec<Owned>,
}

/// The event attributes of the data components (their `&'static` names).
const EVENT_ATTRS: &[&str] =
    &["OnStateChange", "OnListChanged", "OnCurrentChanged", "OnPositionChanged", "OnCurrentItemChanged", "OnAddingNew", "OnRowValidating", "OnDataError"];

/// The element names of the non-visual data components.
pub const ELEMENTS: &[&str] = &["DbConnection", "DbCommand", "TableAdapter", "BindingSource", "ErrorProvider", "LocalDatabase"];

/// Every element this crate registers (the components and the `BindingNavigator` control).
pub const ALL_ELEMENTS: &[&str] = &["DbConnection", "DbCommand", "TableAdapter", "BindingSource", "ErrorProvider", "LocalDatabase", "BindingNavigator"];

impl DataContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// The data components declared in a `.kbview` text (anywhere in the tree), with their
    /// properties and the handlers their `On*` attributes name. Other elements are ignored. Unnamed
    /// components get the names the view runtime gives them (`bindingSource1`…).
    pub fn from_view(text: &str) -> Result<Self, DataError> {
        use kubuno_desktop_views::ast::{AstNode, Document, Element};
        let parse = kubuno_desktop_views::syntax::parse(text);
        let doc = Document::cast(parse.syntax()).ok_or_else(|| DataError::Validation("the view does not parse".to_string()))?;
        let root = doc.root_element().ok_or_else(|| DataError::Validation("the view has no root element".to_string()))?;
        let mut ctx = Self::new();
        let mut stack: Vec<Element> = vec![root];
        let mut counters = [0usize; 6];
        while let Some(el) = stack.pop() {
            stack.extend(el.children().collect::<Vec<_>>().into_iter().rev());
            let Some(kind) = el.name().and_then(|n| ELEMENTS.iter().position(|e| *e == n)) else { continue };
            counters[kind] += 1;
            let class = ELEMENTS[kind];
            let name = el.attribute("x:Name").and_then(|a| a.value()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("{}{}{}", class[..1].to_ascii_lowercase(), &class[1..], counters[kind]));
            let mut slot = SlotEvents::new(el.stable_id(), class);
            slot.name = Some(name.clone());
            let mut props: Vec<(String, String)> = Vec::new();
            for attr in el.attributes() {
                let (Some(key), Some(value)) = (attr.name(), attr.value()) else { continue };
                if key == "x:Name" || key.starts_with("xmlns") {
                    continue;
                }
                if let Some(event) = EVENT_ATTRS.iter().find(|e| **e == key) {
                    slot = slot.with_handler(event, value);
                    continue;
                }
                if kubuno_desktop_views::binding::is_binding_expr(&value) {
                    tracing::warn!(target: "kubuno_desktop_data", component = %name, attribute = %key, "bindings on data components need the view runtime; the attribute is ignored here");
                    continue;
                }
                props.push((key, value));
            }
            fn apply<C: ComponentLink>(c: &mut C, props: &[(String, String)], name: &str) {
                for (k, v) in props {
                    if !c.kubuno_set_property(k, &Value::Str(v.clone())) {
                        tracing::warn!(target: "kubuno_desktop_data", component = %name, attribute = %k, "unknown or invalid attribute ignored");
                    }
                }
            }
            let instance: Rc<RefCell<dyn Component>> = match class {
                "DbConnection" => {
                    let mut c = DbConnection::default();
                    apply(&mut c, &props, &name);
                    Rc::new(RefCell::new(c))
                }
                "DbCommand" => {
                    let mut c = DbCommand::default();
                    apply(&mut c, &props, &name);
                    Rc::new(RefCell::new(c))
                }
                "TableAdapter" => {
                    let mut c = TableAdapter::default();
                    apply(&mut c, &props, &name);
                    Rc::new(RefCell::new(c))
                }
                "BindingSource" => {
                    let mut c = BindingSource::default();
                    apply(&mut c, &props, &name);
                    Rc::new(RefCell::new(c))
                }
                "LocalDatabase" => {
                    let mut c = crate::local_database::LocalDatabase::default();
                    apply(&mut c, &props, &name);
                    Rc::new(RefCell::new(c))
                }
                _ => {
                    let mut c = ErrorProvider::default();
                    apply(&mut c, &props, &name);
                    Rc::new(RefCell::new(c))
                }
            };
            ctx.insert(&name, instance, Rc::new(slot));
        }
        Ok(ctx)
    }

    /// Adds a component under `name` (its element: `slot`, for the `On*` handlers).
    fn insert(&mut self, name: &str, instance: Rc<RefCell<dyn Component>>, slot: Rc<SlotEvents>) {
        if self.owned.iter().any(|(n, _, _)| n == name) {
            tracing::warn!(target: "kubuno_desktop_data", name, "two data components have the same name; the first one wins");
        }
        if let Ok(mut c) = instance.try_borrow_mut() {
            c.set_site(Some(Site { name: name.to_string(), design_mode: false, container: None }));
        }
        self.scope.insert(name, instance.clone(), Some(slot.clone()));
        self.owned.push((name.to_string(), instance, slot));
    }

    fn add<C: Component>(&mut self, name: &str, c: C) -> &mut Self {
        let mut slot = SlotEvents::new(String::new(), c.class_name());
        slot.name = Some(name.to_string());
        self.insert(name, Rc::new(RefCell::new(c)), Rc::new(slot));
        self
    }

    pub fn add_connection(&mut self, name: &str, c: DbConnection) -> &mut Self {
        self.add(name, c)
    }
    pub fn add_command(&mut self, name: &str, c: DbCommand) -> &mut Self {
        self.add(name, c)
    }
    pub fn add_adapter(&mut self, name: &str, c: TableAdapter) -> &mut Self {
        self.add(name, c)
    }
    pub fn add_binding_source(&mut self, name: &str, c: BindingSource) -> &mut Self {
        self.add(name, c)
    }
    pub fn add_error_provider(&mut self, name: &str, c: ErrorProvider) -> &mut Self {
        self.add(name, c)
    }
    pub fn add_navigator(&mut self, name: &str, c: BindingNavigator) -> &mut Self {
        self.add(name, c)
    }

    /// The components, as the view runtime would hold them (for `ops` and tools).
    pub fn scope(&self) -> &ComponentScope {
        &self.scope
    }

    fn get_ref<T: Component>(&self, name: &str) -> Option<Ref<'_, T>> {
        let (_, cell, _) = self.owned.iter().find(|(n, _, _)| n == name)?;
        Ref::filter_map(cell.try_borrow().ok()?, |c| c.find_base::<T>()).ok()
    }

    fn get_mut<T: Component>(&self, name: &str) -> Option<RefMut<'_, T>> {
        let (_, cell, _) = self.owned.iter().find(|(n, _, _)| n == name)?;
        RefMut::filter_map(cell.try_borrow_mut().ok()?, |c| c.find_base_mut::<T>()).ok()
    }

    pub fn connection(&self, name: &str) -> Option<Ref<'_, DbConnection>> {
        self.get_ref(name)
    }
    pub fn connection_mut(&self, name: &str) -> Option<RefMut<'_, DbConnection>> {
        self.get_mut(name)
    }
    pub fn command(&self, name: &str) -> Option<Ref<'_, DbCommand>> {
        self.get_ref(name)
    }
    pub fn command_mut(&self, name: &str) -> Option<RefMut<'_, DbCommand>> {
        self.get_mut(name)
    }
    pub fn adapter(&self, name: &str) -> Option<Ref<'_, TableAdapter>> {
        self.get_ref(name)
    }
    pub fn adapter_mut(&self, name: &str) -> Option<RefMut<'_, TableAdapter>> {
        self.get_mut(name)
    }
    pub fn binding_source(&self, name: &str) -> Option<Ref<'_, BindingSource>> {
        self.get_ref(name)
    }
    pub fn binding_source_mut(&self, name: &str) -> Option<RefMut<'_, BindingSource>> {
        self.get_mut(name)
    }
    pub fn error_provider(&self, name: &str) -> Option<Ref<'_, ErrorProvider>> {
        self.get_ref(name)
    }
    pub fn error_provider_mut(&self, name: &str) -> Option<RefMut<'_, ErrorProvider>> {
        self.get_mut(name)
    }

    /// The resolved connection named `name`.
    pub fn connection_handle(&mut self, name: &str) -> Result<ConnectionHandle, DataError> {
        let mut c = self.connection_mut(name).ok_or_else(|| logged("data context", DataError::Config(format!("there is no DbConnection named `{name}`"))))?;
        c.handle()
    }

    /// Runs the command named `command` (its `Connection`); the affected rows.
    pub fn execute(&mut self, command: &str) -> DataTask<u64> {
        let Some(conn) = self.command(command).map(|c| c.connection.clone()) else {
            return DataTask::failed(logged("data context", DataError::Config(format!("there is no DbCommand named `{command}`"))));
        };
        match self.connection_handle(&conn) {
            Ok(h) => self.command(command).map_or_else(|| DataTask::failed(DataError::Cancelled), |c| c.execute_non_query(&h)),
            Err(e) => DataTask::failed(e),
        }
    }

    // ── Bindings ─────────────────────────────────────────────────────────────────────────────

    /// What a binding path reads (§7 of the design): `<source>[.<path>]`, `<errors>.<path>`,
    /// `<connection>.State`. `None` for a path that names no data component.
    pub fn get(&self, path: &str) -> Option<Value> {
        let name = path.split('.').next().unwrap_or(path);
        if let Some(c) = self.connection(name) {
            return match path.split_once('.').map_or("", |(_, r)| r) {
                "State" => Some(Value::Str(c.state().as_str().to_string())),
                "IsOpen" => Some(Value::Bool(c.state() == crate::events::ConnectionState::Open)),
                _ => None,
            };
        }
        self.scope.get(name)?;
        let mut none = MapViewModel::new();
        self.scope.view_model(&mut none, false).get(path)
    }

    /// What a two-way binding writes: `true` when the path names a data component (the write went
    /// there, errors are reported through the binding source), `false` otherwise.
    pub fn set(&mut self, path: &str, value: Value) -> bool {
        let name = path.split('.').next().unwrap_or(path);
        if self.scope.get(name).is_none() {
            return false;
        }
        let mut none = MapViewModel::new();
        self.scope.view_model(&mut none, false).set(path, value);
        true
    }

    /// Lets detail lists follow their masters (after changes made from code).
    pub fn sync(&mut self) -> bool {
        let mut none = MapViewModel::new();
        self.scope.sync_all(&mut none, false)
    }

    // ── Fill and save ────────────────────────────────────────────────────────────────────────

    /// Starts filling binding source `source` from its adapter (UI thread, no I/O here).
    pub fn begin_fill(&mut self, source: &str) -> Result<ops::FillOp, DataError> {
        ops::begin_fill(&self.scope, None, source)
    }

    /// Completes a fill (UI thread): loads the rows, or reports the error. The number of rows.
    pub fn end_fill(&mut self, op: &ops::FillOp, result: Result<crate::adapter::FilledPage, DataError>) -> Result<usize, DataError> {
        let r = ops::end_fill(&self.scope, None, op, result);
        self.sync();
        r
    }

    /// Starts saving the changes of `sources` in one transaction (see [`ops::begin_save`]).
    pub fn begin_save(&mut self, sources: &[&str]) -> Result<Option<ops::SaveOp>, DataError> {
        ops::begin_save(&self.scope, None, sources)
    }

    /// Completes a save (UI thread). The number of saved rows.
    pub fn end_save(&mut self, op: ops::SaveOp, result: Result<crate::adapter::UpdateOutcome, DataError>) -> Result<usize, DataError> {
        let r = ops::end_save(&self.scope, None, op, result);
        self.sync();
        r
    }

    /// Raises `StateChange` on the connections whose state changed.
    pub fn sync_states(&mut self) {
        for (_, cell, _) in &self.owned {
            if let Ok(mut c) = cell.try_borrow_mut() {
                if let Some(conn) = c.find_base_mut::<DbConnection>() {
                    conn.sync_state();
                }
            }
        }
    }

    /// The events waiting for their `.kbview` handlers, oldest first per component.
    pub fn take_events(&mut self) -> Vec<PendingEvent> {
        let mut out = Vec::new();
        for (name, cell, slot) in &self.owned {
            let (class, queued) = match cell.try_borrow_mut() {
                Ok(mut c) => (c.class_name(), c.component_core_mut().take_queued()),
                Err(_) => continue,
            };
            for (attr, args) in queued {
                if let Some(handler) = slot.handler(attr) {
                    out.push(PendingEvent { component: name.clone(), class, handler: handler.to_string(), args });
                }
            }
        }
        out
    }
}

/// Delivers the data components' events to the view model's typed handlers (the `On*` attributes
/// of their elements). Call it after changing data components from the view model.
pub fn pump<V: HasDataContext + ViewModel + EventSink>(vm: &mut V) {
    // Handlers may raise more events: a few rounds, bounded.
    for _ in 0..8 {
        let events = vm.data_context().take_events();
        if events.is_empty() {
            return;
        }
        for mut ev in events {
            let sender = ElementRef { name: Some(&ev.component), element: ev.class, id: "", bounds: Default::default(), focus_id: None, attributes: &[] };
            if !dispatch_typed(vm, &ev.handler, &sender, &mut *ev.args) {
                tracing::warn!(target: "kubuno_desktop_data", handler = %ev.handler, component = %ev.component, "the view names a handler the view model does not have");
            }
        }
    }
}

/// Fills binding source `source` of a view model's [`DataContext`] from an async handler (a view
/// model painting without `Runtime`); in a view, use [`crate::fill`].
pub async fn fill<V: HasDataContext + ViewModel + EventSink + 'static>(ui: &UiHandle<V>, source: &str) -> Result<usize, DataError> {
    let mut op = ui
        .update(|vm| {
            let r = vm.data_context().begin_fill(source);
            pump(vm);
            r
        })
        .ok_or(DataError::Closed)??;
    let result = op.take_task().await;
    ui.update(|vm| {
        let r = vm.data_context().end_fill(&op, result);
        pump(vm);
        r
    })
    .ok_or(DataError::Closed)?
}

/// Saves binding source `source` of a view model's [`DataContext`] from an async handler; in a
/// view, use [`crate::save`].
pub async fn save<V: HasDataContext + ViewModel + EventSink + 'static>(ui: &UiHandle<V>, source: &str) -> Result<usize, DataError> {
    let op = ui
        .update(|vm| {
            let r = vm.data_context().begin_save(&[source]);
            pump(vm);
            r
        })
        .ok_or(DataError::Closed)??;
    let Some(mut op) = op else { return Ok(0) };
    let result = op.take_task().await;
    ui.update(|vm| {
        let r = vm.data_context().end_save(op, result);
        pump(vm);
        r
    })
    .ok_or(DataError::Closed)?
}

/// Runs a fill to completion on the current thread (tests, tools, `main` before the window opens).
pub fn fill_blocking(ctx: &mut DataContext, source: &str) -> Result<usize, DataError> {
    let mut op = ctx.begin_fill(source)?;
    let result = crate::rt::block_on(op.take_task())?;
    ctx.end_fill(&op, result)
}

/// Runs a save to completion on the current thread (tests, tools).
pub fn save_blocking(ctx: &mut DataContext, source: &str) -> Result<usize, DataError> {
    save_all_blocking(ctx, &[source])
}

/// Saves several binding sources in one transaction on the current thread (masters first).
pub fn save_all_blocking(ctx: &mut DataContext, sources: &[&str]) -> Result<usize, DataError> {
    match ctx.begin_save(sources)? {
        None => Ok(0),
        Some(mut op) => {
            let result = crate::rt::block_on(op.take_task())?;
            ctx.end_save(op, result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: &str = r#"
        <Panel DesignWidth="400" DesignHeight="300">
          <DbConnection x:Name="db" Provider="Sqlite" ConnectionString="sqlite::memory:" OnStateChange="db_state"/>
          <TableAdapter x:Name="peopleAdapter" Connection="db" SelectCommand="SELECT id, name FROM people" UpdateTable="people"/>
          <BindingSource x:Name="people" DataSource="peopleAdapter" Sort="name DESC" OnCurrentChanged="current_changed"/>
          <ErrorProvider x:Name="errors" DataSource="people"/>
          <Stack><BindingSource Filter="{Binding F}"/></Stack>
          <TextField Text="{Binding Source=people, Path=name, Mode=TwoWay}"/>
        </Panel>"#;

    #[test]
    fn components_come_from_the_view() {
        let ctx = DataContext::from_view(VIEW).expect("view");
        let db = ctx.connection("db").expect("db");
        assert_eq!(db.provider, crate::Provider::Sqlite);
        assert_eq!(db.connection_string, "sqlite::memory:");
        drop(db);
        let a = ctx.adapter("peopleAdapter").expect("adapter");
        assert_eq!((a.connection.as_str(), a.update_table.as_str()), ("db", "people"));
        drop(a);
        assert_eq!(ctx.binding_source("people").map(|b| b.sort.clone()), Some("name DESC".to_string()));
        assert!(ctx.binding_source("bindingSource2").is_some(), "unnamed components get a generated name");
        assert_eq!(ctx.error_provider("errors").map(|e| e.data_source.clone()), Some("people".to_string()));
        assert_eq!(ctx.get("db.State"), Some(Value::Str("Closed".into())));
        assert_eq!(ctx.get("people.Count"), Some(Value::F32(0.0)));
        assert_eq!(ctx.get("unknown.Count"), None);
        assert_eq!(ctx.scope().names(), ["db", "peopleAdapter", "people", "errors", "bindingSource2"]);
    }
}
