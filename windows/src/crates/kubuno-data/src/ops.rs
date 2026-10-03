//! Fill and save over the named components of a view (`kubuno_views::scope::ComponentScope`,
//! `vskubuno/docs/DATA.md` §7): the one implementation behind the async helpers of a view
//! ([`fill`], [`save`], [`save_all`]), the internal fills the view runtime starts (a page, a
//! parameterized detail list: [`fill_scope`]), the Save button of a `BindingNavigator`
//! ([`save_scope`]) and the blocking helpers of a standalone [`crate::DataContext`].
//!
//! Every operation follows EVT-6's rule (no borrow across `.await`): it starts on the UI thread
//! (`begin_*`: validate, plan, raise `StateChange`), awaits a [`DataTask`] of the data runtime, and
//! completes on the UI thread (`end_*`: load or accept the rows, raise `ListChanged(Reset)`, report
//! errors to the binding source and so to its ErrorProvider). Given the view model (`vm`), the
//! components' `.kbview` handlers run synchronously; without it, their events are queued for the
//! view runtime.

use kubuno_views::binding::ViewModel;
use kubuno_views::component::Component;
use kubuno_views::events::{EventSink, TypedViewModel, UiHandle};
use kubuno_views::scope::ComponentScope;

use crate::adapter::{run_plans, split_outcome, FilledPage, PagingMode, TableAdapter, UpdateOutcome, UpdatePlan};
use crate::binding_source::BindingSource;
use crate::connection::{ConnectionHandle, DbConnection};
use crate::error::{logged, DataError};
use crate::events::ConnectionState;
use crate::rt::DataTask;
use crate::value::DbValue;

/// Runs `f` on component `name` of `scope`, with its XML handlers run synchronously against `vm`
/// when there is one.
fn call<T: Component, R>(scope: &ComponentScope, vm: &mut Option<&mut dyn ViewModel>, name: &str, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    match vm.as_deref_mut() {
        Some(vm) => scope.with_dispatch(vm, name, f),
        None => scope.with(name, f),
    }
}

fn missing(what: &str, name: &str) -> DataError {
    DataError::Config(format!("there is no {what} named `{name}`"))
}

/// Raises `StateChange` on connection `conn` when its state changed.
fn sync_connection(scope: &ComponentScope, vm: &mut Option<&mut dyn ViewModel>, conn: &str) {
    call(scope, vm, conn, |c: &mut DbConnection| c.sync_state());
}

/// Reports `e` on binding source `source` (its `DataError`, its ErrorProvider) and returns it.
fn report(scope: &ComponentScope, vm: &mut Option<&mut dyn ViewModel>, source: &str, e: DataError) -> DataError {
    let e = logged("data operation", e);
    call(scope, vm, source, |bs: &mut BindingSource| {
        bs.set_busy(false);
        // A request that failed is not retried on its own (the next frame would start it again, and
        // again: an AutoFill without its connection string, a page of a broken query…).
        let _ = bs.take_fill_request();
        bs.report_error("", e.to_string());
    });
    e
}

/// The adapter and connection of binding source `source`.
fn wiring(scope: &ComponentScope, source: &str) -> Result<(String, String, ConnectionHandle), DataError> {
    let adapter = scope.with_ref::<BindingSource, _>(source, |bs| bs.adapter_name().to_string()).ok_or_else(|| missing("BindingSource", source))?;
    if adapter.is_empty() {
        return Err(DataError::Config(format!("`{source}` has no TableAdapter (set DataSource, or TableAdapter for a detail list)")));
    }
    let conn = scope.with_ref::<TableAdapter, _>(&adapter, |a| a.connection.clone()).ok_or_else(|| DataError::Config(format!("`{source}`: `{adapter}` is not a TableAdapter")))?;
    let handle = scope.with::<DbConnection, _>(&conn, |c| c.handle()).ok_or_else(|| missing("DbConnection", &conn))??;
    Ok((adapter, conn, handle))
}

/// A fill started by [`begin_fill`].
pub struct FillOp {
    pub source: String,
    connection: String,
    keyset: Option<String>,
    pub task: DataTask<FilledPage>,
}

impl FillOp {
    /// The task, taken out to be awaited (the rest completes the fill).
    pub fn take_task(&mut self) -> DataTask<FilledPage> {
        std::mem::replace(&mut self.task, DataTask::failed(DataError::Closed))
    }
}

/// Starts filling binding source `source` from its adapter (UI thread, no I/O here): the page it
/// asked for, a detail list's master rows when its select is parameterized by the relation.
pub fn begin_fill(scope: &ComponentScope, mut vm: Option<&mut dyn ViewModel>, source: &str) -> Result<FillOp, DataError> {
    let (adapter, connection, handle) = match wiring(scope, source) {
        Ok(w) => w,
        Err(e) => return Err(report(scope, &mut vm, source, e)),
    };
    if handle.state() == ConnectionState::Closed {
        handle.set_state(ConnectionState::Connecting);
    }
    sync_connection(scope, &mut vm, &connection);
    let (relation_param, keyset_col) = scope
        .with_ref::<TableAdapter, _>(&adapter, |a| {
            let rel = scope.with_ref::<BindingSource, _>(source, |bs| bs.relation()).flatten();
            (rel.and_then(|r| a.relation_parameter(&r)), (a.page_size > 0 && a.paging_mode == PagingMode::Keyset).then(|| a.keyset_column_name()))
        })
        .unwrap_or_default();
    let prepared = call(scope, &mut vm, source, |bs: &mut BindingSource| {
        let mut request = bs.take_fill_request().unwrap_or_else(|| bs.current_request());
        // A parameterized detail reads its master's rows.
        if let (Some(p), Some(key)) = (&relation_param, bs.master_key().cloned()) {
            if !request.params.iter().any(|(n, _)| n.eq_ignore_ascii_case(p)) {
                request.params.push((p.clone(), key));
            }
        }
        let after = match &keyset_col {
            Some(_) => {
                let (page, after) = bs.keyset_start(request.page);
                request.page = page;
                after
            }
            None => None,
        };
        bs.set_busy(true);
        (request, after, bs.progress().clone())
    });
    let Some((request, after, progress)) = prepared else { return Err(report(scope, &mut vm, source, missing("BindingSource", source))) };
    if relation_param.is_some() && !request.params.iter().any(|(n, _)| relation_param.as_deref().is_some_and(|p| n.eq_ignore_ascii_case(p))) {
        // A detail without a master row reads nothing.
        return Err(report(scope, &mut vm, source, DataError::Validation("the master list has no current row".to_string())));
    }
    let task = scope.with_ref::<TableAdapter, _>(&adapter, |a| a.fill_page(&handle, &request, after, Some(progress))).ok_or_else(|| missing("TableAdapter", &adapter))?;
    Ok(FillOp { source: source.to_string(), connection, keyset: keyset_col, task })
}

/// Completes a fill (UI thread): loads the rows, or reports the error. The number of rows.
pub fn end_fill(scope: &ComponentScope, mut vm: Option<&mut dyn ViewModel>, op: &FillOp, result: Result<FilledPage, DataError>) -> Result<usize, DataError> {
    sync_connection(scope, &mut vm, &op.connection);
    match result {
        Ok(page) => {
            let n = page.table.rows().len();
            let next_key = op.keyset.as_ref().and_then(|col| {
                let c = page.table.column_index(col)?;
                page.table.rows().last().and_then(|r| r.values.get(c).cloned())
            });
            call(scope, &mut vm, &op.source, |bs: &mut BindingSource| {
                if op.keyset.is_some() {
                    bs.record_keyset(page.page, if n as u32 >= page.page_size { next_key } else { None });
                }
                bs.load_page(page.table, page.page, page.page_size, page.total);
            })
            .ok_or(DataError::Closed)?;
            Ok(n)
        }
        Err(e) => Err(report(scope, &mut vm, &op.source, e)),
    }
}

/// A save started by [`begin_save`].
pub struct SaveOp {
    pub sources: Vec<String>,
    pub plans: Vec<UpdatePlan>,
    connection: String,
    pub task: DataTask<UpdateOutcome>,
}

impl SaveOp {
    /// The task, taken out to be awaited (the rest completes the save).
    pub fn take_task(&mut self) -> DataTask<UpdateOutcome> {
        std::mem::replace(&mut self.task, DataTask::failed(DataError::Closed))
    }
}

/// Starts saving the changes of binding sources `sources` (UI thread) in **one** transaction:
/// ends their edits (a row that does not validate stops here), plans each adapter's statements —
/// a detail list's references to its master's new rows follow the keys the database gives them.
/// List masters before their details. `Ok(None)`: nothing to save.
pub fn begin_save(scope: &ComponentScope, mut vm: Option<&mut dyn ViewModel>, sources: &[&str]) -> Result<Option<SaveOp>, DataError> {
    let mut plans = Vec::with_capacity(sources.len());
    let mut first: Option<(String, ConnectionHandle, std::time::Duration)> = None;
    for &source in sources {
        let (adapter, connection, handle) = match wiring(scope, source) {
            Ok(w) => w,
            Err(e) => return Err(report(scope, &mut vm, source, e)),
        };
        match &first {
            Some((c, _, _)) if *c != connection => {
                return Err(report(scope, &mut vm, source, DataError::Config(format!("`{source}` uses another connection: one transaction needs one connection"))));
            }
            Some(_) => {}
            None => {
                let timeout = scope.with_ref::<TableAdapter, _>(&adapter, |a| if a.command_timeout > 0 { std::time::Duration::from_secs(u64::from(a.command_timeout)) } else { handle.command_timeout() }).unwrap_or_else(|| handle.command_timeout());
                first = Some((connection.clone(), handle.clone(), timeout));
            }
        }
        match call(scope, &mut vm, source, |bs: &mut BindingSource| bs.end_edit()) {
            Some(Ok(())) => {}
            Some(Err(e)) => return Err(e),
            None => return Err(missing("BindingSource", source)),
        }
        let plan = scope
            .with_ref::<BindingSource, _>(source, |bs| {
                let fk = bs.relation().and_then(|r| bs.table().column_index(&r.child));
                scope.with_ref::<TableAdapter, _>(&adapter, |a| a.plan_update_with(handle.provider(), bs.table(), fk))
            })
            .flatten()
            .ok_or_else(|| missing("TableAdapter", &adapter))?;
        match plan {
            Ok(p) => plans.push(p),
            Err(e) => return Err(report(scope, &mut vm, source, e)),
        }
    }
    let Some((connection, handle, timeout)) = first else { return Ok(None) };
    if plans.iter().all(UpdatePlan::is_empty) {
        return Ok(None);
    }
    for &source in sources {
        call(scope, &mut vm, source, |bs: &mut BindingSource| bs.set_busy(true));
    }
    let task = run_plans(&handle, &plans, timeout);
    Ok(Some(SaveOp { sources: sources.iter().map(|s| s.to_string()).collect(), plans, connection, task }))
}

/// Completes a save (UI thread): applies each committed plan (generated keys and new row versions
/// written back, a detail's references to its master's new rows rewritten), or reports the error
/// (the rows keep their changes: nothing was committed). The number of saved rows.
pub fn end_save(scope: &ComponentScope, mut vm: Option<&mut dyn ViewModel>, op: SaveOp, result: Result<UpdateOutcome, DataError>) -> Result<usize, DataError> {
    sync_connection(scope, &mut vm, &op.connection);
    match result {
        Ok(outcome) => {
            let key_map = outcome.key_map.clone();
            let outcomes = split_outcome(&op.plans, outcome);
            let mut saved = 0;
            for ((source, plan), out) in op.sources.iter().zip(&op.plans).zip(outcomes) {
                saved += plan.rows.len();
                call(scope, &mut vm, source, |bs: &mut BindingSource| {
                    bs.table_mut().apply_update(plan, &out);
                    bs.remap_keys(None, &key_map);
                    bs.after_save();
                });
            }
            Ok(saved)
        }
        Err(e) => {
            let e = logged("save", e);
            for (i, source) in op.sources.iter().enumerate() {
                call(scope, &mut vm, source, |bs: &mut BindingSource| {
                    bs.set_busy(false);
                    if i == 0 {
                        bs.report_error("", e.to_string());
                    }
                });
            }
            Err(e)
        }
    }
}

/// Fills binding source `source` from an async handler of the view (its XML handlers run
/// synchronously). The number of rows.
///
/// ```ignore
/// async fn on_load(ui: UiHandle<Self>) { let _ = kubuno_data::fill(&ui, "customers").await; }
/// ```
pub async fn fill<V: ViewModel + EventSink + 'static>(ui: &UiHandle<V>, source: &str) -> Result<usize, DataError> {
    let scope = ui.components().ok_or(DataError::Closed)?;
    let mut op = ui.update(|vm| begin_fill(&scope, Some(&mut TypedViewModel(vm)), source)).ok_or(DataError::Closed)??;
    let result = op.take_task().await;
    ui.update(|vm| end_fill(&scope, Some(&mut TypedViewModel(vm)), &op, result)).ok_or(DataError::Closed)?
}

/// Saves binding source `source`'s changes from an async handler of the view (one transaction).
/// The number of saved rows (`0`: nothing to save).
pub async fn save<V: ViewModel + EventSink + 'static>(ui: &UiHandle<V>, source: &str) -> Result<usize, DataError> {
    save_all(ui, &[source]).await
}

/// Saves the changes of several binding sources in **one** transaction (masters first: a new
/// customer and its new orders are saved together, the orders taking the customer's generated
/// key). The number of saved rows.
pub async fn save_all<V: ViewModel + EventSink + 'static>(ui: &UiHandle<V>, sources: &[&str]) -> Result<usize, DataError> {
    let scope = ui.components().ok_or(DataError::Closed)?;
    let op = ui.update(|vm| begin_save(&scope, Some(&mut TypedViewModel(vm)), sources)).ok_or(DataError::Closed)??;
    let Some(mut op) = op else { return Ok(0) };
    let result = op.take_task().await;
    ui.update(|vm| end_save(&scope, Some(&mut TypedViewModel(vm)), op, result)).ok_or(DataError::Closed)?
}

/// Fills binding source `source` of `scope` without the view model (its events are queued for
/// the view runtime): what the runtime starts for a page or a detail list, and what code without a
/// `UiHandle` uses.
pub async fn fill_scope(scope: &ComponentScope, source: &str) -> Result<usize, DataError> {
    let mut op = begin_fill(scope, None, source)?;
    let result = op.take_task().await;
    end_fill(scope, None, &op, result)
}

/// Saves binding sources of `scope` without the view model (a `BindingNavigator`'s Save button).
pub async fn save_scope(scope: &ComponentScope, sources: &[&str]) -> Result<usize, DataError> {
    let Some(mut op) = begin_save(scope, None, sources)? else { return Ok(0) };
    let result = op.take_task().await;
    end_save(scope, None, op, result)
}

/// The value a detail list's relation reads from its master (for tests and tools).
pub fn master_key_of(scope: &ComponentScope, detail: &str) -> Option<DbValue> {
    scope.with_ref::<BindingSource, _>(detail, |bs| bs.master_key().cloned()).flatten()
}
