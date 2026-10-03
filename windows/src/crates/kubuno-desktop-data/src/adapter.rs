//! `<TableAdapter>` (`vskubuno/docs/DATA.md` §4): fills a [`Table`] from its `SelectCommand` and
//! saves the table's changes back to its `UpdateTable` with generated (or custom), parameterized
//! `DELETE`/`UPDATE`/`INSERT` statements run in **one transaction**: all or nothing, each statement
//! checked to affect exactly one row (else a concurrency violation and a rollback). Nothing is
//! applied to the table before the commit succeeded ([`Table::apply_update`]).
//!
//! DATA-3 adds:
//! - **Optimistic concurrency** (`ConflictOption`): `CompareAllSearchableValues` (an update or a
//!   delete matches the row's original values), `CompareRowVersion` (its `RowVersionColumn`: a
//!   `rowversion`, PostgreSQL's `xmin` read as `xmin::text AS xmin`, a trigger-maintained version),
//!   the new version read back in the same statement (`RETURNING`, `OUTPUT INSERTED`).
//! - **Custom DML** (`InsertCommand`, `UpdateCommand`, `DeleteCommand`): parameterized text whose
//!   `@column` parameters take the row's values and `@Original_column` its original values.
//! - **Paging** (`PageSize`, `PagingMode`): `LIMIT … OFFSET …` (`OFFSET … FETCH NEXT` on SQL
//!   Server) or keyset (`WHERE key > @last ORDER BY key`), with the total row count.
//! - **Relations**: a detail's rows reference their master's temporary key until the master is
//!   inserted in the same transaction (`crate::ops::save_all`).

use kubuno_desktop_views::prelude::*;

use crate::binding_source::{FillRequest, Relation};
use crate::connection::ConnectionHandle;
use crate::error::{logged, DataError};
use crate::provider::{Expect, Provider, Statement};
use crate::rt::{self, DataTask, Progress};
use crate::sql::{quote_for, top_level_order_by, trim_statement, validate_type_name};
use crate::table::{RowState, Table};
use crate::value::{DbKind, DbValue};

/// How an adapter detects that a row changed in the database since it was read (ADO.NET
/// `ConflictOption`).
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConflictOption {
    /// The last write wins: updates and deletes match the key only.
    #[default]
    OverwriteChanges,
    /// Updates and deletes match the key and every original value (NULL-aware).
    CompareAllSearchableValues,
    /// Updates and deletes match the key and the row version (`RowVersionColumn`).
    CompareRowVersion,
}

/// How a paged adapter reads a page.
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PagingMode {
    /// `LIMIT n OFFSET m` (any page, cost grows with the offset).
    #[default]
    Offset,
    /// `WHERE key > @last ORDER BY key LIMIT n` (constant cost, pages in order).
    Keyset,
}

/// `<TableAdapter>` (see the module doc).
#[derive(Component)]
#[kubuno(extends = Component)]
#[toolbox(icon = "table", category = "Data")]
#[default_property("SelectCommand")]
pub struct TableAdapter {
    base: ComponentCore,
    /// The x:Name of the DbConnection the adapter reads from and writes to.
    #[property]
    #[category("Data")]
    pub connection: String,
    /// The query that fills the table. Values are @name parameters, never written into the text.
    #[property]
    #[category("Data")]
    pub select_command: String,
    /// The table changes are saved to (schema.table allowed). Empty: the adapter is read-only.
    #[property]
    #[category("Data")]
    pub update_table: String,
    /// The key columns, separated by commas. Empty: the table's primary key, else a column named id.
    #[property]
    #[category("Data")]
    pub primary_key: String,
    /// Whether the database generates the key of new rows (a serial, an identity, an INTEGER PRIMARY KEY).
    #[property]
    #[category("Data")]
    #[default_value(true)]
    pub auto_increment_key: bool,
    /// Replaces the generated INSERT: parameterized text whose @column parameters take the new row's values.
    #[property]
    #[category("Data")]
    pub insert_command: String,
    /// Replaces the generated UPDATE: @column takes the new value, @Original_column the value read.
    #[property]
    #[category("Data")]
    pub update_command: String,
    /// Replaces the generated DELETE: @Original_column takes the value read.
    #[property]
    #[category("Data")]
    pub delete_command: String,
    /// How a row changed by someone else since it was read is detected: OverwriteChanges, CompareAllSearchableValues, CompareRowVersion.
    #[property]
    #[category("Data")]
    #[default_value("OverwriteChanges")]
    pub conflict_option: ConflictOption,
    /// The row version column compared by CompareRowVersion (a rowversion, xmin read as xmin::text AS xmin, a version kept by a trigger).
    #[property]
    #[category("Data")]
    pub row_version_column: String,
    /// The rows per page (0: every row at once).
    #[property]
    #[category("Data")]
    pub page_size: u32,
    /// How pages are read: Offset (any page) or Keyset (in order, constant cost).
    #[property]
    #[category("Data")]
    #[default_value("Offset")]
    pub paging_mode: PagingMode,
    /// The ordered, unique column keyset paging reads after (empty: the key column).
    #[property]
    #[category("Data")]
    pub keyset_column: String,
    /// How long a fill or a save may run, in seconds (0: the connection's CommandTimeout).
    #[property]
    #[category("Behavior")]
    pub command_timeout: u32,
    params: Vec<(String, DbValue)>,
}

impl Default for TableAdapter {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            connection: String::new(),
            select_command: String::new(),
            update_table: String::new(),
            primary_key: String::new(),
            auto_increment_key: true,
            insert_command: String::new(),
            update_command: String::new(),
            delete_command: String::new(),
            conflict_option: ConflictOption::OverwriteChanges,
            row_version_column: String::new(),
            page_size: 0,
            paging_mode: PagingMode::Offset,
            keyset_column: String::new(),
            command_timeout: 0,
            params: Vec::new(),
        }
    }
}

/// What a planned statement does to its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedKind {
    Delete,
    Update,
    Insert,
}

/// One row of an [`UpdatePlan`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedRow {
    pub row_id: u64,
    pub kind: PlannedKind,
    /// The row's values when the plan was made (what the database will hold after the commit).
    pub snapshot: Vec<DbValue>,
    /// The columns receiving what the statement returns, in order (a generated key, a new row
    /// version).
    pub returns: Vec<usize>,
}

/// The statements that save a table's changes, and the rows they save.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UpdatePlan {
    pub(crate) statements: Vec<Statement>,
    pub rows: Vec<PlannedRow>,
    /// Modified rows whose values equal their original values (nothing to write): accepted as is.
    pub unchanged: Vec<u64>,
    /// The column holding a master's key (a detail table), rewritten when the master's temporary
    /// keys are replaced.
    pub fk_column: Option<usize>,
}

impl UpdatePlan {
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty() && self.unchanged.is_empty()
    }

    /// The SQL of each statement (for tests and logs: values are parameters, never in the text).
    pub fn sql(&self) -> Vec<&str> {
        self.statements.iter().map(|s| s.sql.as_str()).collect()
    }
}

/// What the database returned for a plan: per statement the returned values (a generated key, a new
/// row version) or nothing, and the temporary keys it replaced by generated ones.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UpdateOutcome {
    pub returned: Vec<Option<Vec<DbValue>>>,
    pub key_map: Vec<(DbValue, DbValue)>,
}

/// A page read by [`TableAdapter::fill_page`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FilledPage {
    pub table: Table,
    pub page: u32,
    pub page_size: u32,
    /// The rows of every page (counted when paging).
    pub total: Option<u64>,
}

/// What an insert or an update returns: the `OUTPUT` clause (SQL Server), the `RETURNING` tail, and
/// the query that reads it afterwards (MySQL).
type Returning = (String, String, Option<(String, Vec<DbValue>)>);

/// A result with its (plan, statement) position.
type Indexed = ((usize, usize), Option<Vec<DbValue>>);

/// Whether a column can be compared in a WHERE clause (CompareAllSearchableValues).
fn searchable(kind: DbKind) -> bool {
    !matches!(kind, DbKind::Bytes | DbKind::Json | DbKind::Other)
}

impl TableAdapter {
    pub fn new(connection: impl Into<String>, select_command: impl Into<String>) -> Self {
        Self { connection: connection.into(), select_command: select_command.into(), ..Self::default() }
    }

    pub fn with_update_table(mut self, table: impl Into<String>) -> Self {
        self.update_table = table.into();
        self
    }

    pub fn with_primary_key(mut self, key: impl Into<String>) -> Self {
        self.primary_key = key.into();
        self
    }

    pub fn name(&self) -> &str {
        crate::events::name_of(&self.base)
    }

    /// Sets a parameter of the select command (`@name`).
    pub fn param(&mut self, name: &str, value: impl Into<DbValue>) -> &mut Self {
        let name = name.trim_start_matches('@').to_string();
        self.params.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
        self.params.push((name, value.into()));
        self
    }

    fn timeout(&self, conn: &ConnectionHandle) -> std::time::Duration {
        if self.command_timeout > 0 {
            std::time::Duration::from_secs(u64::from(self.command_timeout))
        } else {
            conn.command_timeout()
        }
    }

    /// The parameter of the select command a detail's relation fills (`@customer_id` for
    /// `customer_id = id`), when it has one: the detail then reads its master's rows only.
    pub fn relation_parameter(&self, relation: &Relation) -> Option<String> {
        let names = crate::sql::rewrite_named(&self.select_command, Provider::Postgres).ok()?.names;
        [relation.child.as_str(), relation.parent.as_str()].into_iter().filter(|n| !n.is_empty()).find(|n| names.iter().any(|p| p.eq_ignore_ascii_case(n))).map(str::to_string)
    }

    /// Reads the select command's rows (with the connection's retry policy), typed from the
    /// driver's column description and the update table's schema (nullability, lengths, keys).
    pub fn fill(&self, conn: &ConnectionHandle) -> DataTask<Table> {
        let task = self.fill_page(conn, &FillRequest::default(), None, None);
        rt::spawn(async move { task.await.map(|p| p.table) })
    }

    /// Reads one page (or every row when `PageSize` is 0) of the select command, with `request`'s
    /// extra parameters (a detail's master key), counting the rows read in `progress`. `after`: the
    /// keyset value the page starts after (keyset paging).
    pub fn fill_page(&self, conn: &ConnectionHandle, request: &FillRequest, after: Option<DbValue>, progress: Option<Progress>) -> DataTask<FilledPage> {
        let provider = conn.provider();
        let base = trim_statement(&self.select_command).to_string();
        let mut named = self.params.clone();
        for (n, v) in &request.params {
            named.retain(|(p, _)| !p.eq_ignore_ascii_case(n));
            named.push((n.clone(), v.clone()));
        }
        let page_size = self.page_size;
        let page = if page_size == 0 { 0 } else { request.page };
        let (text, count_text) = match self.paged_text(provider, &base, page, after.is_some()) {
            Ok(t) => t,
            Err(e) => return DataTask::failed(logged("fill", e)),
        };
        if page_size > 0 {
            named.push(("kubuno_limit".to_string(), DbValue::Int(i64::from(page_size))));
            match (self.paging_mode, after) {
                (PagingMode::Offset, _) => named.push(("kubuno_offset".to_string(), DbValue::Int(i64::from(page) * i64::from(page_size)))),
                (PagingMode::Keyset, Some(a)) => named.push(("kubuno_after".to_string(), a)),
                (PagingMode::Keyset, None) => {}
            }
        }
        let prepared = crate::command::prepare(&text, provider, &named, &[]);
        let (sql, params) = match prepared {
            Ok(p) => p,
            Err(e) => return DataTask::failed(logged("fill", e)),
        };
        let count = match count_text {
            Some(t) => {
                let count_params: Vec<(String, DbValue)> = named.iter().filter(|(n, _)| !n.starts_with("kubuno_")).cloned().collect();
                match crate::command::prepare(&t, provider, &count_params, &[]) {
                    Ok(p) => Some(p),
                    Err(e) => return DataTask::failed(logged("fill", e)),
                }
            }
            None => None,
        };
        let update_table = self.update_table.trim().to_string();
        if !update_table.is_empty() {
            if let Err(e) = crate::sql::validate_identifier(&update_table, true) {
                return DataTask::failed(logged("fill", e));
            }
        }
        let keys: Vec<String> = self.primary_key.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
        let version = self.row_version_column.trim().to_string();
        let auto_key = self.auto_increment_key;
        let name = if self.name().is_empty() { update_table.clone() } else { self.name().to_string() };
        let (conn, timeout) = (conn.clone(), self.timeout(conn));
        rt::spawn(async move {
            let pool = conn.pool().await?;
            let retry = conn.retry();
            let mut table = retry
                .run("fill", || {
                    if let Some(p) = &progress {
                        p.reset();
                    }
                    pool.query_table(&sql, &params, timeout, progress.as_ref())
                })
                .await?;
            let total = match &count {
                Some((csql, cparams)) => match retry.run("count", || pool.scalar(csql, cparams, timeout)).await? {
                    DbValue::Int(n) => u64::try_from(n).ok(),
                    other => other.to_display().parse::<u64>().ok(),
                },
                None => None,
            };
            table.name = name;
            if !update_table.is_empty() {
                let schema = pool.table_schema(&update_table, timeout).await?;
                if schema.is_empty() {
                    return Err(logged("fill", DataError::Validation(format!("the table `{update_table}` does not exist or has no columns"))));
                }
                for col in &mut table.columns {
                    match schema.iter().find(|s| s.name.eq_ignore_ascii_case(&col.name)) {
                        Some(s) => {
                            col.nullable = s.nullable;
                            col.max_length = s.max_length;
                            col.primary_key = s.primary_key;
                            col.auto_increment = s.auto_increment;
                            col.read_only = s.read_only;
                            if !s.native.is_empty() {
                                col.ty.native = s.native.clone();
                            }
                        }
                        // Not a column of the update table (a join, an expression): never written.
                        None => col.read_only = true,
                    }
                }
            }
            // The row version is written by the database only.
            if !version.is_empty() {
                if let Some(c) = table.columns.iter_mut().find(|c| c.name.eq_ignore_ascii_case(&version)) {
                    c.read_only = true;
                }
            }
            apply_keys(&mut table, &keys, auto_key);
            tracing::debug!(target: "kubuno_desktop_data", table = %table.name, rows = table.rows().len(), page, "filled");
            Ok(FilledPage { table, page, page_size, total })
        })
    }

    /// The column keyset paging reads after.
    pub fn keyset_column_name(&self) -> String {
        match self.keyset_column.trim() {
            "" => self.primary_key.split(',').next().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).unwrap_or_else(|| "id".to_string()),
            k => k.to_string(),
        }
    }

    /// The select text of a page, and the text counting every row (`None` without paging).
    fn paged_text(&self, provider: Provider, base: &str, page: u32, has_after: bool) -> Result<(String, Option<String>), DataError> {
        if self.page_size == 0 {
            return Ok((base.to_string(), None));
        }
        let unordered = match top_level_order_by(base) {
            Some(i) => base[..i].trim_end(),
            None => base,
        };
        let count = format!("SELECT COUNT(*) FROM ({unordered}) AS kubuno_count");
        let text = match self.paging_mode {
            PagingMode::Offset => match provider {
                Provider::SqlServer => {
                    let ordered = if top_level_order_by(base).is_some() { base.to_string() } else { format!("{base} ORDER BY (SELECT NULL)") };
                    format!("{ordered} OFFSET @kubuno_offset ROWS FETCH NEXT @kubuno_limit ROWS ONLY")
                }
                _ => format!("{base} LIMIT @kubuno_limit OFFSET @kubuno_offset"),
            },
            PagingMode::Keyset => {
                let col = quote_for(provider, &self.keyset_column_name(), false)?;
                let filter = if has_after && page > 0 { format!(" WHERE kubuno_page.{col} > @kubuno_after") } else { String::new() };
                match provider {
                    Provider::SqlServer => format!("SELECT TOP (@kubuno_limit) * FROM ({unordered}) AS kubuno_page{filter} ORDER BY kubuno_page.{col}"),
                    _ => format!("SELECT * FROM ({unordered}) AS kubuno_page{filter} ORDER BY kubuno_page.{col} LIMIT @kubuno_limit"),
                }
            }
        };
        Ok((text, Some(count)))
    }

    /// Plans the statements that save `table`'s changes (see the module doc). Pure: no I/O.
    pub fn plan_update(&self, provider: Provider, table: &Table) -> Result<UpdatePlan, DataError> {
        self.plan_update_with(provider, table, None)
    }

    /// [`Self::plan_update`] for a detail table whose column `fk_column` holds its master's key
    /// (the master's temporary keys are replaced in the transaction).
    pub fn plan_update_with(&self, provider: Provider, table: &Table, fk_column: Option<usize>) -> Result<UpdatePlan, DataError> {
        let target = self.update_table.trim();
        if target.is_empty() {
            return Err(DataError::Config("set UpdateTable to save changes".to_string()));
        }
        let q = |name: &str, schema: bool| quote_for(provider, name, schema);
        let quoted_table = q(target, true)?;
        let keys = table.key_columns();
        let version = match (self.conflict_option, self.row_version_column.trim()) {
            (ConflictOption::CompareRowVersion, "") => return Err(DataError::Config("CompareRowVersion needs RowVersionColumn".to_string())),
            (ConflictOption::CompareRowVersion, v) => Some(table.column_index(v).ok_or_else(|| DataError::Validation(format!("the row version column `{v}` is not read by the SelectCommand")))?),
            _ => None,
        };
        // PostgreSQL's system column `xmin` compares and returns as text.
        let version_expr = |c: usize| -> Result<String, DataError> {
            let name = &table.columns[c].name;
            let quoted = q(name, false)?;
            Ok(if provider == Provider::Postgres && name.eq_ignore_ascii_case("xmin") { format!("{quoted}::text") } else { quoted })
        };
        let mut plan = UpdatePlan { fk_column, ..UpdatePlan::default() };
        let (mut deletes, mut updates, mut inserts) = (Vec::new(), Vec::new(), Vec::new());
        // One placeholder for a new parameter (PostgreSQL casts it to the column's type).
        let push = |params: &mut Vec<DbValue>, fk: &mut Vec<usize>, value: DbValue, col: usize| -> Result<String, DataError> {
            params.push(value);
            if Some(col) == fk_column {
                fk.push(params.len() - 1);
            }
            let ph = provider.placeholder(params.len());
            let column = &table.columns[col];
            if provider == Provider::Postgres && !column.ty.native.is_empty() && column.ty.kind != DbKind::Other && !(version == Some(col) && column.name.eq_ignore_ascii_case("xmin")) {
                validate_type_name(&column.ty.native)?;
                Ok(format!("CAST({ph} AS {})", column.ty.native))
            } else {
                Ok(ph)
            }
        };
        // `key = @k [AND …]`, plus the concurrency check, for a row read as `original`.
        let where_clause = |params: &mut Vec<DbValue>, fk: &mut Vec<usize>, original: &[DbValue]| -> Result<String, DataError> {
            let mut parts = Vec::new();
            for &k in &keys {
                let value = original.get(k).cloned().unwrap_or(DbValue::Null);
                if value.is_null() {
                    return Err(DataError::Validation(format!("the key `{}` of a row to save is empty", table.columns[k].name)));
                }
                parts.push(format!("{} = {}", q(&table.columns[k].name, false)?, push(params, fk, value, k)?));
            }
            match self.conflict_option {
                ConflictOption::CompareAllSearchableValues => {
                    for (c, col) in table.columns.iter().enumerate() {
                        if keys.contains(&c) || col.read_only || !searchable(col.ty.kind) {
                            continue;
                        }
                        match original.get(c).cloned().unwrap_or(DbValue::Null) {
                            DbValue::Null => parts.push(format!("{} IS NULL", q(&col.name, false)?)),
                            v => parts.push(format!("{} = {}", q(&col.name, false)?, push(params, fk, v, c)?)),
                        }
                    }
                }
                ConflictOption::CompareRowVersion => {
                    if let Some(v) = version {
                        let value = original.get(v).cloned().unwrap_or(DbValue::Null);
                        let lhs = version_expr(v)?;
                        parts.push(match value {
                            DbValue::Null => format!("{lhs} IS NULL"),
                            value => format!("{lhs} = {}", push(params, fk, value, v)?),
                        });
                    }
                }
                ConflictOption::OverwriteChanges => {}
            }
            Ok(parts.join(" AND "))
        };
        // What an insert or an update returns (and, on MySQL, the query that reads it after).
        let returning = |cols: &[usize], key_where: Option<(&str, &[DbValue])>| -> Result<Returning, DataError> {
            if cols.is_empty() {
                return Ok((String::new(), String::new(), None));
            }
            let exprs: Vec<String> = cols.iter().map(|&c| if version == Some(c) { version_expr(c) } else { q(&table.columns[c].name, false) }).collect::<Result<_, _>>()?;
            Ok(match provider {
                Provider::SqlServer => (format!(" OUTPUT {}", exprs.iter().map(|e| format!("INSERTED.{e}")).collect::<Vec<_>>().join(", ")), String::new(), None),
                Provider::MySql => {
                    let (clause, params) = key_where.map(|(c, p)| (c.to_string(), p.to_vec())).unwrap_or_default();
                    (String::new(), String::new(), Some((format!("SELECT {} FROM {quoted_table} WHERE {clause}", exprs.join(", ")), params)))
                }
                _ => (String::new(), format!(" RETURNING {}", exprs.join(", ")), None),
            })
        };
        let custom = |text: &str, row: &crate::table::DataRow, fk: &mut Vec<usize>| -> Result<(String, Vec<DbValue>), DataError> {
            let prepared = crate::sql::rewrite_named(text, provider)?;
            let original = row.original.as_ref().unwrap_or(&row.values);
            let mut params = Vec::with_capacity(prepared.names.len());
            for name in &prepared.names {
                let (source, column, is_original) = match name.get(..9).filter(|p| p.eq_ignore_ascii_case("Original_")) {
                    Some(_) => (original, &name[9..], true),
                    None => (&row.values, name.as_str(), false),
                };
                let c = table.column_index(column).ok_or_else(|| DataError::Validation(format!("the parameter @{name} names no column of the table")))?;
                if Some(c) == fk_column && !is_original {
                    fk.push(params.len());
                }
                params.push(source.get(c).cloned().unwrap_or(DbValue::Null));
            }
            Ok((prepared.sql, params))
        };
        for row in table.rows() {
            match row.state {
                RowState::Deleted if keys.is_empty() && self.delete_command.trim().is_empty() => {
                    return Err(DataError::Validation(format!("`{target}` needs a primary key (PrimaryKey) to update or delete rows")));
                }
                RowState::Modified if keys.is_empty() && self.update_command.trim().is_empty() => {
                    return Err(DataError::Validation(format!("`{target}` needs a primary key (PrimaryKey) to update or delete rows")));
                }
                RowState::Deleted => {
                    let original = row.original.as_ref().unwrap_or(&row.values);
                    let mut fk = Vec::new();
                    let (sql, params) = if self.delete_command.trim().is_empty() {
                        let mut params = Vec::new();
                        let clause = where_clause(&mut params, &mut fk, original)?;
                        (format!("DELETE FROM {quoted_table} WHERE {clause}"), params)
                    } else {
                        custom(&self.delete_command, row, &mut fk)?
                    };
                    let st = Statement::new(sql, params, Expect::One, format!("the deletion of a row of {target}"));
                    deletes.push((st, PlannedRow { row_id: row.id, kind: PlannedKind::Delete, snapshot: row.values.clone(), returns: Vec::new() }));
                }
                RowState::Modified => {
                    let original = row.original.as_ref().unwrap_or(&row.values);
                    let changed: Vec<usize> = (0..table.columns.len()).filter(|&c| !table.columns[c].read_only && row.values.get(c) != original.get(c)).collect();
                    if changed.is_empty() {
                        plan.unchanged.push(row.id);
                        continue;
                    }
                    let mut fk = Vec::new();
                    let returns: Vec<usize> = version.into_iter().collect();
                    let (sql, params, follow_up) = if self.update_command.trim().is_empty() {
                        let mut params = Vec::new();
                        let mut sets = Vec::with_capacity(changed.len());
                        for c in changed {
                            let ph = push(&mut params, &mut fk, row.values[c].clone(), c)?;
                            sets.push(format!("{} = {ph}", q(&table.columns[c].name, false)?));
                        }
                        let clause = where_clause(&mut params, &mut fk, original)?;
                        // MySQL reads the new version by key after the update.
                        let mut key_params = Vec::new();
                        let mut key_parts = Vec::new();
                        for &k in &keys {
                            key_params.push(row.values.get(k).cloned().unwrap_or(DbValue::Null));
                            key_parts.push(format!("{} = ?", q(&table.columns[k].name, false)?));
                        }
                        let (output, tail, follow) = returning(&returns, Some((&key_parts.join(" AND "), &key_params)))?;
                        (format!("UPDATE {quoted_table} SET {}{output} WHERE {clause}{tail}", sets.join(", ")), params, follow)
                    } else {
                        let (sql, params) = custom(&self.update_command, row, &mut fk)?;
                        (sql, params, None)
                    };
                    let expect = if !returns.is_empty() && self.update_command.trim().is_empty() && provider != Provider::MySql { Expect::Returning } else { Expect::One };
                    let returns = if self.update_command.trim().is_empty() { returns } else { Vec::new() };
                    let mut st = Statement::new(sql, params, expect, format!("the update of a row of {target}"));
                    st.fk_params = fk;
                    st.follow_up = follow_up;
                    updates.push((st, PlannedRow { row_id: row.id, kind: PlannedKind::Update, snapshot: row.values.clone(), returns }));
                }
                RowState::Added => {
                    // A generated key (NULL, or a temporary negative key given by `add_new`).
                    let generated: Option<usize> = match keys.as_slice() {
                        [k] if table.columns[*k].auto_increment && row.values.get(*k).is_none_or(|v| v.is_null() || matches!(v, DbValue::Int(i) if *i < 0)) => Some(*k),
                        _ => None,
                    };
                    let temp_key = generated.and_then(|k| row.values.get(k).cloned()).filter(|v| !v.is_null());
                    let mut fk = Vec::new();
                    let returns: Vec<usize> = generated.into_iter().chain(version).collect();
                    let (sql, params, expect, follow_up) = if self.insert_command.trim().is_empty() {
                        let mut params = Vec::new();
                        let mut cols = Vec::new();
                        let mut phs = Vec::new();
                        for (c, col) in table.columns.iter().enumerate() {
                            if col.read_only || Some(c) == generated {
                                continue;
                            }
                            phs.push(push(&mut params, &mut fk, row.values.get(c).cloned().unwrap_or(DbValue::Null), c)?);
                            cols.push(q(&col.name, false)?);
                        }
                        // MySQL: the generated key is the connection's last insert id.
                        let key_where = generated.map(|k| q(&table.columns[k].name, false).map(|n| format!("{n} = LAST_INSERT_ID()"))).transpose()?;
                        let (output, tail, follow) = if provider == Provider::MySql {
                            match (generated, returns.len()) {
                                (Some(_), 1) => (String::new(), String::new(), None),
                                _ => returning(&returns, key_where.as_deref().map(|w| (w, &[][..])))?,
                            }
                        } else {
                            returning(&returns, None)?
                        };
                        let sql = if cols.is_empty() {
                            match provider {
                                Provider::SqlServer => format!("INSERT INTO {quoted_table}{output} DEFAULT VALUES"),
                                _ => format!("INSERT INTO {quoted_table} DEFAULT VALUES{tail}"),
                            }
                        } else {
                            format!("INSERT INTO {quoted_table} ({}){output} VALUES ({}){tail}", cols.join(", "), phs.join(", "))
                        };
                        let expect = match (provider, generated, returns.is_empty()) {
                            (_, _, true) => Expect::One,
                            (Provider::MySql, Some(_), _) => Expect::LastInsertId,
                            (Provider::MySql, None, _) => Expect::One,
                            _ => Expect::Returning,
                        };
                        (sql, params, expect, follow)
                    } else {
                        let (sql, params) = custom(&self.insert_command, row, &mut fk)?;
                        let upper = sql.to_ascii_uppercase();
                        let returns_key = generated.is_some() && (upper.contains("RETURNING") || upper.contains("OUTPUT"));
                        (sql, params, if returns_key { Expect::Returning } else { Expect::One }, None)
                    };
                    let returns = if self.insert_command.trim().is_empty() || expect == Expect::Returning { returns } else { Vec::new() };
                    let mut st = Statement::new(sql, params, expect, format!("the insertion of a row into {target}"));
                    st.fk_params = fk;
                    st.follow_up = follow_up;
                    st.temp_key = temp_key;
                    inserts.push((st, PlannedRow { row_id: row.id, kind: PlannedKind::Insert, snapshot: row.values.clone(), returns }));
                }
                RowState::Unchanged | RowState::Detached => {}
            }
        }
        // Deletes first (a unique value they free can be reused), then updates, then inserts.
        for (st, row) in deletes.into_iter().chain(updates).chain(inserts) {
            plan.statements.push(st);
            plan.rows.push(row);
        }
        Ok(plan)
    }

    /// Runs `plan` in one transaction on the data runtime.
    pub fn update(&self, conn: &ConnectionHandle, plan: &UpdatePlan) -> DataTask<UpdateOutcome> {
        run_plans(conn, std::slice::from_ref(plan), self.timeout(conn))
    }
}

/// Runs the statements of several plans (a master's then its details', or any adapters of one
/// connection) in **one** transaction: every delete first (details before masters), then every
/// update, then every insert (masters before details, their temporary keys replaced). The outcome
/// of each plan, in order.
pub(crate) fn run_plans(conn: &ConnectionHandle, plans: &[UpdatePlan], timeout: std::time::Duration) -> DataTask<UpdateOutcome> {
    let mut order: Vec<(usize, usize)> = Vec::new();
    for kind in [PlannedKind::Delete, PlannedKind::Update, PlannedKind::Insert] {
        let plan_order: Vec<usize> = if kind == PlannedKind::Delete { (0..plans.len()).rev().collect() } else { (0..plans.len()).collect() };
        for p in plan_order {
            for (i, row) in plans[p].rows.iter().enumerate() {
                if row.kind == kind {
                    order.push((p, i));
                }
            }
        }
    }
    let statements: Vec<Statement> = order.iter().map(|&(p, i)| plans[p].statements[i].clone()).collect();
    let conn = conn.clone();
    rt::spawn(async move {
        if statements.is_empty() {
            return Ok(UpdateOutcome::default());
        }
        // The connection may be retried; the transaction never is (it may have committed).
        let pool = conn.pool().await?;
        let tx = pool.run_in_transaction(&statements, timeout).await?;
        tracing::info!(target: "kubuno_desktop_data", statements = statements.len(), "changes saved");
        // Back in plan order (the results come in execution order).
        let returned: Vec<Option<Vec<DbValue>>> = (0..statements.len()).map(|k| tx.returned.get(k).cloned().flatten()).collect();
        Ok(UpdateOutcome { returned: reorder(&order, returned), key_map: tx.key_map })
    })
}

/// Puts results given in execution order back in (plan, statement) order, flattened.
fn reorder(order: &[(usize, usize)], results: Vec<Option<Vec<DbValue>>>) -> Vec<Option<Vec<DbValue>>> {
    let mut indexed: Vec<Indexed> = order.iter().copied().zip(results).collect();
    indexed.sort_by_key(|(k, _)| *k);
    indexed.into_iter().map(|(_, r)| r).collect()
}

/// Splits the flattened outcome of [`run_plans`] into one outcome per plan.
pub(crate) fn split_outcome(plans: &[UpdatePlan], outcome: UpdateOutcome) -> Vec<UpdateOutcome> {
    let mut rest = outcome.returned.into_iter();
    plans.iter().map(|p| UpdateOutcome { returned: rest.by_ref().take(p.rows.len()).collect(), key_map: outcome.key_map.clone() }).collect()
}

/// Marks the key columns: `keys` when given, else the schema's primary key, else a column named `id`.
fn apply_keys(table: &mut Table, keys: &[String], auto_key: bool) {
    if !keys.is_empty() {
        for col in &mut table.columns {
            col.primary_key = keys.iter().any(|k| k.eq_ignore_ascii_case(&col.name));
        }
    } else if !table.columns.iter().any(|c| c.primary_key) {
        if let Some(id) = table.columns.iter_mut().find(|c| c.name.eq_ignore_ascii_case("id")) {
            id.primary_key = true;
            id.auto_increment = id.auto_increment || (auto_key && id.ty.kind == DbKind::Int);
        }
    }
    if !auto_key {
        for col in &mut table.columns {
            col.auto_increment = false;
        }
    }
    for col in &mut table.columns {
        if col.primary_key && col.auto_increment {
            col.nullable = true;
        }
    }
}

impl Table {
    /// Applies a committed plan: saved rows become `Unchanged` with the saved values as original
    /// values (a row edited again during the save stays `Modified` against them), returned values
    /// (generated keys, new row versions) are written back, deleted rows go, and a detail table's
    /// references to its master's temporary keys take the generated keys.
    pub fn apply_update(&mut self, plan: &UpdatePlan, outcome: &UpdateOutcome) {
        if let Some(c) = plan.fk_column {
            self.remap_column(c, &outcome.key_map);
        }
        for id in &plan.unchanged {
            if let Some(r) = self.row_by_id_mut(*id) {
                if r.state == RowState::Modified {
                    r.state = RowState::Unchanged;
                }
            }
        }
        for (i, planned) in plan.rows.iter().enumerate() {
            let returned = outcome.returned.get(i).cloned().flatten().unwrap_or_default();
            match planned.kind {
                PlannedKind::Delete => {
                    if self.row_by_id(planned.row_id).is_some_and(|r| r.state == RowState::Deleted) {
                        self.remove_row(planned.row_id);
                    }
                }
                PlannedKind::Update | PlannedKind::Insert => {
                    let Some(row) = self.row_by_id_mut(planned.row_id) else { continue };
                    let mut saved = planned.snapshot.clone();
                    if let Some(c) = plan.fk_column {
                        if let Some(v) = saved.get_mut(c) {
                            if let Some((_, real)) = outcome.key_map.iter().find(|(t, _)| t == v) {
                                *v = real.clone();
                            }
                        }
                    }
                    for (&c, value) in planned.returns.iter().zip(returned) {
                        if let Some(slot) = saved.get_mut(c) {
                            *slot = value.clone();
                        }
                        if let Some(slot) = row.values.get_mut(c) {
                            *slot = value;
                        }
                    }
                    row.state = if row.values == saved { RowState::Unchanged } else { RowState::Modified };
                    row.original = Some(saved);
                    row.row_error = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::DataColumn;
    use crate::value::DbType;

    fn customers(provider: Provider) -> Table {
        let ty = |k: DbKind, pg: &str, lite: &str| DbType::new(k, if provider == Provider::Postgres { pg } else { lite });
        let mut id = DataColumn::new("id", ty(DbKind::Int, "int4", "INTEGER"));
        id.primary_key = true;
        id.auto_increment = true;
        let mut t = Table::new(
            "customers",
            vec![id, DataColumn::new("name", ty(DbKind::Text, "text", "TEXT")), DataColumn::new("created", ty(DbKind::DateTime, "timestamptz", "DATETIME"))],
        );
        t.load_row(vec![DbValue::Int(1), "Ada".into(), "2026-01-01T00:00:00Z".into()]);
        t.load_row(vec![DbValue::Int(2), "Linus".into(), DbValue::Null]);
        t.load_row(vec![DbValue::Int(3), "Grace".into(), DbValue::Null]);
        t
    }

    fn modify(t: &mut Table, index: usize, column: usize, value: DbValue) -> u64 {
        let id = t.rows()[index].id;
        let r = t.row_by_id_mut(id).expect("row");
        r.values[column] = value;
        r.state = RowState::Modified;
        id
    }

    #[test]
    fn plans_parameterized_dml_in_order() {
        let mut t = customers(Provider::Sqlite);
        let ids: Vec<u64> = t.rows().iter().map(|r| r.id).collect();
        modify(&mut t, 0, 1, "Ada L.".into());
        t.delete_row(ids[1]);
        let untouched = ids[2];
        t.row_by_id_mut(untouched).expect("row").state = RowState::Modified;
        t.add_row(vec![DbValue::Null, "Robert'); DROP TABLE customers; --".into(), DbValue::Null]);
        let adapter = TableAdapter::new("db", "SELECT * FROM customers").with_update_table("customers");
        let plan = adapter.plan_update(Provider::Sqlite, &t).expect("plan");
        assert_eq!(
            plan.sql(),
            [
                "DELETE FROM \"customers\" WHERE \"id\" = ?1",
                "UPDATE \"customers\" SET \"name\" = ?1 WHERE \"id\" = ?2",
                "INSERT INTO \"customers\" (\"name\", \"created\") VALUES (?1, ?2) RETURNING \"id\"",
            ]
        );
        assert_eq!(plan.statements[2].params[0], DbValue::Text("Robert'); DROP TABLE customers; --".into()));
        assert_eq!(plan.unchanged, [untouched]);
        assert_eq!(plan.rows.iter().map(|r| r.kind).collect::<Vec<_>>(), [PlannedKind::Delete, PlannedKind::Update, PlannedKind::Insert]);
    }

    #[test]
    fn postgres_parameters_are_cast_to_the_column_type() {
        let mut t = customers(Provider::Postgres);
        modify(&mut t, 0, 2, "2026-09-30 12:00:00".into());
        let adapter = TableAdapter::new("db", "SELECT * FROM crm.customers").with_update_table("crm.customers");
        let plan = adapter.plan_update(Provider::Postgres, &t).expect("plan");
        assert_eq!(plan.sql(), ["UPDATE \"crm\".\"customers\" SET \"created\" = CAST($1 AS timestamptz) WHERE \"id\" = CAST($2 AS int4)"]);
    }

    #[test]
    fn other_providers_quote_and_return_keys_their_way() {
        let mut t = customers(Provider::Sqlite);
        modify(&mut t, 0, 1, "Ada L.".into());
        t.add_row(vec![DbValue::Int(-1), "Barbara".into(), DbValue::Null]);
        let adapter = TableAdapter::new("db", "SELECT * FROM customers").with_update_table("dbo.customers");
        let ms = adapter.plan_update(Provider::SqlServer, &t).expect("plan");
        assert_eq!(
            ms.sql(),
            [
                "UPDATE [dbo].[customers] SET [name] = @P1 WHERE [id] = @P2",
                "INSERT INTO [dbo].[customers] ([name], [created]) OUTPUT INSERTED.[id] VALUES (@P1, @P2)",
            ]
        );
        assert_eq!(ms.statements[1].temp_key, Some(DbValue::Int(-1)), "a temporary key maps to the generated one");
        let my = TableAdapter::new("db", "SELECT * FROM customers").with_update_table("customers").plan_update(Provider::MySql, &t).expect("plan");
        assert_eq!(my.sql(), ["UPDATE `customers` SET `name` = ? WHERE `id` = ?", "INSERT INTO `customers` (`name`, `created`) VALUES (?, ?)"]);
        assert_eq!(my.statements[1].expect, Expect::LastInsertId);
    }

    #[test]
    fn optimistic_concurrency_on_original_values_and_row_versions() {
        let mut t = customers(Provider::Sqlite);
        modify(&mut t, 1, 1, "Linus T.".into());
        let mut a = TableAdapter::new("db", "x").with_update_table("customers");
        a.conflict_option = ConflictOption::CompareAllSearchableValues;
        let plan = a.plan_update(Provider::Sqlite, &t).expect("plan");
        assert_eq!(plan.sql(), ["UPDATE \"customers\" SET \"name\" = ?1 WHERE \"id\" = ?2 AND \"name\" = ?3 AND \"created\" IS NULL"]);
        assert_eq!(plan.statements[0].params, vec![DbValue::Text("Linus T.".into()), DbValue::Int(2), DbValue::Text("Linus".into())]);

        let mut v = customers(Provider::Postgres);
        v.columns.push(DataColumn::new("xmin", DbType::new(DbKind::Text, "text")));
        v.columns[3].read_only = true;
        for r in 0..3 {
            let id = v.rows()[r].id;
            let row = v.row_by_id_mut(id).expect("row");
            row.values.push(DbValue::Text(format!("{}", 700 + r)));
            if let Some(o) = row.original.as_mut() {
                o.push(DbValue::Text(format!("{}", 700 + r)));
            }
        }
        modify(&mut v, 0, 1, "Ada L.".into());
        let mut b = TableAdapter::new("db", "x").with_update_table("customers");
        b.conflict_option = ConflictOption::CompareRowVersion;
        b.row_version_column = "xmin".into();
        let plan = b.plan_update(Provider::Postgres, &v).expect("plan");
        assert_eq!(plan.sql(), ["UPDATE \"customers\" SET \"name\" = CAST($1 AS text) WHERE \"id\" = CAST($2 AS int4) AND \"xmin\"::text = $3 RETURNING \"xmin\"::text"]);
        assert_eq!(plan.rows[0].returns, vec![3]);
        b.row_version_column = String::new();
        assert!(matches!(b.plan_update(Provider::Postgres, &v), Err(DataError::Config(_))));
    }

    #[test]
    fn custom_commands_take_current_and_original_values() {
        let mut t = customers(Provider::Sqlite);
        modify(&mut t, 0, 1, "Ada L.".into());
        let mut a = TableAdapter::new("db", "x").with_update_table("customers");
        a.update_command = "UPDATE customers SET name = @name WHERE id = @Original_id AND name = @Original_name".into();
        let plan = a.plan_update(Provider::Sqlite, &t).expect("plan");
        assert_eq!(plan.sql(), ["UPDATE customers SET name = ?1 WHERE id = ?2 AND name = ?3"]);
        assert_eq!(plan.statements[0].params, vec![DbValue::Text("Ada L.".into()), DbValue::Int(1), DbValue::Text("Ada".into())]);
        a.update_command = "UPDATE customers SET x = @nope".into();
        assert!(matches!(a.plan_update(Provider::Sqlite, &t), Err(DataError::Validation(m)) if m.contains("@nope")));
    }

    #[test]
    fn paged_selects() {
        let mut a = TableAdapter::new("db", "SELECT id, name FROM customers ORDER BY id;");
        a.page_size = 50;
        let base = trim_statement(&a.select_command).to_string();
        let (text, count) = a.paged_text(Provider::Postgres, &base, 2, false).expect("paged");
        assert_eq!(text, "SELECT id, name FROM customers ORDER BY id LIMIT @kubuno_limit OFFSET @kubuno_offset");
        assert_eq!(count.as_deref(), Some("SELECT COUNT(*) FROM (SELECT id, name FROM customers) AS kubuno_count"));
        let (ms, _) = a.paged_text(Provider::SqlServer, "SELECT id FROM t", 1, false).expect("paged");
        assert_eq!(ms, "SELECT id FROM t ORDER BY (SELECT NULL) OFFSET @kubuno_offset ROWS FETCH NEXT @kubuno_limit ROWS ONLY");
        a.paging_mode = PagingMode::Keyset;
        let (k, _) = a.paged_text(Provider::Sqlite, &base, 1, true).expect("keyset");
        assert_eq!(k, "SELECT * FROM (SELECT id, name FROM customers) AS kubuno_page WHERE kubuno_page.\"id\" > @kubuno_after ORDER BY kubuno_page.\"id\" LIMIT @kubuno_limit");
    }

    #[test]
    fn refuses_what_it_cannot_save_safely() {
        let mut t = customers(Provider::Sqlite);
        let first = t.rows()[0].id;
        t.delete_row(first);
        assert!(matches!(TableAdapter::new("db", "x").plan_update(Provider::Sqlite, &t), Err(DataError::Config(_))));
        assert!(matches!(TableAdapter::new("db", "x").with_update_table("customers; drop").plan_update(Provider::Sqlite, &t), Err(DataError::Validation(_))));
        let mut no_key = t.clone();
        for c in &mut no_key.columns {
            c.primary_key = false;
        }
        assert!(matches!(TableAdapter::new("db", "x").with_update_table("customers").plan_update(Provider::Sqlite, &no_key), Err(DataError::Validation(m)) if m.contains("primary key")));
    }

    #[test]
    fn a_committed_plan_is_applied_with_generated_keys() {
        let mut t = customers(Provider::Sqlite);
        let added = t.add_row(vec![DbValue::Null, "Barbara".into(), DbValue::Null]);
        let second = t.rows()[1].id;
        t.delete_row(second);
        let adapter = TableAdapter::new("db", "x").with_update_table("customers");
        let plan = adapter.plan_update(Provider::Sqlite, &t).expect("plan");
        // Edited again while the save runs: stays Modified against what was saved.
        t.row_by_id_mut(added).expect("row").values[1] = "Barbara L.".into();
        t.apply_update(&plan, &UpdateOutcome { returned: vec![None, Some(vec![DbValue::Int(4)])], key_map: Vec::new() });
        assert!(t.row_by_id(second).is_none());
        let row = t.row_by_id(added).expect("row");
        assert_eq!(row.values[0], DbValue::Int(4));
        assert_eq!(row.state, RowState::Modified);
        assert_eq!(row.original.as_ref().map(|o| o[1].clone()), Some(DbValue::Text("Barbara".into())));
    }

    #[test]
    fn plans_of_masters_and_details_run_in_dependency_order() {
        let mut m = customers(Provider::Sqlite);
        let gone = m.rows()[2].id;
        m.delete_row(gone);
        m.add_row(vec![DbValue::Int(-1), "New".into(), DbValue::Null]);
        let mut d = Table::new("orders", vec![DataColumn::new("id", DbType::new(DbKind::Int, "INTEGER")), DataColumn::new("customer_id", DbType::new(DbKind::Int, "INTEGER"))]);
        d.columns[0].primary_key = true;
        d.columns[0].auto_increment = true;
        d.load_row(vec![DbValue::Int(9), DbValue::Int(3)]);
        let old = d.rows()[0].id;
        d.delete_row(old);
        d.add_row(vec![DbValue::Null, DbValue::Int(-1)]);
        let master = TableAdapter::new("db", "x").with_update_table("customers").plan_update(Provider::Sqlite, &m).expect("plan");
        let detail = TableAdapter::new("db", "x").with_update_table("orders").plan_update_with(Provider::Sqlite, &d, Some(1)).expect("plan");
        assert_eq!(detail.statements[1].fk_params, vec![0], "the master key parameter of the detail insert");
        let plans = [master, detail];
        // Execution order: detail delete, master delete, master insert, detail insert.
        let mut order: Vec<String> = Vec::new();
        for kind in [PlannedKind::Delete, PlannedKind::Update, PlannedKind::Insert] {
            let po: Vec<usize> = if kind == PlannedKind::Delete { vec![1, 0] } else { vec![0, 1] };
            for p in po {
                for (i, r) in plans[p].rows.iter().enumerate() {
                    if r.kind == kind {
                        order.push(plans[p].statements[i].sql.clone());
                    }
                }
            }
        }
        assert!(order[0].starts_with("DELETE FROM \"orders\"") && order[1].starts_with("DELETE FROM \"customers\""));
        assert!(order[2].starts_with("INSERT INTO \"customers\"") && order[3].starts_with("INSERT INTO \"orders\""));
        let split = split_outcome(&plans, UpdateOutcome { returned: vec![None, Some(vec![DbValue::Int(4)]), None, Some(vec![DbValue::Int(10)])], key_map: vec![(DbValue::Int(-1), DbValue::Int(4))] });
        assert_eq!(split[0].returned, vec![None, Some(vec![DbValue::Int(4)])]);
        let mut d2 = d.clone();
        d2.apply_update(&plans[1], &split[1]);
        assert_eq!(d2.rows()[0].values, vec![DbValue::Int(10), DbValue::Int(4)], "the detail references the generated master key");
    }

    #[test]
    fn keys_default_to_id() {
        let mut t = Table::new("t", vec![DataColumn::new("ID", DbType::new(DbKind::Int, "INTEGER")), DataColumn::new("x", DbType::default())]);
        apply_keys(&mut t, &[], true);
        assert!(t.columns[0].primary_key && t.columns[0].auto_increment);
        apply_keys(&mut t, &["x".to_string()], false);
        assert!(!t.columns[0].primary_key && t.columns[1].primary_key && !t.columns[0].auto_increment);
    }
}
