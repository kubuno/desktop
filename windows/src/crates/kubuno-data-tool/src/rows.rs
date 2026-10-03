//! Running SQL on a session and reading what comes back: cancellation-safe task awaiting, cell
//! accessors tolerant of the small differences between drivers, and the display form of values.

use std::time::Duration;

use kubuno_data::{Canceller, ConnectionHandle, DataTask, DbCommand, DbValue, Table};
use serde::Serialize;

use crate::error::{ToolError, ToolResult};

/// Cancels the data task when the request's future is dropped (an aborted request): awaiting a
/// task and dropping it would otherwise leave the operation running.
struct CancelOnDrop(Canceller);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Awaits a data task, cancelling it if the caller is dropped first.
pub async fn wait<T>(task: DataTask<T>) -> ToolResult<T> {
    let _guard = CancelOnDrop(task.canceller());
    Ok(task.await?)
}

/// A catalog query with named parameters (`@schema`): the values are always bound, never written
/// into the text. The command is built and dropped before the await (it is not `Send`).
fn catalog_task(handle: &ConnectionHandle, sql: &str, params: &[(&str, &str)]) -> DataTask<Table> {
    let mut command = DbCommand::with_text(sql);
    for (name, value) in params {
        command.param(name, *value);
    }
    command.query(handle)
}

/// Runs a fixed catalog query with bound parameters.
pub async fn catalog(handle: &ConnectionHandle, sql: &str, params: &[(&str, &str)]) -> ToolResult<Table> {
    wait(catalog_task(handle, sql, params)).await
}

/// Runs a fixed catalog query and returns the cells of its rows.
pub async fn catalog_rows(handle: &ConnectionHandle, sql: &str, params: &[(&str, &str)]) -> ToolResult<Vec<Vec<DbValue>>> {
    Ok(catalog(handle, sql, params).await?.rows().iter().map(|r| r.values.clone()).collect())
}

/// Runs SQL text verbatim (user text, `data.top`).
pub async fn raw_query(handle: &ConnectionHandle, sql: &str, timeout: Option<Duration>) -> ToolResult<Table> {
    wait(handle.query_raw(sql, timeout)).await
}

pub async fn raw_execute(handle: &ConnectionHandle, sql: &str, timeout: Option<Duration>) -> ToolResult<u64> {
    wait(handle.execute_raw(sql, timeout)).await
}

// ---- cells -----------------------------------------------------------------------------------

static NULL: DbValue = DbValue::Null;

/// The cell `i` of a row (`NULL` past the end).
pub fn cell(row: &[DbValue], i: usize) -> &DbValue {
    row.get(i).unwrap_or(&NULL)
}

pub fn text(v: &DbValue) -> String {
    match v {
        DbValue::Null => String::new(),
        DbValue::Bool(b) => b.to_string(),
        DbValue::Int(i) => i.to_string(),
        DbValue::Float(f) => f.to_string(),
        DbValue::Text(s) => s.clone(),
        DbValue::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

pub fn opt_text(v: &DbValue) -> Option<String> {
    (!v.is_null()).then(|| text(v))
}

pub fn int(v: &DbValue) -> i64 {
    match v {
        DbValue::Int(i) => *i,
        DbValue::Float(f) => *f as i64,
        DbValue::Bool(b) => i64::from(*b),
        DbValue::Text(s) => s.trim().parse().unwrap_or(0),
        DbValue::Bytes(b) => String::from_utf8_lossy(b).trim().parse().unwrap_or(0),
        DbValue::Null => 0,
    }
}

pub fn flag(v: &DbValue) -> bool {
    match v {
        DbValue::Bool(b) => *b,
        DbValue::Int(i) => *i != 0,
        DbValue::Float(f) => *f != 0.0,
        DbValue::Text(s) => matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "y" | "t"),
        DbValue::Bytes(b) => b.first().is_some_and(|x| *x != 0 && *x != b'0'),
        DbValue::Null => false,
    }
}

// ---- display ---------------------------------------------------------------------------------

/// How many bytes of a binary value are shown.
const BYTES_SHOWN: usize = 64;

/// A value as display text: integers exact, floats shortest round-trip, booleans `true`/`false`,
/// bytes `0x…` (the first 64, then `…`); `None` for NULL.
pub fn display(v: &DbValue) -> Option<String> {
    match v {
        DbValue::Null => None,
        DbValue::Bool(b) => Some(b.to_string()),
        DbValue::Int(i) => Some(i.to_string()),
        DbValue::Float(f) => Some(f.to_string()),
        DbValue::Text(s) => Some(s.clone()),
        DbValue::Bytes(b) => {
            let mut out = String::with_capacity(2 + BYTES_SHOWN * 2 + 3);
            out.push_str("0x");
            for byte in b.iter().take(BYTES_SHOWN) {
                out.push_str(&format!("{byte:02X}"));
            }
            if b.len() > BYTES_SHOWN {
                out.push('…');
            }
            Some(out)
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResultColumn {
    pub name: String,
    pub db_type: String,
}

/// A result set as the protocol carries it.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResultSet {
    pub columns: Vec<ResultColumn>,
    pub rows: Vec<Vec<Option<String>>>,
    pub truncated: bool,
}

/// The first `max_rows` rows of a table as a result set.
pub fn result_set(table: &Table, max_rows: usize) -> ResultSet {
    let columns = table.columns.iter().map(|c| ResultColumn { name: c.name.clone(), db_type: c.ty.native.clone() }).collect();
    let rows = table.rows().iter().take(max_rows).map(|r| r.values.iter().map(display).collect()).collect();
    ResultSet { columns, rows, truncated: table.rows().len() > max_rows }
}

/// A `Database` error for a request that ran but produced something unusable.
pub fn unexpected(what: &str) -> ToolError {
    tracing::error!(what, "unexpected catalog result");
    ToolError::database(format!("unexpected result from the database: {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_data::{DataColumn, DbType};

    #[test]
    fn values_display_as_the_protocol_says() {
        assert_eq!(display(&DbValue::Null), None);
        assert_eq!(display(&DbValue::Int(i64::MAX)).as_deref(), Some("9223372036854775807"));
        assert_eq!(display(&DbValue::Float(0.1)).as_deref(), Some("0.1"));
        assert_eq!(display(&DbValue::Float(1e21)).as_deref(), Some("1000000000000000000000"));
        assert_eq!(display(&DbValue::Bool(true)).as_deref(), Some("true"));
        assert_eq!(display(&DbValue::Bytes(vec![0, 255, 16])).as_deref(), Some("0x00FF10"));
        let long = display(&DbValue::Bytes(vec![1; 100])).expect("bytes");
        assert_eq!(long.len(), 2 + 128 + '…'.len_utf8());
        assert!(long.ends_with('…'));
    }

    #[test]
    fn result_sets_are_truncated_at_max_rows() {
        let mut t = Table::new("", vec![DataColumn::new("n", DbType::sqlite("INTEGER")), DataColumn::new("s", DbType::sqlite("TEXT"))]);
        for i in 0..5 {
            t.load_row(vec![DbValue::Int(i), if i == 2 { DbValue::Null } else { DbValue::Text(format!("v{i}")) }]);
        }
        let rs = result_set(&t, 3);
        assert!(rs.truncated);
        assert_eq!(rs.rows.len(), 3);
        assert_eq!(rs.rows[2], vec![Some("2".to_string()), None]);
        assert_eq!(rs.columns[1], ResultColumn { name: "s".into(), db_type: "TEXT".into() });
        assert!(!result_set(&t, 5).truncated);
        let json = serde_json::to_string(&rs).expect("json");
        assert!(json.contains("\"dbType\":\"TEXT\"") && json.contains("null"), "{json}");
    }

    #[test]
    fn cells_are_read_leniently() {
        assert_eq!(int(&DbValue::Text(" 42 ".into())), 42);
        assert!(flag(&DbValue::Text("YES".into())) && flag(&DbValue::Int(1)) && !flag(&DbValue::Null));
        assert_eq!(text(&DbValue::Bytes(b"abc".to_vec())), "abc");
        assert_eq!(cell(&[], 3), &DbValue::Null);
    }
}
